use std::fs;
use std::path::PathBuf;

use pavlov_mod_core::api::{Api, API_BASE};
use pavlov_mod_core::assets::{
    analyze_assets, asset_report, assets_in_pak, check_selection, save_selection, AssetReport,
    MissingReference, Selection,
};
use pavlov_mod_core::community::{self, CommentPage, ModEvent, TeamMember};
use pavlov_mod_core::cook::{stage_local, CookOptions};
use pavlov_mod_core::create::{create_mod, setup_plugin, ModKind, NewMod, Workspace};
use pavlov_mod_core::editor::{editor_state, restart_editor, save_work, EditorState};
use pavlov_mod_core::library::{
    check_folders as read_folders, check_token, load_library, local_mods, read_mod, AccessMemory,
    FolderCheck, Library, LocalMod, ModInfo,
};
use pavlov_mod_core::limits;
use pavlov_mod_core::manage::{
    self, list_files, make_live, set_logo, FilePage, MediaList, MetadataPair, ModEdit,
};
use pavlov_mod_core::package::{analyze_pak, pak_files, PakReport};
use pavlov_mod_core::pipeline::{self, discard_pending, pending_version, Release};
use pavlov_mod_core::process::Failure;
use pavlov_mod_core::progress::{clear_cancel, emit, request_cancel, set_sink, Event};
use pavlov_mod_core::source::{browse, download, SourcePage};
use pavlov_mod_core::targets::{find_cook, group, Target};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const DEFAULT_ENGINE: &str = r"C:\Program Files\Epic Games\UE_5.1\Engine";

#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    project: String,
    engine: String,
    #[serde(default)]
    stage: String,
    /// Only needed with a personal token, which can't ask mod.io who it belongs to.
    #[serde(default)]
    username: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            project: String::new(),
            engine: DEFAULT_ENGINE.to_string(),
            stage: String::new(),
            username: String::new(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(text)?;
    fs::create_dir_all(&dir).map_err(text)?;
    Ok(dir.join("settings.json"))
}

fn text(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

/// The token is encrypted with Windows DPAPI and kept in the app's config folder.
/// Credential Manager rejects secrets over 2560 bytes, and a mod.io token passes
/// that once stored as UTF-16.
fn token_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(text)?;
    fs::create_dir_all(&dir).map_err(text)?;
    Ok(dir.join("token.bin"))
}

/// What mod.io has said about this account. It belongs to the token, so it goes with it.
fn access_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(text)?;
    Ok(dir.join("access.json"))
}

fn read_token(app: &AppHandle) -> Result<String, String> {
    let sealed = fs::read(token_path(app)?)
        .map_err(|_| "Add your mod.io token in Settings first.".to_string())?;
    let plain = unprotect(&sealed)?;
    String::from_utf8(plain)
        .map_err(|_| "The saved mod.io token is damaged. Add it again in Settings.".to_string())
}

fn api(app: &AppHandle) -> Result<Api, String> {
    let token = read_token(app)?;
    Api::new(API_BASE, &token).map_err(text)
}

/// Run a request someone is waiting on, off the main thread and without long retries.
async fn quick<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&Api) -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let api = Api::quick(API_BASE, &read_token(&app)?).map_err(text)?;
        work(&api).map_err(text)
    })
    .await
    .map_err(text)?
}

#[cfg(windows)]
fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            return Err(format!(
                "could not encrypt the token: {}",
                std::io::Error::last_os_error()
            ));
        }
        let sealed = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Ok(sealed)
    }
}

#[cfg(windows)]
fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: sealed.len() as u32,
            pbData: sealed.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let mut description = std::ptr::null_mut();
        let ok = CryptUnprotectData(
            &input,
            &mut description,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if !description.is_null() {
            LocalFree(description as _);
        }
        if ok == 0 {
            return Err("A saved login could not be read. Add it again in Settings.".to_string());
        }
        let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Ok(plain)
    }
}

#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("saving a token is only supported on Windows".to_string())
}

#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("saving a token is only supported on Windows".to_string())
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<Settings, String> {
    let path = settings_path(&app)?;
    Ok(fs::read_to_string(path)
        .ok()
        .and_then(|saved| serde_json::from_str(&saved).ok())
        .unwrap_or_default())
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let text_out = serde_json::to_string_pretty(&settings).map_err(text)?;
    fs::write(settings_path(&app)?, text_out).map_err(text)
}

