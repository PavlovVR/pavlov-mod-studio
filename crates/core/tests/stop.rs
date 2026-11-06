//! In its own test binary because cancelling is process-wide.
#![cfg(windows)]
use std::fs;
use std::process::Command;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

use pavlov_mod_core::process::{is_running, run_tool, Tool};
use pavlov_mod_core::progress::{clear_cancel, request_cancel};

/// RunUAT is a batch file that starts other programs, and those must stop too.
#[test]
fn stop_ends_the_tool_and_everything_it_started() {
    let dir = std::env::temp_dir().join(format!("pms-stop-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let helper_name = format!("pms-stop-helper-{}.exe", std::process::id());
    let helper = dir.join(&helper_name);
    fs::copy(r"C:\Windows\System32\ping.exe", &helper).unwrap();
    let marker = dir.join("finished.txt");
    let script = dir.join("tool.bat");
    let body = format!(
        "@echo off\r\necho started\r\n\"{}\" -n 60 127.0.0.1\r\necho done > \"{}\"\r\n",
        helper.display(),
        marker.display()
    );
    fs::write(&script, body).unwrap();

    clear_cancel();
    spawn(|| {
        sleep(Duration::from_secs(2));
        request_cancel();
    });
    let tool = Tool {
        name: "RunUAT",
        log: None,
        cancellable: true,
        keep_stdout: false,
    };
    let started = Instant::now();
    let error = run_tool(&tool, &mut Command::new(&script)).err().unwrap();
    clear_cancel();

    assert!(error.to_string().starts_with("Stopped."), "{error}");
    assert!(started.elapsed() < Duration::from_secs(20));
    sleep(Duration::from_millis(500));
    assert!(!is_running(&helper_name), "the helper was left running");
    assert!(!marker.exists(), "the batch file kept going");
}
