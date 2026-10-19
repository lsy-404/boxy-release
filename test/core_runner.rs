use super::*;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct TestHost {
    stop: Arc<AtomicBool>,
    approvals: AtomicUsize,
}

impl TestHost {
    fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            approvals: AtomicUsize::new(0),
        }
    }
}

impl ConnectorHost for TestHost {
    fn open_editor(&self, _url: Url) -> Result<(), String> {
        Ok(())
    }

    fn approve_command(&self, _command: &str, _shell: &str, _cwd: &Path) -> bool {
        self.approvals.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

fn browser_url() -> String {
    format!(
        "https://service.example.test/d_{}/?authorization={}",
        "a".repeat(32),
        "A".repeat(43)
    )
}

#[test]
fn accepts_the_configured_device_route() {
    assert!(validate_browser_url("https://service.example.test", &browser_url()).is_ok());
}

#[test]
fn rejects_another_origin_or_invalid_access_parameter() {
    assert!(validate_browser_url(
        "https://service.example.test",
        &browser_url().replace("service.example.test", "other.example.test")
    )
    .is_err());
    assert!(validate_browser_url(
        "https://service.example.test",
        "https://service.example.test/d_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/?authorization=short"
    )
    .is_err());
    assert!(validate_browser_url(
        "https://service.example.test",
        &format!("{}&other=value", browser_url())
    )
    .is_err());
}

#[test]
fn stops_before_reading_a_session_or_opening_a_remote_operation() {
    let host = TestHost::new();
    host.stop.store(true, Ordering::SeqCst);
    let config = Config {
        server_url: "https://service.example.test".into(),
        session_path: PathBuf::from("missing-session"),
    };
    assert!(run_connected(&config, &host).is_ok());
    assert_eq!(host.approvals.load(Ordering::SeqCst), 0);
}

#[test]
fn interruptible_wait_observes_stop_promptly() {
    let host = TestHost::new();
    let signal = Arc::clone(&host.stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(120));
        signal.store(true, Ordering::SeqCst);
    });
    assert!(wait_or_stop(&host, Duration::from_secs(10)));
}

#[test]
fn cancellation_kills_a_started_shell_without_completing_its_work() {
    let host = TestHost::new();
    let signal = Arc::clone(&host.stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        signal.store(true, Ordering::SeqCst);
    });
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command Start-Sleep -Seconds 30"
    } else {
        "sleep 30"
    };
    let started = std::time::Instant::now();
    let result = run_shell_command(
        default_shell(),
        command,
        &std::env::temp_dir(),
        Duration::from_secs(60),
        &host,
    )
    .unwrap();
    assert!(result.is_none());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn cancelled_command_is_completed_with_a_distinct_receipt() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0u8; 2048];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..count]);
            if let Some((headers, body)) = String::from_utf8_lossy(&request).split_once("\r\n\r\n")
            {
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap();
                if body.len() >= length {
                    break;
                }
            }
        }
        let response = r#"{"state":"accepted"}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
        request
    });

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sv2");
    std::fs::create_dir_all(root.join("Dreamtonics/license")).unwrap();
    let session_path = root.join("Dreamtonics/license/session");
    let config = Config {
        server_url: format!("http://{address}"),
        session_path,
    };
    let task: DirectoryTask = serde_json::from_value(serde_json::json!({
        "id": "command-request",
        "kind": "command",
        "command": if cfg!(windows) {
            "powershell -NoProfile -Command Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        },
        "timeoutSeconds": 60
    }))
    .unwrap();
    let host = TestHost::new();
    let signal = Arc::clone(&host.stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        signal.store(true, Ordering::SeqCst);
    });
    let mut last_reported = String::new();
    execute_directory_task(
        &config,
        &SigningKey::from_bytes(&[7; 32]),
        "operation-1",
        &task,
        &mut last_reported,
        &host,
    )
    .unwrap();

    let request = String::from_utf8(server.join().unwrap()).unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert_eq!(
        headers.lines().next().unwrap(),
        "POST /api/v2/connector/operations/operation-1/directory-tasks/command-request/complete HTTP/1.1"
    );
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["cancelled"], true);
    assert!(body.get("timedOut").is_none());
}

#[test]
fn runs_a_shell_command_and_captures_output() {
    let host = TestHost::new();
    let command = if cfg!(windows) {
        "echo hello"
    } else {
        "printf hello"
    };
    let result = run_shell_command(
        default_shell(),
        command,
        &std::env::temp_dir(),
        Duration::from_secs(30),
        &host,
    )
    .unwrap()
    .unwrap();
    assert!(!result.timed_out);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("hello"));
}

#[test]
fn reports_a_nonzero_exit_code() {
    let host = TestHost::new();
    let command = if cfg!(windows) { "exit /b 3" } else { "exit 3" };
    let result = run_shell_command(
        default_shell(),
        command,
        &std::env::temp_dir(),
        Duration::from_secs(30),
        &host,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.exit_code, Some(3));
}

#[test]
fn stops_a_command_that_exceeds_its_deadline() {
    let host = TestHost::new();
    let command = if cfg!(windows) {
        "ping -n 6 127.0.0.1 > nul"
    } else {
        "sleep 5"
    };
    let result = run_shell_command(
        default_shell(),
        command,
        &std::env::temp_dir(),
        Duration::from_secs(1),
        &host,
    )
    .unwrap()
    .unwrap();
    assert!(result.timed_out);
}

#[test]
fn truncates_oversized_output_on_a_character_boundary() {
    let text = "a".repeat(1024);
    assert_eq!(truncate_output(text.as_bytes()), text);
    let oversized = "é".repeat(MAX_COMMAND_OUTPUT_BYTES);
    let truncated = truncate_output(oversized.as_bytes());
    let body = truncated.strip_suffix("\n[output truncated]").unwrap();
    assert!(body.len() <= MAX_COMMAND_OUTPUT_BYTES);
    assert!(oversized.starts_with(body));
}

#[test]
fn each_run_uses_an_in_memory_signing_identity() {
    let first = new_device_signing_key();
    let second = new_device_signing_key();
    assert_ne!(
        first.verifying_key().to_bytes(),
        second.verifying_key().to_bytes()
    );
}
