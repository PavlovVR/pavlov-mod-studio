use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::process::{run_tool, Failure, Tool, ToolRun};
use crate::targets::Cook;

/// Same layout as the modkit's zip: metadata.json and the pak at the root.
pub fn build_zip(cook: &Cook, out: &Path, compress: bool) -> Result<()> {
    let newest = fs::metadata(&cook.pak)?
        .modified()?
        .max(fs::metadata(&cook.metadata)?.modified()?);
    if out.is_file() && fs::metadata(out)?.modified()? >= newest {
        return Ok(());
    }
    let method = if compress {
        CompressionMethod::Deflated
    } else {
        CompressionMethod::Stored
    };
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(true);
    let tmp = out.with_extension("tmp");
    let file = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut zip = ZipWriter::new(file);
    for path in [&cook.metadata, &cook.pak] {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("bad file name")?;
        zip.start_file(name, options)?;
        io::copy(&mut File::open(path)?, &mut zip)?;
    }
    zip.finish()?;
    fs::rename(&tmp, out)?;
    Ok(())
}

const UNREAL_PAK: Tool = Tool {
    name: "UnrealPak.exe",
    log: None,
    cancellable: false,
    keep_stdout: true,
};

fn list_pak(engine: &Path, pak: &Path) -> Result<ToolRun> {
    let mut command = Command::new(engine.join("Binaries/Win64/UnrealPak.exe"));
    run_tool(&UNREAL_PAK, command.arg(pak).arg("-List"))
}

/// Every file inside a pak.
pub fn pak_files(engine: &Path, pak: &Path) -> Result<Vec<String>> {
    let run = list_pak(engine, pak)?;
    Ok(pak_paths(&run.stdout)
        .into_iter()
        .map(|(path, _)| path)
        .collect())
}

fn audit_failure(run: &ToolRun, summary: String) -> Failure {
    Failure {
        summary: summary.clone(),
        ..run.failure("audit", None, &UNREAL_PAK, &summary)
    }
}

/// Refuse a pak holding anything outside this mod, its source plugins and the engine.
/// Fails closed: if the pak can't be read, it doesn't pass.
pub fn audit_pak(engine: &Path, pak: &Path, allowed: &[String]) -> Result<usize> {
    let run = list_pak(engine, pak)?;
    let paths = pak_paths(&run.stdout);
    let files: Vec<&str> = paths.iter().map(|(path, _)| path.as_str()).collect();
    if files.is_empty() {
        let summary = format!("could not read the file list of {}", pak.display());
        return Err(audit_failure(&run, summary).into());
    }
    let foreign: Vec<&&str> = files
        .iter()
        .filter(|file| !is_allowed(file, allowed))
        .collect();
    let (shared, blocked): (Vec<&&str>, Vec<&&str>) =
        foreign.into_iter().partition(|file| is_modkit_plugin(file));
    if let Some(first) = blocked.first() {
        let summary = format!(
            "{} contains {} file(s) from outside this mod, for example {first}. Nothing was uploaded.",
            pak.display(),
            blocked.len()
        );
        return Err(audit_failure(&run, summary).into());
    }
    if let Some(first) = shared.first() {
        crate::say!(
            "warning: {} file(s) come from modkit plugins, for example {first}",
            shared.len()
        );
    }
    Ok(files.len())
}

