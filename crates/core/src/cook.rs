use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};

use crate::assets::{
    cook_plan, remove_cooked, require_safe_selection, still_in_pak, CookPlan, Selection,
};
use crate::definition::{check_definition, mod_kind};
use crate::package::{audit_pak, pak_files};
use crate::process::{kind_of, run_tool, Failure, Tool};
use crate::targets::{find_cook, plugin_rules, Target};

const BASE_NEVER_COOK: &str = "/Game+/CustomMapTools+/JsonUtils+/VRFramework+/modio";

pub struct CookOptions {
    pub project: PathBuf,
    pub engine: PathBuf,
    pub mod_id: u64,
}

struct Paths {
    root: String,
    plugin: String,
    stage: String,
    cooker: String,
}

impl Paths {
    fn new(options: &CookOptions, excluded: &[String], left_out: &[String]) -> Self {
        let root = options.project.to_string_lossy().replace('\\', "/");
        let plugin = format!("UGC{}", options.mod_id);
        let stage = format!("{root}/Saved/Cooked/Workshop/{plugin}");
        let others: String = excluded.iter().map(|name| format!("+/{name}")).collect();
        let folders: String = left_out.iter().map(|path| format!("+{path}")).collect();
        let cooker =
            format!("-AdditionalCookerOptions=-NeverCookDir={BASE_NEVER_COOK}{others}{folders}");
        Self {
            root,
            plugin,
            stage,
            cooker,
        }
    }
}

/// PC builds use the PavlovUGCPak UAT command, which also places the pak.
fn pc_args(p: &Paths, platform: &str, folder: &str, server: bool) -> Vec<String> {
    let Paths {
        root,
        plugin,
        stage,
        cooker,
    } = p;
    let mut args = vec![
        "-Compile".to_string(),
        format!("-ScriptDir={root}/"),
        "PavlovUGCPak".to_string(),
        format!("-Project={root}/Pavlov.uproject"),
        format!("-Platform={platform}"),
        format!("-PluginPath={root}/Plugins/{plugin}/{plugin}.uplugin"),
        "-BasedOnReleaseVersion=Pavlov_1.0.0".to_string(),
        format!("-StagingDirectory={stage}"),
        "-CookFlavor=ASTC".to_string(),
        "-BuildFlavor=ASTC".to_string(),
        "-Stage=False".to_string(),
        "-ModioUploader=True".to_string(),
        format!("-MetaDataPath={stage}/metadata.json"),
        format!("-PlatformDir={folder}"),
        "-IsCodeBasedProject=False".to_string(),
        cooker.clone(),
        format!("-CookDir=/{plugin}/"),
    ];
    if server {
        args.extend(["-Server".to_string(), "-NoClient".to_string()]);
    }
    args
}

/// Quest builds use plain BuildCookRun; the pak is placed afterwards.
fn quest_args(p: &Paths, platform: &str, flavor: &str, server: bool) -> Vec<String> {
    let Paths {
        root,
        plugin,
        stage,
        cooker,
    } = p;
    let mut args = vec![
        "BuildCookRun".to_string(),
        format!("-project={root}/Pavlov.uproject"),
        format!("-dlcname={plugin}"),
        "-basedonreleaseversion=PavlovShack_1.0.0".to_string(),
        format!("-StagingDirectory={stage}"),
        format!("-targetplatform={platform}"),
        "-DLCIncludeEngineContent".to_string(),
        "-DLCPakPluginFile".to_string(),
        "-nodebuginfo".to_string(),
        "-noP4".to_string(),
        "-cook".to_string(),
        "-stage".to_string(),
        "-pak".to_string(),
        "-distribution".to_string(),
        format!("-cookflavor={flavor}"),
        cooker.clone(),
        format!("-CookDir=/{plugin}/"),
    ];
    if server {
        args.extend(["-Server".to_string(), "-NoClient".to_string()]);
    }
    args
}

/// Rename the staged pak the way the modkit does, replacing the old build.
fn place_quest_pak(p: &Paths, staged_as: &str, folder: &str) -> Result<()> {
    let plugin = &p.plugin;
    let stage = Path::new(&p.stage);
    let source = stage
        .join(staged_as)
        .join("Pavlov/Plugins")
        .join(plugin)
        .join("Content/Paks")
        .join(staged_as)
        .join(format!("{plugin}Pavlov-{staged_as}.pak"));
    if !source.is_file() {
        bail!("cook finished but the pak is missing: {}", source.display());
    }
    let dest = stage.join(folder).join(plugin);
    fs::create_dir_all(&dest)?;
    fs::rename(
        &source,
        dest.join(format!("{plugin}pakchunk0-{staged_as}.pak")),
    )?;
    fs::copy(stage.join("metadata.json"), dest.join("metadata.json"))?;
    Ok(())
}

