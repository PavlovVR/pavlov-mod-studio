//! Browse and download shared source plugins.
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use zip::ZipArchive;

use crate::api::{Api, GAME_ID};
use crate::say;

#[derive(Serialize)]
pub struct SourceMod {
    pub id: u64,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub logo: Option<String>,
    pub filesize: u64,
}

#[derive(Serialize)]
pub struct SourcePage {
    pub mods: Vec<SourceMod>,
    pub total: u64,
}

fn to_source_mod(item: &Value) -> Option<SourceMod> {
    Some(SourceMod {
        id: item["id"].as_u64()?,
        name: item["name"].as_str()?.to_string(),
        summary: item["summary"].as_str().unwrap_or_default().to_string(),
        author: item["submitted_by"]["username"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        logo: item["logo"]["thumb_320x180"].as_str().map(str::to_string),
        filesize: item["modfile"]["filesize"].as_u64().unwrap_or(0),
    })
}

/// `sort` takes mod.io values, such as `-date_live`, `-downloads_total` or `name`.
pub fn browse(api: &Api, search: &str, sort: &str, offset: u64, limit: u64) -> Result<SourcePage> {
    let path = format!("/games/{GAME_ID}/mods");
    let pattern = format!("*{}*", search.trim());
    let offset = offset.to_string();
    let limit = limit.to_string();
    let page = api.send("browse source plugins", &[], || {
        let mut query = vec![
            ("tags", "Mod"),
            ("_sort", sort),
            ("_offset", offset.as_str()),
            ("_limit", limit.as_str()),
        ];
        if !search.trim().is_empty() {
            query.push(("name-lk", pattern.as_str()));
        }
        api.client()
            .get(api.url(&path))
            .header("X-Modio-Platform", "source")
            .query(&query)
    })?;
    let empty = Vec::new();
    Ok(SourcePage {
        mods: page["data"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(to_source_mod)
            .collect(),
        total: page["result_total"].as_u64().unwrap_or(0),
    })
}

/// Find the folder holding `<plugin>.uplugin`, whether the zip wraps it or not.
fn plugin_root(extracted: &Path, plugin: &str) -> Result<PathBuf> {
    let descriptor = format!("{plugin}.uplugin");
    if extracted.join(&descriptor).is_file() {
        return Ok(extracted.to_path_buf());
    }
    for entry in fs::read_dir(extracted)? {
        let path = entry?.path();
        if path.join(&descriptor).is_file() {
            return Ok(path);
        }
    }
    bail!("the download does not contain {descriptor}");
}

/// Extract a source zip over a plugin. The old folder is kept as a backup
/// until the new one is in place, so a failure can't lose your work.
pub fn install_zip(zip_path: &Path, project: &Path, mod_id: u64) -> Result<()> {
    let plugin = format!("UGC{mod_id}");
    let downloads = project.join("Saved/Downloads");
    let extracted = downloads.join(&plugin);
    if extracted.exists() {
        fs::remove_dir_all(&extracted)?;
    }
    fs::create_dir_all(&extracted)?;
    // ZipArchive::extract refuses paths that escape the target folder.
    ZipArchive::new(File::open(zip_path)?)
        .context("the download is not a valid zip")?
        .extract(&extracted)?;
    let root = plugin_root(&extracted, &plugin)?;

    let destination = project.join("Plugins").join(&plugin);
    let backup = downloads.join(format!("{plugin}.backup"));
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    if destination.exists() {
        fs::rename(&destination, &backup).context("close Unreal, it may be using this plugin")?;
    }
    if let Err(error) = fs::rename(&root, &destination) {
        if backup.exists() {
            fs::rename(&backup, &destination)?;
        }
        return Err(error).context("could not move the plugin into place");
    }
    fs::remove_dir_all(&extracted).ok();
    say!("installed {plugin}");
    Ok(())
}

pub fn download(api: &Api, project: &Path, mod_id: u64) -> Result<()> {
    let path = format!("/games/{GAME_ID}/mods/{mod_id}");
    let details = api.send("read mod", &[], || {
        api.client()
            .get(api.url(&path))
            .header("X-Modio-Platform", "source")
    })?;
    let url = details["modfile"]["download"]["binary_url"]
        .as_str()
        .context("this mod has no source file to download")?;
    let downloads = project.join("Saved/Downloads");
    fs::create_dir_all(&downloads)?;
    let zip_path = downloads.join(format!("UGC{mod_id}.zip"));
    api.download(url, &zip_path)?;
    install_zip(&zip_path, project, mod_id)?;
    fs::remove_file(&zip_path).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pms-source-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("Plugins")).unwrap();
        fs::create_dir_all(dir.join("Saved/Downloads")).unwrap();
        dir
    }

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, text) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn replaces_the_plugin_and_keeps_a_backup() {
        let dir = project("replace");
        let plugin = dir.join("Plugins/UGC7");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("old.txt"), "old").unwrap();
        let zip = dir.join("in.zip");
        make_zip(&zip, &[("UGC7.uplugin", "{}"), ("Content/a.uasset", "x")]);
        install_zip(&zip, &dir, 7).unwrap();
        assert!(plugin.join("Content/a.uasset").is_file());
        assert!(!plugin.join("old.txt").exists());
        assert!(dir.join("Saved/Downloads/UGC7.backup/old.txt").is_file());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn accepts_a_wrapped_zip() {
        let dir = project("wrapped");
        let zip = dir.join("in.zip");
        make_zip(&zip, &[("UGC8/UGC8.uplugin", "{}")]);
        install_zip(&zip, &dir, 8).unwrap();
        assert!(dir.join("Plugins/UGC8/UGC8.uplugin").is_file());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_bad_zip_leaves_the_existing_plugin_alone() {
        let dir = project("bad");
        let plugin = dir.join("Plugins/UGC9");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("keep.txt"), "keep").unwrap();
        let zip = dir.join("in.zip");
        make_zip(&zip, &[("something_else.txt", "x")]);
        assert!(install_zip(&zip, &dir, 9).is_err());
        assert!(plugin.join("keep.txt").is_file());
        fs::remove_dir_all(dir).ok();
    }
}
