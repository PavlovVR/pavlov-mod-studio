//! Manage a published mod: its files, live versions and details.
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use reqwest::blocking::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{text_form, Api, GAME_ID};
use crate::library::{to_mod_info, ModInfo};

#[derive(Serialize)]
pub struct ModFile {
    pub id: i64,
    pub version: String,
    pub filesize: u64,
    pub date_added: i64,
    pub platforms: Vec<String>,
    pub scanned: bool,
    pub filename: String,
    pub changelog: String,
    pub md5: String,
    pub uncompressed: u64,
    /// mod.io found something harmful in the file.
    pub flagged: bool,
    /// Platforms where this file is waiting on approval or was denied.
    pub pending: Vec<String>,
    pub denied: Vec<String>,
    pub download: Option<String>,
}

fn platforms_with_status(file: &Value, status: i64) -> Vec<String> {
    file["platforms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["status"].as_i64() == Some(status))
        .filter_map(|p| p["platform"].as_str().map(str::to_string))
        .collect()
}

fn mod_path(mod_id: u64) -> String {
    format!("/games/{GAME_ID}/mods/{mod_id}")
}

pub fn list_files(api: &Api, mod_id: u64) -> Result<Vec<ModFile>> {
    let path = format!("{}/files", mod_path(mod_id));
    let files = api.pages("list files", &path, &[("_sort", "-id")])?;
    let empty = Vec::new();
    Ok(files
        .iter()
        .filter_map(|file| {
            Some(ModFile {
                id: file["id"].as_i64()?,
                version: file["version"].as_str().unwrap_or_default().to_string(),
                filesize: file["filesize"].as_u64().unwrap_or(0),
                date_added: file["date_added"].as_i64().unwrap_or(0),
                platforms: file["platforms"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .filter_map(|p| p["platform"].as_str().map(str::to_string))
                    .collect(),
                scanned: file["virus_status"].as_i64() == Some(1),
                filename: file["filename"].as_str().unwrap_or_default().to_string(),
                changelog: file["changelog"].as_str().unwrap_or_default().to_string(),
                md5: file["filehash"]["md5"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                uncompressed: file["filesize_uncompressed"].as_u64().unwrap_or(0),
                flagged: file["virus_positive"].as_i64().unwrap_or(0) != 0,
                pending: platforms_with_status(file, 0),
                denied: platforms_with_status(file, 2),
                download: file["download"]["binary_url"].as_str().map(str::to_string),
            })
        })
        .collect())
}

/// Make a file live for its platforms. This is also how a release is rolled back.
pub fn make_live(api: &Api, mod_id: u64, file_id: i64) -> Result<()> {
    let path = format!("{}/files/{file_id}", mod_path(mod_id));
    api.send("publish file", &[], || {
        api.client().put(api.url(&path)).form(&[("active", "true")])
    })?;
    Ok(())
}

/// Change a file's version label or changelog. The build itself can't be changed.
pub fn edit_file(
    api: &Api,
    mod_id: u64,
    file_id: i64,
    version: Option<&str>,
    changelog: Option<&str>,
) -> Result<()> {
    let mut fields = Vec::new();
    if let Some(version) = version {
        if version.trim().is_empty() {
            bail!("A file needs a version");
        }
        fields.push(("version", version.trim()));
    }
    if let Some(changelog) = changelog {
        fields.push(("changelog", changelog.trim()));
    }
    if fields.is_empty() {
        return Ok(());
    }
    let path = format!("{}/files/{file_id}", mod_path(mod_id));
    api.send("edit file", &[], || {
        api.client().put(api.url(&path)).form(&fields)
    })?;
    Ok(())
}

/// Delete a file for good. mod.io refuses this for a live file.
pub fn delete_file(api: &Api, mod_id: u64, file_id: i64) -> Result<()> {
    let path = format!("{}/files/{file_id}", mod_path(mod_id));
    api.send("delete file", &[], || api.client().delete(api.url(&path)))?;
    Ok(())
}

/// Only the fields being changed, because mod.io applies whatever it's sent.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct ModEdit {
    pub name: Option<String>,
    pub name_id: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub homepage_url: Option<String>,
    pub visible: Option<bool>,
    pub maturity_option: Option<u64>,
    pub community_options: Option<u64>,
    pub credit_options: Option<u64>,
}

impl ModEdit {
    fn fields(&self) -> Result<Vec<(&'static str, String)>> {
        let mut fields = Vec::new();
        if let Some(name) = &self.name {
            let length = name.trim().chars().count();
            if !(3..=80).contains(&length) {
                bail!("A mod name needs 3 to 80 characters");
            }
            fields.push(("name", name.trim().to_string()));
        }
        if let Some(name_id) = &self.name_id {
            let slug = name_id.trim();
            let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(c);
            if slug.is_empty() || slug.len() > 80 || !slug.chars().all(valid) {
                bail!("A web address name can only use lowercase letters, numbers, - and _");
            }
            fields.push(("name_id", slug.to_string()));
        }
        if let Some(summary) = &self.summary {
            let length = summary.trim().chars().count();
            if length == 0 || length > 250 {
                bail!("A summary needs 1 to 250 characters");
            }
            fields.push(("summary", summary.trim().to_string()));
        }
        if let Some(description) = &self.description {
            if description.chars().count() > 50_000 {
                bail!("A description can't be over 50,000 characters");
            }
            fields.push(("description", description.clone()));
        }
        if let Some(url) = &self.homepage_url {
            let url = url.trim();
            if !url.is_empty() && !url.starts_with("http://") && !url.starts_with("https://") {
                bail!("A homepage must start with http:// or https://");
            }
            fields.push(("homepage_url", url.to_string()));
        }
        if let Some(visible) = self.visible {
            fields.push(("visible", u8::from(visible).to_string()));
        }
        if let Some(value) = self.maturity_option {
            fields.push(("maturity_option", value.to_string()));
        }
        if let Some(value) = self.community_options {
            fields.push(("community_options", value.to_string()));
        }
        if let Some(value) = self.credit_options {
            fields.push(("credit_options", value.to_string()));
        }
        Ok(fields)
    }
}

pub fn edit_mod(api: &Api, mod_id: u64, edit: &ModEdit) -> Result<()> {
    let fields = edit.fields()?;
    if fields.is_empty() {
        return Ok(());
    }
    let path = mod_path(mod_id);
    api.send("edit mod", &[], || {
        api.client()
            .post(api.url(&path))
            .multipart(text_form(&fields))
    })?;
    Ok(())
}

/// Close a mod. It disappears from the game and API, and only the mod.io website can restore it.
pub fn delete_mod(api: &Api, mod_id: u64) -> Result<()> {
    let path = mod_path(mod_id);
    api.send("delete mod", &[], || api.client().delete(api.url(&path)))?;
    Ok(())
}

fn image_part(path: &Path) -> Result<Part> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("that image has no file name")?
        .to_string();
    let lower = name.to_lowercase();
    if ![".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e)) {
        bail!("{name} must be a PNG or JPG image");
    }
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() > 8 * 1024 * 1024 {
        bail!("{name} is over mod.io's 8 MB image limit");
    }
    Ok(Part::bytes(bytes).file_name(name))
}

/// Add screenshots to the mod's gallery.
pub fn add_images(api: &Api, mod_id: u64, images: &[std::path::PathBuf]) -> Result<()> {
    if images.is_empty() {
        return Ok(());
    }
    for image in images {
        image_part(image)?;
    }
    let path = format!("{}/media", mod_path(mod_id));
    api.send("add images", &[], || {
        let mut form = Form::new();
        for (index, image) in images.iter().enumerate() {
            if let Ok(part) = image_part(image) {
                form = form.part(format!("image{index}"), part);
            }
        }
        api.client().post(api.url(&path)).multipart(form)
    })?;
    Ok(())
}

/// Which gallery entries a change is about. Images go by file name, links by URL.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct MediaList {
    pub images: Vec<String>,
    pub youtube: Vec<String>,
    pub sketchfab: Vec<String>,
}

