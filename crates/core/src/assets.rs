//! What's in a mod, what uses what, and which assets the modder chose to leave out.
//!
//! Unreal cooks a whole mod plugin whatever it's told, so leaving assets out is done here.
//! What counts as used comes from two sources, because neither is enough alone: Unreal's
//! dependency tree can't see a path kept in a string, and a scan of the files can't see
//! everything Unreal resolves.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env::temp_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::process::{run_tool, Failure, Tool};

const SCRIPT: &str = include_str!("scripts/dependencies.py");
const DEFINITION: &str = "definition";
const COOKED_EXTENSIONS: [&str; 6] = ["uasset", "umap", "uexp", "ubulk", "uptnl", "ufont"];
/// Skip short names when matching strings, or ordinary words would count as references.
const MIN_BARE_NAME: usize = 6;
const USED_BY_SHOWN: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The mod can't work without it.
    Required,
    /// Unreal's own tree reaches it from the definition.
    Used,
    /// Only a string points at it, which Unreal's tree can't see. Always kept by the filter.
    InString,
    /// Another mod in this modkit uses it.
    OtherMods,
    /// Nothing found uses it.
    Unused,
    /// Not checked yet.
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct Asset {
    /// Path inside the plugin's Content folder, without an extension.
    pub path: String,
    pub size: u64,
    pub status: Status,
    /// Assets that point at this one.
    pub used_by: Vec<String>,
    /// Indexes of the assets this one points at.
    pub uses: Vec<usize>,
    /// Assets in other mods that point at this one, as `UGC123:path`.
    pub outside: Vec<String>,
}

/// Another mod in this modkit that uses this one.
#[derive(Debug, Serialize)]
pub struct Consumer {
    pub mod_id: u64,
    /// How many of this mod's assets it points at directly.
    pub assets: usize,
}

#[derive(Debug, Serialize)]
pub struct AssetReport {
    pub assets: Vec<Asset>,
    /// Statuses are filled in and match the files on disk.
    pub analysed: bool,
    /// The definition leads somewhere. If not, the mod's assets are for other mods to use,
    /// and nothing here can tell which of them matter.
    pub rooted: bool,
    pub excluded: Vec<String>,
    pub confirmed: bool,
    pub pinned: Vec<String>,
    pub consumers: Vec<Consumer>,
}

/// What the modder left out, saved outside the plugin so it never ships.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub excluded: Vec<String>,
    /// The modder saw that kept assets point at left-out ones, and wants to go ahead.
    #[serde(default)]
    pub confirmed: bool,
    /// Assets to always keep, for when something uses them that nothing here can detect.
    #[serde(default)]
    pub pinned: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MissingReference {
    pub kept: String,
    pub missing: String,
}

/// How a selection reaches the cook.
#[derive(Debug, Default, PartialEq)]
pub struct CookPlan {
    /// Whole folders, as mount paths, for `-NeverCookDir`. This keeps the asset registry right.
    pub folders: Vec<String>,
    /// Single assets, deleted from the cooked output before packaging.
    pub files: Vec<String>,
}

/// What analysis found, kept until the mod's files change.
#[derive(Serialize, Deserialize)]
struct SavedAnalysis {
    tree: BTreeMap<String, Vec<String>>,
    scanned: HashMap<String, HashSet<String>>,
    /// What other mods' files name in this mod, keyed by `UGC123:path`.
    #[serde(default)]
    outside: HashMap<String, HashSet<String>>,
    /// The mod's assets when this ran. Dates can't show this: a copied file keeps its old
    /// date, and a deleted one leaves none. Analyses saved before this have it empty.
    #[serde(default)]
    files: BTreeSet<String>,
}

impl SavedAnalysis {
    fn covers(&self, assets: &[Source]) -> bool {
        self.files.is_empty()
            || (self.files.len() == assets.len()
                && assets.iter().all(|asset| self.files.contains(&asset.path)))
    }
}

struct Source {
    path: String,
    file: PathBuf,
    size: u64,
}

fn plugin_name(mod_id: u64) -> String {
    format!("UGC{mod_id}")
}

fn content_dir(project: &Path, mod_id: u64) -> PathBuf {
    project
        .join("Plugins")
        .join(plugin_name(mod_id))
        .join("Content")
}

fn store_dir(project: &Path) -> PathBuf {
    project.join("Saved/PavlovModStudio")
}

