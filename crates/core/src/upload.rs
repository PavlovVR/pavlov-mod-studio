use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::multipart::Form;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{text_form, Api, GAME_ID};
use crate::definition::mod_kind;
use crate::manage::make_live;
use crate::package::{audit_pak, build_zip, sha256};
use crate::progress::{check_cancel, emit, Event};
use crate::share::{mark_shared, share_zip};
use crate::targets::{find_cook, plugin_rules, Target};

/// mod.io requires exactly 50 MiB per part, except the last.
pub const PART_SIZE: u64 = 50 * 1024 * 1024;
const DUPLICATE_RANGE: i64 = 29015;
const PROCESSING_LIMIT: Duration = Duration::from_secs(1800);

pub struct Options {
    pub project: PathBuf,
    pub engine: PathBuf,
    pub mod_id: u64,
    pub version: Option<String>,
    pub changelog: String,
    pub inactive: bool,
    pub compress: bool,
    pub skip_existing: bool,
    /// Upload everything hidden, then make all platforms live at the end.
    pub together: bool,
    pub part_size: u64,
    pub poll_seconds: u64,
    /// Only for tests with fake paks. Real uploads always audit.
    pub skip_audit: bool,
}

#[derive(Serialize, Deserialize)]
struct State {
    upload_id: String,
    sha256: String,
    size: u64,
    part_size: u64,
    done: BTreeSet<u64>,
}

impl State {
    fn load(path: &Path, sha256: &str, size: u64, part_size: u64) -> Option<Self> {
        let state: State = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        let same = state.sha256 == sha256 && state.size == size && state.part_size == part_size;
        same.then_some(state)
    }

    fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

struct ModBlob {
    name: String,
    mod_type: String,
    engine_version: String,
    version: i64,
}

impl ModBlob {
    /// The text the modkit attaches to each file.
    fn file_text(&self) -> String {
        format!(
            "{{\r\n\t\"ModType\": {},\r\n\t\"EngineVersion\": {}\r\n}}",
            Value::from(self.mod_type.as_str()),
            Value::from(self.engine_version.as_str())
        )
    }

