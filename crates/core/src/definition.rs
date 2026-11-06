use std::env::temp_dir;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::editor;
use crate::process::{is_running, run_tool, Failure, Tool};
use crate::targets::Target;

const SCRIPT: &str = include_str!("scripts/definition.py");
const CREATE_SCRIPT: &str = include_str!("scripts/create_definition.py");

pub struct Definition {
    pub class: String,
    pub version: i64,
}

/// Each headless run is a fresh Unreal session, so a read after a write
/// proves the value really reached the file on disk.
fn run_headless(
    engine: &Path,
    project: &Path,
    script_text: &str,
    env: &[(&str, String)],
) -> Result<Definition> {
    let dir = temp_dir().join(format!("pavlov-mod-studio-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let script = dir.join("script.py");
    let result_file = dir.join("result.json");
    fs::remove_file(&result_file).ok();
    fs::write(&script, script_text)?;

    let log = project.join("Saved/Logs/pavlov_definition.log");
    // Stopping the editor while it saves could damage the definition, so it always finishes.
    let tool = Tool {
        name: "UnrealEditor-Cmd.exe",
        log: Some(&log),
        cancellable: false,
        keep_stdout: false,
    };
    let mut command = Command::new(engine.join("Binaries/Win64/UnrealEditor-Cmd.exe"));
    command
        .arg(project.join("Pavlov.uproject"))
        .arg("-run=pythonscript")
        .arg(format!("-script={}", script.display()))
        .args(["-unattended", "-nullrhi", "-nosplash", "-nopause"])
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .env("PMS_OUT", &result_file);
    // Unreal can exit non-zero for unrelated plugin errors, so trust the result file.
    let run = run_tool(&tool, &mut command)?;

    let Ok(text) = fs::read_to_string(&result_file) else {
        let failure = run.failure("definition", None, &tool, "Unreal did not report a result.");
        return Err(failure.into());
    };
    fs::remove_dir_all(&dir).ok();
    let result: Value = serde_json::from_str(&text)?;
    if let Some(error) = result["error"].as_str() {
        let failure = Failure {
            summary: error.to_string(),
            ..run.failure("definition", None, &tool, error)
        };
        return Err(failure.into());
    }
    Ok(Definition {
        class: result["class"].as_str().unwrap_or_default().to_string(),
        version: result["version"].as_i64().context("no version in result")?,
    })
}

/// Create a new definition, then confirm it exists from a fresh session.
pub fn create_definition(
    engine: &Path,
    project: &Path,
    plugin: &str,
    class: &str,
) -> Result<Definition> {
    let env = [
        ("PMS_PLUGIN", plugin.to_string()),
        ("PMS_CLASS", class.to_string()),
    ];
    run_headless(engine, project, CREATE_SCRIPT, &env)?;
    read_version(engine, project, &format!("/{plugin}/definition"))
}

/// An asset's name table stores each name as a length, the text, then a zero byte.
/// Unreal only writes a property's name when it has been set.
fn has_name(bytes: &[u8], name: &str) -> bool {
    let mut record = ((name.len() + 1) as u32).to_le_bytes().to_vec();
    record.extend_from_slice(name.as_bytes());
    record.push(0);
    bytes.windows(record.len()).any(|window| window == record)
}

/// The mod type stored in a definition file.
pub fn mod_kind(bytes: &[u8]) -> &'static str {
    if has_name(bytes, "Pavlov_Map") {
        "Map"
    } else if has_name(bytes, "Pavlov_GameMode") {
        "GameMode"
    } else if has_name(bytes, "Pavlov_Mod") {
        "Mod"
    } else {
        "Unknown"
    }
}

/// The modkit's definition checks. This reads the file directly, because asking
/// Unreal for a map property loads the entire map.
fn check_bytes(bytes: &[u8], targets: &[Target]) -> Result<()> {
    let wants_pc = targets.contains(&Target::Windows);
    let wants_quest = targets.contains(&Target::Android);
    if has_name(bytes, "Pavlov_Map") {
        if wants_pc && !has_name(bytes, "Map") {
            bail!("Definition did not contain a map to upload");
        }
        if wants_quest && !has_name(bytes, "QuestMap") {
            bail!("Definition did not contain a quest map to upload");
        }
    } else if has_name(bytes, "Pavlov_GameMode") {
        if !has_name(bytes, "GameLogicClass") {
            bail!("GameMode definition did not contain a gamelogic to upload");
        }
    } else if !has_name(bytes, "Pavlov_Mod") {
        bail!("this is not a Map, Mod or GameMode definition");
    }
    Ok(())
}

pub fn check_definition(project: &Path, mod_id: u64, targets: &[Target]) -> Result<()> {
    let path = project
        .join("Plugins")
        .join(format!("UGC{mod_id}"))
        .join("Content/definition.uasset");
    let bytes = fs::read(&path)
        .with_context(|| format!("cannot find definition file {}", path.display()))?;
    check_bytes(&bytes, targets)
}

pub fn read_version(engine: &Path, project: &Path, asset: &str) -> Result<Definition> {
    run_headless(engine, project, SCRIPT, &[("PMS_ASSET", asset.to_string())])
}

/// Save the version, then confirm it from a fresh session before anything is cooked.
pub fn write_version(
    engine: &Path,
    project: &Path,
    asset: &str,
    version: i64,
) -> Result<Definition> {
    // An open editor locks a definition it has loaded, so it has to do this itself.
    if let Ok(Some(mut open)) = editor::connect(project) {
        crate::say!("Unreal is open, so it's saving the version");
        open.set_version(asset, version)?;
    } else {
        let env = [
            ("PMS_ASSET", asset.to_string()),
            ("PMS_VERSION", version.to_string()),
        ];
        let saved = run_headless(engine, project, SCRIPT, &env);
        if saved.is_err() && is_running("UnrealEditor.exe") {
            bail!(
                "Unreal is open and has this mod's definition loaded, but it didn't answer. \
                Close Unreal and try again."
            );
        }
        saved?;
    }
    let saved = read_version(engine, project, asset)?;
    if saved.version != version {
        bail!("definition says {} after saving {version}", saved.version);
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(names: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for name in names {
            bytes.extend(((name.len() + 1) as u32).to_le_bytes());
            bytes.extend(name.as_bytes());
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn map_needs_a_map_for_each_platform() {
        let both = asset(&["Pavlov_Map", "Map", "QuestMap"]);
        assert!(check_bytes(&both, &[Target::Windows, Target::Android]).is_ok());
        let pc_only = asset(&["Pavlov_Map", "Map"]);
        assert!(check_bytes(&pc_only, &[Target::Windows]).is_ok());
        assert!(check_bytes(&pc_only, &[Target::Android]).is_err());
        let quest_only = asset(&["Pavlov_Map", "QuestMap"]);
        assert!(check_bytes(&quest_only, &[Target::Windows]).is_err());
    }

    #[test]
    fn game_mode_needs_game_logic() {
        assert!(check_bytes(
            &asset(&["Pavlov_GameMode", "GameLogicClass"]),
            &[Target::Windows]
        )
        .is_ok());
        assert!(check_bytes(&asset(&["Pavlov_GameMode"]), &[Target::Windows]).is_err());
    }

    #[test]
    fn mod_has_no_extra_requirement_and_junk_is_refused() {
        assert!(check_bytes(&asset(&["Pavlov_Mod"]), &[Target::Android]).is_ok());
        assert!(check_bytes(&asset(&["Something"]), &[Target::Windows]).is_err());
    }
}
