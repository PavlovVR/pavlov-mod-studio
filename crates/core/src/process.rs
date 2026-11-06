//! Running the Unreal tools: live output, a log file, and failures that say what broke.
use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::progress::{emit, is_cancelled, Event, Level, OutputLine};

const MAX_ERRORS: usize = 50;
const TAIL_LINES: usize = 30;
const BATCH_LINES: usize = 200;
const BATCH_EVERY: Duration = Duration::from_millis(100);
/// Helper processes can inherit a tool's output and outlive it, so don't wait for them.
const LINGER: Duration = Duration::from_secs(3);

/// Keep child processes from flashing a console window over the app.
pub fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

pub fn is_running(exe: &str) -> bool {
    hidden(Command::new("tasklist").args(["/FI", &format!("IMAGENAME eq {exe}"), "/NH"]))
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(exe))
        .unwrap_or(false)
}

/// What went wrong in a run, for the app and the command line to explain.
#[derive(Clone, Debug, Serialize)]
pub struct Failure {
    /// `compile`, `cook`, `stage`, `package`, `audit` or `definition`.
    pub kind: String,
    /// The build it happened on, when there is one.
    pub target: Option<String>,
    pub tool: String,
    pub summary: String,
    pub errors: Vec<String>,
    pub exit_code: Option<i32>,
    pub log_path: Option<String>,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let title = match self.kind.as_str() {
            "compile" => "Compile failed",
            "stage" => "Staging failed",
            "package" => "Packaging failed",
            "audit" => "Content check failed",
            "definition" => "Definition update failed",
            _ => "Cook failed",
        };
        write!(f, "{title}")?;
        if let Some(target) = &self.target {
            write!(f, " on {target}")?;
        }
        write!(f, ": {}", self.summary)?;
        if let Some(path) = &self.log_path {
            write!(f, " Full log: {path}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Failure {}

/// Lines that every healthy run prints, taken from real modkit logs. A cook prints the first
/// one about 27,000 times, because the base game's assets aren't in the modkit, and the
/// headless editor prints the "errors" whatever happens. They stay in the log file.
fn is_noise(line: &str) -> bool {
    let trimmed = line.trim();
    (line.contains("Could not resolve package") && line.contains("/Releases/"))
        || line.contains("is not initialized properly")
        || line.contains("AttemptToFindUninitializedScriptStructMembers")
        || line.contains("LogAudioMixer: Error: XAudio")
        || line.contains("[Callstack]")
        || line.contains("=== Critical error")
        || trimmed == "LogWindows: Error:"
}

/// Sort a line of Unreal output. Counts such as "0 error(s)" are not errors.
pub fn level_of(line: &str) -> Level {
    if is_noise(line) {
        return Level::Noise;
    }
    let text = line.trim_start();
    let error = text.starts_with("ERROR:")
        || text.starts_with("Error:")
        || line.contains(": Error:")
        || line.contains("Fatal error")
        || line.contains(" error CS")
        || line.contains(": error ")
        || line.contains("BUILD FAILED");
    if error {
        return Level::Error;
    }
    let warning = text.starts_with("WARNING:")
        || text.starts_with("Warning:")
        || line.contains(": Warning:")
        || line.contains(": warning ");
    if warning {
        Level::Warning
    } else {
        Level::Info
    }
}

fn kind_of_line(line: &str) -> Option<&'static str> {
    let has = |needles: &[&str]| needles.iter().any(|needle| line.contains(needle));
    if has(&[" error CS", "Compilation failed", "Unable to compile"]) {
        Some("compile")
    } else if has(&["LogCook", "Cook failed", "Cook Failed"]) {
        Some("cook")
    } else if has(&["UnrealPak", "PakFile", "Failed to create pak"]) {
        Some("package")
    } else if has(&["Stage Failed", "Staging failed", "LogStage"]) {
        Some("stage")
    } else {
        None
    }
}

/// Which part of a build broke. The first error is the cause, because Unreal adds general
/// lines such as BUILD FAILED to the end of every failure. Cooking is what's left.
pub fn kind_of(errors: &[String], tail: &[String]) -> &'static str {
    errors
        .iter()
        .chain(tail)
        .find_map(|line| kind_of_line(line))
        .unwrap_or("cook")
}

pub struct Tool<'a> {
    pub name: &'a str,
    pub log: Option<&'a Path>,
    /// Stop can end this tool partway. Leave this off if that could damage a file.
    pub cancellable: bool,
    /// Keep standard output for the caller to read.
    pub keep_stdout: bool,
}