impl MediaList {
    fn fields(&self) -> Vec<(&'static str, &str)> {
        let mut fields = Vec::new();
        for (key, items) in [
            ("images[]", &self.images),
            ("youtube[]", &self.youtube),
            ("sketchfab[]", &self.sketchfab),
        ] {
            fields.extend(items.iter().map(|item| (key, item.as_str())));
        }
        fields
    }
}

/// Add video and 3D model links to the gallery.
pub fn add_links(api: &Api, mod_id: u64, youtube: &[String], sketchfab: &[String]) -> Result<()> {
    if youtube.is_empty() && sketchfab.is_empty() {
        return Ok(());
    }
    for link in youtube {
        if !link.contains("youtube.com/") && !link.contains("youtu.be/") {
            bail!("{link} is not a YouTube link");
        }
    }
    for link in sketchfab {
        if !link.contains("sketchfab.com/") {
            bail!("{link} is not a Sketchfab link");
        }
    }
    let path = format!("{}/media", mod_path(mod_id));
    api.send("add links", &[], || {
        let mut form = Form::new();
        for link in youtube {
            form = form.text("youtube[]", link.clone());
        }
        for link in sketchfab {
            form = form.text("sketchfab[]", link.clone());
        }
        api.client().post(api.url(&path)).multipart(form)
    })?;
    Ok(())
}

