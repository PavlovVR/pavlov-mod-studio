use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Windows,
    Linux,
    Android,
    Shack,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Target::Windows => "windows",
            Target::Linux => "linux",
            Target::Android => "android",
            Target::Shack => "shack",
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Target::Windows => "Windows",
            Target::Linux => "LinuxPavServer",
            Target::Android => "Android_ASTC",
            Target::Shack => "LinuxShackServer",
        }
    }

    fn pak_suffix(self) -> &'static str {
        match self {
            Target::Windows => "Windows",
            Target::Linux | Target::Shack => "LinuxServer",
            Target::Android => "Android_ASTC",
        }
    }

    /// Where Unreal puts this build's cooked files inside the plugin.
    pub fn cooked_folder(self) -> &'static str {
        match self {
            Target::Windows => "Windows",
            Target::Linux | Target::Shack => "LinuxServer",
            Target::Android => "Android_ASTC",
        }
    }

    /// Platform name on mod.io. The Shack server is published under "mac".
    pub fn platform(self) -> &'static str {
        match self {
            Target::Windows => "windows",
            Target::Linux => "linux",
            Target::Android => "oculus",
            Target::Shack => "mac",
        }
    }
}

/// Each choice is a client plus its server, in the modkit's order.
pub fn group(name: &str) -> Result<&'static [Target]> {
    Ok(match name {
        "pc" => &[Target::Windows, Target::Linux],
        "quest" => &[Target::Android, Target::Shack],
        "both" => &[
            Target::Windows,
            Target::Linux,
            Target::Android,
            Target::Shack,
        ],
        other => bail!("unknown group '{other}'; use pc, quest or both"),
    })
}

/// Which plugins may end up inside a mod's pak.
pub struct PluginRules {
    /// Other mods: never cooked into this mod.
    pub excluded: Vec<String>,
    /// This mod, plus shared `[SOURCE]` plugins meant to be built into it.
    pub allowed: Vec<String>,
}

/// Same outcome as the modkit: every other UGC plugin is excluded, except source plugins.
pub fn plugin_rules(project: &Path, mod_id: u64) -> Result<PluginRules> {
    let this_mod = format!("UGC{mod_id}");
    let mut rules = PluginRules {
        excluded: Vec::new(),
        allowed: vec![this_mod.clone()],
    };
    for entry in fs::read_dir(project.join("Plugins")).context("reading the Plugins folder")? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let descriptor = project
            .join("Plugins")
            .join(&name)
            .join(format!("{name}.uplugin"));
        if !name.contains("UGC") || name == this_mod || !descriptor.is_file() {
            continue;
        }
        let text = fs::read_to_string(&descriptor)?;
        let plugin: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("reading {}", descriptor.display()))?;
        let is_source = plugin["FriendlyName"]
            .as_str()
            .is_some_and(|n| n.starts_with("[SOURCE]"));
        if is_source {
            rules.allowed.push(name);
        } else {
            rules.excluded.push(name);
        }
    }
    if !project.join("Plugins").join(&this_mod).is_dir() {
        bail!("cannot find plugin {this_mod} in {}", project.display());
    }
    rules.excluded.sort();
    Ok(rules)
}

pub struct Cook {
    pub pak: PathBuf,
    pub metadata: PathBuf,
}

pub fn find_cook(project: &Path, mod_id: u64, target: Target) -> Result<Cook> {
    let dir = project
        .join("Saved/Cooked/Workshop")
        .join(format!("UGC{mod_id}"))
        .join(target.folder())
        .join(format!("UGC{mod_id}"));
    let pak = dir.join(format!("UGC{mod_id}pakchunk0-{}.pak", target.pak_suffix()));
    let metadata = dir.join("metadata.json");
    for file in [&pak, &metadata] {
        if !file.is_file() {
            bail!(
                "missing cook output: {} (cook in the editor first)",
                file.display()
            );
        }
    }
    Ok(Cook { pak, metadata })
}