    /// The text the modkit stores on the mod itself.
    fn mod_text(&self, version: i64) -> String {
        format!(
            "{{\r\n\t\"ModType\": {},\r\n\t\"EngineVersion\": {},\r\n\t\"VersionUUID\": {version}\r\n}}",
            Value::from(self.mod_type.as_str()),
            Value::from(self.engine_version.as_str())
        )
    }
}

/// Zip every build first, so a missing cook can't leave a half-finished upload.
pub fn prepare(options: &Options, targets: &[Target]) -> Result<Vec<(Target, PathBuf)>> {
    let rules = plugin_rules(&options.project, options.mod_id)?;
    let mut zips = Vec::new();
    for &target in targets {
        let cook = find_cook(&options.project, options.mod_id, target)?;
        if !options.skip_audit {
            audit_pak(&options.engine, &cook.pak, &rules.allowed)?;
        }
        let kind = if options.compress { "upload" } else { "stored" };
        let parent = cook
            .pak
            .parent()
            .and_then(Path::parent)
            .context("bad cook path")?;
        let zip = parent.join(format!("{}.{kind}.zip", target.name()));
        build_zip(&cook, &zip, options.compress)?;
        let size = fs::metadata(&zip)?.len();
        crate::say!(
            "[{}] {size} bytes, {} parts, platform '{}'",
            target.name(),
            size.div_ceil(options.part_size),
            target.platform()
        );
        zips.push((target, zip));
    }
    Ok(zips)
}

/// The modkit's next version: the highest version on mod.io plus one.
pub fn next_version(api: &Api, mod_id: u64) -> Result<i64> {
    let path = format!("/games/{GAME_ID}/mods/{mod_id}/files");
    let files = api.send("list files", &[], || {
        api.client()
            .get(api.url(&path))
            .query(&[("_limit", "100"), ("_sort", "-id")])
    })?;
    let empty = Vec::new();
    let highest = files["data"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|file| file["version"].as_str()?.parse::<i64>().ok())
        .max()
        .unwrap_or(0);
    Ok(highest.max(0) + 1)
}

/// Store the version on the mod, which the modkit does before cooking.
pub fn set_mod_version(api: &Api, mod_id: u64, version: i64) -> Result<()> {
    let mod_path = format!("/games/{GAME_ID}/mods/{mod_id}");
    let blob = read_mod_blob(api, &mod_path)?;
    if blob.version != version {
        let text = blob.mod_text(version);
        api.send("update mod", &[], || {
            api.client()
                .post(api.url(&mod_path))
                .multipart(text_form(&[("metadata_blob", text.as_str())]))
        })?;
        crate::say!("Successfully updated mod metadata");
    }
    Ok(())
}

pub fn publish(api: &Api, options: &Options, zips: &[(Target, PathBuf)]) -> Result<()> {
    let mod_path = format!("/games/{GAME_ID}/mods/{}", options.mod_id);
    let blob = read_mod_blob(api, &mod_path)?;
    let version = match &options.version {
        Some(version) => {
            let number: i64 = version
                .parse()
                .context("--version must be a whole number")?;
            set_mod_version(api, options.mod_id, number)?;
            version.clone()
        }
        None => blob.version.to_string(),
    };
    crate::say!("version {version}");

    let mut uploaded = Vec::new();
    for (index, (target, zip)) in zips.iter().enumerate() {
        if index > 0 {
            sleep(Duration::from_secs(1)); // the modkit waits a second between platforms
        }
        let id = upload_target(
            api,
            options,
            &mod_path,
            (target.name(), target.platform()),
            zip,
            &version,
            &blob,
        )?;
        uploaded.extend(id);
    }
    if options.together && !options.inactive {
        crate::progress::stage("go-live", None);
        for id in uploaded {
            make_live(api, options.mod_id, id)?;
        }
    }
    crate::say!("Upload Complete");
    Ok(())
}

/// Share what the modder chose on mod.io, for content mods only like the modkit.
pub fn upload_source(api: &Api, options: &Options) -> Result<()> {
    let plugin = format!("UGC{}", options.mod_id);
    let dir = options.project.join("Plugins").join(&plugin);
    let bytes = fs::read(dir.join("Content/definition.uasset"))
        .context("cannot find this mod's definition")?;
    if mod_kind(&bytes) != "Mod" {
        bail!("Cannot distribute Maps or GameModes, this feature is for Content Mods.");
    }
    let stage = options.project.join("Saved/Cooked/Workshop").join(&plugin);
    fs::create_dir_all(&stage)?;
    let zip = stage.join("source.upload.zip");
    let mod_path = format!("/games/{GAME_ID}/mods/{}", options.mod_id);
    let blob = read_mod_blob(api, &mod_path)?;
    let made = share_zip(&options.project, options.mod_id, &blob.name, &zip)?;
    crate::say!(
        "[source] {} files, {} bytes, checked against your list",
        made.files,
        made.bytes
    );
    let version = options
        .version
        .clone()
        .unwrap_or_else(|| blob.version.to_string());
    let uploaded = upload_target(
        api,
        options,
        &mod_path,
        ("source", "source"),
        &zip,
        &version,
        &blob,
    )?;
    // Everything else has gone live by now, and this would otherwise stay hidden for good.
    if options.together && !options.inactive {
        if let Some(id) = uploaded {
            make_live(api, options.mod_id, id)?;
        }
    }
    mark_shared(&options.project, options.mod_id)?;
    Ok(())
}

/// Share without building anything, for when only the shared files changed.
pub fn share_now(api: &Api, project: &Path, mod_id: u64, changelog: String) -> Result<()> {
    let options = Options {
        project: project.to_path_buf(),
        engine: PathBuf::new(),
        mod_id,
        version: None,
        changelog,
        inactive: false,
        compress: true,
        skip_existing: false,
        together: false,
        part_size: PART_SIZE,
        poll_seconds: 10,
        skip_audit: true,
    };
    crate::progress::stage("package", None);
    upload_source(api, &options)?;
    crate::say!("Upload Complete");
    Ok(())
}

fn read_mod_blob(api: &Api, mod_path: &str) -> Result<ModBlob> {
    let details = api.send("read mod", &[], || api.client().get(api.url(mod_path)))?;
    let text = details["metadata_blob"].as_str().unwrap_or("{}");
    let blob: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let field = |name: &str| blob[name].as_str().map(str::to_string);
    let missing =
        || anyhow!("mod metadata_blob is missing fields; update the mod from the editor first");
    Ok(ModBlob {
        name: details["name"].as_str().unwrap_or_default().to_string(),
        mod_type: field("ModType").ok_or_else(missing)?,
        engine_version: field("EngineVersion").ok_or_else(missing)?,
        version: blob["VersionUUID"].as_i64().ok_or_else(missing)?,
    })
}

fn already_uploaded(
    api: &Api,
    mod_path: &str,
    version: &str,
    platform: &str,
) -> Result<Option<i64>> {
    let path = format!("{mod_path}/files");
    let files = api.send("list files", &[], || {
        api.client()
            .get(api.url(&path))
            .query(&[("_limit", "100"), ("_sort", "-id")])
    })?;
    let empty = Vec::new();
    Ok(files["data"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .find_map(|file| {
            let same_version = file["version"].as_str() == Some(version);
            let same_platform = file["platforms"].as_array().is_some_and(|list| {
                list.iter()
                    .any(|p| p["platform"].as_str() == Some(platform))
            });
            (same_version && same_platform)
                .then(|| file["id"].as_i64())
                .flatten()
        }))
}

fn upload_target(
    api: &Api,
    options: &Options,
    mod_path: &str,
    (name, platform): (&str, &str),
    zip: &Path,
    version: &str,
    blob: &ModBlob,
) -> Result<Option<i64>> {
    if options.skip_existing {
        if let Some(id) = already_uploaded(api, mod_path, version, platform)? {
            crate::say!("[{name}] skipped: version {version} is already on mod.io as file {id}");
            return Ok(None);
        }
    }

    let size = fs::metadata(zip)?.len();
    let parts = size.div_ceil(options.part_size);
    let digest = sha256(zip)?;
    let state_path = zip.with_extension("state.json");
    let mut state = match State::load(&state_path, &digest, size, options.part_size) {
        Some(state) => {
            crate::say!(
                "[{name}] resuming: {}/{parts} parts already sent",
                state.done.len()
            );
            state
        }
        None => {
            let filename = format!("modfile_{}.zip", options.mod_id);
            let path = format!("{mod_path}/files/multipart");
            let session = api.send("create session", &[], || {
                api.client()
                    .post(api.url(&path))
                    .form(&[("filename", filename.as_str()), ("nonce", digest.as_str())])
            })?;
            let upload_id = session["upload_id"]
                .as_str()
                .context("no upload_id returned")?
                .to_string();
            let state = State {
                upload_id,
                sha256: digest,
                size,
                part_size: options.part_size,
                done: BTreeSet::new(),
            };
            state.save(&state_path)?;
            state
        }
    };

    crate::progress::stage("upload", Some(name));
    upload_parts(api, mod_path, name, zip, &mut state, &state_path, parts)?;

    let upload_id = state.upload_id.clone();
    if session_status(api, mod_path, &upload_id)? == Some(0) {
        let path = format!("{mod_path}/files/multipart/complete");
        api.send("complete upload", &[], || {
            api.client()
                .post(api.url(&path))
                .query(&[("upload_id", upload_id.as_str())])
        })?;
    }
    wait_for_session(api, mod_path, &upload_id, options.poll_seconds)?;

    let path = format!("{mod_path}/files");
    let hidden = options.inactive || options.together;
    let active = if hidden { "false" } else { "true" };
    let text = blob.file_text();
    // This endpoint only accepts multipart/form-data.
    let modfile = api.send("attach file", &[], || {
        let mut form = Form::new()
            .text("upload_id", upload_id.clone())
            .text("version", version.to_string())
            .text("active", active)
            .text("platforms[]", platform.to_string())
            .text("metadata_blob", text.clone());
        if !options.changelog.is_empty() {
            form = form.text("changelog", options.changelog.clone());
        }
        api.client().post(api.url(&path)).multipart(form)
    })?;
    fs::remove_file(&state_path).ok();
    crate::say!(
        "[{name}] done: modfile {}, version {version}, platform {platform}",
        modfile["id"]
    );
    Ok(modfile["id"].as_i64())
}

fn upload_parts(
    api: &Api,
    mod_path: &str,
    target: &str,
    zip: &Path,
    state: &mut State,
    state_path: &Path,
    parts: u64,
) -> Result<()> {
    let path = format!("{mod_path}/files/multipart");
    let mut file = File::open(zip)?;
    let report = |done: u64| {
        emit(Event::Parts {
            target: target.to_string(),
            done,
            total: parts,
        });
    };
    report(state.done.len() as u64);
    for index in 0..parts {
        if state.done.contains(&index) {
            continue;
        }
        check_cancel()?;
        let start = index * state.part_size;
        let end = (start + state.part_size).min(state.size) - 1;
        let mut body = vec![0; (end - start + 1) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut body)?;
        let range = format!("bytes {start}-{end}/{}", state.size);
        let began = Instant::now();
        let result = api.send("upload part", &[DUPLICATE_RANGE], || {
            api.client()
                .put(api.url(&path))
                .query(&[("upload_id", state.upload_id.as_str())])
                .header("Content-Range", range.as_str())
                .body(body.clone())
        })?;
        let note = if result["error_ref"].as_i64() == Some(DUPLICATE_RANGE) {
            "already on server".to_string()
        } else {
            format!("{}s", began.elapsed().as_secs())
        };
        state.done.insert(index);
        state.save(state_path)?;
        report(state.done.len() as u64);
        crate::say!("part {}/{parts} ok ({note})", index + 1);
    }
    Ok(())
}

fn session_status(api: &Api, mod_path: &str, upload_id: &str) -> Result<Option<i64>> {
    let path = format!("{mod_path}/files/multipart/sessions");
    let sessions = api.send("list sessions", &[], || api.client().get(api.url(&path)))?;
    let empty = Vec::new();
    Ok(sessions["data"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .find(|s| s["upload_id"].as_str() == Some(upload_id))
        .and_then(|s| s["status"].as_i64()))
}

fn wait_for_session(api: &Api, mod_path: &str, upload_id: &str, poll_seconds: u64) -> Result<()> {
    let started = Instant::now();
    while started.elapsed() < PROCESSING_LIMIT {
        match session_status(api, mod_path, upload_id)? {
            Some(3) | None => return Ok(()), // finished sessions may drop off the list
            Some(4) => bail!("mod.io cancelled the upload session"),
            Some(status) => crate::say!("waiting for mod.io to process the file (status {status})"),
        }
        sleep(Duration::from_secs(poll_seconds));
    }
    bail!("timed out waiting for mod.io to process the file")
}
