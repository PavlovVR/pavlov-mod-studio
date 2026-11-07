//! What the app shows: mods on this PC and their state on mod.io.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{Api, GAME_ID};
use crate::definition::mod_kind;

#[derive(Serialize)]
pub struct LocalMod {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub source: bool,
}

/// Every UGC plugin in the modkit that has a mod.io id in its folder name.
pub fn local_mods(project: &Path) -> Result<Vec<LocalMod>> {
    let plugins = project.join("Plugins");
    let mut mods = Vec::new();
    for entry in fs::read_dir(&plugins).context("this folder has no Plugins folder")? {
        let folder = entry?.file_name().to_string_lossy().into_owned();
        let Some(id) = folder.strip_prefix("UGC").and_then(|id| id.parse().ok()) else {
            continue;
        };
        let dir = plugins.join(&folder);
        let Ok(text) = fs::read_to_string(dir.join(format!("{folder}.uplugin"))) else {
            continue;
        };
        let plugin: Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or(Value::Null);
        let name = plugin["FriendlyName"]
            .as_str()
            .unwrap_or(&folder)
            .to_string();
        let source = name.starts_with("[SOURCE]");
        let kind = fs::read(dir.join("Content/definition.uasset"))
            .map(|bytes| mod_kind(&bytes))
            .unwrap_or("Unknown")
            .to_string();
        mods.push(LocalMod {
            id,
            name,
            kind,
            source,
        });
    }
    mods.sort_by_key(|m| (m.source, m.id));
    Ok(mods)
}

#[derive(Serialize)]
pub struct PlatformState {
    pub platform: String,
    pub file_id: i64,
    pub version: Option<String>,
}

#[derive(Serialize)]
pub struct ModStatus {
    pub name: String,
    pub summary: String,
    pub logo: Option<String>,
    pub profile_url: Option<String>,
    pub platforms: Vec<PlatformState>,
}

/// The folders Settings would save, once checked.
#[derive(Serialize)]
pub struct FolderCheck {
    /// Unreal's folder, moved down into `Engine` if the folder above it was chosen.
    pub engine: String,
    pub problems: Vec<String>,
}

const UNREAL: &str = "Binaries/Win64/UnrealEditor-Cmd.exe";

/// Catch a wrong folder when it's chosen, instead of at the first cook.
pub fn check_folders(project: &Path, engine: &Path) -> FolderCheck {
    let mut problems = Vec::new();
    if !project.as_os_str().is_empty() && !project.join("Pavlov.uproject").is_file() {
        problems.push("The modkit folder doesn't have Pavlov.uproject in it.".to_string());
    }
    let inner = engine.join("Engine");
    let engine = if !engine.join(UNREAL).is_file() && inner.join(UNREAL).is_file() {
        inner
    } else {
        engine.to_path_buf()
    };
    if !engine.join(UNREAL).is_file() {
        problems.push(
            "Unreal isn't in that folder. Choose the Engine folder inside UE_5.1.".to_string(),
        );
    }
    FolderCheck {
        engine: engine.to_string_lossy().into_owned(),
        problems,
    }
}

pub fn mod_status(api: &Api, mod_id: u64) -> Result<ModStatus> {
    let path = format!("/games/{GAME_ID}/mods/{mod_id}");
    let details = api.send("read mod", &[], || api.client().get(api.url(&path)))?;
    let empty = Vec::new();
    let live: Vec<String> = details["platforms"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|p| p["modfile_live"].as_i64())
        .map(|id| id.to_string())
        .collect();
    // Only the live files matter here, and a mod can have thousands of others.
    let files = if live.is_empty() {
        Vec::new()
    } else {
        let ids = live.join(",");
        let files_path = format!("{path}/files");
        api.pages("list files", &files_path, &[("id-in", ids.as_str())])?
    };
    let versions: HashMap<i64, String> = files
        .iter()
        .filter_map(|f| Some((f["id"].as_i64()?, f["version"].as_str()?.to_string())))
        .collect();
    let platforms = details["platforms"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|p| {
            let file_id = p["modfile_live"].as_i64()?;
            Some(PlatformState {
                platform: p["platform"].as_str()?.to_string(),
                file_id,
                version: versions.get(&file_id).cloned(),
            })
        })
        .collect();
    Ok(ModStatus {
        name: details["name"].as_str().unwrap_or_default().to_string(),
        summary: details["summary"].as_str().unwrap_or_default().to_string(),
        logo: details["logo"]["thumb_320x180"]
            .as_str()
            .map(str::to_string),
        profile_url: details["profile_url"].as_str().map(str::to_string),
        platforms,
    })
}

