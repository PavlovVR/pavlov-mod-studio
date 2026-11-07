//! Talking to an open Unreal editor, through the Python remote execution the modkit turns on.
//!
//! An open editor locks any definition it has loaded, so a headless session can't save one.
//! Asking the editor itself to save it works, and keeps its copy in memory right as well.
use std::env;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::process::is_running;

const EDITOR: &str = "UnrealEditor.exe";
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 1);
const PORT: u16 = 6766;
const FIND_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// Long enough for someone to answer Unreal's save dialog.
const DIALOG_TIMEOUT: Duration = Duration::from_secs(600);
const QUIT_TIMEOUT: Duration = Duration::from_secs(90);
const CANCELLED: &str = "PMS_CANCELLED";
const LOAD_PLUGIN: &str = include_str!("scripts/load_plugin.py");

/// What the app needs to know before a build.
#[derive(Debug, Default, Serialize)]
pub struct EditorState {
    pub open: bool,
    /// It answered, and it has this project open.
    pub connected: bool,
    pub unsaved: usize,
}

fn message(kind: &str, source: &str, dest: Option<&str>, data: Option<Value>) -> Vec<u8> {
    let mut body = json!({ "version": 1, "magic": "ue_py", "type": kind, "source": source });
    if let Some(dest) = dest {
        body["dest"] = json!(dest);
    }
    if let Some(data) = data {
        body["data"] = data;
    }
    body.to_string().into_bytes()
}

fn tidy_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn same_project(root: &str, project: &Path) -> bool {
    tidy_path(root) == tidy_path(&project.to_string_lossy())
}

/// Find the editor with this project open, out of any that answered.
fn matching_node(reply: &Value, id: &str, project: &Path) -> Option<String> {
    let to_us = reply["dest"].as_str().is_none_or(|dest| dest == id);
    if reply["type"] != "pong" || !to_us {
        return None;
    }
    let root = reply["data"]["project_root"].as_str()?;
    same_project(root, project).then(|| reply["source"].as_str().map(str::to_string))?
}

fn open_socket() -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, PORT));
    socket.bind(&SockAddr::from(address))?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_ttl_v4(0)?;
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
    let socket: UdpSocket = socket.into();
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    Ok(socket)
}

pub struct Editor {
    id: String,
    node: String,
    socket: UdpSocket,
    stream: TcpStream,
}

impl Drop for Editor {
    fn drop(&mut self) {
        let close = message("close_connection", &self.id, Some(&self.node), None);
        self.socket.send_to(&close, (GROUP, PORT)).ok();
    }
}

