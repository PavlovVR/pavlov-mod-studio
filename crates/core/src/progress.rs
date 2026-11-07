//! Progress reporting, so the command line and the app can show the same run.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use serde::Serialize;

use crate::process::Failure;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Printed by every healthy run, so it goes to the log file only.
    Noise,
    Info,
    Warning,
    Error,
}

/// One line from an Unreal tool.
#[derive(Clone, Debug, Serialize)]
pub struct OutputLine {
    pub tool: String,
    pub text: String,
    pub level: Level,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Log {
        line: String,
    },
    /// Tool output, sent in batches because a cook prints thousands of lines.
    Output {
        lines: Vec<OutputLine>,
    },
    Stage {
        stage: String,
        target: Option<String>,
    },
    Parts {
        target: String,
        done: u64,
        total: u64,
    },
    Finished {
        ok: bool,
        message: String,
        failure: Option<Failure>,
    },
    /// Requests are held back for this long, by mod.io or by the app's own limit.
    Waiting {
        seconds: u64,
    },
}

static CANCELLED: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Print tool output on the command line, not just errors.
pub fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::SeqCst);
}

pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// Ask the current run to stop at the next safe point. A cancelled upload can be resumed.
pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub fn clear_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

pub fn check_cancel() -> anyhow::Result<()> {
    if is_cancelled() {
        anyhow::bail!("Stopped. You can resume this release.");
    }
    Ok(())
}

type Sink = Box<dyn Fn(Event) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();

/// Send progress somewhere other than the terminal. Only the first call counts.
pub fn set_sink(sink: impl Fn(Event) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(sink));
}

pub fn emit(event: Event) {
    match SINK.get() {
        Some(sink) => sink(event),
        None => {
            let verbose = VERBOSE.load(Ordering::SeqCst);
            match event {
                Event::Log { line } => println!("{line}"),
                Event::Output { lines } => lines
                    .iter()
                    .filter(|line| verbose || line.level == Level::Error)
                    .for_each(|line| println!("{}", line.text)),
                Event::Waiting { seconds } => println!("waiting {seconds}s for mod.io"),
                _ => {}
            }
        }
    }
}

pub fn stage(stage: &str, target: Option<&str>) {
    emit(Event::Stage {
        stage: stage.to_string(),
        target: target.map(str::to_string),
    });
}

#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::progress::emit($crate::progress::Event::Log { line: format!($($arg)*) })
    };
}
