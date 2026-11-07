# How it works

Design notes for contributors. Read this before changing anything that spans several files.

Pavlov Mod Studio is a Windows desktop app that replaces the Pavlov modkit's Upload button. It drives a separate modkit, which is an Unreal Engine 5.1 project, from outside the editor. [modkit-flow.md](modkit-flow.md) is the behaviour it has to match. Match the modkit first, then improve on it.

## Layout

All logic lives in `crates/core`. `crates/cli` and `app/src-tauri` are thin wrappers, so new behaviour goes in core and gets exposed from both.

The order of a publish is defined in `pipeline.rs`: check the definition, bump the version, update mod metadata, cook, audit, zip, upload.

## Key design points

- **Progress:** core reports through `progress.rs` (`say!`, `stage`, `emit`), never `println!`. The CLI prints logs, and the app forwards events to `app/src/main.ts`. Cancellation is also here, checked between chunks and between builds.
- **Resume happens at two levels.** `pipeline.rs` writes `release.json` once a release is cooked, so a retry skips the version bump and cook. `upload.rs` writes a `.state.json` beside each zip listing sent chunks. Anything that changes a zip or a build's identity must keep these consistent.
- **Foreign content protection:** `targets.rs::plugin_rules` derives the cook exclusion list, and `package.rs::audit_pak` lists every pak after cooking and again before upload. Never make this list user-typed, and never skip the audit outside tests. Anything outside this mod and the engine stops the build, except files from non-UGC modkit plugins such as CustomMapTools, which are a warning.
- **Tools:** every Unreal tool goes through `process.rs::run_tool`, which streams output to the screen and a log, and lets Stop kill the process tree. A failure is a typed `Failure` naming what broke, which reaches the app through `Event::Finished`. Don't run these tools any other way, or their output and Stop are lost. Leave `cancellable` off for anything that could damage a file if ended partway.
- **Leaving assets out:** `assets.rs`. Unreal's `ModifyDLCCook` adds every asset in a mod plugin to the cook, so no flag can limit it. A folder that's wholly left out goes through `-NeverCookDir`. Single assets are deleted from the cooked output, then packaged with `-SkipCook`. The pak is checked afterwards, and the selection is part of `release.json`. These user choices only ever remove this mod's assets, so they sit on top of `plugin_rules`, which stays derived. What counts as used is Unreal's tree plus a scan of the files, because neither is enough alone. Filter only used must keep leaving out only what both agree on. Mods can use each other's assets, so other mods in the modkit count as users, except what they leave out of their own builds. This is local only: mods outside the modkit aren't seen, so Always keep is the way to protect what they need.
- **Headless Unreal:** `definition.rs` runs the Python in `crates/core/src/scripts/` through `UnrealEditor-Cmd.exe`, passing inputs by environment variable and reading a result file, because Unreal's exit code is unreliable. Each run is a fresh session, so a read after a write proves the value reached disk.
- **Open Unreal:** `editor.rs` talks to a running editor through Python remote execution, which the modkit turns on in `DefaultEngine.ini`. An open editor locks a definition it has loaded, so a headless session can't save it, and the editor has to save the version itself. Cook and stage are fine with it open.
- **Definition checks** read the `.uasset` name table directly (`definition.rs::has_name`), because reading a map property through Unreal's Python loads the whole map.
- **Rate limits:** every request goes through `limits.rs`, from `api.rs::send`, so nothing can skip it. It keeps the app under 100 requests and 50 writes a minute, and after a 429 nothing is sent until the wait is over, even across a restart. Requests someone is waiting on fail straight away during a wait from mod.io, and the app shows a countdown through `Event::Waiting` and reloads what stopped. Long jobs sit it out. `Api::pages` reads a whole list, so it's only for lists that stay short: files and comments use `Api::page`, because a mod can have thousands. Team answers are kept in `access.json` for a week, because they'd otherwise cost a request per mod.
- **One copy at a time:** `tauri-plugin-single-instance` makes a second launch bring the open window forward, because `limits.rs` counts requests per process and two copies could publish the same mod at once. Debug builds and copies started with `WEBVIEW2_USER_DATA_FOLDER` skip it, so they can run beside an installed copy. The updater exits before its installer runs, so the reopened app finds nothing holding the lock.
- **Secrets:** the app encrypts the user's token with DPAPI in `app/src-tauri/src/lib.rs`. Credential Manager can't be used, because mod.io tokens exceed its 2560-byte limit.
- **Updates:** Tauri's updater reads `latest.json` from the latest GitHub release. `scripts/update_manifest.py` writes it during a release. Windows ends the app the moment the installer starts, so `installUpdate` in `main.ts` must never run during a run. A build only produces update files with `tauri.release.conf.json`, so anyone can build without the signing key.

