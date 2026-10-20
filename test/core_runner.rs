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

#[test]
fn poll_retries_transport_and_temporary_statuses_with_fresh_signatures() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for status in [None, Some(503), Some(429), Some(200)] {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            requests.push(request);
            if let Some(status) = status {
                let body = r#"{"protocolVersion":3,"state":"waiting"}"#;
                write_http_response(&mut stream, status, body);
            }
        }
        requests
    });

    let key = SigningKey::from_bytes(&[31; 32]);
    let host = TestHost::new();
    let response = get_signed_poll::<ConnectorOperationResponse>(
        &format!("http://{address}"),
        "/api/v2/connector/operations/op",
        &key,
        &host,
    )
    .unwrap()
    .unwrap();
    assert_eq!(response.state, "waiting");

    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 4);
    assert_poll_nonces_are_fresh(
        &requests,
        "/api/v2/connector/operations/op",
        &key.verifying_key(),
    );
}

#[test]
fn poll_stops_after_a_bounded_number_of_consecutive_transport_failures() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut count = 0;
        for incoming in listener.incoming().take((MAX_POLL_RETRIES + 1) as usize) {
            let mut stream = incoming.unwrap();
            let _ = read_http_request(&mut stream);
            count += 1;
        }
        count
    });

    let key = SigningKey::from_bytes(&[32; 32]);
    let host = TestHost::new();
    let started = std::time::Instant::now();
    let error = get_signed_poll::<ConnectorOperationResponse>(
        &format!("http://{address}"),
        "/api/v2/connector/operations/op",
        &key,
        &host,
    )
    .err()
    .unwrap();
    assert!(error.contains("repeated temporary network errors"));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(server.join().unwrap(), MAX_POLL_RETRIES + 1);
}

#[test]
fn poll_retry_wait_is_cancellable() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_http_request(&mut stream);
        drop(stream);
    });

    let host = TestHost::new();
    let signal = Arc::clone(&host.stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(75));
        signal.store(true, Ordering::SeqCst);
    });
    let key = SigningKey::from_bytes(&[33; 32]);
    let started = std::time::Instant::now();
    assert!(get_signed_poll::<ConnectorOperationResponse>(
        &format!("http://{address}"),
        "/api/v2/connector/operations/op",
        &key,
        &host,
    )
    .unwrap()
    .is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
}

#[test]
fn poll_does_not_retry_unauthorized_or_invalid_json_responses() {
    use std::net::TcpListener;

    for (status, body, expected_error) in [
        (401, r#"{"error":"unauthorized"}"#, "rejected poll (401)"),
        (200, "not-json", "invalid poll JSON"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, status, &body);
        });
        let key = SigningKey::from_bytes(&[34; 32]);
        let error = get_signed_poll::<ConnectorOperationResponse>(
            &format!("http://{address}"),
            "/api/v2/connector/operations/op",
            &key,
            &TestHost::new(),
        )
        .err()
        .unwrap();
        assert!(error.contains(expected_error));
        server.join().unwrap();
    }
    for status in [408, 429, 502, 503, 504] {
        assert!(retryable_poll_status(status));
    }
    for status in [400, 401, 403, 404, 500] {
        assert!(!retryable_poll_status(status));
    }
}

