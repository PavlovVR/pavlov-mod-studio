//! Create a mod: on mod.io first, then the local plugin and definition.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use reqwest::blocking::multipart::{Form, Part};
use serde_json::{json, Value};

use crate::api::{Api, GAME_ID};
use crate::definition::create_definition;
use crate::editor::show_plugin_content;
use crate::eula::require_eula;
use crate::say;

const ENGINE_VERSION: &str = "5.1.1";
const TEMPLATE_ICON: &str =
    "Plugins/Editor/PluginBrowser/Templates/ContentOnly/Resources/Icon128.png";
/// What the modkit uses when no logo is chosen. mod.io requires a logo, and this
/// reads the modder's own copy, so this project doesn't redistribute it.
const DEFAULT_LOGO: &str = "Plugins/CustomMapTools/Resources/PavlovThumbnail.jpg";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModKind {
    Map,
    Mod,
    GameMode,
}

impl ModKind {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "Map" => ModKind::Map,
            "Mod" => ModKind::Mod,
            "GameMode" => ModKind::GameMode,
            other => bail!("unknown mod type '{other}'; use Map, Mod or GameMode"),
        })
    }

    pub fn tag(self) -> &'static str {
        match self {
            ModKind::Map => "Map",
            ModKind::Mod => "Mod",
            ModKind::GameMode => "GameMode",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            ModKind::Map => "Pavlov_Map",
            ModKind::Mod => "Pavlov_Mod",
            ModKind::GameMode => "Pavlov_GameMode",
        }
    }
}

pub struct NewMod {
    pub name: String,
    pub summary: String,
    pub kind: ModKind,
    /// Leave empty to use the modkit's default logo.
    pub logo: Option<PathBuf>,
    /// Create the listing hidden, so only the team can see it.
    pub hidden: bool,
}

pub struct Workspace {
    pub project: PathBuf,
    pub engine: PathBuf,
}

fn validate(new: &NewMod) -> Result<()> {
    let length = new.name.trim().chars().count();
    if !(3..=32).contains(&length) {
        bail!("The name needs to be 3-32 characters long");
    }
    if new.summary.trim().is_empty() {
        bail!("Add a summary so people know what the mod is");
    }
    if let Some(logo) = &new.logo {
        let extension = logo
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
            bail!("The logo must be a PNG or JPG image");
        }
    }
    Ok(())
}

/// The chosen logo, or the modkit's default when there isn't one.
fn logo_file(project: &Path, new: &NewMod) -> Result<PathBuf> {
    if let Some(logo) = &new.logo {
        return Ok(logo.clone());
    }
    let default = project.join(DEFAULT_LOGO);
    if !default.is_file() {
        bail!(
            "Choose a logo, because the modkit's default is missing from {}",
            default.display()
        );
    }
    Ok(default)
}

fn mod_blob(kind: ModKind) -> String {
    let fields = format!(
        "{{\r\n\t\"ModType\": {},\r\n\t\"EngineVersion\": {},\r\n\t\"VersionUUID\": 0\r\n}}",
        json!(kind.tag()),
        json!(ENGINE_VERSION)
    );
    fields
}

/// Create the mod on mod.io and return its id.
pub fn create_on_modio(api: &Api, project: &Path, new: &NewMod) -> Result<u64> {
    validate(new)?;
    let path = logo_file(project, new)?;
    let logo = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("logo.png")
        .to_string();
    let url = api.url(&format!("/games/{GAME_ID}/mods"));
    let created: Value = api.send("create mod", &[], || {
        let form = Form::new()
            .text("name", new.name.trim().to_string())
            .text("summary", new.summary.trim().to_string())
            .text("tags[]", new.kind.tag())
            .text("metadata_blob", mod_blob(new.kind))
            .part(
                "logo",
                Part::bytes(logo.clone()).file_name(file_name.clone()),
            );
        // The modkit never sends this, so normal mods leave it to mod.io.
        let form = if new.hidden {
            form.text("visible", "0")
        } else {
            form
        };
        api.client().post(&url).multipart(form)
    })?;
    created["id"]
        .as_u64()
        .context("mod.io did not return a mod id")
}

fn plugin_descriptor(plugin: &str, kind: ModKind) -> String {
    let text = json!({
        "FileVersion": 3,
        "Version": 1,
        "VersionName": "1.0",
        "FriendlyName": plugin,
        "Description": "",
        "Category": kind.tag(),
        "CreatedBy": "",
        "CreatedByURL": "",
        "DocsURL": "",
        "MarketplaceURL": "",
        "SupportURL": "",
        "CanContainContent": true,
        "IsBetaVersion": false,
        "IsExperimentalVersion": false,
        "Installed": false
    });
    serde_json::to_string_pretty(&text).unwrap_or_default()
}