#[derive(Serialize)]
pub struct ModStats {
    pub downloads_total: u64,
    pub downloads_today: u64,
    pub subscribers: u64,
    pub ratings_positive: u64,
    pub ratings_negative: u64,
    pub rating_text: String,
    pub rank: u64,
    pub rank_total: u64,
}

#[derive(Serialize)]
pub struct LiveFile {
    pub platform: String,
    pub file_id: i64,
}

#[derive(Serialize)]
pub struct GalleryImage {
    pub filename: String,
    pub thumb: String,
}

/// A mod as mod.io describes it, in full.
#[derive(Serialize)]
pub struct ModInfo {
    pub id: u64,
    pub name: String,
    pub name_id: String,
    pub summary: String,
    pub description: String,
    pub homepage_url: String,
    pub logo: Option<String>,
    pub profile_url: Option<String>,
    pub visible: bool,
    pub kind: String,
    pub author: String,
    pub date_added: i64,
    pub date_updated: i64,
    pub maturity_option: u64,
    pub community_options: u64,
    pub credit_options: u64,
    pub stats: ModStats,
    pub platforms: Vec<LiveFile>,
    pub images: Vec<GalleryImage>,
    pub youtube: Vec<String>,
    pub sketchfab: Vec<String>,
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The type is the game's only tag group, and it's what the game filters on.
fn kind_of(item: &Value) -> String {
    item["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tag| tag["name"].as_str())
        .find(|name| matches!(*name, "Map" | "Mod" | "GameMode"))
        .unwrap_or("Unknown")
        .to_string()
}

pub fn to_mod_info(item: &Value) -> Option<ModInfo> {
    let stats = &item["stats"];
    let count = |key: &str| stats[key].as_u64().unwrap_or(0);
    let images = item["media"]["images"].as_array().into_iter().flatten();
    Some(ModInfo {
        id: item["id"].as_u64()?,
        name: text_of(&item["name"]),
        name_id: text_of(&item["name_id"]),
        summary: text_of(&item["summary"]),
        description: text_of(&item["description"]),
        homepage_url: text_of(&item["homepage_url"]),
        logo: item["logo"]["thumb_640x360"]
            .as_str()
            .or(item["logo"]["thumb_320x180"].as_str())
            .map(str::to_string),
        profile_url: item["profile_url"].as_str().map(str::to_string),
        visible: item["visible"].as_i64() != Some(0),
        kind: kind_of(item),
        author: text_of(&item["submitted_by"]["username"]),
        date_added: item["date_added"].as_i64().unwrap_or(0),
        date_updated: item["date_updated"].as_i64().unwrap_or(0),
        maturity_option: item["maturity_option"].as_u64().unwrap_or(0),
        community_options: item["community_options"].as_u64().unwrap_or(0),
        credit_options: item["credit_options"].as_u64().unwrap_or(0),
        stats: ModStats {
            downloads_total: count("downloads_total"),
            downloads_today: count("downloads_today"),
            subscribers: count("subscribers_total"),
            ratings_positive: count("ratings_positive"),
            ratings_negative: count("ratings_negative"),
            rating_text: text_of(&stats["ratings_display_text"]),
            rank: count("popularity_rank_position"),
            rank_total: count("popularity_rank_total_mods"),
        },
        platforms: item["platforms"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some(LiveFile {
                    platform: p["platform"].as_str()?.to_string(),
                    file_id: p["modfile_live"].as_i64()?,
                })
            })
            .collect(),
        images: images
            .filter_map(|image| {
                Some(GalleryImage {
                    filename: image["filename"].as_str()?.to_string(),
                    thumb: image["thumb_320x180"]
                        .as_str()
                        .or(image["original"].as_str())?
                        .to_string(),
                })
            })
            .collect(),
        youtube: strings(&item["media"]["youtube"]),
        sketchfab: strings(&item["media"]["sketchfab"]),
    })
}

pub fn read_mod(api: &Api, mod_id: u64) -> Result<ModInfo> {
    let path = format!("/games/{GAME_ID}/mods/{mod_id}");
    let item = api.send("read mod", &[], || api.client().get(api.url(&path)))?;
    to_mod_info(&item).context("mod.io returned a mod this app can't read")
}

/// Read many mods in one request per hundred.
pub fn read_mods(api: &Api, ids: &[u64]) -> Result<Vec<ModInfo>> {
    let path = format!("/games/{GAME_ID}/mods");
    let mut mods = Vec::new();
    for chunk in ids.chunks(100) {
        let list = chunk
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let items = api.pages("read mods", &path, &[("id-in", list.as_str())])?;
        mods.extend(items.iter().filter_map(to_mod_info));
    }
    Ok(mods)
}

/// Personal tokens are refused here with 11139, so `None` means "can't tell who this is".
fn account_name(api: &Api) -> Result<Option<String>> {
    let me = api.send("read account", &[11139], || {
        api.client().get(api.url("/me"))
    })?;
    if me["error_ref"].is_i64() {
        return Ok(None);
    }
    Ok(me["username"].as_str().map(str::to_string))
}

fn on_team(api: &Api, mod_id: u64, username: &str) -> Result<bool> {
    let team = crate::community::team(api, mod_id)?;
    Ok(team
        .iter()
        .any(|member| member.username.eq_ignore_ascii_case(username)))
}

/// How long a team answer is trusted. Teams rarely change, and asking costs a request per mod.
const TEAM_MEMORY_SECONDS: u64 = 7 * 24 * 60 * 60;

/// What mod.io has already said about this account, so it isn't asked again on every launch.
#[derive(Default, Serialize, Deserialize)]
pub struct AccessMemory {
    #[serde(default)]
    pub username: String,
    /// mod.io refused `/me`, which won't change for this token.
    #[serde(default)]
    pub no_account: bool,
    /// Whether the user is on each mod's team, and when that was read.
    #[serde(default)]
    pub teams: HashMap<u64, (bool, u64)>,
}

impl AccessMemory {
    fn for_user(&mut self, username: &str) {
        if !self.username.eq_ignore_ascii_case(username) {
            self.username = username.to_string();
            self.teams.clear();
        }
    }