#[test]
fn connected_run_retries_lost_pending_writeback_without_repeating_posts_or_tasks() {
    use ed25519_dalek::VerifyingKey;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let host = TestHost::new();
    let stop = Arc::clone(&host.stop);
    let server = std::thread::spawn(move || {
        let mut start_count = 0;
        let mut poll_requests = Vec::new();
        let mut task_receipts = 0;
        let mut writeback_receipts = 0;
        let mut device_key = None;
        let original_session = b"source!!";
        let updated_session = b"updated!";
        let operation_path = "/api/v2/connector/operations/operation-1";
        let mut waiting_task_sent = false;
        let mut pending_response_dropped = false;
        let mut replayed_pending = false;
        loop {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            let (headers, body) = http_parts(&request);
            let first_line = headers.lines().next().unwrap();
            if first_line == "POST /api/v2/connector/operations HTTP/1.1" {
                start_count += 1;
                let parsed: serde_json::Value = serde_json::from_slice(body).unwrap();
                let public_key = URL_SAFE_NO_PAD
                    .decode(parsed["devicePublicKey"].as_str().unwrap())
                    .unwrap();
                device_key = Some(
                    VerifyingKey::from_bytes(public_key.as_slice().try_into().unwrap()).unwrap(),
                );
                let response = format!(
                    r#"{{"protocolVersion":3,"operationId":"operation-1","browserUrl":"http://{address}/d_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/?authorization=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}}"#
                );
                write_http_response(&mut stream, 200, &response);
            } else if first_line.starts_with("GET ") {
                poll_requests.push(request.clone());
                if poll_requests.len() == 1 {
                    drop(stream);
                    continue;
                }
                if !waiting_task_sent {
                    waiting_task_sent = true;
                    write_http_response(
                        &mut stream,
                        200,
                        r#"{"protocolVersion":3,"state":"waiting","directoryTasks":[{"id":"inspect-once","kind":"product_inspect"}]}"#,
                    );
                } else if !pending_response_dropped {
                    pending_response_dropped = true;
                    let response = serde_json::json!({
                        "protocolVersion": 3,
                        "state": "ready_for_writeback",
                        "ciphertext": URL_SAFE_NO_PAD.encode(updated_session),
                        "sourceSha256": sha256_base64url(original_session),
                        "expiresAt": (unix_timestamp_millis() + 120_000).to_string(),
                    });
                    let response = serde_json::to_vec(&response).unwrap();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).unwrap();
                    stream.write_all(&response[..response.len() / 2]).unwrap();
                } else {
                    replayed_pending = true;
                    let response = serde_json::json!({
                        "protocolVersion": 3,
                        "state": "ready_for_writeback",
                        "ciphertext": URL_SAFE_NO_PAD.encode(updated_session),
                        "sourceSha256": sha256_base64url(original_session),
                        "expiresAt": (unix_timestamp_millis() + 120_000).to_string(),
                    });
                    write_http_response(&mut stream, 200, &serde_json::to_string(&response).unwrap());
                }
            } else if first_line
                == "POST /api/v2/connector/operations/operation-1/directory-tasks/inspect-once/complete HTTP/1.1"
            {
                task_receipts += 1;
                write_http_response(&mut stream, 200, r#"{"state":"accepted"}"#);
            } else if first_line == "POST /api/v2/connector/operations/operation-1/receipt HTTP/1.1" {
                writeback_receipts += 1;
                stop.store(true, Ordering::SeqCst);
                write_http_response(
                    &mut stream,
                    200,
                    &format!(
                        r#"{{"state":"writeback_completed","sourceSha256":"{}"}}"#,
                        sha256_base64url(updated_session)
                    ),
                );
                break;
            } else {
                panic!("unexpected request: {first_line}");
            }
        }
        let key = device_key.unwrap();
        assert_eq!(start_count, 1);
        assert_eq!(poll_requests.len(), 4);
        assert_eq!(task_receipts, 1);
        assert_eq!(writeback_receipts, 1);
        assert!(replayed_pending);
        assert_poll_nonces_are_fresh(&poll_requests, operation_path, &key);
    });

    let temp = tempfile::tempdir().unwrap();
    let session_path = temp
        .path()
        .join("Dreamtonics")
        .join("Synthesizer V Studio 2")
        .join("license")
        .join("session");
    std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
    std::fs::write(&session_path, b"source!!").unwrap();
    let config = Config {
        server_url: format!("http://{address}"),
        session_path: session_path.clone(),
    };
    run_connected(&config, &host).unwrap();
    server.join().unwrap();
    assert_eq!(std::fs::read(session_path).unwrap(), b"updated!");
}

fn http_parts(request: &[u8]) -> (&str, &[u8]) {
    let split = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    (
        std::str::from_utf8(&request[..split]).unwrap(),
        &request[split + 4..],
    )
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    use std::io::Read;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some((headers, body)) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|split| (&request[..split], &request[split + 4..]))
        {
            let headers = String::from_utf8_lossy(headers);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if body.len() >= length {
                break;
            }
        }
    }
    request
}

fn write_http_response(stream: &mut std::net::TcpStream, status: u16, body: &str) {
    use std::io::Write;
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Test Response",
    };
    write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

fn assert_poll_nonces_are_fresh(
    requests: &[Vec<u8>],
    path: &str,
    verifying_key: &ed25519_dalek::VerifyingKey,
) {
    use ed25519_dalek::{Signature, Verifier};
    let mut nonces = std::collections::HashSet::new();
    for request in requests {
        let (headers, _) = http_parts(request);
        let nonce = headers
            .lines()
            .find_map(|line| header_value(line, "x-connector-nonce"))
            .unwrap();
        let signature = headers
            .lines()
            .find_map(|line| header_value(line, "x-connector-signature"))
            .unwrap();
        assert!(nonces.insert(nonce.clone()));
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
        verifying_key
            .verify(format!("GET\n{path}\n{nonce}").as_bytes(), &signature)
            .unwrap();
    }
}

fn header_value<'a>(line: &'a str, expected: &str) -> Option<String> {
    let (name, value) = line.split_once(':')?;
    name.eq_ignore_ascii_case(expected)
        .then(|| value.trim().to_string())
}