fn accept(listener: &TcpListener) -> Result<TcpStream> {
    listener.set_nonblocking(true)?;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if started.elapsed() > CONNECT_TIMEOUT {
                    bail!("Unreal didn't answer");
                }
                sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Connect to the editor that has this project open. `None` means there isn't one to talk to.
pub fn connect(project: &Path) -> Result<Option<Editor>> {
    if !is_running(EDITOR) {
        return Ok(None);
    }
    let socket = open_socket().context("could not look for the open editor")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    let id = format!("pavlov-mod-studio-{}-{stamp}", std::process::id());
    let started = Instant::now();
    let mut pinged = None::<Instant>;
    let mut buffer = vec![0; 8192];
    let node = loop {
        if started.elapsed() > FIND_TIMEOUT {
            return Ok(None);
        }
        if pinged.is_none_or(|at| at.elapsed() > Duration::from_secs(1)) {
            socket.send_to(&message("ping", &id, None, None), (GROUP, PORT))?;
            pinged = Some(Instant::now());
        }
        let Ok((size, _)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let reply: Value = serde_json::from_slice(&buffer[..size]).unwrap_or(Value::Null);
        if let Some(node) = matching_node(&reply, &id, project) {
            break node;
        }
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let open = json!({ "command_ip": "127.0.0.1", "command_port": port });
    let request = message("open_connection", &id, Some(&node), Some(open));
    socket.send_to(&request, (GROUP, PORT))?;
    let stream = accept(&listener)?;
    Ok(Some(Editor {
        id,
        node,
        socket,
        stream,
    }))
}

impl Editor {
    /// Run Python in the editor and return what it printed.
    fn run(&mut self, code: &str, unattended: bool, timeout: Duration) -> Result<Vec<String>> {
        let command =
            json!({ "command": code, "unattended": unattended, "exec_mode": "ExecuteFile" });
        let request = message("command", &self.id, Some(&self.node), Some(command));
        self.stream.write_all(&request)?;
        self.stream.set_read_timeout(Some(timeout))?;
        let mut received = Vec::new();
        let mut chunk = vec![0; 8192];
        let result: Value = loop {
            let size = self
                .stream
                .read(&mut chunk)
                .context("Unreal stopped answering")?;
            if size == 0 {
                bail!("Unreal closed the connection");
            }
            received.extend_from_slice(&chunk[..size]);
            if let Ok(value) = serde_json::from_slice(&received) {
                break value;
            }
        };
        let data = &result["data"];
        let output: Vec<String> = data["output"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|line| line["output"].as_str().map(|text| text.trim().to_string()))
            .collect();
        if data["success"] != true {
            let reason = data["result"].as_str().unwrap_or("it refused");
            let last = reason.lines().last().unwrap_or(reason);
            bail!("Unreal couldn't do that: {last}");
        }
        Ok(output)
    }

    fn unsaved(&mut self) -> Result<usize> {
        let code = "import unreal\n\
            utils = unreal.EditorLoadingAndSavingUtils\n\
            print(len(utils.get_dirty_content_packages()) + len(utils.get_dirty_map_packages()))";
        let output = self.run(code, true, COMMAND_TIMEOUT)?;
        Ok(output.last().and_then(|n| n.parse().ok()).unwrap_or(0))
    }

    /// Have the editor set and save a definition's version.
    pub fn set_version(&mut self, asset: &str, version: i64) -> Result<()> {
        let path = serde_json::to_string(asset)?;
        let code = format!(
            "import unreal\n\
            library = unreal.EditorAssetLibrary\n\
            asset = library.load_asset({path})\n\
            if asset is None:\n    raise RuntimeError('definition not found')\n\
            asset.set_editor_property('Version', {version})\n\
            if not library.save_loaded_asset(asset, False):\n    raise RuntimeError('could not save the definition')\n"
        );
        self.run(&code, true, COMMAND_TIMEOUT)?;
        Ok(())
    }
}

/// A descriptor marked so `FPluginUtils::LoadPlugin` will mount it. Unreal only reads this as it
/// loads, so the original goes straight back and the next launch sees an ordinary plugin.
fn marked_for_loading(original: &[u8]) -> Result<Vec<u8>> {
    let text = String::from_utf8_lossy(original);
    let mut plugin: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .context("this plugin's .uplugin isn't valid")?;
    plugin["ExplicitlyLoaded"] = Value::from(true);
    Ok(serde_json::to_vec_pretty(&plugin)?)
}

/// Load a plugin into an open editor, so it appears without a restart. Returns false if there's
/// no editor to load it into, or the editor already had a plugin by this name, which only a
/// restart can replace.
pub fn load_into_editor(project: &Path, plugin: &str) -> Result<bool> {
    let Some(mut editor) = connect(project)? else {
        return Ok(false);
    };
    let descriptor = project
        .join("Plugins")
        .join(plugin)
        .join(format!("{plugin}.uplugin"));
    let original = fs::read(&descriptor).context("cannot find this plugin's .uplugin")?;
    fs::write(&descriptor, marked_for_loading(&original)?)?;
    let path = serde_json::to_string(&descriptor.to_string_lossy().replace('\\', "/"))?;
    let code = format!("DESCRIPTOR = {path}\n{LOAD_PLUGIN}");
    let result = editor.run(&code, true, COMMAND_TIMEOUT);
    // Whatever happened, including the editor crashing, the plugin stays an ordinary one.
    fs::write(&descriptor, original)?;
    Ok(result?.last().is_some_and(|line| line == "loaded"))
}

/// Never fails: not being able to reach the editor just means there's nothing to report.
pub fn editor_state(project: &Path) -> EditorState {
    let open = is_running(EDITOR);
    let Ok(Some(mut editor)) = connect(project) else {
        return EditorState {
            open,
            ..EditorState::default()
        };
    };
    EditorState {
        open,
        connected: true,
        unsaved: editor.unsaved().unwrap_or(0),
    }
}

/// Save everything unsaved in the open editor, so a build includes it.
pub fn save_work(project: &Path) -> Result<()> {
    let mut editor = connect(project)?.context("couldn't reach the open editor")?;
    let code = "import unreal\n\
        unreal.EditorLoadingAndSavingUtils.save_dirty_packages(True, True)";
    editor.run(code, true, COMMAND_TIMEOUT)?;
    Ok(())
}

/// Close the editor and open it again, which is the only way it can load a new plugin.
/// Unreal asks about unsaved work itself. Returns false if that was cancelled.
pub fn restart_editor(project: &Path, engine: &Path) -> Result<bool> {
    let mut editor = connect(project)?
        .context("couldn't reach the open editor. Close it and open the modkit again")?;
    let code = format!(
        "import unreal\n\
        if unreal.EditorLoadingAndSavingUtils.save_dirty_packages_with_dialog(True, True):\n    unreal.SystemLibrary.quit_editor()\n\
        else:\n    print('{CANCELLED}')\n"
    );
    // The editor closing partway through the reply is the expected way this ends.
    if let Ok(output) = editor.run(&code, false, DIALOG_TIMEOUT) {
        if output.iter().any(|line| line.contains(CANCELLED)) {
            return Ok(false);
        }
    }
    drop(editor);
    let started = Instant::now();
    while is_running(EDITOR) {
        if started.elapsed() > QUIT_TIMEOUT {
            bail!("Unreal didn't close. Close it and open the modkit again");
        }
        sleep(Duration::from_millis(500));
    }
    Command::new(engine.join("Binaries/Win64/UnrealEditor.exe"))
        .arg(project.join("Pavlov.uproject"))
        // It outlives this process, so it must not hold on to this process's output.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("could not open Unreal again")?;
    Ok(true)
}

fn editor_settings_path() -> Option<PathBuf> {
    let folder = env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(folder)
            .join("UnrealEngine/5.1/Saved/Config/WindowsEditor/EditorSettings.ini"),
    )
}

/// Turn on the setting inside the ini's text, leaving everything else as it was.
fn with_plugin_content(text: &str) -> String {
    const SECTION: &str = "[/Script/UnrealEd.ContentBrowserSettings]";
    const SETTING: &str = "DisplayPluginFolders";
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(start) = lines.iter().position(|line| line.trim() == SECTION) else {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(SECTION.to_string());
        lines.push(format!("{SETTING}=True"));
        return lines.join(newline) + newline;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let existing = (start + 1..end).find(|index| lines[*index].starts_with(&format!("{SETTING}=")));
    match existing {
        Some(index) => lines[index] = format!("{SETTING}=True"),
        None => lines.insert(start + 1, format!("{SETTING}=True")),
    }
    lines.join(newline) + newline
}

/// New mods are plugins, so the Content Browser needs Show Plugin Content on to list them.
pub fn show_plugin_content(project: &Path) -> Result<()> {
    if let Some(path) = editor_settings_path() {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let updated = with_plugin_content(&text);
        if updated != text {
            if let Some(folder) = path.parent() {
                fs::create_dir_all(folder)?;
            }
            fs::write(&path, updated)?;
        }
    }
    if let Some(mut editor) = connect(project)? {
        let code = "import unreal\n\
            unreal.SystemLibrary.execute_console_command(None, 'set ContentBrowserSettings DisplayPluginFolders 1')";
        editor.run(code, true, COMMAND_TIMEOUT)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};

    use super::{marked_for_loading, matching_node, message, same_project, with_plugin_content};

    #[test]
    fn marking_a_plugin_keeps_everything_else() {
        let original = "\u{feff}{\"FriendlyName\": \"UGC7\", \"CanContainContent\": true}";
        let marked = marked_for_loading(original.as_bytes()).unwrap();
        let plugin: Value = serde_json::from_slice(&marked).unwrap();
        assert_eq!(plugin["ExplicitlyLoaded"], true);
        assert_eq!(plugin["FriendlyName"], "UGC7");
        assert_eq!(plugin["CanContainContent"], true);
    }

    #[test]
    fn a_broken_descriptor_is_refused_before_anything_is_written() {
        assert!(marked_for_loading(b"not json").is_err());
    }

    #[test]
    fn messages_match_unreals_protocol() {
        let sent: Value = serde_json::from_slice(&message("ping", "me", None, None)).unwrap();
        assert_eq!(
            sent,
            json!({ "version": 1, "magic": "ue_py", "type": "ping", "source": "me" })
        );
        let sent: Value = serde_json::from_slice(&message(
            "command",
            "me",
            Some("it"),
            Some(json!({ "a": 1 })),
        ))
        .unwrap();
        assert_eq!(sent["dest"], "it");
        assert_eq!(sent["data"]["a"], 1);
    }

    #[test]
    fn matches_a_project_however_its_path_is_written() {
        let project = Path::new(r"D:\Modding\Modkit");
        assert!(same_project("D:/Modding/Modkit/", project));
        assert!(same_project("d:/modding/modkit", project));
        assert!(!same_project("D:/Modding/Other/", project));
    }

    #[test]
    fn only_an_editor_with_this_project_counts() {
        let project = Path::new("C:/Modkit");
        let pong = |root: &str, dest: &str| json!({ "type": "pong", "source": "node", "dest": dest, "data": { "project_root": root } });
        assert_eq!(
            matching_node(&pong("C:/Modkit/", "me"), "me", project).as_deref(),
            Some("node")
        );
        assert!(matching_node(&pong("C:/Other/", "me"), "me", project).is_none());
        assert!(matching_node(&pong("C:/Modkit/", "someone"), "me", project).is_none());
        assert!(matching_node(&json!({ "type": "ping" }), "me", project).is_none());
    }

    #[test]
    fn turns_plugin_content_on_in_an_existing_section() {
        let text = "[/Script/UnrealEd.ContentBrowserSettings]\r\nbShowAllFolder=True\r\nDisplayPluginFolders=False\r\n\r\n[Other]\r\nDisplayPluginFolders=False\r\n";
        let updated = with_plugin_content(text);
        assert!(updated.contains("bShowAllFolder=True\r\nDisplayPluginFolders=True\r\n"));
        assert!(updated.contains("[Other]\r\nDisplayPluginFolders=False"));
    }

    #[test]
    fn adds_the_setting_to_a_section_without_it() {
        let updated =
            with_plugin_content("[/Script/UnrealEd.ContentBrowserSettings]\nA=1\n\n[B]\nC=2\n");
        assert!(updated.contains("ContentBrowserSettings]\nDisplayPluginFolders=True\nA=1"));
        assert!(updated.contains("[B]\nC=2"));
    }

    #[test]
    fn adds_the_section_when_its_missing() {
        assert!(with_plugin_content("")
            .contains("[/Script/UnrealEd.ContentBrowserSettings]\nDisplayPluginFolders=True"));
        assert!(with_plugin_content("[A]\nB=1\n").starts_with("[A]\nB=1\n\n[/Script"));
    }

    #[test]
    fn leaves_a_correct_file_alone() {
        let text = "[/Script/UnrealEd.ContentBrowserSettings]\nDisplayPluginFolders=True\n";
        assert_eq!(with_plugin_content(text), text);
    }
}
