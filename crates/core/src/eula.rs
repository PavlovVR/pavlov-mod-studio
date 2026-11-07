//! Vankrupt's licence for the modkit. Using the modkit means accepting it, and this app does the
//! modkit's work from outside the editor, so it has to ask too.
//!
//! The text is read from the modder's own modkit, so it's always the current one and this project
//! doesn't redistribute it. What's remembered is which text was accepted, so a changed licence is
//! asked about again.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::assets::store_dir;

const LICENCE: &str = "Plugins/CustomMapTools/EULA.txt";

#[derive(Debug, Serialize)]
pub struct Eula {
    pub text: String,
    pub accepted: bool,
}

fn accepted_path(project: &Path) -> PathBuf {
    store_dir(project).join("eula.accepted")
}

fn licence_bytes(project: &Path) -> Result<Vec<u8>> {
    fs::read(project.join(LICENCE))
        .context("This modkit has no EULA.txt in Plugins/CustomMapTools, so it may be incomplete")
}

/// Git can change a file's line endings, and that doesn't make it a different licence.
fn id_of(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    hex::encode(Sha256::digest(text.as_bytes()))
}

pub fn read_eula(project: &Path) -> Result<Eula> {
    let bytes = licence_bytes(project)?;
    let saved = fs::read_to_string(accepted_path(project)).unwrap_or_default();
    Ok(Eula {
        accepted: saved.trim() == id_of(&bytes),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

pub fn accept_eula(project: &Path) -> Result<()> {
    let bytes = licence_bytes(project)?;
    fs::create_dir_all(store_dir(project))?;
    fs::write(accepted_path(project), id_of(&bytes))?;
    Ok(())
}

/// Nothing that uses the modkit runs until its licence is accepted.
pub fn require_eula(project: &Path) -> Result<()> {
    if !read_eula(project)?.accepted {
        bail!("Accept Pavlov's EULA first. Open the app, or run the eula command with --accept.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modkit(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pms-eula-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("Plugins/CustomMapTools")).unwrap();
        fs::write(dir.join(LICENCE), "END USER LICENSE AGREEMENT").unwrap();
        dir
    }

    #[test]
    fn nothing_is_accepted_to_begin_with() {
        let dir = modkit("start");
        let eula = read_eula(&dir).unwrap();
        assert_eq!(eula.text, "END USER LICENSE AGREEMENT");
        assert!(!eula.accepted);
        assert!(require_eula(&dir).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn accepting_lets_work_go_ahead() {
        let dir = modkit("accept");
        accept_eula(&dir).unwrap();
        assert!(read_eula(&dir).unwrap().accepted);
        assert!(require_eula(&dir).is_ok());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_changed_licence_is_asked_about_again() {
        let dir = modkit("changed");
        accept_eula(&dir).unwrap();
        fs::write(dir.join(LICENCE), "A NEW AGREEMENT").unwrap();
        assert!(!read_eula(&dir).unwrap().accepted);
        assert!(require_eula(&dir).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn different_line_endings_are_the_same_licence() {
        let dir = modkit("endings");
        fs::write(dir.join(LICENCE), "ONE\nTWO\n").unwrap();
        accept_eula(&dir).unwrap();
        fs::write(dir.join(LICENCE), "ONE\r\nTWO\r\n").unwrap();
        assert!(read_eula(&dir).unwrap().accepted);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_modkit_without_the_licence_can_not_go_ahead() {
        let dir = modkit("missing");
        fs::remove_file(dir.join(LICENCE)).unwrap();
        assert!(read_eula(&dir).is_err());
        assert!(accept_eula(&dir).is_err());
        assert!(require_eula(&dir).is_err());
        fs::remove_dir_all(dir).ok();
    }
}