## Frontend

The frontend is plain TypeScript with no framework. `app/src/main.ts` holds one `state` object and re-renders with `render()`, using morphdom, with event delegation on `#app`. Styling is Tailwind, with colour tokens and shared components in `app/src/styles.css`.

The look is neutral and serious and follows the Windows light or dark setting: greys, blue only for focus and progress, green and red only for outcomes, and no themed decoration.

A run lives in the panel under the main view, which `renderRun()` updates on its own, so progress never disturbs what the user is typing.

mod.io data is read once and kept. `library.rs::load_library` reads every mod in one batched request at launch, and nothing else is read until someone opens a mod. `cache` in `main.ts` keeps each mod's files, comments and so on until a Refresh or a change. Switching mods must never fetch what's already held. Requests someone is waiting on use `Api::quick`, which fails in seconds, and long jobs keep `Api::new` and its patient retries.

Anything typed must live in `state`, because background results re-render the page. A Page draft that hasn't been changed must not outlive the data it was copied from, or a save could overwrite newer edits.

## mod.io specifics

Learned from real API behaviour, so don't "fix" these:

- Editing a mod is `POST /games/{id}/mods/{id}`, and it must be `multipart/form-data` even with no file, or mod.io refuses it, despite its docs. Use `api.rs::text_form`. Editing a file, such as `active=true` to make it live or roll back, is `PUT .../files/{id}`.
- Attaching a finished upload to `POST .../files` must be `multipart/form-data`.
- Parts must be exactly 50 MiB except the last. A duplicate range returns error 29015, which counts as already sent.
- Personal tokens have been refused with 403 (11139) on `/me` and `/me/mods`, so ownership is enforced by mod.io on write, not checked first. Others read both, so `account_name` handles both. mod.io doesn't document 11139, and which tokens get it hasn't been pinned down.
- Tokens are made on the Personal access tokens tab of `/me/access`, which replaces OAuth access. The form starts on Read only and 30 Days, so the steps in Settings ask for All my mods, User actions, Read, Write and 1 Year. A bad or expired token is 401 (11005), which `library.rs::check_token` uses to test a token before saving it. That check can't see what a token is allowed to do.
- A token gets 120 requests a minute, 60 of them writes, on a rolling window, and failed requests count. A 429 carries `retry-after`, where 0 means wait 60 seconds, and 11009 means only that endpoint is limited. mod.io warns that carrying on after a 429 can get a token revoked.
- The Quest client is platform `oculus`, and the Shack server is platform `mac`. The app shows the platform names, Windows, Linux, Oculus and Mac, with an icon marking servers.
- Which mods are yours comes from matching a username from Settings against the author and team. If that isn't available, nothing is hidden.
- Deleting a mod only closes it, and only the website can restore or permanently delete it. A live file can't be deleted (15009). The API can't change a team, so the app shows it read-only.
- The game's only tag group is `Type`, which has to match `ModType`, so the app doesn't edit tags.
- The mod's `metadata_blob` holds `ModType`, `EngineVersion` and `VersionUUID`. Each file's blob has only the first two, as CRLF and tab formatted JSON.

Most write paths (create, edit, logo, make live, share source, and everything in `manage.rs` and `community.rs`) are tested against the fake server, so expect possible differences from real mod.io.

## Gotchas

- In Git Bash, set `MSYS_NO_PATHCONV=1` for any command with a leading-slash argument such as `/UGC123`. Bash rewrites it into a Windows path silently.
- Unreal stores a name's trailing number apart from its text, so `prop_12` appears in a file as `prop`, and non-English names exist only as UTF-16. `assets.rs` handles both. Missing either makes used assets look unused.
- A healthy cook prints about 27,000 `Could not resolve package ... /Releases/` warnings, and the headless editor prints `LogClass: Error:` lines whatever happens. `process.rs::is_noise` keeps these in the log file and off the screen, so errors and warnings stay meaningful. Add to that list only from real logs of runs that succeeded.
- A running editor can't load a new plugin: `MountNewlyCreatedPlugin` isn't reachable from Python. The only answer is restarting it, which the app offers after creating a mod.
- Show Plugin Content is `DisplayPluginFolders` in the per-user `EditorSettings.ini` under `%LOCALAPPDATA%\UnrealEngine\5.1`, not in the project.
- `UnrealPak -List` gives paths relative to the pak's mount point, and a small mod with no engine files mounts deeper than the root. Always read paths through `package.rs::pak_paths`.
- A plugin named with `[SOURCE]` in its FriendlyName is built into mods rather than excluded. That rule was inferred from matching real plugins, not found as a literal check in the modkit.
- morphdom can move an element when a row appears above it, and moving an element resets its scroll. `renderRun()` restores the log's position for this reason.