pub fn delete_media(api: &Api, mod_id: u64, media: &MediaList) -> Result<()> {
    let fields = media.fields();
    if fields.is_empty() {
        return Ok(());
    }
    let path = format!("{}/media", mod_path(mod_id));
    api.send("delete media", &[], || {
        api.client().delete(api.url(&path)).form(&fields)
    })?;
    Ok(())
}

/// Set the gallery order. Each list holds every entry of its kind, in the new order.
pub fn reorder_media(api: &Api, mod_id: u64, media: &MediaList) -> Result<()> {
    let fields = media.fields();
    if fields.is_empty() {
        return Ok(());
    }
    let path = format!("{}/media/reorder", mod_path(mod_id));
    api.send("reorder media", &[], || {
        api.client().put(api.url(&path)).form(&fields)
    })?;
    Ok(())
}

pub fn dependencies(api: &Api, mod_id: u64) -> Result<Vec<ModInfo>> {
    let path = format!("{}/dependencies", mod_path(mod_id));
    let items = api.pages("list dependencies", &path, &[])?;
    Ok(items.iter().filter_map(to_mod_info).collect())
}

fn dependency_fields(ids: &[u64]) -> Vec<(&'static str, String)> {
    ids.iter()
        .map(|id| ("dependencies[]", id.to_string()))
        .collect()
}

pub fn add_dependencies(api: &Api, mod_id: u64, ids: &[u64]) -> Result<()> {
    if ids.contains(&mod_id) {
        bail!("A mod can't depend on itself");
    }
    let path = format!("{}/dependencies", mod_path(mod_id));
    let fields = dependency_fields(ids);
    api.send("add dependencies", &[], || {
        api.client().post(api.url(&path)).form(&fields)
    })?;
    Ok(())
}

pub fn remove_dependencies(api: &Api, mod_id: u64, ids: &[u64]) -> Result<()> {
    let path = format!("{}/dependencies", mod_path(mod_id));
    let fields = dependency_fields(ids);
    api.send("remove dependencies", &[], || {
        api.client().delete(api.url(&path)).form(&fields)
    })?;
    Ok(())
}

#[derive(Serialize)]
pub struct MetadataPair {
    pub key: String,
    pub value: String,
}

pub fn metadata(api: &Api, mod_id: u64) -> Result<Vec<MetadataPair>> {
    let path = format!("{}/metadatakvp", mod_path(mod_id));
    let items = api.pages("list metadata", &path, &[])?;
    Ok(items
        .iter()
        .filter_map(|item| {
            Some(MetadataPair {
                key: item["metakey"].as_str()?.to_string(),
                value: item["metavalue"].as_str()?.to_string(),
            })
        })
        .collect())
}

fn metadata_field(key: &str, value: &str) -> Result<String> {
    let key = key.trim();
    if key.is_empty() || key.contains(':') || key.chars().count() > 255 {
        bail!("A metadata key needs 1 to 255 characters and can't contain a colon");
    }
    if value.is_empty() || value.chars().count() > 255 {
        bail!("A metadata value needs 1 to 255 characters");
    }
    Ok(format!("{key}:{value}"))
}