fn walk(folder: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(folder).with_context(|| format!("reading {}", folder.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

fn sources(project: &Path, mod_id: u64) -> Result<Vec<Source>> {
    let content = content_dir(project, mod_id);
    let mut files = Vec::new();
    walk(&content, &mut files)?;
    let mut assets: Vec<Source> = files
        .into_iter()
        .filter(|file| {
            file.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("uasset") || e.eq_ignore_ascii_case("umap"))
        })
        .filter_map(|file| {
            let relative = file.strip_prefix(&content).ok()?.with_extension("");
            let path = relative.to_string_lossy().replace('\\', "/");
            let mut size = file.metadata().ok()?.len();
            for extra in ["uexp", "ubulk"] {
                size += file.with_extension(extra).metadata().map_or(0, |m| m.len());
            }
            Some(Source { path, file, size })
        })
        .collect();
    assets.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(assets)
}

/// Unreal stores a name's trailing number apart from its text, so `prop_12` appears in a
/// file as `prop`. Taken from assets the scan missed for exactly this reason.
fn without_number(name: &str) -> &str {
    match name.rsplit_once('_') {
        Some((base, digits))
            if !base.is_empty()
                && !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => name,
    }
}

/// Everything needed to turn text found in a file into assets of this mod.
struct Lookup {
    mount: String,
    paths: HashSet<String>,
    families: HashMap<String, Vec<String>>,
    folders: HashMap<String, Vec<String>>,
    bare: HashMap<String, Vec<String>>,
}

impl Lookup {
    fn new(mount: String, paths: &[&str]) -> Self {
        let mut lookup = Self {
            mount,
            paths: paths.iter().map(|p| p.to_string()).collect(),
            families: HashMap::new(),
            folders: HashMap::new(),
            bare: HashMap::new(),
        };
        for path in paths {
            let owned = path.to_string();
            lookup
                .families
                .entry(without_number(path).to_string())
                .or_default()
                .push(owned.clone());
            let short = path.rsplit('/').next().unwrap_or(path);
            if short.chars().count() >= MIN_BARE_NAME {
                lookup
                    .bare
                    .entry(short.to_string())
                    .or_default()
                    .push(owned.clone());
            }
            let mut end = 0;
            while let Some(slash) = path[end..].find('/') {
                end += slash;
                lookup
                    .folders
                    .entry(path[..end].to_string())
                    .or_default()
                    .push(owned.clone());
                end += 1;
            }
        }
        lookup
    }

    /// Read one run of text, which may hold a path, an object path, or plain names.
    fn read(&self, text: &str, found: &mut HashSet<String>) {
        if let Some(start) = text.find(&self.mount) {
            let path = &text[start + self.mount.len()..];
            let path = path.split(['.', ':']).next().unwrap_or(path);
            let path = path.trim_end_matches(['/', ' ']);
            if self.paths.contains(path) {
                found.insert(path.to_string());
            } else if let Some(family) = self.families.get(path) {
                found.extend(family.iter().cloned());
            } else if let Some(contents) = self.folders.get(path) {
                // A folder path usually means the mod builds asset paths while running.
                found.extend(contents.iter().cloned());
            }
        }
        for word in text.split(['/', '.', ':', ' ']) {
            let word = word.strip_suffix("_C").unwrap_or(word);
            if let Some(owners) = self.bare.get(word) {
                found.extend(owners.iter().cloned());
            }
        }
    }
}

fn ends_text(unit: u32) -> bool {
    unit < 0x20
        || matches!(
            char::from_u32(unit),
            Some(
                '"' | '\''
                    | '<'
                    | '>'
                    | '|'
                    | '*'
                    | '?'
                    | ','
                    | ';'
                    | '='
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '\\'
            )
        )
}

/// Find every asset named in a file, in the single-byte and UTF-16 text Unreal writes.
fn scan(data: &[u8], lookup: &Lookup) -> HashSet<String> {
    let mut found = HashSet::new();
    for run in data.split(|byte| ends_text(u32::from(*byte))) {
        if run.len() >= 4 {
            match std::str::from_utf8(run) {
                Ok(text) => lookup.read(text, &mut found),
                // Unreal writes Latin-1 names one byte per character.
                Err(_) => {
                    let text: String = run.iter().map(|b| char::from(*b)).collect();
                    lookup.read(&text, &mut found);
                }
            }
        }
    }
    // UTF-16 text can start on either byte, and non-English names only exist in this form.
    for offset in 0..2 {
        let mut units = Vec::new();
        for at in (offset..data.len().saturating_sub(1)).step_by(2) {
            let unit = u16::from_le_bytes([data[at], data[at + 1]]);
            if ends_text(u32::from(unit)) {
                if units.len() >= 4 {
                    lookup.read(&String::from_utf16_lossy(&units), &mut found);
                }
                units.clear();
            } else {
                units.push(unit);
            }
        }
        if units.len() >= 4 {
            lookup.read(&String::from_utf16_lossy(&units), &mut found);
        }
    }
    found
}

fn contains(data: &[u8], needle: &[u8]) -> bool {
    data.windows(needle.len()).any(|window| window == needle)
}

/// One file to read, and the name its findings go under.
struct ScanItem<'a> {
    key: String,
    own_path: &'a str,
    file: &'a Path,
    size: u64,
}

/// Read many files at once. With `needles`, a file holding none of them is skipped.
fn scan_each(
    mut items: Vec<ScanItem>,
    lookup: &Lookup,
    needles: &[Vec<u8>],
) -> Result<Vec<(String, HashSet<String>)>> {
    let workers = thread::available_parallelism()
        .map_or(4, usize::from)
        .min(8);
    items.sort_by_key(|item| std::cmp::Reverse(item.size));
    let mut parts: Vec<Vec<ScanItem>> = (0..workers).map(|_| Vec::new()).collect();
    for (position, item) in items.into_iter().enumerate() {
        parts[position % workers].push(item);
    }
    let results = thread::scope(|scope| {
        let handles: Vec<_> = parts
            .into_iter()
            .map(|part| {
                scope.spawn(move || -> Result<Vec<(String, HashSet<String>)>> {
                    let mut edges = Vec::with_capacity(part.len());
                    for item in part {
                        let data = fs::read(item.file)
                            .with_context(|| format!("reading {}", item.file.display()))?;
                        if !needles.is_empty() && !needles.iter().any(|n| contains(&data, n)) {
                            continue;
                        }
                        let mut found = scan(&data, lookup);
                        found.remove(item.own_path);
                        edges.push((item.key, found));
                    }
                    Ok(edges)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a scan thread panicked"))
            .collect::<Result<Vec<_>>>()
    })?;
    Ok(results.into_iter().flatten().collect())
}

/// Which of the mod's assets name which others, read straight from the files.
fn scan_references(mod_id: u64, assets: &[Source]) -> Result<HashMap<String, HashSet<String>>> {
    let paths: Vec<&str> = assets.iter().map(|a| a.path.as_str()).collect();
    let lookup = Lookup::new(format!("/{}/", plugin_name(mod_id)), &paths);
    let items = assets
        .iter()
        .map(|asset| ScanItem {
            key: asset.path.clone(),
            own_path: &asset.path,
            file: &asset.file,
            size: asset.size,
        })
        .collect();
    Ok(scan_each(items, &lookup, &[])?.into_iter().collect())
}

/// Every other mod in this modkit, by id.
fn other_mods(project: &Path, mod_id: u64) -> Result<Vec<u64>> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(project.join("Plugins")).context("reading the Plugins folder")? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_prefix("UGC").and_then(|id| id.parse().ok()) {
            if id != mod_id && content_dir(project, id).is_dir() {
                ids.push(id);
            }
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

/// What other mods' files name in this mod. Unreal's tree covers real references, and this
/// also finds paths kept in strings. Only full paths count here, because a bare name in
/// another mod's file says nothing about this mod.
fn outside_references(
    project: &Path,
    mod_id: u64,
    assets: &[Source],
) -> Result<HashMap<String, HashSet<String>>> {
    let paths: Vec<&str> = assets.iter().map(|a| a.path.as_str()).collect();
    let mount = format!("/{}/", plugin_name(mod_id));
    let mut lookup = Lookup::new(mount.clone(), &paths);
    lookup.bare.clear();
    let wide: Vec<u8> = mount.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let others: Vec<(u64, Vec<Source>)> = other_mods(project, mod_id)?
        .into_iter()
        .map(|id| Ok((id, sources(project, id)?)))
        .collect::<Result<_>>()?;
    let items = others
        .iter()
        .flat_map(|(id, files)| {
            files.iter().map(move |asset| ScanItem {
                key: format!("{}:{}", plugin_name(*id), asset.path),
                own_path: "",
                file: &asset.file,
                size: asset.size,
            })
        })
        .collect();
    let found = scan_each(items, &lookup, &[mount.into_bytes(), wide])?;
    Ok(found
        .into_iter()
        .filter(|(_, targets)| !targets.is_empty())
        .collect())
}

/// What other mods leave out of their own builds, as `UGC123:path`. It doesn't ship, so it
/// doesn't need anything from this mod.
fn left_out_elsewhere(project: &Path, mod_id: u64) -> Result<HashSet<String>> {
    let mut labels = HashSet::new();
    for id in other_mods(project, mod_id)? {
        let name = plugin_name(id);
        for path in load_selection(project, id)?.excluded {
            labels.insert(format!("{name}:{path}"));
        }
    }
    Ok(labels)
}

fn reach<'a>(graph: &'a HashMap<String, HashSet<String>>, roots: &[&'a str]) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut stack: Vec<&str> = roots.to_vec();
    while let Some(item) = stack.pop() {
        if seen.insert(item) {
            if let Some(next) = graph.get(item) {
                stack.extend(next.iter().map(String::as_str));
            }
        }
    }
    seen
}

fn tree_path(project: &Path, mod_id: u64) -> PathBuf {
    store_dir(project).join(format!("{}.tree.json", plugin_name(mod_id)))
}

fn newest_change(assets: &[Source]) -> SystemTime {
    assets
        .iter()
        .filter_map(|asset| asset.file.metadata().ok()?.modified().ok())
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// When any mod last changed, because a change to one can change what another needs.
fn newest_anywhere(project: &Path, mod_id: u64, assets: &[Source]) -> Result<SystemTime> {
    let mut newest = newest_change(assets);
    for id in other_mods(project, mod_id)? {
        newest = newest.max(newest_change(&sources(project, id)?));
    }
    Ok(newest)
}

/// Ask Unreal for its dependency tree. Reads only, and takes a fresh editor session.
fn unreal_tree(project: &Path, engine: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let dir = temp_dir().join(format!("pavlov-mod-studio-tree-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let script = dir.join("script.py");
    let result_file = dir.join("result.json");
    fs::remove_file(&result_file).ok();
    fs::write(&script, SCRIPT)?;
    let log = project.join("Saved/Logs/pavlov_assets.log");
    let tool = Tool {
        name: "UnrealEditor-Cmd.exe",
        log: Some(&log),
        cancellable: false,
        keep_stdout: false,
    };
    let exe = engine.join("Binaries/Win64/UnrealEditor-Cmd.exe");
    if !exe.is_file() {
        bail!("could not find UnrealEditor-Cmd.exe; check the engine path");
    }
    let mut command = Command::new(exe);
    command
        .arg(project.join("Pavlov.uproject"))
        .arg("-run=pythonscript")
        .arg(format!("-script={}", script.display()))
        .args(["-unattended", "-nullrhi", "-nosplash", "-nopause"])
        .env("PMS_OUT", &result_file);
    let run = run_tool(&tool, &mut command)?;
    let Ok(text) = fs::read_to_string(&result_file) else {
        let fallback = "Unreal did not report its dependency tree.";
        return Err(run.failure("definition", None, &tool, fallback).into());
    };
    fs::remove_dir_all(&dir).ok();
    let result: Value = serde_json::from_str(&text)?;
    if let Some(error) = result["error"].as_str() {
        bail!("Unreal could not read the dependency tree: {error}");
    }
    Ok(serde_json::from_value(result["packages"].clone())?)
}

fn relative<'a>(package: &'a str, mount: &str) -> Option<&'a str> {
    package.strip_prefix(mount)
}

/// Turn `/UGC123/path` into `UGC123:path`.
fn outside_label(package: &str) -> Option<String> {
    let (plugin, path) = package.strip_prefix('/')?.split_once('/')?;
    plugin
        .starts_with("UGC")
        .then(|| format!("{plugin}:{path}"))
}

fn report_from(
    assets: &[Source],
    mod_id: u64,
    found: Option<&SavedAnalysis>,
    selection: Selection,
    left_out: &HashSet<String>,
) -> AssetReport {
    let unknown = |asset: &Source| Asset {
        path: asset.path.clone(),
        size: asset.size,
        status: Status::Unknown,
        used_by: Vec::new(),
        uses: Vec::new(),
        outside: Vec::new(),
    };
    let Some(SavedAnalysis {
        tree,
        scanned,
        outside: scanned_outside,
        ..
    }) = found
    else {
        return AssetReport {
            assets: assets.iter().map(unknown).collect(),
            analysed: false,
            rooted: false,
            excluded: selection.excluded,
            confirmed: selection.confirmed,
            pinned: selection.pinned,
            consumers: Vec::new(),
        };
    };
    let mount = format!("/{}/", plugin_name(mod_id));
    let own = |package: &String| relative(package, &mount).map(str::to_string);
    let mut theirs: HashMap<String, HashSet<String>> = HashMap::new();
    let mut outside: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (package, targets) in tree {
        match own(package) {
            Some(from) => {
                theirs.insert(from, targets.iter().filter_map(own).collect());
            }
            None => {
                if let Some(label) = outside_label(package).filter(|l| !left_out.contains(l)) {
                    for target in targets.iter().filter_map(own) {
                        outside.entry(target).or_default().insert(label.clone());
                    }
                }
            }
        }
    }
    for (label, targets) in scanned_outside {
        if left_out.contains(label) {
            continue;
        }
        for target in targets {
            outside
                .entry(target.clone())
                .or_default()
                .insert(label.clone());
        }
    }
    let existing: HashSet<&str> = assets.iter().map(|a| a.path.as_str()).collect();
    outside.retain(|target, _| existing.contains(target.as_str()));
    let mut per_mod: BTreeMap<u64, usize> = BTreeMap::new();
    for labels in outside.values() {
        let mods: BTreeSet<u64> = labels
            .iter()
            .filter_map(|label| label.strip_prefix("UGC")?.split(':').next()?.parse().ok())
            .collect();
        for id in mods {
            *per_mod.entry(id).or_default() += 1;
        }
    }
    let consumers = per_mod
        .into_iter()
        .map(|(mod_id, assets)| Consumer { mod_id, assets })
        .collect();
    let mut merged = theirs.clone();
    for (from, targets) in scanned {
        merged
            .entry(from.clone())
            .or_default()
            .extend(targets.iter().cloned());
    }
    let unreal = reach(&theirs, &[DEFINITION]);
    let everything = reach(&merged, &[DEFINITION]);
    let roots: Vec<&str> = outside.keys().map(String::as_str).collect();
    let others = reach(&merged, &roots);
    let rooted = merged
        .get(DEFINITION)
        .is_some_and(|targets| !targets.is_empty());

    let index: HashMap<&str, usize> = assets
        .iter()
        .enumerate()
        .map(|(position, asset)| (asset.path.as_str(), position))
        .collect();
    let mut used_by: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for (from, targets) in &merged {
        for target in targets {
            used_by.entry(target).or_default().insert(from);
        }
    }
    let list = assets
        .iter()
        .map(|asset| {
            let path = asset.path.as_str();
            let status = if path == DEFINITION {
                Status::Required
            } else if unreal.contains(path) {
                Status::Used
            } else if everything.contains(path) {
                Status::InString
            } else if others.contains(path) {
                Status::OtherMods
            } else {
                Status::Unused
            };
            let mut uses: Vec<usize> = merged
                .get(path)
                .into_iter()
                .flatten()
                .filter_map(|target| index.get(target.as_str()).copied())
                .collect();
            uses.sort_unstable();
            Asset {
                path: asset.path.clone(),
                size: asset.size,
                status,
                used_by: used_by
                    .get(path)
                    .into_iter()
                    .flatten()
                    .take(USED_BY_SHOWN)
                    .map(|name| name.to_string())
                    .collect(),
                uses,
                outside: outside
                    .get(path)
                    .into_iter()
                    .flatten()
                    .take(USED_BY_SHOWN)
                    .cloned()
                    .collect(),
            }
        })
        .collect();
    AssetReport {
        assets: list,
        analysed: true,
        rooted,
        excluded: selection.excluded,
        confirmed: selection.confirmed,
        pinned: selection.pinned,
        consumers,
    }
}

/// The mod's assets, with statuses if they've been worked out since the files last changed.
pub fn asset_report(project: &Path, mod_id: u64) -> Result<AssetReport> {
    let assets = sources(project, mod_id)?;
    let selection = load_selection(project, mod_id)?;
    let saved = tree_path(project, mod_id);
    let newest = newest_anywhere(project, mod_id, &assets)?;
    let fresh = saved
        .metadata()
        .and_then(|m| m.modified())
        .is_ok_and(|saved_at| saved_at >= newest);
    if fresh {
        let text = fs::read_to_string(&saved).unwrap_or_default();
        if let Ok(found) = serde_json::from_str::<SavedAnalysis>(&text) {
            if found.covers(&assets) {
                let left_out = left_out_elsewhere(project, mod_id)?;
                return Ok(report_from(
                    &assets,
                    mod_id,
                    Some(&found),
                    selection,
                    &left_out,
                ));
            }
        }
    }
    Ok(report_from(
        &assets,
        mod_id,
        None,
        selection,
        &HashSet::new(),
    ))
}

/// Work out what's used, from Unreal's tree and a scan of the mod's files.
pub fn analyze_assets(project: &Path, engine: &Path, mod_id: u64) -> Result<AssetReport> {
    let assets = sources(project, mod_id)?;
    let tree = unreal_tree(project, engine)?;
    let mount = format!("/{}/", plugin_name(mod_id));
    let known = tree.keys().filter(|p| p.starts_with(&mount)).count();
    if known == 0 && !assets.is_empty() {
        bail!("Unreal's dependency tree doesn't include this mod. Is its plugin enabled?");
    }
    let found = SavedAnalysis {
        tree,
        scanned: scan_references(mod_id, &assets)?,
        outside: outside_references(project, mod_id, &assets)?,
        files: assets.iter().map(|asset| asset.path.clone()).collect(),
    };
    let selection = load_selection(project, mod_id)?;
    let left_out = left_out_elsewhere(project, mod_id)?;
    let report = report_from(&assets, mod_id, Some(&found), selection, &left_out);
    fs::create_dir_all(store_dir(project))?;
    fs::write(tree_path(project, mod_id), serde_json::to_string(&found)?)?;
    Ok(report)
}

fn selection_path(project: &Path, mod_id: u64) -> PathBuf {
    store_dir(project).join(format!("{}.selection.json", plugin_name(mod_id)))
}

/// The saved selection, without assets that no longer exist.
pub fn load_selection(project: &Path, mod_id: u64) -> Result<Selection> {
    let Ok(text) = fs::read_to_string(selection_path(project, mod_id)) else {
        return Ok(Selection::default());
    };
    let mut selection: Selection = serde_json::from_str(&text).unwrap_or_default();
    let existing: HashSet<String> = sources(project, mod_id)?
        .into_iter()
        .map(|a| a.path)
        .collect();
    selection
        .excluded
        .retain(|path| existing.contains(path) && path != DEFINITION);
    selection.pinned.retain(|path| existing.contains(path));
    selection.excluded.sort();
    Ok(selection)
}

pub fn save_selection(project: &Path, mod_id: u64, selection: &Selection) -> Result<()> {
    if selection.excluded.iter().any(|path| path == DEFINITION) {
        bail!("A mod can't be built without its definition");
    }
    fs::create_dir_all(store_dir(project))?;
    let path = selection_path(project, mod_id);
    if selection.excluded.is_empty() && selection.pinned.is_empty() {
        fs::remove_file(path).ok();
        return Ok(());
    }
    fs::write(path, serde_json::to_string_pretty(selection)?)?;
    Ok(())
}

/// Identifies a selection, so a resumed release can't mix two different builds.
pub fn selection_id(selection: &Selection) -> String {
    if selection.excluded.is_empty() {
        return String::new();
    }
    let mut sorted = selection.excluded.clone();
    sorted.sort();
    hex::encode(Sha256::digest(sorted.join("\n").as_bytes()))
}

/// Kept assets, here and in other mods, that point at left-out ones.
pub fn check_selection(project: &Path, mod_id: u64) -> Result<Vec<MissingReference>> {
    let selection = load_selection(project, mod_id)?;
    if selection.excluded.is_empty() {
        return Ok(Vec::new());
    }
    let excluded: HashSet<&String> = selection.excluded.iter().collect();
    let assets = sources(project, mod_id)?;
    let scanned = scan_references(mod_id, &assets)?;
    let mut missing: Vec<MissingReference> = scanned
        .iter()
        .filter(|(from, _)| !excluded.contains(from))
        .flat_map(|(from, targets)| {
            targets
                .iter()
                .filter(|target| excluded.contains(target))
                .map(|target| MissingReference {
                    kept: from.clone(),
                    missing: target.clone(),
                })
        })
        .collect();
    let left_out = left_out_elsewhere(project, mod_id)?;
    for (label, targets) in outside_references(project, mod_id, &assets)? {
        if left_out.contains(&label) {
            continue;
        }
        for target in targets.iter().filter(|target| excluded.contains(target)) {
            missing.push(MissingReference {
                kept: label.clone(),
                missing: target.clone(),
            });
        }
    }
    missing.sort_by(|a, b| (&a.kept, &a.missing).cmp(&(&b.kept, &b.missing)));
    Ok(missing)
}

/// Stop a build that would ship broken references nobody agreed to.
pub fn require_safe_selection(project: &Path, mod_id: u64) -> Result<Selection> {
    let selection = load_selection(project, mod_id)?;
    if selection.excluded.is_empty() || selection.confirmed {
        return Ok(selection);
    }
    let missing = check_selection(project, mod_id)?;
    if let Some(first) = missing.first() {
        return Err(Failure {
            kind: "audit".to_string(),
            target: None,
            tool: "Pavlov Mod Studio".to_string(),
            summary: format!(
                "{} kept assets point at assets you left out, such as {} using {}.",
                missing.len(),
                first.kept,
                first.missing
            ),
            errors: missing
                .iter()
                .take(20)
                .map(|m| format!("{} uses {}", m.kept, m.missing))
                .collect(),
            exit_code: None,
            log_path: None,
        }
        .into());
    }
    Ok(selection)
}

/// Only plain names go on Unreal's command line. It splits on `+`, and non-English
/// characters don't survive RunUAT.bat.
fn safe_folder(folder: &str) -> bool {
    folder
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/'))
}

/// Use whole folders where every asset inside is left out, and single files for the rest.
pub fn plan_cook(mod_id: u64, all: &[String], excluded: &[String]) -> CookPlan {
    let left_out: HashSet<&str> = excluded.iter().map(String::as_str).collect();
    let mut contents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for path in all {
        let mut end = 0;
        while let Some(slash) = path[end..].find('/') {
            end += slash;
            contents.entry(&path[..end]).or_default().push(path);
            end += 1;
        }
    }
    let empty_folders: Vec<&str> = contents
        .iter()
        .filter(|(folder, inside)| {
            safe_folder(folder) && inside.iter().all(|path| left_out.contains(path))
        })
        .map(|(folder, _)| *folder)
        .collect();
    let topmost: Vec<&str> = empty_folders
        .iter()
        .copied()
        .filter(|folder| {
            !empty_folders
                .iter()
                .any(|other| folder.starts_with(&format!("{other}/")))
        })
        .collect();
    let covered = |path: &str| {
        topmost
            .iter()
            .any(|folder| path.starts_with(&format!("{folder}/")))
    };
    let mut files: Vec<String> = excluded.iter().filter(|p| !covered(p)).cloned().collect();
    files.sort();
    CookPlan {
        folders: topmost
            .iter()
            .map(|folder| format!("/{}/{folder}", plugin_name(mod_id)))
            .collect(),
        files,
    }
}

/// The plan for this mod's saved selection.
pub fn cook_plan(project: &Path, mod_id: u64, selection: &Selection) -> Result<CookPlan> {
    if selection.excluded.is_empty() {
        return Ok(CookPlan::default());
    }
    let all: Vec<String> = sources(project, mod_id)?
        .into_iter()
        .map(|a| a.path)
        .collect();
    Ok(plan_cook(mod_id, &all, &selection.excluded))
}

/// Delete single assets from cooked output, so packaging leaves them out.
pub fn remove_cooked(project: &Path, mod_id: u64, folder: &str, files: &[String]) -> Result<usize> {
    let plugin = plugin_name(mod_id);
    let content = project
        .join("Plugins")
        .join(&plugin)
        .join("Saved/Cooked")
        .join(folder)
        .join("Pavlov/Plugins")
        .join(&plugin)
        .join("Content");
    if !content.is_dir() {
        bail!("cannot find the cooked files in {}", content.display());
    }
    let mut removed = 0;
    for file in files {
        for extension in COOKED_EXTENSIONS {
            let path = content.join(format!("{file}.{extension}"));
            if path.is_file() {
                fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Which left-out assets a pak's file list still holds.
pub fn still_in_pak<'a>(mod_id: u64, files: &[&str], excluded: &'a [String]) -> Vec<&'a str> {
    let prefix = format!("Pavlov/Plugins/{}/Content/", plugin_name(mod_id));
    let packaged: HashSet<&str> = files
        .iter()
        .filter_map(|file| file.strip_prefix(&prefix))
        .filter_map(|file| file.rsplit_once('.').map(|(stem, _)| stem))
        .collect();
    excluded
        .iter()
        .map(String::as_str)
        .filter(|path| packaged.contains(path))
        .collect()
}

/// The mod's assets inside a pak, as content paths.
pub fn assets_in_pak(mod_id: u64, files: &[&str]) -> Vec<String> {
    let prefix = format!("Pavlov/Plugins/{}/Content/", plugin_name(mod_id));
    let mut found: Vec<String> = files
        .iter()
        .filter_map(|file| file.strip_prefix(&prefix))
        .filter_map(|file| {
            file.strip_suffix(".uasset")
                .or_else(|| file.strip_suffix(".umap"))
        })
        .map(str::to_string)
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::{plan_cook, scan, selection_id, still_in_pak, without_number, Lookup, Selection};

    use super::{BTreeMap, HashMap, PathBuf, SavedAnalysis, Source};

    fn analysis_of(files: &[&str]) -> SavedAnalysis {
        SavedAnalysis {
            tree: BTreeMap::new(),
            scanned: HashMap::new(),
            outside: HashMap::new(),
            files: files.iter().map(|file| file.to_string()).collect(),
        }
    }

    fn source_named(path: &str) -> Source {
        Source {
            path: path.to_string(),
            file: PathBuf::from(path),
            size: 1,
        }
    }

    #[test]
    fn a_saved_analysis_only_covers_the_files_it_saw() {
        let saved = analysis_of(&["a", "b"]);
        let now = |names: &[&str]| names.iter().map(|n| source_named(n)).collect::<Vec<_>>();
        assert!(saved.covers(&now(&["b", "a"])));
        assert!(!saved.covers(&now(&["a", "b", "copied in"])), "added");
        assert!(!saved.covers(&now(&["a"])), "removed");
        assert!(!saved.covers(&now(&["a", "renamed"])), "renamed");
    }

    #[test]
    fn an_older_saved_analysis_is_still_trusted_by_date() {
        assert!(analysis_of(&[]).covers(&[source_named("a")]));
    }

    fn lookup(paths: &[&str]) -> Lookup {
        Lookup::new("/UGC1/".to_string(), paths)
    }

    fn found(data: &[u8], paths: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = scan(data, &lookup(paths)).into_iter().collect();
        names.sort();
        names
    }

    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn finds_a_full_path() {
        let data = b"\x00\x00/UGC1/Music/Song\x00\x00";
        assert_eq!(found(data, &["Music/Song", "Music/Other"]), ["Music/Song"]);
    }

    #[test]
    fn finds_an_object_path() {
        let data = b"\x00/UGC1/Maps/Town.Town\x00";
        assert_eq!(found(data, &["Maps/Town"]), ["Maps/Town"]);
    }

    /// Real song names from a playlist mod, which only exist in UTF-16 inside the file.
    #[test]
    fn finds_a_non_english_path() {
        let name = "playlist/وداعة_الله";
        let mut data = vec![0, 0];
        data.extend(wide(&format!("/UGC1/{name}")));
        data.extend([0, 0]);
        assert_eq!(found(&data, &[name]), [name]);
    }

    #[test]
    fn finds_utf16_starting_on_an_odd_byte() {
        let mut data = vec![0, 0, 0];
        data.extend(wide("/UGC1/Sounds/Hit"));
        data.extend([0, 0]);
        assert_eq!(found(&data, &["Sounds/Hit"]), ["Sounds/Hit"]);
    }

    /// `prop_dynamic_31991` appears in a file as `prop_dynamic`, with the number kept apart.
    #[test]
    fn finds_a_name_whose_number_is_stored_apart() {
        let data = b"\x00/UGC1/Props/prop_dynamic\x00";
        let paths = [
            "Props/prop_dynamic_31991",
            "Props/prop_dynamic_5058",
            "Props/other",
        ];
        assert_eq!(
            found(data, &paths),
            ["Props/prop_dynamic_31991", "Props/prop_dynamic_5058"]
        );
    }

    #[test]
    fn a_folder_path_keeps_everything_inside() {
        let data = b"\x00/UGC1/Sounds/Zombie/\x00";
        let paths = ["Sounds/Zombie/a", "Sounds/Zombie/b", "Sounds/other"];
        assert_eq!(found(data, &paths), ["Sounds/Zombie/a", "Sounds/Zombie/b"]);
    }

    #[test]
    fn finds_a_bare_name_in_a_string() {
        let data = b"\x00zombie_madness1\x00";
        assert_eq!(
            found(data, &["Sounds/zombie_madness1"]),
            ["Sounds/zombie_madness1"]
        );
    }

    #[test]
    fn finds_a_blueprint_class_name() {
        let data = b"\x00BP_Doorway_C\x00";
        assert_eq!(
            found(data, &["Blueprints/BP_Doorway"]),
            ["Blueprints/BP_Doorway"]
        );
    }

    #[test]
    fn a_longer_name_is_not_a_match() {
        let data = b"\x00boombox_staticmesh_readyright\x00";
        assert!(found(data, &["Items/boombox_staticmesh"]).is_empty());
    }

    #[test]
    fn short_names_are_not_matched_as_bare_words() {
        assert!(found(b"\x00Rock\x00", &["Props/Rock"]).is_empty());
    }

    #[test]
    fn another_mods_path_is_not_this_mod() {
        assert!(found(b"\x00/UGC2/Music/Song\x00", &["Music/Song2"]).is_empty());
    }

    #[test]
    fn strips_only_a_trailing_number() {
        assert_eq!(without_number("prop_12"), "prop");
        assert_eq!(without_number("prop"), "prop");
        assert_eq!(without_number("prop_12a"), "prop_12a");
        assert_eq!(without_number("_12"), "_12");
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_fully_left_out_folder_becomes_one_entry() {
        let all = strings(&["definition", "Music/a", "Music/b", "Music/Sub/c", "Keep/d"]);
        let plan = plan_cook(1, &all, &strings(&["Music/a", "Music/b", "Music/Sub/c"]));
        assert_eq!(plan.folders, ["/UGC1/Music"]);
        assert!(plan.files.is_empty());
    }

    #[test]
    fn a_partly_left_out_folder_uses_single_files() {
        let all = strings(&["Music/a", "Music/b"]);
        let plan = plan_cook(1, &all, &strings(&["Music/a"]));
        assert!(plan.folders.is_empty());
        assert_eq!(plan.files, ["Music/a"]);
    }

    #[test]
    fn a_mix_uses_both() {
        let all = strings(&["A/x", "A/y", "B/z", "B/w", "root"]);
        let plan = plan_cook(1, &all, &strings(&["A/x", "A/y", "B/z", "root"]));
        assert_eq!(plan.folders, ["/UGC1/A"]);
        assert_eq!(plan.files, ["B/z", "root"]);
    }

    /// `+` splits Unreal's list, and RunUAT.bat mangles non-English text.
    #[test]
    fn unsafe_folder_names_fall_back_to_single_files() {
        let all = strings(&["a+b/x", "موسيقى/y"]);
        let plan = plan_cook(1, &all, &all.clone());
        assert!(plan.folders.is_empty());
        assert_eq!(plan.files.len(), 2);
    }

    #[test]
    fn a_selection_is_the_same_in_any_order() {
        let one = Selection {
            excluded: strings(&["a", "b"]),
            ..Selection::default()
        };
        // Confirming and pinning don't change what gets built.
        let two = Selection {
            excluded: strings(&["b", "a"]),
            confirmed: true,
            pinned: strings(&["c"]),
        };
        assert_eq!(selection_id(&one), selection_id(&two));
        assert_ne!(selection_id(&one), selection_id(&Selection::default()));
        assert_eq!(selection_id(&Selection::default()), "");
    }

    #[test]
    fn labels_another_mods_asset() {
        assert_eq!(
            super::outside_label("/UGC2/Maps/Town").as_deref(),
            Some("UGC2:Maps/Town")
        );
        assert_eq!(super::outside_label("/Game/Maps/Town"), None);
    }

    #[test]
    fn spots_left_out_assets_still_in_a_pak() {
        let files = [
            "Pavlov/Plugins/UGC1/Content/Music/a.uasset",
            "Pavlov/Plugins/UGC1/Content/Keep.uexp",
            "Engine/Content/x.uasset",
        ];
        let excluded = strings(&["Music/a", "Music/b"]);
        assert_eq!(still_in_pak(1, &files, &excluded), ["Music/a"]);
    }
}