#[tauri::command]
fn check_folders(project: String, engine: String) -> FolderCheck {
    read_folders(&PathBuf::from(project), &PathBuf::from(engine))
}

#[tauri::command]
fn has_token(app: AppHandle) -> bool {
    read_token(&app).is_ok()
}

#[tauri::command]
fn save_token(app: AppHandle, token: String) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("The token is empty.".to_string());
    }
    fs::write(token_path(&app)?, protect(token.as_bytes())?).map_err(text)?;
    fs::remove_file(access_path(&app)?).ok();
    Ok(())
}

#[tauri::command]
fn remove_token(app: AppHandle) -> Result<(), String> {
    fs::remove_file(access_path(&app)?).ok();
    match fs::remove_file(token_path(&app)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(text(error)),
    }
}

#[tauri::command]
async fn check_mod_token(token: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let api = Api::quick(API_BASE, token.trim()).map_err(text)?;
        check_token(&api).map_err(text)
    })
    .await
    .map_err(text)?
}

#[tauri::command]
fn list_mods(project: String) -> Result<Vec<LocalMod>, String> {
    local_mods(&PathBuf::from(project)).map_err(text)
}

#[tauri::command]
async fn library(app: AppHandle, ids: Vec<u64>, username: String) -> Result<Library, String> {
    let path = access_path(&app)?;
    quick(app, move |api| {
        let mut memory: AccessMemory = fs::read_to_string(&path)
            .ok()
            .and_then(|saved| serde_json::from_str(&saved).ok())
            .unwrap_or_default();
        let library = load_library(api, &ids, Some(&username), &mut memory);
        fs::write(&path, serde_json::to_string(&memory)?).ok();
        library
    })
    .await
}

/// What this app has used of mod.io's limits. This doesn't ask mod.io anything.
#[tauri::command]
fn api_usage() -> limits::Usage {
    limits::usage()
}

#[tauri::command]
async fn mod_info(app: AppHandle, mod_id: u64) -> Result<ModInfo, String> {
    quick(app, move |api| read_mod(api, mod_id)).await
}

#[derive(Deserialize)]
struct CreateRequest {
    name: String,
    summary: String,
    kind: String,
    logo: String,
}

#[tauri::command]
async fn create_new_mod(app: AppHandle, request: CreateRequest) -> Result<u64, String> {
    let settings = load_settings(app.clone())?;
    tauri::async_runtime::spawn_blocking(move || {
        let new = NewMod {
            name: request.name,
            summary: request.summary,
            kind: ModKind::parse(&request.kind).map_err(text)?,
            logo: PathBuf::from(request.logo),
            hidden: false,
        };
        let workspace = Workspace {
            project: PathBuf::from(settings.project),
            engine: PathBuf::from(settings.engine),
        };
        create_mod(&api(&app)?, &workspace, &new).map_err(text)
    })
    .await
    .map_err(text)?
}

#[tauri::command]
async fn retry_setup(app: AppHandle, mod_id: u64, kind: String) -> Result<(), String> {
    let settings = load_settings(app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let workspace = Workspace {
            project: PathBuf::from(settings.project),
            engine: PathBuf::from(settings.engine),
        };
        setup_plugin(&workspace, mod_id, ModKind::parse(&kind).map_err(text)?).map_err(text)
    })
    .await
    .map_err(text)?
}

#[tauri::command]
async fn browse_source(
    app: AppHandle,
    search: String,
    sort: String,
    offset: u64,
) -> Result<SourcePage, String> {
    quick(app, move |api| browse(api, &search, &sort, offset, 24)).await
}

#[tauri::command]
async fn download_source(app: AppHandle, mod_id: u64) -> Result<(), String> {
    let settings = load_settings(app.clone())?;
    tauri::async_runtime::spawn_blocking(move || {
        download(&api(&app)?, &PathBuf::from(settings.project), mod_id).map_err(text)
    })
    .await
    .map_err(text)?
}

#[tauri::command]
async fn mod_files(
    app: AppHandle,
    mod_id: u64,
    offset: u64,
    live: Vec<i64>,
) -> Result<FilePage, String> {
    quick(app, move |api| list_files(api, mod_id, offset, &live)).await
}