fn run_uat(engine: &Path, args: &[String], log: &Path, target: &str) -> Result<()> {
    let tool = Tool {
        name: "RunUAT",
        log: Some(log),
        cancellable: true,
        keep_stdout: false,
    };
    let bat = engine.join("Build/BatchFiles/RunUAT.bat");
    // Windows starts a batch file through cmd, which would hide a wrong path behind an exit code.
    if !bat.is_file() {
        bail!("could not find RunUAT.bat; check the engine path");
    }
    let run = run_tool(&tool, Command::new(bat).args(args))?;
    if !run.success {
        let code = run.exit_code.unwrap_or(-1);
        let fallback = format!("RunUAT stopped with exit code {code}.");
        let kind = kind_of(&run.errors, &run.tail);
        return Err(run.failure(kind, Some(target), &tool, &fallback).into());
    }
    Ok(())
}

/// Cut single left-out assets from the cooked files, then package again without cooking.
/// A whole folder never reaches this step, because the cooker already skipped it.
fn apply_file_selection(
    options: &CookOptions,
    target: Target,
    plan: &CookPlan,
    args: &[String],
    log: &Path,
) -> Result<()> {
    if plan.files.is_empty() {
        return Ok(());
    }
    let removed = remove_cooked(
        &options.project,
        options.mod_id,
        target.cooked_folder(),
        &plan.files,
    )?;
    crate::say!(
        "[{}] removed {removed} cooked files, packaging again",
        target.name()
    );
    let mut package = args.to_vec();
    package.push("-SkipCook".to_string());
    run_uat(&options.engine, &package, log, target.name())
}

/// Prove the pak matches what the modder chose, rather than trusting the steps above.
fn verify_selection(
    options: &CookOptions,
    pak: &Path,
    selection: &Selection,
    target: Target,
) -> Result<()> {
    if selection.excluded.is_empty() {
        return Ok(());
    }
    let files = pak_files(&options.engine, pak)?;
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    let left = still_in_pak(options.mod_id, &names, &selection.excluded);
    if let Some(first) = left.first() {
        return Err(Failure {
            kind: "audit".to_string(),
            target: Some(target.name().to_string()),
            tool: "UnrealPak.exe".to_string(),
            summary: format!(
                "{} assets you left out are still in the build, such as {first}. Nothing was uploaded.",
                left.len()
            ),
            errors: left.iter().take(20).map(|path| path.to_string()).collect(),
            exit_code: None,
            log_path: None,
        }
        .into());
    }
    crate::say!(
        "[{}] confirmed {} assets are left out",
        target.name(),
        selection.excluded.len()
    );
    Ok(())
}

/// The small file the modkit puts beside each pak.
fn write_metadata(options: &CookOptions, stage: &Path) -> Result<()> {
    let definition = options
        .project
        .join("Plugins")
        .join(format!("UGC{}", options.mod_id))
        .join("Content/definition.uasset");
    let mod_type = match mod_kind(&fs::read(definition)?) {
        "Map" => 0,
        "Mod" => 1,
        "GameMode" => 2,
        other => bail!("cannot cook a definition of type {other}"),
    };
    fs::create_dir_all(stage)?;
    let text = format!("{{\r\n\t\"EngineVersion\": \"5.1.1\",\r\n\t\"ModType\": {mod_type}\r\n}}");
    fs::write(stage.join("metadata.json"), text)?;
    Ok(())
}

/// Build the PC client into a folder for offline testing, without touching mod.io.
pub fn stage_local(options: &CookOptions, folder: &Path) -> Result<()> {
    check_definition(&options.project, options.mod_id, &[Target::Windows])?;
    let rules = plugin_rules(&options.project, options.mod_id)?;
    let selection = require_safe_selection(&options.project, options.mod_id)?;
    let plan = cook_plan(&options.project, options.mod_id, &selection)?;
    let mut paths = Paths::new(options, &rules.excluded, &plan.folders);
    paths.stage = folder.to_string_lossy().replace('\\', "/");
    write_metadata(options, folder)?;
    let args: Vec<String> = pc_args(&paths, "Windows", "Windows", false)
        .into_iter()
        .map(|arg| match arg.as_str() {
            "-Stage=False" => "-Stage=True".to_string(),
            "-ModioUploader=True" => "-ModioUploader=False".to_string(),
            _ => arg,
        })
        .collect();
    let log = options.project.join("Saved/Logs/pavlov_stage.log");
    crate::progress::stage("cook", Some("windows"));
    run_uat(&options.engine, &args, &log, "windows")?;
    apply_file_selection(options, Target::Windows, &plan, &args, &log)?;

    let data = folder.join(&paths.plugin).join("Data");
    let pak = fs::read_dir(&data)
        .with_context(|| format!("staging did not create {}", data.display()))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .find(|path| path.extension().is_some_and(|e| e == "pak"))
        .context("staging finished without a pak")?;
    let files = audit_pak(&options.engine, &pak, &rules.allowed)?;
    verify_selection(options, &pak, &selection, Target::Windows)?;
    crate::say!("Staged mod, {files} files checked: {}", data.display());
    Ok(())
}