pub fn add_metadata(api: &Api, mod_id: u64, key: &str, value: &str) -> Result<()> {
    let field = metadata_field(key, value)?;
    let path = format!("{}/metadatakvp", mod_path(mod_id));
    api.send("add metadata", &[], || {
        api.client()
            .post(api.url(&path))
            .form(&[("metadata[]", field.as_str())])
    })?;
    Ok(())
}

pub fn remove_metadata(api: &Api, mod_id: u64, key: &str, value: &str) -> Result<()> {
    let field = metadata_field(key, value)?;
    let path = format!("{}/metadatakvp", mod_path(mod_id));
    api.send("remove metadata", &[], || {
        api.client()
            .delete(api.url(&path))
            .form(&[("metadata[]", field.as_str())])
    })?;
    Ok(())
}

pub fn edit_details(api: &Api, mod_id: u64, name: &str, summary: &str) -> Result<()> {
    // mod.io would accept a blank field and wipe what's there.
    if name.trim().chars().count() < 3 || summary.trim().is_empty() {
        bail!("A mod needs a name of at least 3 characters and a summary");
    }
    let path = mod_path(mod_id);
    api.send("edit mod", &[], || {
        api.client().post(api.url(&path)).multipart(text_form(&[
            ("name", name.trim()),
            ("summary", summary.trim()),
        ]))
    })?;
    Ok(())
}

pub fn set_logo(api: &Api, mod_id: u64, logo: &Path) -> Result<()> {
    let bytes = fs::read(logo).with_context(|| format!("reading {}", logo.display()))?;
    let file_name = logo
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("logo.png")
        .to_string();
    let path = format!("{}/media", mod_path(mod_id));
    api.send("set logo", &[], || {
        let part = Part::bytes(bytes.clone()).file_name(file_name.clone());
        api.client()
            .post(api.url(&path))
            .multipart(Form::new().part("logo", part))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{metadata_field, MediaList, ModEdit};

    #[test]
    fn an_edit_only_sends_what_changed() {
        let edit = ModEdit {
            visible: Some(false),
            ..ModEdit::default()
        };
        assert_eq!(edit.fields().unwrap(), vec![("visible", "0".to_string())]);
    }

    #[test]
    fn an_edit_can_clear_optional_text() {
        let edit = ModEdit {
            description: Some(String::new()),
            homepage_url: Some(" ".to_string()),
            ..ModEdit::default()
        };
        assert_eq!(edit.fields().unwrap().len(), 2);
    }

    #[test]
    fn an_edit_rejects_values_that_would_damage_the_listing() {
        let bad = [
            ModEdit {
                name: Some("ab".to_string()),
                ..ModEdit::default()
            },
            ModEdit {
                name: Some("x".repeat(81)),
                ..ModEdit::default()
            },
            ModEdit {
                summary: Some("  ".to_string()),
                ..ModEdit::default()
            },
            ModEdit {
                summary: Some("x".repeat(251)),
                ..ModEdit::default()
            },
            ModEdit {
                homepage_url: Some("example.com".to_string()),
                ..ModEdit::default()
            },
            ModEdit {
                name_id: Some("Has Spaces".to_string()),
                ..ModEdit::default()
            },
            ModEdit {
                name_id: Some(String::new()),
                ..ModEdit::default()
            },
        ];
        for edit in bad {
            assert!(edit.fields().is_err());
        }
    }

    #[test]
    fn long_real_names_are_allowed() {
        let edit = ModEdit {
            name: Some("[MOD] Boombox - Middle Eastern Playlist #02".to_string()),
            ..ModEdit::default()
        };
        assert!(edit.fields().is_ok());
    }

    #[test]
    fn media_fields_use_mod_io_array_names() {
        let media = MediaList {
            images: vec!["a.png".to_string()],
            youtube: vec!["y".to_string()],
            sketchfab: Vec::new(),
        };
        assert_eq!(
            media.fields(),
            vec![("images[]", "a.png"), ("youtube[]", "y")]
        );
    }

    #[test]
    fn metadata_joins_key_and_value_with_a_colon() {
        assert_eq!(metadata_field(" a ", "b:c").unwrap(), "a:b:c");
        assert!(metadata_field("a:b", "c").is_err());
        assert!(metadata_field("", "c").is_err());
        assert!(metadata_field("a", "").is_err());
    }
}