/// Lay out the plugin the way the modkit's ContentOnly template does.
fn write_plugin_files(dir: &Path, plugin: &str, kind: ModKind, icon: &Path) -> Result<()> {
    fs::create_dir_all(dir.join("Content"))?;
    fs::create_dir_all(dir.join("Resources"))?;
    fs::write(
        dir.join(format!("{plugin}.uplugin")),
        plugin_descriptor(plugin, kind),
    )?;
    fs::copy(icon, dir.join("Resources/Icon128.png"))
        .with_context(|| format!("copying the plugin icon from {}", icon.display()))?;
    Ok(())
}

/// Create the local plugin and definition. Safe to run again if it stopped midway,
/// and refuses to touch a mod that already has a definition.
pub fn setup_plugin(workspace: &Workspace, mod_id: u64, kind: ModKind) -> Result<()> {
    require_eula(&workspace.project)?;
    let plugin = format!("UGC{mod_id}");
    let dir = workspace.project.join("Plugins").join(&plugin);
    if dir.join("Content/definition.uasset").exists() {
        bail!("{plugin} already has a definition; nothing was changed");
    }
    write_plugin_files(&dir, &plugin, kind, &workspace.engine.join(TEMPLATE_ICON))?;
    say!("plugin {plugin} created");
    create_definition(&workspace.engine, &workspace.project, &plugin, kind.class())?;
    say!("definition created ({})", kind.class());
    // A new mod is a plugin, and it's invisible in the Content Browser without this.
    if let Err(error) = show_plugin_content(&workspace.project) {
        say!("could not turn on Show Plugin Content: {error:#}");
    }
    Ok(())
}

/// The full Create button. If mod.io succeeds but local setup fails, the error
/// names the id so setup can be retried without creating a duplicate mod.
pub fn create_mod(api: &Api, workspace: &Workspace, new: &NewMod) -> Result<u64> {
    require_eula(&workspace.project)?;
    let mod_id = create_on_modio(api, &workspace.project, new)?;
    say!("created mod {mod_id} on mod.io");
    setup_plugin(workspace, mod_id, new.kind).with_context(|| {
        format!("mod {mod_id} exists on mod.io, but local setup failed; retry setup for this id")
    })?;
    Ok(mod_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_mod(name: &str, summary: &str, logo: &str) -> NewMod {
        NewMod {
            name: name.to_string(),
            summary: summary.to_string(),
            kind: ModKind::Map,
            logo: (!logo.is_empty()).then(|| PathBuf::from(logo)),
            hidden: false,
        }
    }

    #[test]
    fn validates_like_the_modkit() {
        assert!(validate(&new_mod("Dust", "A map", "logo.png")).is_ok());
        assert!(validate(&new_mod("Dust", "A map", "logo.JPEG")).is_ok());
        assert!(validate(&new_mod("ab", "A map", "logo.png")).is_err());
        assert!(validate(&new_mod(&"x".repeat(33), "A map", "logo.png")).is_err());
        assert!(validate(&new_mod("Dust", "  ", "logo.png")).is_err());
        assert!(validate(&new_mod("Dust", "A map", "logo.gif")).is_err());
        assert!(validate(&new_mod("Dust", "A map", "")).is_ok());
    }

    #[test]
    fn no_logo_uses_the_modkit_default() {
        let dir = std::env::temp_dir().join(format!("pms-logo-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        let chosen = new_mod("Dust", "A map", "mine.png");
        assert_eq!(logo_file(&dir, &chosen).unwrap(), PathBuf::from("mine.png"));
        let plain = new_mod("Dust", "A map", "");
        let missing = logo_file(&dir, &plain).unwrap_err().to_string();
        assert!(missing.contains("Choose a logo"), "{missing}");
        fs::create_dir_all(dir.join("Plugins/CustomMapTools/Resources")).unwrap();
        fs::write(dir.join(DEFAULT_LOGO), "jpg").unwrap();
        assert_eq!(logo_file(&dir, &plain).unwrap(), dir.join(DEFAULT_LOGO));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn descriptor_matches_a_content_only_plugin() {
        let text = plugin_descriptor("UGC123", ModKind::GameMode);
        let plugin: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(plugin["FriendlyName"], "UGC123");
        assert_eq!(plugin["Category"], "GameMode");
        assert_eq!(plugin["CanContainContent"], true);
        assert!(plugin.get("Modules").is_none());
    }

    #[test]
    fn mod_blob_has_the_modkit_fields() {
        let blob: Value = serde_json::from_str(&mod_blob(ModKind::Map)).unwrap();
        assert_eq!(blob["ModType"], "Map");
        assert_eq!(blob["EngineVersion"], ENGINE_VERSION);
    }
}