#[derive(Serialize)]
pub struct PakItem {
    pub name: String,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct PakReport {
    pub files: usize,
    pub total_bytes: u64,
    pub folders: Vec<PakItem>,
    pub largest: Vec<PakItem>,
}

fn entry_size(line: &str) -> Option<u64> {
    let rest = &line[line.find("size: ")? + 6..];
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// The folder that matters in a pak path, such as `Content/Music`.
fn group_of(file: &str) -> String {
    let path = file.split("/Content/").nth(1).unwrap_or(file);
    let mut parts = path.split('/');
    match (parts.next(), parts.next()) {
        (Some(first), Some(_)) => first.to_string(),
        _ => "Top level".to_string(),
    }
}

fn build_report(text: &str) -> PakReport {
    let mut items: Vec<PakItem> = pak_paths(text)
        .into_iter()
        .filter_map(|(name, line)| {
            Some(PakItem {
                name,
                bytes: entry_size(line)?,
            })
        })
        .collect();
    let mut folders: BTreeMap<String, u64> = BTreeMap::new();
    for item in &items {
        *folders.entry(group_of(&item.name)).or_default() += item.bytes;
    }
    let mut folders: Vec<PakItem> = folders
        .into_iter()
        .map(|(name, bytes)| PakItem { name, bytes })
        .collect();
    folders.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    let files = items.len();
    let total_bytes = items.iter().map(|item| item.bytes).sum();
    items.truncate(20);
    PakReport {
        files,
        total_bytes,
        folders,
        largest: items,
    }
}

/// Show what takes up the space in a cooked pak.
pub fn analyze_pak(engine: &Path, pak: &Path) -> Result<PakReport> {
    let report = build_report(&list_pak(engine, pak)?.stdout);
    if report.files == 0 {
        bail!("could not read the file list of {}", pak.display());
    }
    Ok(report)
}

/// Zip a plugin folder for sharing, leaving out generated files.
pub fn zip_folder(dir: &Path, out: &Path, skip: &[&str]) -> Result<()> {
    let tmp = out.with_extension("tmp");
    let mut zip = ZipWriter::new(File::create(&tmp)?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder)? {
            let path = entry?.path();
            let relative = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
            if skip.iter().any(|name| relative == *name) {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
            } else {
                zip.start_file(relative, options)?;
                io::copy(&mut File::open(&path)?, &mut zip)?;
            }
        }
    }
    zip.finish()?;
    fs::rename(&tmp, out)?;
    Ok(())
}

fn pak_entry(line: &str) -> Option<&str> {
    let start = line.find('"')? + 1;
    let length = line[start..].find("\" offset")?;
    Some(&line[start..start + length])
}

/// Entries are listed relative to the pak's mount point, which is the folder all its files
/// share. A small mod with no engine files mounts at `Pavlov/`, so its entries lose that prefix.
fn mount_prefix(text: &str) -> String {
    let mount = text
        .lines()
        .find_map(|line| line.split_once("Mount point ").map(|(_, path)| path.trim()))
        .unwrap_or_default();
    mount.trim_start_matches("../").to_string()
}

/// Every file in a listing with its full path, and the line it came from.
fn pak_paths(text: &str) -> Vec<(String, &str)> {
    let prefix = mount_prefix(text);
    text.lines()
        .filter_map(|line| Some((format!("{prefix}{}", pak_entry(line)?), line)))
        .collect()
}

/// A plugin every modder has, such as CustomMapTools. Its files are the only unexpected ones
/// worth letting through: another mod's are someone's work, and the game's would replace it.
fn is_modkit_plugin(file: &str) -> bool {
    file.strip_prefix("Pavlov/Plugins/")
        .is_some_and(|rest| !rest.starts_with("UGC"))
}

fn is_allowed(file: &str, allowed: &[String]) -> bool {
    file.starts_with("Engine/")
        || allowed.iter().any(|plugin| {
            file.starts_with(&format!("Pavlov/Plugins/{plugin}/"))
                || file == format!("Pavlov/Plugins/{plugin}.upluginmanifest")
                || file.starts_with(&format!("Pavlov/Content/ShaderArchive-{plugin}_"))
        })
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real report: a small mod's pak, which mounts below the root.
    #[test]
    fn a_small_paks_files_get_their_full_path() {
        let text = concat!(
            "LogPakFile: Display: Mount point ../../../Pavlov/\n",
            "LogPakFile: Display: \"Content/ShaderArchive-UGC6505134_Chunk0-SF_VULKAN_ES31_ANDROID-VULKAN_ES3_1_ANDROID.ushaderbytecode\" offset: 0, size: 10 bytes, sha1: A\n",
            "LogPakFile: Display: \"Plugins/UGC6505134/Content/Map.umap\" offset: 10, size: 20 bytes, sha1: B\n",
        );
        let paths: Vec<String> = pak_paths(text).into_iter().map(|(path, _)| path).collect();
        assert_eq!(paths[1], "Pavlov/Plugins/UGC6505134/Content/Map.umap");
        let allowed = vec!["UGC6505134".to_string()];
        assert!(
            paths.iter().all(|path| is_allowed(path, &allowed)),
            "{paths:?}"
        );
    }

    #[test]
    fn a_normal_paks_paths_are_unchanged() {
        let text = concat!(
            "LogPakFile: Display: Mount point ../../../\n",
            "LogPakFile: Display: \"Engine/Content/a.uasset\" offset: 0, size: 10 bytes, sha1: A\n",
        );
        assert_eq!(pak_paths(text)[0].0, "Engine/Content/a.uasset");
    }

    #[test]
    fn only_modkit_plugins_are_let_through() {
        assert!(is_modkit_plugin(
            "Pavlov/Plugins/CustomMapTools/Content/a.uasset"
        ));
        for file in [
            "Pavlov/Plugins/UGC123/Content/a.uasset",
            "Pavlov/Plugins/UGC123.upluginmanifest",
            "Pavlov/Content/ShaderArchive-UGC123_Chunk0-PCD3D.ushaderbytecode",
            "Pavlov/Content/Maps/Base.umap",
            "Somewhere/Else.uasset",
        ] {
            assert!(!is_modkit_plugin(file), "{file}");
        }
    }

    fn rules() -> Vec<String> {
        vec!["UGC6498140".to_string(), "UGC3569962".to_string()]
    }

    #[test]
    fn accepts_a_clean_build() {
        for file in [
            "Pavlov/Plugins/UGC6498140/Content/definition.uasset",
            "Pavlov/Plugins/UGC6498140.upluginmanifest",
            "Pavlov/Plugins/UGC3569962/Content/Tools/Shared.uasset",
            "Pavlov/Content/ShaderArchive-UGC6498140_Chunk0-PCD3D_SM5-PCD3D_SM5.ushaderbytecode",
            "Engine/Content/EngineFonts/DroidSansMono.uasset",
        ] {
            assert!(is_allowed(file, &rules()), "{file}");
        }
    }

    #[test]
    fn rejects_other_mods_content() {
        for file in [
            // The real leak from a mangled exclusion list.
            "Pavlov/Plugins/UGC4431695/Content/DO_NOT_COPY/BoomBox_Interface.uasset",
            "Pavlov/Plugins/UGC4438346/Content/BlackSmokeRP/Map.umap",
            "Pavlov/Plugins/UGC64981400/Content/Lookalike.uasset",
            "Pavlov/Plugins/UGC4431695.upluginmanifest",
            "Pavlov/Content/ShaderArchive-UGC4431695_Chunk0.ushaderbytecode",
            "Pavlov/Content/BaseGame.uasset",
        ] {
            assert!(!is_allowed(file, &rules()), "{file}");
        }
    }

    #[test]
    fn reports_what_takes_up_space() {
        let text = concat!(
            "LogPakFile: Display: \"Pavlov/Plugins/UGC1/Content/Music/a.uexp\" offset: 0, size: 900 bytes, sha1: 00\n",
            "LogPakFile: Display: \"Pavlov/Plugins/UGC1/Content/Music/b.uexp\" offset: 9, size: 100 bytes, sha1: 00\n",
            "LogPakFile: Display: \"Pavlov/Plugins/UGC1/Content/definition.uasset\" offset: 10, size: 50 bytes, sha1: 00\n",
            "LogPakFile: Display: 3 files (1050 bytes)\n",
        );
        let report = build_report(text);
        assert_eq!(report.files, 3);
        assert_eq!(report.total_bytes, 1050);
        assert_eq!(report.folders[0].name, "Music");
        assert_eq!(report.folders[0].bytes, 1000);
        assert_eq!(report.largest[0].bytes, 900);
    }

    #[test]
    fn reads_unrealpak_list_lines() {
        let line = r#"LogPakFile: Display: "Pavlov/a.uasset" offset: 0, size: 5 bytes"#;
        assert_eq!(pak_entry(line), Some("Pavlov/a.uasset"));
        assert_eq!(pak_entry("LogPakFile: Display: Mount point"), None);
    }
}
