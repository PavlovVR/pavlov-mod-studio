//! Browse and download shared source plugins.
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use zip::ZipArchive;

use crate::api::{Api, GAME_ID};
use crate::eula::require_eula;
use crate::library::local_mods;
use crate::say;
use crate::share::has_choice;

/// How a listed plugin compares with what's in the modkit.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Installed {
    No,
    Current,
    Outdated,
    /// Shared from this modkit, so a download would replace the real project.
    Shared,
}

#[derive(Serialize)]
pub struct SourceMod {
    pub id: u64,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub logo: Option<String>,
    pub filesize: u64,
    pub profile_url: Option<String>,
    pub platforms: Vec<String>,
    pub installed: Installed,
}

#[derive(Serialize)]
pub struct SourcePage {
    pub mods: Vec<SourceMod>,
    pub total: u64,
}

pub struct SourceQuery<'a> {
    pub search: &'a str,
    /// A mod.io value, such as `-date_live`, `-downloads_total` or `name`.
    pub sort: &'a str,
    pub offset: u64,
    pub limit: u64,
    /// Only what's in this modkit, so an update on a later page isn't missed.
    pub installed: bool,
}

fn records_path(project: &Path) -> PathBuf {
    project.join("Saved/PavlovModStudio/sources.json")
}

