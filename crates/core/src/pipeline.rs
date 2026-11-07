//! The modkit's Upload button: version, definition, mod metadata, cook, upload.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::Api;
use crate::assets::{load_selection, selection_id};
use crate::cook::{cook, CookOptions};
use crate::definition::{check_definition, write_version};
use crate::eula::require_eula;
use crate::progress::stage;
use crate::say;
use crate::share::require_choice;
use crate::targets::Target;
use crate::upload::{
    next_version, prepare, publish, set_mod_version, upload_source, Options, PART_SIZE,
};

pub struct Release {
    pub cook: CookOptions,
    pub changelog: String,
    pub inactive: bool,
    /// Make all platforms live together at the end.
    pub together: bool,
    /// Also share what's on the mod's share list.
    pub share_source: bool,
}

/// Written once a release is cooked, so a failed upload can pick up where it
/// stopped instead of bumping the version and cooking again.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Checkpoint {
    version: i64,
    targets: Vec<String>,
    /// Which assets were left out. A different choice is a different build.
    #[serde(default)]
    selection: String,
}

fn checkpoint_path(project: &Path, mod_id: u64) -> PathBuf {
    project
        .join("Saved/Cooked/Workshop")
        .join(format!("UGC{mod_id}"))
        .join("release.json")
}

fn load_checkpoint(path: &Path, targets: &[String], selection: &str) -> Option<Checkpoint> {
    let saved: Checkpoint = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    (saved.targets == targets && saved.selection == selection).then_some(saved)
}

fn current_selection(project: &Path, mod_id: u64) -> Result<String> {
    Ok(selection_id(&load_selection(project, mod_id)?))
}

/// The version of a stopped release that these exact choices would resume.
pub fn pending_version(project: &Path, mod_id: u64, targets: &[Target]) -> Option<i64> {
    let names: Vec<String> = targets.iter().map(|t| t.name().to_string()).collect();
    let selection = current_selection(project, mod_id).ok()?;
    load_checkpoint(&checkpoint_path(project, mod_id), &names, &selection).map(|c| c.version)
}

/// Forget a stopped release, so the next publish starts from a fresh cook.
pub fn discard_pending(project: &Path, mod_id: u64) {
    fs::remove_file(checkpoint_path(project, mod_id)).ok();
}

pub fn run(api: &Api, release: Release, targets: &[Target]) -> Result<()> {
    let Release {
        cook: cook_options,
        changelog,
        inactive,
        together,
        share_source,
    } = release;
    let mod_id = cook_options.mod_id;
    let names: Vec<String> = targets.iter().map(|t| t.name().to_string()).collect();
    let path = checkpoint_path(&cook_options.project, mod_id);

    stage("check", None);
    require_eula(&cook_options.project)?;
    check_definition(&cook_options.project, mod_id, targets)?;
    // A cook takes minutes, so find out now rather than at the end.
    if share_source {
        require_choice(&cook_options.project, mod_id)?;
    }

    let selection = current_selection(&cook_options.project, mod_id)?;
    let resumed = load_checkpoint(&path, &names, &selection);
    let version = match &resumed {
        Some(saved) => {
            say!("resuming version {}", saved.version);
            saved.version
        }
        None => {
            stage("version", None);
            let version = next_version(api, mod_id)?;
            say!("new version {version}");
            let asset = format!("/UGC{mod_id}/definition");
            let saved =
                write_version(&cook_options.engine, &cook_options.project, &asset, version)?;
            say!(
                "definition ({}) saved at version {}",
                saved.class,
                saved.version
            );
            set_mod_version(api, mod_id, version)?;
            cook(&cook_options, targets)?;
            let checkpoint = Checkpoint {
                version,
                targets: names,
                selection,
            };
            fs::write(&path, serde_json::to_string(&checkpoint)?)
                .context("could not save release progress")?;
            version
        }
    };

    let options = Options {
        project: cook_options.project,
        engine: cook_options.engine,
        mod_id,
        version: Some(version.to_string()),
        changelog,
        inactive,
        // On a resume, builds that already finished aren't sent twice.
        skip_existing: resumed.is_some(),
        together,
        part_size: PART_SIZE,
        poll_seconds: 10,
        skip_audit: false,
    };
    stage("package", None);
    let zips = prepare(&options, targets)?;
    publish(api, &options, &zips)?;
    if share_source {
        upload_source(api, &options)?;
    }
    fs::remove_file(&path).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(dir: &Path) -> PathBuf {
        let path = dir.join("release.json");
        let checkpoint = Checkpoint {
            version: 6,
            targets: vec!["android".to_string(), "shack".to_string()],
            selection: "abc".to_string(),
        };
        fs::write(&path, serde_json::to_string(&checkpoint).unwrap()).unwrap();
        path
    }

    #[test]
    fn resumes_only_the_same_release() {
        let dir = std::env::temp_dir().join(format!("pms-checkpoint-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = saved(&dir);
        let quest = vec!["android".to_string(), "shack".to_string()];
        let pc = vec!["windows".to_string(), "linux".to_string()];
        assert_eq!(load_checkpoint(&path, &quest, "abc").unwrap().version, 6);
        assert!(load_checkpoint(&path, &pc, "abc").is_none());
        // A different selection is a different build, so it can't resume this one.
        assert!(load_checkpoint(&path, &quest, "").is_none());
        assert!(load_checkpoint(&path, &quest, "other").is_none());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_or_broken_checkpoint_means_a_fresh_release() {
        let dir = std::env::temp_dir().join(format!("pms-broken-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release.json");
        assert!(load_checkpoint(&path, &[], "").is_none());
        fs::write(&path, "not json").unwrap();
        assert!(load_checkpoint(&path, &[], "").is_none());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_release_stopped_by_an_older_version_still_resumes() {
        let dir = std::env::temp_dir().join(format!("pms-older-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release.json");
        let older = r#"{"version":6,"targets":["windows"],"compress":false,"selection":""}"#;
        fs::write(&path, older).unwrap();
        let targets = vec!["windows".to_string()];
        assert_eq!(load_checkpoint(&path, &targets, "").unwrap().version, 6);
        fs::remove_dir_all(dir).ok();
    }
}