#[tauri::command]
async fn publish_file(app: AppHandle, mod_id: u64, file_id: i64) -> Result<(), String> {
    quick(app, move |api| make_live(api, mod_id, file_id)).await
}

#[tauri::command]
async fn update_file(
    app: AppHandle,
    mod_id: u64,
    file_id: i64,
    version: Option<String>,
    changelog: Option<String>,
) -> Result<(), String> {
    quick(app, move |api| {
        manage::edit_file(
            api,
            mod_id,
            file_id,
            version.as_deref(),
            changelog.as_deref(),
        )
    })
    .await
}

#[tauri::command]
async fn remove_file(app: AppHandle, mod_id: u64, file_id: i64) -> Result<(), String> {
    quick(app, move |api| manage::delete_file(api, mod_id, file_id)).await
}

#[tauri::command]
async fn update_mod(
    app: AppHandle,
    mod_id: u64,
    edit: ModEdit,
    logo: Option<String>,
) -> Result<(), String> {
    quick(app, move |api| {
        manage::edit_mod(api, mod_id, &edit)?;
        if let Some(logo) = logo.filter(|path| !path.is_empty()) {
            set_logo(api, mod_id, &PathBuf::from(logo))?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn close_mod(app: AppHandle, mod_id: u64) -> Result<(), String> {
    quick(app, move |api| manage::delete_mod(api, mod_id)).await
}

#[tauri::command]
async fn add_images(app: AppHandle, mod_id: u64, paths: Vec<String>) -> Result<(), String> {
    quick(app, move |api| {
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        manage::add_images(api, mod_id, &paths)
    })
    .await
}

#[tauri::command]
async fn add_links(
    app: AppHandle,
    mod_id: u64,
    youtube: Vec<String>,
    sketchfab: Vec<String>,
) -> Result<(), String> {
    quick(app, move |api| {
        manage::add_links(api, mod_id, &youtube, &sketchfab)
    })
    .await
}

#[tauri::command]
async fn remove_media(app: AppHandle, mod_id: u64, media: MediaList) -> Result<(), String> {
    quick(app, move |api| manage::delete_media(api, mod_id, &media)).await
}

#[tauri::command]
async fn order_media(app: AppHandle, mod_id: u64, media: MediaList) -> Result<(), String> {
    quick(app, move |api| manage::reorder_media(api, mod_id, &media)).await
}

#[tauri::command]
async fn mod_dependencies(app: AppHandle, mod_id: u64) -> Result<Vec<ModInfo>, String> {
    quick(app, move |api| manage::dependencies(api, mod_id)).await
}

#[tauri::command]
async fn add_dependency(app: AppHandle, mod_id: u64, dependency: u64) -> Result<(), String> {
    quick(app, move |api| {
        manage::add_dependencies(api, mod_id, &[dependency])
    })
    .await
}

#[tauri::command]
async fn remove_dependency(app: AppHandle, mod_id: u64, dependency: u64) -> Result<(), String> {
    quick(app, move |api| {
        manage::remove_dependencies(api, mod_id, &[dependency])
    })
    .await
}

#[tauri::command]
async fn mod_metadata(app: AppHandle, mod_id: u64) -> Result<Vec<MetadataPair>, String> {
    quick(app, move |api| manage::metadata(api, mod_id)).await
}

#[tauri::command]
async fn add_metadata(
    app: AppHandle,
    mod_id: u64,
    key: String,
    value: String,
) -> Result<(), String> {
    quick(app, move |api| {
        manage::add_metadata(api, mod_id, &key, &value)
    })
    .await
}

#[tauri::command]
async fn remove_metadata(
    app: AppHandle,
    mod_id: u64,
    key: String,
    value: String,
) -> Result<(), String> {
    quick(app, move |api| {
        manage::remove_metadata(api, mod_id, &key, &value)
    })
    .await
}

#[tauri::command]
async fn mod_comments(app: AppHandle, mod_id: u64, offset: u64) -> Result<CommentPage, String> {
    quick(app, move |api| community::comments(api, mod_id, offset)).await
}

#[tauri::command]
async fn post_comment(
    app: AppHandle,
    mod_id: u64,
    content: String,
    reply_id: Option<u64>,
) -> Result<(), String> {
    quick(app, move |api| {
        community::add_comment(api, mod_id, &content, reply_id)
    })
    .await
}

#[tauri::command]
async fn update_comment(
    app: AppHandle,
    mod_id: u64,
    comment_id: u64,
    content: String,
) -> Result<(), String> {
    quick(app, move |api| {
        community::edit_comment(api, mod_id, comment_id, &content)
    })
    .await
}

#[tauri::command]
async fn remove_comment(app: AppHandle, mod_id: u64, comment_id: u64) -> Result<(), String> {
    quick(app, move |api| {
        community::delete_comment(api, mod_id, comment_id)
    })
    .await
}

#[tauri::command]
async fn mod_team(app: AppHandle, mod_id: u64) -> Result<Vec<TeamMember>, String> {
    quick(app, move |api| community::team(api, mod_id)).await
}

#[tauri::command]
async fn mod_events(app: AppHandle, mod_id: u64) -> Result<Vec<ModEvent>, String> {
    quick(app, move |api| community::events(api, mod_id)).await
}

fn build_target(name: &str) -> Target {
    match name {
        "linux" => Target::Linux,
        "android" => Target::Android,
        "shack" => Target::Shack,
        _ => Target::Windows,
    }
}

#[tauri::command]
async fn analyze_build(app: AppHandle, mod_id: u64, target: String) -> Result<PakReport, String> {
    let settings = load_settings(app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let target = build_target(&target);
        let cook = find_cook(&PathBuf::from(&settings.project), mod_id, target).map_err(text)?;
        analyze_pak(&PathBuf::from(settings.engine), &cook.pak).map_err(text)
    })
    .await
    .map_err(text)?
}

/// Run local work that can take a while, off the main thread.
async fn local<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(Settings) -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    let settings = load_settings(app)?;
    tauri::async_runtime::spawn_blocking(move || work(settings).map_err(text))
        .await
        .map_err(text)?
}

#[tauri::command]
async fn editor_status(app: AppHandle) -> Result<EditorState, String> {
    local(app, move |s| Ok(editor_state(&PathBuf::from(s.project)))).await
}

#[tauri::command]
async fn save_unreal_work(app: AppHandle) -> Result<(), String> {
    local(app, move |s| save_work(&PathBuf::from(s.project))).await
}

#[tauri::command]
async fn restart_unreal(app: AppHandle) -> Result<bool, String> {
    local(app, move |s| {
        restart_editor(&PathBuf::from(s.project), &PathBuf::from(s.engine))
    })
    .await
}

#[tauri::command]
async fn mod_assets(app: AppHandle, mod_id: u64) -> Result<AssetReport, String> {
    local(app, move |s| {
        asset_report(&PathBuf::from(s.project), mod_id)
    })
    .await
}

#[tauri::command]
async fn find_used_assets(app: AppHandle, mod_id: u64) -> Result<AssetReport, String> {
    local(app, move |s| {
        analyze_assets(&PathBuf::from(s.project), &PathBuf::from(s.engine), mod_id)
    })
    .await
}

#[tauri::command]
async fn store_selection(app: AppHandle, mod_id: u64, selection: Selection) -> Result<(), String> {
    local(app, move |s| {
        save_selection(&PathBuf::from(s.project), mod_id, &selection)
    })
    .await
}

#[tauri::command]
async fn missing_references(app: AppHandle, mod_id: u64) -> Result<Vec<MissingReference>, String> {
    local(app, move |s| {
        check_selection(&PathBuf::from(s.project), mod_id)
    })
    .await
}

#[tauri::command]
async fn built_assets(app: AppHandle, mod_id: u64, target: String) -> Result<Vec<String>, String> {
    local(app, move |s| {
        let target = build_target(&target);
        let cook = find_cook(&PathBuf::from(&s.project), mod_id, target)?;
        let files = pak_files(&PathBuf::from(s.engine), &cook.pak)?;
        let names: Vec<&str> = files.iter().map(String::as_str).collect();
        Ok(assets_in_pak(mod_id, &names))
    })
    .await
}

#[tauri::command]
fn pending_release(
    app: AppHandle,
    mod_id: u64,
    group_name: String,
    compress: bool,
) -> Result<Option<i64>, String> {
    let settings = load_settings(app)?;
    let targets = group(&group_name).map_err(text)?;
    Ok(pending_version(
        &PathBuf::from(settings.project),
        mod_id,
        targets,
        compress,
    ))
}

#[tauri::command]
fn discard_release(app: AppHandle, mod_id: u64) -> Result<(), String> {
    let settings = load_settings(app)?;
    discard_pending(&PathBuf::from(settings.project), mod_id);
    Ok(())
}

/// Tell the app how a run ended, and why if it failed.
fn finish(result: anyhow::Result<()>) {
    clear_cancel();
    let failure = result
        .as_ref()
        .err()
        .and_then(|error| error.downcast_ref::<Failure>().cloned());
    emit(Event::Finished {
        ok: result.is_ok(),
        message: result.err().map(text).unwrap_or_default(),
        failure,
    });
}

#[tauri::command]
fn stage_mod(app: AppHandle, mod_id: u64) -> Result<(), String> {
    let settings = load_settings(app)?;
    if settings.stage.is_empty() {
        return Err("Choose a staging folder in Settings first.".to_string());
    }
    clear_cancel();
    std::thread::spawn(move || {
        let options = CookOptions {
            project: PathBuf::from(settings.project),
            engine: PathBuf::from(settings.engine),
            mod_id,
        };
        finish(stage_local(&options, &PathBuf::from(settings.stage)));
    });
    Ok(())
}

#[derive(Deserialize)]
struct PublishRequest {
    mod_id: u64,
    group: String,
    changelog: String,
    inactive: bool,
    compress: bool,
    together: bool,
    share_source: bool,
}

#[tauri::command]
fn cancel_run() {
    request_cancel();
}

#[tauri::command]
fn start_publish(app: AppHandle, request: PublishRequest) -> Result<(), String> {
    let settings = load_settings(app.clone())?;
    let targets = group(&request.group).map_err(text)?;
    let api = api(&app)?;
    clear_cancel();
    std::thread::spawn(move || {
        let release = Release {
            cook: CookOptions {
                project: PathBuf::from(settings.project),
                engine: PathBuf::from(settings.engine),
                mod_id: request.mod_id,
            },
            changelog: request.changelog,
            inactive: request.inactive,
            compress: request.compress,
            together: request.together,
            share_source: request.share_source,
        };
        finish(pipeline::run(&api, release, targets));
    });
    Ok(())
}

/// Bring the open window forward when someone opens the app again.
fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// A second copy would keep its own count of mod.io requests and could publish the same mod at
/// the same time. Development builds, and test runs given their own WebView2 folder, can sit
/// beside an installed copy.
fn one_copy_only() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    // This has to be the first plugin, so a second copy stops before anything else starts.
    if one_copy_only() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app);
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            if let Ok(dir) = app.path().app_config_dir() {
                limits::remember_in(dir.join("mod-io-wait.txt"));
            }
            let handle = app.handle().clone();
            set_sink(move |event| {
                let _ = handle.emit("progress", event);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            check_folders,
            has_token,
            api_usage,
            save_token,
            remove_token,
            check_mod_token,
            list_mods,
            library,
            mod_info,
            create_new_mod,
            retry_setup,
            browse_source,
            download_source,
            mod_files,
            publish_file,
            update_file,
            remove_file,
            update_mod,
            close_mod,
            add_images,
            add_links,
            remove_media,
            order_media,
            mod_dependencies,
            add_dependency,
            remove_dependency,
            mod_metadata,
            add_metadata,
            remove_metadata,
            mod_comments,
            post_comment,
            update_comment,
            remove_comment,
            mod_team,
            mod_events,
            editor_status,
            save_unreal_work,
            restart_unreal,
            mod_assets,
            find_used_assets,
            store_selection,
            missing_references,
            built_assets,
            analyze_build,
            stage_mod,
            pending_release,
            discard_release,
            start_publish,
            cancel_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pavlov Mod Studio");
}

#[cfg(all(test, windows))]
mod tests {
    use super::{protect, unprotect};

    /// Credential Manager refused tokens this long, which is why the app
    /// encrypts them itself.
    #[test]
    fn a_mod_io_token_survives_a_round_trip() {
        let token = "a".repeat(1908);
        let sealed = protect(token.as_bytes()).unwrap();
        assert!(sealed.len() > token.len());
        assert_eq!(unprotect(&sealed).unwrap(), token.as_bytes());
    }
}