/// Which mod.io file each plugin was installed from, by mod id.
fn load_records(project: &Path) -> BTreeMap<u64, u64> {
    fs::read_to_string(records_path(project))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn remember_file(project: &Path, mod_id: u64, file_id: u64) -> Result<()> {
    let mut records = load_records(project);
    records.insert(mod_id, file_id);
    let path = records_path(project);
    fs::create_dir_all(path.parent().context("bad records path")?)?;
    fs::write(path, serde_json::to_string_pretty(&records)?)?;
    Ok(())
}

fn seconds_modified(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

fn installed_state(
    project: &Path,
    records: &BTreeMap<u64, u64>,
    mod_id: u64,
    file: &Value,
) -> Installed {
    let plugin = format!("UGC{mod_id}");
    let descriptor = project
        .join("Plugins")
        .join(&plugin)
        .join(format!("{plugin}.uplugin"));
    if !descriptor.is_file() {
        return Installed::No;
    }
    if has_choice(project, mod_id) {
        return Installed::Shared;
    }
    let outdated = match (records.get(&mod_id), file["id"].as_u64()) {
        (Some(saved), Some(live)) => *saved != live,
        // The modkit keeps no record of what it installed, so only the dates can tell.
        _ => seconds_modified(&descriptor)
            .zip(file["date_added"].as_u64())
            .is_some_and(|(plugin, live)| plugin < live),
    };
    if outdated {
        Installed::Outdated
    } else {
        Installed::Current
    }
}

fn to_source_mod(item: &Value, project: &Path, records: &BTreeMap<u64, u64>) -> Option<SourceMod> {
    let id = item["id"].as_u64()?;
    Some(SourceMod {
        id,
        name: item["name"].as_str()?.to_string(),
        summary: item["summary"].as_str().unwrap_or_default().to_string(),
        author: item["submitted_by"]["username"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        logo: item["logo"]["thumb_320x180"].as_str().map(str::to_string),
        filesize: item["modfile"]["filesize"].as_u64().unwrap_or(0),
        profile_url: item["profile_url"].as_str().map(str::to_string),
        platforms: item["platforms"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|entry| entry["platform"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        installed: installed_state(project, records, id, &item["modfile"]),
    })
}

pub fn browse(api: &Api, project: &Path, request: &SourceQuery) -> Result<SourcePage> {
    let path = format!("/games/{GAME_ID}/mods");
    let pattern = format!("*{}*", request.search.trim());
    let offset = request.offset.to_string();
    let limit = request.limit.to_string();
    let ids = if request.installed {
        let local: Vec<String> = local_mods(project)?
            .iter()
            .map(|m| m.id.to_string())
            .collect();
        if local.is_empty() {
            return Ok(SourcePage {
                mods: Vec::new(),
                total: 0,
            });
        }
        local.join(",")
    } else {
        String::new()
    };
    let page = api.send("browse source plugins", &[], || {
        let mut query = vec![
            ("tags", "Mod"),
            ("_sort", request.sort),
            ("_offset", offset.as_str()),
            ("_limit", limit.as_str()),
        ];
        if !request.search.trim().is_empty() {
            query.push(("name-lk", pattern.as_str()));
        }
        if request.installed {
            query.push(("id-in", ids.as_str()));
        }
        api.client()
            .get(api.url(&path))
            .header("X-Modio-Platform", "source")
            .query(&query)
    })?;
    let empty = Vec::new();
    let records = load_records(project);
    Ok(SourcePage {
        mods: page["data"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|item| to_source_mod(item, project, &records))
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
    require_eula(project)?;
    // A share can hold part of a mod, so this would swap the real project for that part.
    if has_choice(project, mod_id) {
        bail!("You share this mod from this modkit, so downloading it would replace your project with the part you shared.");
    }
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
    // Without this the plugin still works, and its dates say it's current until the next file.
    if let Some(file_id) = details["modfile"]["id"].as_u64() {
        if let Err(error) = remember_file(project, mod_id, file_id) {
            say!("could not record the installed version: {error:#}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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

    fn with_plugin(name: &str, id: u64) -> PathBuf {
        let dir = project(name);
        let plugin = dir.join(format!("Plugins/UGC{id}"));
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join(format!("UGC{id}.uplugin")), "{}").unwrap();
        dir
    }

    #[test]
    fn a_missing_plugin_is_not_installed() {
        let dir = project("missing");
        let file = json!({ "id": 5, "date_added": 1 });
        let state = installed_state(&dir, &BTreeMap::new(), 7, &file);
        assert_eq!(state, Installed::No);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_saved_file_decides_whether_an_update_exists() {
        let dir = with_plugin("recorded", 7);
        remember_file(&dir, 7, 5).unwrap();
        let records = load_records(&dir);
        let same = json!({ "id": 5, "date_added": u64::MAX });
        let newer = json!({ "id": 6, "date_added": 1 });
        assert_eq!(
            installed_state(&dir, &records, 7, &same),
            Installed::Current
        );
        assert_eq!(
            installed_state(&dir, &records, 7, &newer),
            Installed::Outdated
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn without_a_record_the_dates_decide() {
        let dir = with_plugin("unrecorded", 7);
        let records = BTreeMap::new();
        let older = json!({ "id": 5, "date_added": 1 });
        let newer = json!({ "id": 5, "date_added": u64::MAX });
        assert_eq!(
            installed_state(&dir, &records, 7, &older),
            Installed::Current
        );
        assert_eq!(
            installed_state(&dir, &records, 7, &newer),
            Installed::Outdated
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_shared_plugin_never_offers_an_update() {
        let dir = with_plugin("shared", 7);
        fs::create_dir_all(dir.join("Saved/PavlovModStudio")).unwrap();
        fs::write(
            dir.join("Saved/PavlovModStudio/UGC7.share.json"),
            r#"{"files":["Content/a.uasset"]}"#,
        )
        .unwrap();
        let file = json!({ "id": 5, "date_added": u64::MAX });
        let state = installed_state(&dir, &BTreeMap::new(), 7, &file);
        assert_eq!(state, Installed::Shared);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_damaged_record_counts_as_no_record() {
        let dir = project("damaged");
        fs::create_dir_all(dir.join("Saved/PavlovModStudio")).unwrap();
        fs::write(records_path(&dir), "not json").unwrap();
        assert!(load_records(&dir).is_empty());
        remember_file(&dir, 7, 5).unwrap();
        assert_eq!(load_records(&dir).get(&7), Some(&5));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_what_the_list_shows() {
        let dir = project("listing");
        let item = json!({
            "id": 7, "name": "Trees", "profile_url": "https://mod.io/g/pavlov/m/trees",
            "platforms": [{ "platform": "source" }, { "platform": "windows" }],
            "modfile": { "id": 5, "filesize": 10 },
        });
        let parsed = to_source_mod(&item, &dir, &BTreeMap::new()).unwrap();
        assert_eq!(
            parsed.profile_url.as_deref(),
            Some("https://mod.io/g/pavlov/m/trees")
        );
        assert_eq!(parsed.platforms, ["source", "windows"]);
        assert_eq!(parsed.installed, Installed::No);
        fs::remove_dir_all(dir).ok();
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
