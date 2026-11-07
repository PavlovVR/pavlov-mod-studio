//! Choosing exactly what a mod shares with other modders.
//!
//! Everything here works from a list the modder builds by hand. Nothing is on it to begin with,
//! nothing joins it by itself, and the zip is read back before it's sent, to prove it holds
//! what's on the list and nothing else. The plugin's descriptor always goes too, because a
//! download can't be installed without it.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::assets::{ends_text, plugin_name, sources, store_dir, Lookup, Source};
use crate::package::zip_exact;

/// Unreal makes these, so they're never offered.
const GENERATED: [&str; 3] = ["Saved", "Intermediate", "Binaries"];
/// Files Unreal splits off an asset. They go wherever the asset goes.
const SIDECARS: [&str; 3] = ["uexp", "ubulk", "uptnl"];
const ASSETS: [&str; 2] = ["uasset", "umap"];
/// Every real shared plugin's name starts with this.
const SOURCE_MARK: &str = "[SOURCE]";
const MISSING_SHOWN: usize = 200;
const LONGEST_PATH: usize = 1024;

/// How a file looked when it was last shared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Stamp {
    size: u64,
    modified: u64,
}

/// What the modder chose, saved outside the plugin so it never ships.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ShareChoice {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    last: BTreeMap<String, Stamp>,
}

/// A file in the plugin, with the files that travel with it.
struct Item {
    path: String,
    size: u64,
    modified: u64,
    files: Vec<(String, PathBuf)>,
}