    fn on_team(&self, mod_id: u64, now: u64) -> Option<bool> {
        self.teams
            .get(&mod_id)
            .filter(|(_, read)| now.saturating_sub(*read) < TEAM_MEMORY_SECONDS)
            .map(|(mine, _)| *mine)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Everything the app shows in its mod list, and which of those mods the user can manage.
#[derive(Serialize)]
pub struct Library {
    pub mods: Vec<ModInfo>,
    /// False when mod.io couldn't say which mods are the user's, so nothing is hidden.
    pub access_known: bool,
    pub owned: Vec<u64>,
    pub user: Option<String>,
    pub note: Option<String>,
}

/// The user's mods come from `/me/mods`, which also finds hidden mods and ones missing from
/// this modkit. Tokens that can't read it fall back to matching a username the user gave.
/// Try a token before keeping it, so a bad paste fails here and not at the first publish.
pub fn check_token(api: &Api) -> Result<()> {
    let path = format!("/games/{GAME_ID}/mods");
    let reply = api.send("check your token", &[11005], || {
        api.client().get(api.url(&path)).query(&[("_limit", "1")])
    })?;
    if reply["error_ref"] == 11005 {
        bail!("mod.io didn't accept that token. Copy it again, or make a new one.");
    }
    Ok(())
}

pub fn load_library(
    api: &Api,
    local: &[u64],
    username: Option<&str>,
    memory: &mut AccessMemory,
) -> Result<Library> {
    let mut note = None;
    let account = if memory.no_account {
        Ok(None)
    } else {
        account_name(api)
    };
    let mine = match account {
        Ok(Some(user)) => {
            let query = [("game_id", GAME_ID.to_string())];
            let query: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
            match api.pages("read your mods", "/me/mods", &query) {
                Ok(items) => Some((user, items)),
                Err(error) => {
                    note = Some(format!("{error:#}"));
                    None
                }
            }
        }
        Ok(None) => {
            memory.no_account = true;
            None
        }
        Err(error) => {
            note = Some(format!("{error:#}"));
            None
        }
    };
    if let Some((user, items)) = mine {
        let mut mods: Vec<ModInfo> = items.iter().filter_map(to_mod_info).collect();
        let owned: Vec<u64> = mods.iter().map(|m| m.id).collect();
        let known: HashSet<u64> = owned.iter().copied().collect();
        let others: Vec<u64> = local
            .iter()
            .copied()
            .filter(|id| !known.contains(id))
            .collect();
        mods.extend(read_mods(api, &others)?);
        return Ok(Library {
            mods,
            access_known: true,
            owned,
            user: Some(user),
            note,
        });
    }
    let mods = read_mods(api, local)?;
    let Some(username) = username.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(Library {
            mods,
            access_known: false,
            owned: Vec::new(),
            user: None,
            note,
        });
    };
    memory.for_user(username);
    let now = unix_now();
    let mut owned = Vec::new();
    let mut stopped = false;
    for item in &mods {
        let mine = if item.author.eq_ignore_ascii_case(username) {
            true
        } else if let Some(mine) = memory.on_team(item.id, now) {
            mine
        } else if stopped {
            // Nothing is hidden when the answer isn't known, and mod.io still checks every change.
            true
        } else {
            match on_team(api, item.id, username) {
                Ok(mine) => {
                    memory.teams.insert(item.id, (mine, now));
                    mine
                }
                Err(error) => {
                    stopped = true;
                    note = Some(format!("{error:#}"));
                    true
                }
            }
        };
        if mine {
            owned.push(item.id);
        }
    }
    Ok(Library {
        mods,
        access_known: true,
        owned,
        user: Some(username.to_string()),
        note,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::{check_folders, to_mod_info, UNREAL};

    #[test]
    fn folders_are_checked_when_chosen() {
        let dir = std::env::temp_dir().join(format!("pms-folders-{}", std::process::id()));
        let project = dir.join("modkit");
        let engine = dir.join("UE_5.1").join("Engine");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(engine.join("Binaries/Win64")).unwrap();
        fs::write(project.join("Pavlov.uproject"), "{}").unwrap();
        fs::write(engine.join(UNREAL), "").unwrap();
        assert!(check_folders(&project, &engine).problems.is_empty());
        // The folder above Engine is the one people pick.
        let above = check_folders(&project, &dir.join("UE_5.1"));
        assert!(above.problems.is_empty());
        assert_eq!(above.engine, engine.to_string_lossy());
        assert_eq!(check_folders(&dir, &dir).problems.len(), 2);
        // No modkit chosen yet isn't a mistake.
        assert!(check_folders(std::path::Path::new(""), &engine)
            .problems
            .is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_a_mod_as_mod_io_sends_it() {
        let info = to_mod_info(&json!({
            "id": 4423103, "name": "[MOD] Boombox", "name_id": "boombox", "visible": 1,
            "summary": "Music", "description": "<p>Hi</p>", "homepage_url": null,
            "submitted_by": { "username": "MasterHyp" },
            "logo": { "thumb_320x180": "small.png", "thumb_640x360": "big.png" },
            "tags": [{ "name": "Mod" }],
            "stats": { "downloads_total": 678458, "subscribers_total": 946, "ratings_display_text": "Positive" },
            "platforms": [{ "platform": "windows", "modfile_live": 6226730 }, { "platform": "linux" }],
            "media": { "youtube": ["y"], "sketchfab": [], "images": [{ "filename": "a.jpg", "thumb_320x180": "t.jpg" }] },
            "credit_options": 578, "community_options": 131073,
        }))
        .unwrap();
        assert_eq!(info.kind, "Mod");
        assert_eq!(info.logo.as_deref(), Some("big.png"));
        assert_eq!(info.homepage_url, "");
        assert_eq!(info.stats.subscribers, 946);
        assert_eq!(info.platforms.len(), 1);
        assert_eq!(info.images[0].filename, "a.jpg");
        assert!(info.visible);
    }

    #[test]
    fn a_hidden_mod_with_no_extras_still_reads() {
        let info = to_mod_info(&json!({ "id": 1, "visible": 0 })).unwrap();
        assert!(!info.visible);
        assert_eq!(info.kind, "Unknown");
        assert!(info.logo.is_none());
        assert!(info.images.is_empty());
    }

    #[test]
    fn a_mod_needs_an_id() {
        assert!(to_mod_info(&json!({ "name": "x" })).is_none());
    }
}