#[derive(Default)]
pub struct ToolRun {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub errors: Vec<String>,
    pub tail: Vec<String>,
}

impl ToolRun {
    /// Report this run as a failure, leading with the first error that explains itself.
    pub fn failure(
        &self,
        kind: &str,
        target: Option<&str>,
        tool: &Tool,
        fallback: &str,
    ) -> Failure {
        // A crash opens with lines naming only a source file. The reason comes after them.
        let wrapper = |line: &&String| {
            line.contains("appError called") || line.contains("Fatal error: [File:")
        };
        let summary = self
            .errors
            .iter()
            .find(|line| !wrapper(line))
            .or(self.errors.first())
            .cloned()
            .unwrap_or_else(|| fallback.to_string());
        Failure {
            kind: kind.to_string(),
            target: target.map(str::to_string),
            tool: tool.name.to_string(),
            summary,
            errors: self.errors.clone(),
            exit_code: self.exit_code,
            log_path: tool.log.map(|path| path.display().to_string()),
        }
    }
}

fn pump(source: impl Read + Send + 'static, sender: Sender<(bool, String)>, stdout: bool) {
    thread::spawn(move || {
        let mut reader = BufReader::new(source);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    // Unreal doesn't always write valid UTF-8.
                    let text = String::from_utf8_lossy(&bytes);
                    let line = text.trim_end_matches(['\r', '\n']).to_string();
                    if sender.send((stdout, line)).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

/// A batch file starts other processes, so ending only the first would leave a cook running.
fn kill_tree(child: &mut Child) {
    hidden(Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]))
        .output()
        .ok();
    child.kill().ok();
    child.wait().ok();
}

fn open_log(path: &Path) -> Result<BufWriter<File>> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
    }
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    Ok(BufWriter::new(file))
}

fn flush(batch: &mut Vec<OutputLine>, sent: &mut Instant) {
    if !batch.is_empty() {
        emit(Event::Output {
            lines: std::mem::take(batch),
        });
    }
    *sent = Instant::now();
}

