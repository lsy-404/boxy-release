#![cfg(windows)]
#![allow(dead_code)]

use std::{os::windows::process::CommandExt, process::Command, time::Duration};

mod windows_process {
    use std::{os::windows::process::CommandExt, process::Command};

    pub fn configure_background(command: &mut Command) {
        command.creation_flags(0x0800_0000);
    }
}

#[path = "../connector/src/session_io.rs"]
mod session_io;

#[test]
fn folder_request_succeeds_when_opener_exits_after_accepting_the_request() {
    let directory = tempfile::tempdir().unwrap();
    let mut opener = Command::new("powershell.exe");
    opener
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 3; exit 7; #",
        ])
        .creation_flags(0x0800_0000);

    let started = std::time::Instant::now();
    session_io::spawn_directory_opener(opener, directory.path()).unwrap();

    assert!(started.elapsed() < Duration::from_secs(2));
}