#[derive(Debug, Serialize)]
pub struct ShareItem {
    /// Path inside the plugin folder.
    pub path: String,
    pub size: u64,
    /// `changed` or `added` since the last share, if there was one.
    pub change: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ShareReport {
    pub items: Vec<ShareItem>,
    /// Always shared, so it can't be unticked.
    pub descriptor: String,
    pub files: Vec<String>,
    /// On the list, but no longer on disk.
    pub gone: Vec<String>,
    /// In the last share, but not on the list now.
    pub removed: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ShareLink {
    pub from: String,
    pub needs: String,
}

/// Another plugin that shared assets point at.
#[derive(Debug, PartialEq, Serialize)]
pub struct PluginNeed {
    pub plugin: String,
    pub from: String,
    pub assets: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct ShareCheck {
    /// Shared assets that point at something that isn't shared.
    pub missing: Vec<ShareLink>,
    /// Everything to add so nothing points at an unshared file, including what those need.
    pub add: Vec<String>,
    pub plugins: Vec<PluginNeed>,
}

#[derive(Debug, Serialize)]
pub struct ShareZip {
    pub files: usize,
    pub bytes: u64,
}

fn plugin_dir(project: &Path, mod_id: u64) -> PathBuf {
    project.join("Plugins").join(plugin_name(mod_id))
}

fn descriptor(mod_id: u64) -> String {
    format!("{}.uplugin", plugin_name(mod_id))
}

fn choice_path(project: &Path, mod_id: u64) -> PathBuf {
    store_dir(project).join(format!("{}.share.json", plugin_name(mod_id)))
}

/// Only a plain path inside the plugin, and never one Unreal generated.
fn safe_item(path: &str) -> bool {
    let mut parts = Path::new(path).components();
    let first = parts.next();
    let plain = |part: &Component| matches!(part, Component::Normal(_));
    first.as_ref().is_some_and(plain)
        && parts.all(|part| plain(&part))
        && !GENERATED
            .iter()
            .any(|name| first.is_some_and(|part| part.as_os_str().eq_ignore_ascii_case(name)))
}

fn read_choice(project: &Path, mod_id: u64) -> ShareChoice {
    let text = fs::read_to_string(choice_path(project, mod_id)).unwrap_or_default();
    let mut choice: ShareChoice = serde_json::from_str(&text).unwrap_or_default();
    // The file can be edited by hand, so it gets no more trust than a request would.
    choice.files.retain(|path| safe_item(path));
    choice
}

fn write_choice(project: &Path, mod_id: u64, choice: &ShareChoice) -> Result<()> {
    fs::create_dir_all(store_dir(project))?;
    fs::write(
        choice_path(project, mod_id),
        serde_json::to_string_pretty(choice)?,
    )?;
    Ok(())
}

fn modified_seconds(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs())
}

fn extension_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn collect(dir: &Path, folder: &Path, found: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in fs::read_dir(folder).with_context(|| format!("reading {}", folder.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Hidden files and folders, such as .git, are nobody else's business.
        if name.starts_with('.') || (folder == dir && GENERATED.contains(&name.as_str())) {
            continue;
        }
        let path = entry.path();
        // A link could lead anywhere, so it's left out rather than followed.
        if entry.file_type()?.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect(dir, &path, found)?;
        } else {
            let relative = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
            found.push((relative, path));
        }
    }
    Ok(())
}

/// Everything in the plugin that could be shared, with sidecars folded into their assets.
fn list_items(project: &Path, mod_id: u64) -> Result<Vec<Item>> {
    let dir = plugin_dir(project, mod_id);
    let mut found = Vec::new();
    collect(&dir, &dir, &mut found)?;
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let mains: HashMap<String, String> = found
        .iter()
        .filter(|(path, _)| ASSETS.contains(&extension_of(path).as_str()))
        .map(|(path, _)| (stem_of(path), path.clone()))
        .collect();
    let mut items: BTreeMap<String, Item> = BTreeMap::new();
    for (path, file) in found {
        let sidecar = SIDECARS.contains(&extension_of(&path).as_str());
        let owner = sidecar
            .then(|| mains.get(&stem_of(&path)).cloned())
            .flatten()
            .unwrap_or_else(|| path.clone());
        let size = file.metadata().map_or(0, |m| m.len());
        let modified = modified_seconds(&file);
        let item = items.entry(owner.clone()).or_insert_with(|| Item {
            path: owner,
            size: 0,
            modified: 0,
            files: Vec::new(),
        });
        item.size += size;
        item.modified = item.modified.max(modified);
        item.files.push((path, file));
    }
    Ok(items.into_values().collect())
}

fn stem_of(path: &str) -> String {
    let name = Path::new(path).with_extension("");
    name.to_string_lossy().to_ascii_lowercase()
}

/// Everything there is to choose from, and what's chosen.
pub fn share_report(project: &Path, mod_id: u64) -> Result<ShareReport> {
    let items = list_items(project, mod_id)?;
    let choice = read_choice(project, mod_id);
    let required = descriptor(mod_id);
    let existing: HashSet<&str> = items.iter().map(|item| item.path.as_str()).collect();
    let chosen: HashSet<&str> = choice.files.iter().map(String::as_str).collect();
    let change = |item: &Item| {
        if choice.last.is_empty() || !chosen.contains(item.path.as_str()) {
            return "";
        }
        match choice.last.get(&item.path) {
            None => "added",
            Some(stamp) if stamp.size != item.size || stamp.modified != item.modified => "changed",
            Some(_) => "",
        }
    };
    let list = items
        .iter()
        .map(|item| ShareItem {
            path: item.path.clone(),
            size: item.size,
            change: change(item),
        })
        .collect();
    let (files, gone) = choice
        .files
        .iter()
        .cloned()
        .partition(|path| existing.contains(path.as_str()));
    Ok(ShareReport {
        items: list,
        descriptor: required,
        files,
        gone,
        removed: choice
            .last
            .keys()
            .filter(|path| !chosen.contains(path.as_str()))
            .cloned()
            .collect(),
    })
}

pub fn save_share(project: &Path, mod_id: u64, files: &[String]) -> Result<()> {
    if let Some(bad) = files.iter().find(|path| !safe_item(path)) {
        bail!("{bad} can't be shared");
    }
    let required = descriptor(mod_id);
    let mut choice = read_choice(project, mod_id);
    choice.files = files
        .iter()
        .filter(|path| **path != required)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    write_choice(project, mod_id, &choice)
}

/// The modder has chosen something, or has shared before. Their plugin is the real project then,
/// and a download would replace it with the part they shared.
pub fn has_choice(project: &Path, mod_id: u64) -> bool {
    let choice = read_choice(project, mod_id);
    !choice.files.is_empty() || !choice.last.is_empty()
}

fn chosen_items(project: &Path, mod_id: u64) -> Result<(Vec<Item>, ShareChoice)> {
    let choice = read_choice(project, mod_id);
    let chosen: HashSet<&str> = choice.files.iter().map(String::as_str).collect();
    let items: Vec<Item> = list_items(project, mod_id)?
        .into_iter()
        .filter(|item| chosen.contains(item.path.as_str()))
        .collect();
    if items.is_empty() {
        bail!("The share list is empty. Tick what to share in the Source tab first.");
    }
    Ok((items, choice))
}

/// Stop before any long work if a share would have nothing to send.
pub fn require_choice(project: &Path, mod_id: u64) -> Result<()> {
    chosen_items(project, mod_id).map(|_| ())
}

/// The name the shared copy goes by, in the same form as every real shared plugin. Whoever
/// downloads it may not use this app, so the zip has to look right by itself. The modder's own
/// plugin is never changed.
fn shared_name(current: &str, mod_name: &str) -> String {
    // Some mods already say so in their name, at either end.
    let plain = |name: &str| name.replace(SOURCE_MARK, "").trim().to_string();
    let name = Some(plain(mod_name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| plain(current));
    format!("{SOURCE_MARK} {name}")
}

fn shared_descriptor(text: &str, mod_name: &str) -> Result<String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut plugin: Value =
        serde_json::from_str(text).context("this mod's .uplugin isn't valid")?;
    let current = plugin["FriendlyName"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let name = Value::from(shared_name(&current, mod_name));
    // Only the name changes, so the rest stays exactly as Unreal wrote it.
    let old = Value::from(current).to_string();
    if let Some(key) = text.find("\"FriendlyName\"") {
        if let Some(found) = text[key..].find(&old) {
            let at = key + found;
            let changed = format!("{}{name}{}", &text[..at], &text[at + old.len()..]);
            let parsed: Value = serde_json::from_str(&changed).unwrap_or_default();
            if parsed["FriendlyName"] == name {
                return Ok(changed);
            }
        }
    }
    plugin["FriendlyName"] = name;
    Ok(serde_json::to_string_pretty(&plugin)?)
}

/// Zip what's on the list, and only that.
pub fn share_zip(project: &Path, mod_id: u64, mod_name: &str, out: &Path) -> Result<ShareZip> {
    let (items, _) = chosen_items(project, mod_id)?;
    let required = descriptor(mod_id);
    let text = fs::read_to_string(plugin_dir(project, mod_id).join(&required))
        .context("cannot read this mod's .uplugin")?;
    let text = shared_descriptor(&text, mod_name)?;
    let files: Vec<(String, PathBuf)> = items.into_iter().flat_map(|item| item.files).collect();
    let bytes = zip_exact(out, (&required, &text), &files)?;
    Ok(ShareZip {
        files: files.len() + 1,
        bytes,
    })
}

/// Remember what went out, so the app can say what's changed since.
pub fn mark_shared(project: &Path, mod_id: u64) -> Result<()> {
    let (items, mut choice) = chosen_items(project, mod_id)?;
    choice.last = items
        .into_iter()
        .map(|item| {
            let stamp = Stamp {
                size: item.size,
                modified: item.modified,
            };
            (item.path, stamp)
        })
        .collect();
    write_choice(project, mod_id, &choice)
}

/// What one asset names, kept until the file changes.
#[derive(Clone)]
struct Links {
    size: u64,
    modified: u64,
    assets: Vec<String>,
    plugins: Vec<String>,
}

/// Ticking a box checks again, and only the files that changed should cost anything.
#[derive(Default)]
struct Memo {
    project: PathBuf,
    mod_id: u64,
    assets: usize,
    links: HashMap<String, Links>,
}

static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();

/// The plugin a path starts with, such as `UGC123` in `/UGC123/Api/BP_Api`.
fn plugin_of(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    let (plugin, _) = rest.split_once('/')?;
    let digits = plugin.strip_prefix("UGC")?;
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(plugin)
}

/// What a file names. Unreal keeps a package path as one whole string, so finding each `/UGC`
/// finds them all, without reading every string in a big sound or texture.
fn read_paths(data: &[u8], lookup: &Lookup, own: &str) -> (HashSet<String>, BTreeSet<String>) {
    let mut assets = HashSet::new();
    let mut plugins = BTreeSet::new();
    let mut read = |text: &str| match plugin_of(text) {
        Some(plugin) if plugin == own => lookup.read(text, &mut assets),
        Some(plugin) => {
            plugins.insert(plugin.to_string());
        }
        None => {}
    };
    for (at, window) in data.windows(4).enumerate() {
        if window == b"/UGC" {
            let text: String = data[at..]
                .iter()
                .take(LONGEST_PATH)
                .take_while(|byte| !ends_text(u32::from(**byte)))
                .map(|byte| char::from(*byte))
                .collect();
            read(&text);
        }
    }
    // Names with non-English letters only exist as UTF-16.
    let wide: Vec<u8> = "/UGC".encode_utf16().flat_map(u16::to_le_bytes).collect();
    for (at, window) in data.windows(wide.len()).enumerate() {
        if window == wide {
            let (pairs, _) = data[at..].as_chunks::<2>();
            let units: Vec<u16> = pairs
                .iter()
                .take(LONGEST_PATH)
                .map(|pair| u16::from_le_bytes(*pair))
                .take_while(|unit| !ends_text(u32::from(*unit)))
                .collect();
            read(&String::from_utf16_lossy(&units));
        }
    }
    (assets, plugins)
}

fn links_of(memo: &mut Memo, asset: &Source, lookup: &Lookup, own: &str) -> Result<Links> {
    let modified = modified_seconds(&asset.file);
    if let Some(known) = memo.links.get(&asset.path) {
        if known.size == asset.size && known.modified == modified {
            return Ok(known.clone());
        }
    }
    let data =
        fs::read(&asset.file).with_context(|| format!("reading {}", asset.file.display()))?;
    let (found, plugins) = read_paths(&data, lookup, own);
    let mut assets: Vec<String> = found.into_iter().collect();
    assets.retain(|path| *path != asset.path);
    assets.sort();
    let links = Links {
        size: asset.size,
        modified,
        assets,
        plugins: plugins.into_iter().collect(),
    };
    memo.links.insert(asset.path.clone(), links.clone());
    Ok(links)
}

/// What the chosen files point at that wouldn't be there for whoever downloads them. Only the
/// chosen files and what they lead to are read, so this stays quick on a mod with thousands of
/// assets. Only full paths count, because a bare name could suggest sharing something unrelated.
pub fn check_share(project: &Path, mod_id: u64, files: &[String]) -> Result<ShareCheck> {
    let assets = sources(project, mod_id)?;
    let own = plugin_name(mod_id);
    let dir = plugin_dir(project, mod_id);
    let paths: Vec<&str> = assets.iter().map(|asset| asset.path.as_str()).collect();
    let mut lookup = Lookup::new(format!("/{own}/"), &paths);
    lookup.bare.clear();
    let item_of = |asset: &Source| {
        asset
            .file
            .strip_prefix(&dir)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default()
    };
    let by_path: HashMap<&str, &Source> = assets.iter().map(|a| (a.path.as_str(), a)).collect();
    let shared: HashSet<&str> = files.iter().map(String::as_str).collect();

    let mut memo = MEMO
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // A new or renamed asset changes what a file can be matched against.
    if memo.project != project || memo.mod_id != mod_id || memo.assets != assets.len() {
        *memo = Memo {
            project: project.to_path_buf(),
            mod_id,
            assets: assets.len(),
            links: HashMap::new(),
        };
    }

    let mut check = ShareCheck::default();
    let mut needed = BTreeSet::new();
    let mut plugins: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut stack: Vec<&Source> = assets
        .iter()
        .filter(|asset| shared.contains(item_of(asset).as_str()))
        .collect();
    let mut seen: HashSet<&str> = stack.iter().map(|asset| asset.path.as_str()).collect();
    while let Some(asset) = stack.pop() {
        let from = item_of(asset);
        let is_shared = shared.contains(from.as_str());
        let links = links_of(&mut memo, asset, &lookup, &own)?;
        if is_shared {
            for plugin in links.plugins {
                let entry = plugins.entry(plugin).or_insert((from.clone(), 0));
                entry.1 += 1;
            }
        }
        for target in &links.assets {
            let Some(next) = by_path.get(target.as_str()) else {
                continue;
            };
            let needs = item_of(next);
            if !shared.contains(needs.as_str()) {
                if is_shared {
                    check.missing.push(ShareLink {
                        from: from.clone(),
                        needs: needs.clone(),
                    });
                }
                needed.insert(needs);
            }
            if seen.insert(next.path.as_str()) {
                stack.push(next);
            }
        }
    }
    check
        .missing
        .sort_by(|a, b| (&a.from, &a.needs).cmp(&(&b.from, &b.needs)));
    check.missing.truncate(MISSING_SHOWN);
    check.add = needed.into_iter().collect();
    check.plugins = plugins
        .into_iter()
        .map(|(plugin, (from, assets))| PluginNeed {
            plugin,
            from,
            assets,
        })
        .collect();
    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use zip::ZipArchive;

    const ID: u64 = 7;

    fn project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pms-share-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        let plugin = dir.join("Plugins/UGC7");
        fs::create_dir_all(plugin.join("Content/Api")).unwrap();
        fs::create_dir_all(plugin.join("Saved/Config")).unwrap();
        fs::create_dir_all(plugin.join("Resources")).unwrap();
        fs::create_dir_all(plugin.join(".git")).unwrap();
        let descriptor = r#"{"FileVersion": 3, "FriendlyName": "UGC7", "CanContainContent": true}"#;
        fs::write(plugin.join("UGC7.uplugin"), descriptor).unwrap();
        fs::write(plugin.join("Content/definition.uasset"), "x").unwrap();
        fs::write(
            plugin.join("Content/Api/BP_Api.uasset"),
            "name\0/UGC7/Api/S_Data\0/UGC9/Lib/F_Tools\0end",
        )
        .unwrap();
        fs::write(
            plugin.join("Content/Api/S_Data.uasset"),
            "name\0/UGC7/Secret/M_Skin\0end",
        )
        .unwrap();
        fs::write(plugin.join("Content/Api/S_Data.uexp"), "split off").unwrap();
        fs::create_dir_all(plugin.join("Content/Secret")).unwrap();
        fs::write(plugin.join("Content/Secret/M_Skin.uasset"), "plain").unwrap();
        fs::write(plugin.join("Saved/Config/private.ini"), "x").unwrap();
        fs::write(plugin.join(".git/config"), "x").unwrap();
        fs::write(plugin.join("Resources/Icon128.png"), "png").unwrap();
        fs::write(plugin.join("NOTES.md"), "private notes").unwrap();
        dir
    }

    fn names(zip: &Path) -> Vec<String> {
        let archive = ZipArchive::new(File::open(zip).unwrap()).unwrap();
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        names
    }

    fn descriptor_in(zip: &Path) -> Value {
        let mut archive = ZipArchive::new(File::open(zip).unwrap()).unwrap();
        let file = archive.by_name("UGC7.uplugin").unwrap();
        serde_json::from_reader(file).unwrap()
    }

    fn choose(dir: &Path, files: &[&str]) {
        let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        save_share(dir, ID, &files).unwrap();
    }

    #[test]
    fn nothing_is_chosen_to_begin_with() {
        let dir = project("empty");
        let report = share_report(&dir, ID).unwrap();
        assert!(report.files.is_empty());
        assert!(require_choice(&dir, ID).is_err());
        let zip = dir.join("out.zip");
        assert!(share_zip(&dir, ID, "Tools", &zip).is_err());
        assert!(!zip.exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn generated_and_hidden_files_are_never_offered() {
        let dir = project("offered");
        let report = share_report(&dir, ID).unwrap();
        let paths: Vec<&str> = report.items.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"NOTES.md"));
        assert!(paths.contains(&"Resources/Icon128.png"));
        assert!(!paths.iter().any(|p| p.starts_with("Saved")));
        assert!(!paths.iter().any(|p| p.starts_with(".git")));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_sidecar_is_part_of_its_asset() {
        let dir = project("sidecar");
        let report = share_report(&dir, ID).unwrap();
        let asset = report
            .items
            .iter()
            .find(|item| item.path == "Content/Api/S_Data.uasset")
            .unwrap();
        assert_eq!(
            asset.size,
            "name\0/UGC7/Secret/M_Skin\0end".len() as u64 + 9
        );
        assert!(!report.items.iter().any(|item| item.path.ends_with(".uexp")));
        choose(&dir, &["Content/Api/S_Data.uasset"]);
        let zip = dir.join("out.zip");
        share_zip(&dir, ID, "Tools", &zip).unwrap();
        assert!(names(&zip).contains(&"Content/Api/S_Data.uexp".to_string()));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_zip_holds_only_what_was_chosen() {
        let dir = project("strict");
        choose(&dir, &["Content/Api/BP_Api.uasset"]);
        let zip = dir.join("out.zip");
        let made = share_zip(&dir, ID, "Tools", &zip).unwrap();
        assert_eq!(
            names(&zip),
            vec!["Content/Api/BP_Api.uasset", "UGC7.uplugin"]
        );
        assert_eq!(made.files, 2);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_new_file_is_never_added_by_itself() {
        let dir = project("new");
        choose(&dir, &["Content/Api/BP_Api.uasset"]);
        fs::write(dir.join("Plugins/UGC7/Content/Api/New.uasset"), "x").unwrap();
        let zip = dir.join("out.zip");
        share_zip(&dir, ID, "Tools", &zip).unwrap();
        assert!(!names(&zip).contains(&"Content/Api/New.uasset".to_string()));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn paths_outside_the_plugin_are_refused() {
        let dir = project("escape");
        for bad in [
            "../other.txt",
            "Content/../../x",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "Saved/Config/private.ini",
            "saved/config/private.ini",
            "",
        ] {
            let files = vec![bad.to_string()];
            assert!(save_share(&dir, ID, &files).is_err(), "{bad}");
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_hand_edited_list_gets_no_more_trust() {
        let dir = project("edited");
        fs::create_dir_all(store_dir(&dir)).unwrap();
        let text = r#"{"files": ["../../secret.txt", "Saved/Config/private.ini", "Content/Api/BP_Api.uasset"]}"#;
        fs::write(choice_path(&dir, ID), text).unwrap();
        let zip = dir.join("out.zip");
        share_zip(&dir, ID, "Tools", &zip).unwrap();
        assert_eq!(
            names(&zip),
            vec!["Content/Api/BP_Api.uasset", "UGC7.uplugin"]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_same_choice_makes_the_same_zip() {
        let dir = project("same");
        choose(&dir, &["Content/Api/BP_Api.uasset"]);
        let (first, second) = (dir.join("a.zip"), dir.join("b.zip"));
        share_zip(&dir, ID, "Tools", &first).unwrap();
        share_zip(&dir, ID, "Tools", &second).unwrap();
        assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_shared_copy_is_marked_and_the_plugin_is_left_alone() {
        let dir = project("mark");
        let zip = dir.join("out.zip");
        choose(&dir, &["Content/Api/BP_Api.uasset"]);
        share_zip(&dir, ID, "Tools", &zip).unwrap();
        assert_eq!(descriptor_in(&zip)["FriendlyName"], "[SOURCE] Tools");
        assert_eq!(descriptor_in(&zip)["CanContainContent"], true);
        let local = fs::read_to_string(dir.join("Plugins/UGC7/UGC7.uplugin")).unwrap();
        assert!(local.contains(r#""FriendlyName": "UGC7""#));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn only_the_name_changes_in_the_shared_descriptor() {
        let text = "\u{feff}{\r\n\t\"FileVersion\": 3,\r\n\t\"FriendlyName\": \"UGC7\",\r\n\t\"Category\": \"Mod\"\r\n}";
        let shared = shared_descriptor(text, "Tools").unwrap();
        assert_eq!(
            shared,
            "{\r\n\t\"FileVersion\": 3,\r\n\t\"FriendlyName\": \"[SOURCE] Tools\",\r\n\t\"Category\": \"Mod\"\r\n}"
        );
    }

    #[test]
    fn a_descriptor_with_no_name_gets_one() {
        let shared = shared_descriptor("{\"FileVersion\": 3}", "Tools").unwrap();
        let plugin: Value = serde_json::from_str(&shared).unwrap();
        assert_eq!(plugin["FriendlyName"], "[SOURCE] Tools");
        assert_eq!(plugin["FileVersion"], 3);
    }

    #[test]
    fn a_name_like_the_key_can_not_break_the_file() {
        let text = r#"{"FriendlyName": "FriendlyName", "Category": "Mod"}"#;
        let shared = shared_descriptor(text, "").unwrap();
        let plugin: Value = serde_json::from_str(&shared).unwrap();
        assert_eq!(plugin["FriendlyName"], "[SOURCE] FriendlyName");
        assert_eq!(plugin["Category"], "Mod");
    }

    #[test]
    fn names_never_end_up_marked_twice() {
        assert_eq!(
            shared_name("UGC7", "[MOD] Boombox - Playlist [SOURCE]"),
            "[SOURCE] [MOD] Boombox - Playlist"
        );
        assert_eq!(shared_name("UGC7", "[SOURCE] Tools"), "[SOURCE] Tools");
        assert_eq!(shared_name("[SOURCE] Mine", ""), "[SOURCE] Mine");
    }

    #[test]
    fn finds_what_chosen_assets_need_and_what_those_need() {
        let dir = project("needs");
        let files = vec!["Content/Api/BP_Api.uasset".to_string()];
        let check = check_share(&dir, ID, &files).unwrap();
        assert_eq!(
            check.missing,
            vec![ShareLink {
                from: "Content/Api/BP_Api.uasset".to_string(),
                needs: "Content/Api/S_Data.uasset".to_string(),
            }]
        );
        assert_eq!(
            check.add,
            vec!["Content/Api/S_Data.uasset", "Content/Secret/M_Skin.uasset"]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn nothing_is_missing_once_the_needs_are_shared() {
        let dir = project("complete");
        let files = vec![
            "Content/Api/BP_Api.uasset".to_string(),
            "Content/Api/S_Data.uasset".to_string(),
            "Content/Secret/M_Skin.uasset".to_string(),
        ];
        let check = check_share(&dir, ID, &files).unwrap();
        assert!(check.missing.is_empty() && check.add.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn names_other_plugins_the_chosen_files_point_at() {
        let dir = project("plugins");
        let files = vec!["Content/Api/BP_Api.uasset".to_string()];
        let check = check_share(&dir, ID, &files).unwrap();
        assert_eq!(
            check.plugins,
            vec![PluginNeed {
                plugin: "UGC9".to_string(),
                from: "Content/Api/BP_Api.uasset".to_string(),
                assets: 1,
            }]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_plain_word_is_not_a_plugin() {
        let lookup = Lookup::new("/UGC7/".to_string(), &["Api/BP_Api"]);
        let data = b"/UGC/x\0/UGCabc/y\0/UGC12\0/UGC7/Api/BP_Api\0/UGC8/z\0";
        let (assets, plugins) = read_paths(data, &lookup, "UGC7");
        assert_eq!(plugins.into_iter().collect::<Vec<_>>(), vec!["UGC8"]);
        assert!(assets.contains("Api/BP_Api"));
    }

    #[test]
    fn finds_paths_kept_as_utf16() {
        let lookup = Lookup::new("/UGC7/".to_string(), &["Api/Größe"]);
        let mut data = vec![1, 2, 3];
        data.extend("/UGC7/Api/Größe".encode_utf16().flat_map(u16::to_le_bytes));
        data.extend([0, 0]);
        let (assets, _) = read_paths(&data, &lookup, "UGC7");
        assert!(assets.contains("Api/Größe"));
    }

    #[test]
    fn says_what_changed_since_the_last_share() {
        let dir = project("changes");
        choose(
            &dir,
            &["Content/Api/BP_Api.uasset", "Content/Api/S_Data.uasset"],
        );
        let report = share_report(&dir, ID).unwrap();
        assert!(report.items.iter().all(|item| item.change.is_empty()));

        mark_shared(&dir, ID).unwrap();
        fs::write(
            dir.join("Plugins/UGC7/Content/Api/BP_Api.uasset"),
            "a longer version of this asset than before",
        )
        .unwrap();
        choose(
            &dir,
            &["Content/Api/BP_Api.uasset", "Content/Secret/M_Skin.uasset"],
        );
        let report = share_report(&dir, ID).unwrap();
        let change = |path: &str| {
            report
                .items
                .iter()
                .find(|item| item.path == path)
                .unwrap()
                .change
        };
        assert_eq!(change("Content/Api/BP_Api.uasset"), "changed");
        assert_eq!(change("Content/Secret/M_Skin.uasset"), "added");
        assert_eq!(report.removed, vec!["Content/Api/S_Data.uasset"]);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_deleted_file_is_reported_and_not_shared() {
        let dir = project("gone");
        choose(
            &dir,
            &["Content/Api/BP_Api.uasset", "Content/Api/S_Data.uasset"],
        );
        fs::remove_file(dir.join("Plugins/UGC7/Content/Api/S_Data.uasset")).unwrap();
        fs::remove_file(dir.join("Plugins/UGC7/Content/Api/S_Data.uexp")).unwrap();
        let report = share_report(&dir, ID).unwrap();
        assert_eq!(report.gone, vec!["Content/Api/S_Data.uasset"]);
        assert_eq!(report.files, vec!["Content/Api/BP_Api.uasset"]);
        let zip = dir.join("out.zip");
        share_zip(&dir, ID, "Tools", &zip).unwrap();
        assert!(!names(&zip).contains(&"Content/Api/S_Data.uasset".to_string()));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_plugin_with_a_choice_counts_as_the_real_project() {
        let dir = project("owner");
        assert!(!has_choice(&dir, ID));
        choose(&dir, &["Content/Api/BP_Api.uasset"]);
        assert!(has_choice(&dir, ID));
        fs::remove_dir_all(dir).ok();
    }
}