/// Run a tool, sending its output to the screen and its log as it arrives.
pub fn run_tool(tool: &Tool, command: &mut Command) -> Result<ToolRun> {
    let mut log = tool.log.map(open_log).transpose()?;
    let mut child = hidden(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start {}; check the engine path", tool.name))?;
    let (sender, receiver) = channel();
    pump(
        child.stdout.take().context("no output")?,
        sender.clone(),
        true,
    );
    pump(child.stderr.take().context("no output")?, sender, false);

    let mut run = ToolRun::default();
    let mut seen = HashSet::new();
    let mut tail = VecDeque::with_capacity(TAIL_LINES);
    let mut batch = Vec::new();
    let mut left_out = 0;
    let mut sent = Instant::now();
    let mut exited: Option<(ExitStatus, Instant)> = None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok((stdout, line)) => {
                if let Some(file) = log.as_mut() {
                    writeln!(file, "{line}").ok();
                }
                if stdout && tool.keep_stdout {
                    run.stdout.push_str(&line);
                    run.stdout.push('\n');
                }
                let level = level_of(&line);
                if level == Level::Noise {
                    left_out += 1;
                    continue;
                }
                if level == Level::Error
                    && run.errors.len() < MAX_ERRORS
                    && seen.insert(line.trim().to_string())
                {
                    run.errors.push(line.trim().to_string());
                }
                if tail.len() == TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line.clone());
                batch.push(OutputLine {
                    tool: tool.name.to_string(),
                    text: line,
                    level,
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if batch.len() >= BATCH_LINES || sent.elapsed() >= BATCH_EVERY {
            flush(&mut batch, &mut sent);
        }
        if tool.cancellable && is_cancelled() {
            kill_tree(&mut child);
            flush(&mut batch, &mut sent);
            if let Some(file) = log.as_mut() {
                file.flush().ok();
            }
            bail!("Stopped. Publish again to start this build over.");
        }
        if exited.is_none() {
            exited = child.try_wait()?.map(|status| (status, Instant::now()));
        }
        if exited.is_some_and(|(_, at)| at.elapsed() >= LINGER) {
            break;
        }
    }
    if left_out > 0 {
        let place = if tool.log.is_some() {
            "They are in the log file."
        } else {
            ""
        };
        batch.push(OutputLine {
            tool: tool.name.to_string(),
            text: format!("{left_out} routine lines left out. {place}"),
            level: Level::Info,
        });
    }
    flush(&mut batch, &mut sent);
    if let Some(file) = log.as_mut() {
        file.flush().ok();
    }
    let status = match exited {
        Some((status, _)) => status,
        None => child.wait()?,
    };
    run.success = status.success();
    run.exit_code = status.code();
    run.tail = tail.into();
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::{kind_of, level_of, run_tool, Tool};
    use crate::progress::Level;

    #[test]
    fn sorts_unreal_output_by_level() {
        assert_eq!(level_of("LogCook: Error: Missing asset"), Level::Error);
        assert_eq!(level_of("[2026.11.05][  0]LogCook: Error: x"), Level::Error);
        assert_eq!(level_of("ERROR: Cook failed."), Level::Error);
        assert_eq!(
            level_of("Foo.cs(3,4): error CS1002: ; expected"),
            Level::Error
        );
        assert_eq!(level_of("LogInit: Warning: Slow"), Level::Warning);
        assert_eq!(
            level_of("LogCook: Display: Cooked 12 packages"),
            Level::Info
        );
        assert_eq!(level_of("Success - 0 error(s), 3 warning(s)"), Level::Info);
        assert_eq!(level_of("Loaded ErrorHandler.uasset"), Level::Info);
    }

    /// These lines come from real logs of cooks that succeeded.
    #[test]
    fn harmless_lines_from_real_runs_are_noise() {
        for line in [
            "LogCook: Warning: Could not resolve package /Game/Environment/Rubble/SM_Timber from ../../../../Modkit/Releases/Pavlov_1.0.0/Windows/Metadata/DevelopmentAssetRegistry.bin",
            "LogInit: Display: LogCook: Warning: Could not resolve package /Game/Maps/Glass from ../../Releases/Pavlov_1.0.0/x",
            "LogClass: Error: IntProperty FPavlovPlayerStatus::Cash is not initialized properly. Module:PavlovProxy File:Public/x.h",
            "LogAutomationTest: Error: UObject.Class AttemptToFindUninitializedScriptStructMembers will be marked as failing due to errors",
            "LogAudioMixer: Error: XAudio2 Error: Result -> 0x1: XAUDIO2_E_INVALID_CALL (line: 1)",
            "LogWindows: Error: [Callstack] 0x00007fff0f15ee4c ntdll.dll!UnknownFunction []",
            "LogWindows: Error: ",
            "LogWindows: Error: === Critical error: ===",
        ] {
            assert_eq!(level_of(line), Level::Noise, "{line}");
        }
    }

    #[test]
    fn a_missing_package_of_your_own_is_still_a_warning() {
        let line = "LogCook: Warning: Could not resolve package /UGC1/Maps/Broken";
        assert_eq!(level_of(line), Level::Warning);
    }

    /// From a real UnrealPak crash on a corrupt file.
    #[test]
    fn a_crash_leads_with_its_reason() {
        let tool = Tool {
            name: "UnrealPak.exe",
            log: None,
            cancellable: false,
            keep_stdout: false,
        };
        let run = super::ToolRun {
            errors: vec![
                "LogWindows: Error: appError called: Fatal error: [File:D:/x/PakFileUtilities.cpp] [Line: 5]".to_string(),
                "LogWindows: Error: Fatal error: [File:D:/x/PakFileUtilities.cpp] [Line: 5]".to_string(),
                "LogWindows: Error: Unable to open pak file \"bad.pak\"".to_string(),
            ],
            ..Default::default()
        };
        let failure = run.failure("package", None, &tool, "fallback");
        assert!(
            failure.summary.contains("Unable to open pak file"),
            "{}",
            failure.summary
        );
    }

    #[cfg(windows)]
    #[test]
    fn noise_goes_to_the_log_but_not_the_screen() {
        let body =
            "@echo off\r\necho LogClass: Error: Foo is not initialized properly.\r\necho kept\r\n";
        let (path, log) = script("noise", body);
        let tool = Tool {
            name: "x",
            log: Some(&log),
            cancellable: false,
            keep_stdout: false,
        };
        let run = run_tool(&tool, &mut std::process::Command::new(&path)).unwrap();
        assert!(run.errors.is_empty());
        assert_eq!(run.tail, ["kept"]);
        assert!(std::fs::read_to_string(&log)
            .unwrap()
            .contains("not initialized"));
    }

    #[test]
    fn names_the_part_that_broke() {
        let lines = |text: &str| vec![text.to_string()];
        assert_eq!(
            kind_of(&lines("A.cs(1,1): error CS1002: x"), &[]),
            "compile"
        );
        assert_eq!(kind_of(&lines("LogPakFile: Error: bad"), &[]), "package");
        assert_eq!(kind_of(&lines("ERROR: Stage Failed."), &[]), "stage");
        assert_eq!(kind_of(&lines("LogCook: Error: Missing"), &[]), "cook");
        assert_eq!(kind_of(&[], &[]), "cook");
        let cook_then_summary = [
            "LogCook: Error: Missing".to_string(),
            "BUILD FAILED: Failed while running Cook".to_string(),
        ];
        assert_eq!(kind_of(&cook_then_summary, &[]), "cook");
        assert_eq!(kind_of(&lines("BUILD FAILED: Cook failed."), &[]), "cook");
        let compile_first = [
            "A.cs(1,1): error CS1002: x".to_string(),
            "ERROR: Cook failed.".to_string(),
        ];
        assert_eq!(kind_of(&compile_first, &[]), "compile");
        assert_eq!(
            kind_of(&[], &lines("Running: UnrealPak.exe x.pak")),
            "package"
        );
    }

    #[cfg(windows)]
    fn script(name: &str, body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("pms-tool-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tool.bat");
        std::fs::write(&path, body).unwrap();
        (path, dir.join("logs/deep/run.log"))
    }

    #[cfg(windows)]
    #[test]
    fn captures_output_errors_and_the_exit_code() {
        let body = "@echo off\r\necho LogCook: Display: working\r\necho LogCook: Error: Missing asset\r\necho LogCook: Error: Missing asset\r\necho to stderr 1>&2\r\nexit /b 25\r\n";
        let (path, log) = script("fail", body);
        let tool = Tool {
            name: "RunUAT",
            log: Some(&log),
            cancellable: false,
            keep_stdout: true,
        };
        let run = run_tool(&tool, &mut std::process::Command::new(&path)).unwrap();
        assert!(!run.success);
        assert_eq!(run.exit_code, Some(25));
        assert_eq!(run.errors, ["LogCook: Error: Missing asset"]);
        assert!(run.stdout.contains("working") && !run.stdout.contains("to stderr"));
        let saved = std::fs::read_to_string(&log).unwrap();
        assert!(saved.contains("working") && saved.contains("to stderr"));
        let failure = run.failure("cook", Some("linux"), &tool, "fallback");
        assert_eq!(failure.summary, "LogCook: Error: Missing asset");
        let text = failure.to_string();
        assert!(text.starts_with("Cook failed on linux:") && text.contains("Full log:"));
    }

    #[cfg(windows)]
    #[test]
    fn a_clean_run_succeeds_without_a_log() {
        let (path, _) = script("ok", "@echo off\r\necho hello\r\n");
        let tool = Tool {
            name: "x",
            log: None,
            cancellable: false,
            keep_stdout: false,
        };
        let run = run_tool(&tool, &mut std::process::Command::new(&path)).unwrap();
        assert!(run.success && run.errors.is_empty() && run.stdout.is_empty());
        assert_eq!(run.tail, ["hello"]);
    }

    #[test]
    fn a_missing_tool_says_so() {
        let tool = Tool {
            name: "UnrealPak.exe",
            log: None,
            cancellable: false,
            keep_stdout: false,
        };
        let mut command = std::process::Command::new("Z:/no/such/tool.exe");
        let error = run_tool(&tool, &mut command).err().unwrap().to_string();
        assert!(error.contains("could not start UnrealPak.exe"), "{error}");
    }

    /// A helper that outlives the tool keeps the pipe open, and that must not hang a publish.
    #[cfg(windows)]
    #[test]
    fn a_lingering_helper_does_not_hang_the_run() {
        let body = "@echo off\r\nstart /b ping -n 40 127.0.0.1\r\nexit /b 0\r\n";
        let (path, _) = script("linger", body);
        let tool = Tool {
            name: "x",
            log: None,
            cancellable: false,
            keep_stdout: false,
        };
        let started = std::time::Instant::now();
        let run = run_tool(&tool, &mut std::process::Command::new(&path)).unwrap();
        assert!(run.success);
        assert!(started.elapsed().as_secs() < 15);
    }
}