/// Cook each target in order. The first failure stops the run, like the modkit.
pub fn cook(options: &CookOptions, targets: &[Target]) -> Result<()> {
    check_definition(&options.project, options.mod_id, targets)?;
    let rules = plugin_rules(&options.project, options.mod_id)?;
    crate::say!(
        "excluding {} other mods from the build",
        rules.excluded.len()
    );
    let selection = require_safe_selection(&options.project, options.mod_id)?;
    let plan = cook_plan(&options.project, options.mod_id, &selection)?;
    if !selection.excluded.is_empty() {
        crate::say!(
            "leaving out {} assets: {} folders, {} single files",
            selection.excluded.len(),
            plan.folders.len(),
            plan.files.len()
        );
    }
    let paths = Paths::new(options, &rules.excluded, &plan.folders);
    write_metadata(options, Path::new(&paths.stage))?;
    for &target in targets {
        let name = target.name();
        let log = options
            .project
            .join(format!("Saved/Logs/pavlov_cook_{name}.log"));
        crate::progress::check_cancel()?;
        crate::say!("[{name}] cooking ...");
        crate::progress::stage("cook", Some(name));
        let started = Instant::now();
        let args = match target {
            Target::Windows => pc_args(&paths, "Windows", "Windows", false),
            Target::Linux => pc_args(&paths, "Linux", "LinuxPavServer", true),
            Target::Android => quest_args(&paths, "Android", "ASTC", false),
            Target::Shack => quest_args(&paths, "Linux", "", true),
        };
        run_uat(&options.engine, &args, &log, name)?;
        apply_file_selection(options, target, &plan, &args, &log)?;
        match target {
            Target::Windows | Target::Linux => {}
            Target::Android => place_quest_pak(&paths, "Android_ASTC", "Android_ASTC")?,
            Target::Shack => place_quest_pak(&paths, "LinuxServer", "LinuxShackServer")?,
        }
        let built = find_cook(&options.project, options.mod_id, target)?;
        let files = audit_pak(&options.engine, &built.pak, &rules.allowed)?;
        verify_selection(options, &built.pak, &selection, target)?;
        crate::say!(
            "[{name}] cooked in {}s, {files} files checked",
            started.elapsed().as_secs()
        );
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::run_uat;
    use crate::process::Failure;

    fn engine(name: &str, script: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("pms-uat-{name}-{}", std::process::id()));
        let batch = root.join("Build/BatchFiles");
        std::fs::create_dir_all(&batch).unwrap();
        std::fs::write(batch.join("RunUAT.bat"), script).unwrap();
        root
    }

    #[test]
    fn a_failed_cook_is_reported_as_a_cook_failure() {
        let script = "@echo off\r\necho LogCook: Error: Missing /UGC1/Map\r\nexit /b 25\r\n";
        let root = engine("cook", script);
        let log = root.join("Saved/Logs/cook.log");
        let error = run_uat(&root, &[], &log, "linux").unwrap_err();
        let failure = error.downcast_ref::<Failure>().unwrap();
        assert_eq!(failure.kind, "cook");
        assert_eq!(failure.target.as_deref(), Some("linux"));
        assert_eq!(failure.exit_code, Some(25));
        assert_eq!(failure.summary, "LogCook: Error: Missing /UGC1/Map");
        assert!(log.is_file());
    }

    #[test]
    fn a_failed_script_build_is_reported_as_a_compile_failure() {
        let script = "@echo off\r\necho A.cs(1,1): error CS1002: ; expected\r\nexit /b 1\r\n";
        let root = engine("compile", script);
        let error = run_uat(&root, &[], &root.join("c.log"), "windows").unwrap_err();
        assert_eq!(error.downcast_ref::<Failure>().unwrap().kind, "compile");
    }

    #[test]
    fn a_failure_with_no_error_lines_still_explains_itself() {
        let root = engine("silent", "@echo off\r\nexit /b 7\r\n");
        let error = run_uat(&root, &[], &root.join("c.log"), "android").unwrap_err();
        let failure = error.downcast_ref::<Failure>().unwrap();
        assert!(
            failure.summary.contains("exit code 7"),
            "{}",
            failure.summary
        );
    }

    #[test]
    fn a_missing_engine_says_to_check_the_path() {
        let root = std::env::temp_dir().join("pms-uat-no-such-engine");
        let error = run_uat(&root, &[], &root.join("logs/c.log"), "windows").unwrap_err();
        assert!(format!("{error:#}").contains("check the engine path"));
    }
}
