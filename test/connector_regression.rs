use super::*;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct ReceiptHost;

impl ConnectorHost for ReceiptHost {
    fn open_editor(&self, _url: Url) -> Result<(), String> {
        Ok(())
    }

    fn approve_command(&self, _command: &str, _shell: &str, _cwd: &std::path::Path) -> bool {
        false
    }

    fn should_stop(&self) -> bool {
        false
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boxy-connector-regression-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn validates_session_ciphertext_sizes_before_read_or_writeback() {
    let directory = TestDirectory::new();
    let path = directory.0.join("session");
    fs::write(&path, b"").unwrap();
    assert!(read_session_ciphertext(&path).is_err());
    fs::write(&path, [1; 7]).unwrap();
    assert!(read_session_ciphertext(&path).is_err());
    fs::write(&path, [1; 8]).unwrap();
    assert_eq!(read_session_ciphertext(&path).unwrap(), [1; 8]);
    let current_hash = sha256_base64url(&[1; 8]);
    assert!(replace_session_if_unchanged(&path, &current_hash, &[]).is_err());
    assert!(replace_session_if_unchanged(&path, &current_hash, &[2; 7]).is_err());
    assert_eq!(fs::read(path).unwrap(), [1; 8]);
}

#[test]
fn session_writeback_is_atomic_and_requires_the_current_source_hash() {
    let directory = TestDirectory::new();
    let path = directory.0.join("session");
    let original = [1; 16];
    let updated = [2; 16];
    fs::write(&path, original).unwrap();
    let written_hash =
        replace_session_if_unchanged(&path, &sha256_base64url(&original), &updated).unwrap();
    assert_eq!(fs::read(&path).unwrap(), updated);
    assert_eq!(written_hash, sha256_base64url(&updated));
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    fs::write(&path, [3; 16]).unwrap();
    let changed = fs::read(&path).unwrap();
    assert!(replace_session_if_unchanged(&path, &sha256_base64url(&updated), &original).is_err());
    assert_eq!(fs::read(path).unwrap(), changed);
}

#[test]
fn directory_task_paths_reject_traversal_and_platform_separators() {
    let root = PathBuf::from("/sv2");
    for unsafe_path in [
        "",
        "/session",
        "../outside",
        "a/../../outside",
        "a\\b",
        "a//b",
        "./session",
    ] {
        assert!(
            task_path(&root, unsafe_path).is_err(),
            "accepted {unsafe_path:?}"
        );
    }
    assert_eq!(
        task_path(&root, "license/session").unwrap(),
        root.join("license/session")
    );
}

#[test]
fn v3_protocol_is_required_for_start_and_poll_responses() {
    assert!(require_protocol_v3(Some(3)).is_ok());
    assert!(require_protocol_v3(None).is_err());
    assert!(require_protocol_v3(Some(2)).is_err());
    let start: ConnectorStartResponse = serde_json::from_value(serde_json::json!({
        "protocolVersion": 3,
        "operationId": "operation",
        "browserUrl": "https://service.test/d_x/?authorization=y"
    }))
    .unwrap();
    let poll: ConnectorOperationResponse = serde_json::from_value(serde_json::json!({
        "protocolVersion": 3,
        "state": "waiting",
        "directoryTasks": []
    }))
    .unwrap();
    assert_eq!(start.protocol_version, Some(3));
    assert_eq!(poll.protocol_version, Some(3));
}

#[test]
fn device_name_is_bounded_and_strips_controls() {
    assert_eq!(sanitize_device_name("desk\n\troom"), "deskroom");
    assert_eq!(sanitize_device_name(&"x".repeat(600)).len(), 512);
}

#[test]
fn complete_task_sends_the_signed_serialized_result_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut chunk = [0u8; 2048];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..count]);
            let text = String::from_utf8_lossy(&request);
            if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_string)
                    })
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if body.len() >= length {
                    break;
                }
            }
        }
        let response = r#"{"state":"accepted"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
        String::from_utf8(request).unwrap()
    });

    let config = Config {
        server_url: format!("http://{address}"),
        session_path: PathBuf::from("unused"),
    };
    let body = serde_json::json!({
        "status": 401,
        "responseBody": "fixture body",
        "upstreamAttempts": [{
            "sequence": 1,
            "phase": "request",
            "method": "GET",
            "endpoint": "https://api.example.test/v1/check",
            "status": 401,
            "durationMs": 3,
            "bodyKind": "json",
            "bodyTruncated": false
        }]
    });
    complete_task(
        &config,
        &test_signing_key(),
        "/tasks/complete",
        body,
        &ReceiptHost,
    )
    .unwrap();

    let request = server.join().unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert_eq!(
        headers.lines().next().unwrap(),
        "POST /tasks/complete HTTP/1.1"
    );
    let nonce = headers
        .lines()
        .find_map(|line| line.strip_prefix("X-Connector-Nonce: "))
        .unwrap();
    let signature = headers
        .lines()
        .find_map(|line| line.strip_prefix("X-Connector-Signature: "))
        .unwrap();
    let signature = URL_SAFE_NO_PAD.decode(signature).unwrap();
    test_signing_key()
        .verifying_key()
        .verify_strict(
            format!("POST\n/tasks/complete\n{nonce}").as_bytes(),
            &ed25519_dalek::Signature::from_slice(&signature).unwrap(),
        )
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["responseBody"], "fixture body");
    assert_eq!(body["upstreamAttempts"][0]["status"], 401);
}

#[test]
fn zeroize_json_clears_nested_string_values_and_object_keys() {
    let mut value = serde_json::json!({"credential": "fixture", "nested": ["body"]});
    zeroize_json(&mut value);
    assert_eq!(value, serde_json::json!({}));
}

#[test]
fn session_snapshot_posts_signed_latest_ciphertext_and_recognizes_writeback_pending() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
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
                            .map(str::to_string)
                    })
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if body.len() >= length {
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&request);
        let (headers, body) = text.split_once("\r\n\r\n").unwrap();
        let nonce = headers
            .lines()
            .find_map(|line| line.strip_prefix("X-Connector-Nonce: "))
            .unwrap();
        let signature = headers
            .lines()
            .find_map(|line| line.strip_prefix("X-Connector-Signature: "))
            .unwrap();
        let key = test_signing_key();
        let signature = URL_SAFE_NO_PAD.decode(signature).unwrap();
        key.verifying_key()
            .verify_strict(
                format!("POST\n/api/v2/connector/operations/op/session-snapshot\n{nonce}")
                    .as_bytes(),
                &ed25519_dalek::Signature::from_slice(&signature).unwrap(),
            )
            .unwrap();
        let request_body: serde_json::Value =
            serde_json::from_str(&body[..body.len().min(1024 * 1024)]).unwrap();
        assert_eq!(request_body["ciphertext"], URL_SAFE_NO_PAD.encode([9; 16]));
        assert_eq!(request_body["sourceSha256"], sha256_base64url(&[9; 16]));
        let response = r#"{"state":"writeback_pending","sourceSha256":"pending"}"#;
        write!(stream, "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
    });
    let config = Config {
        server_url: format!("http://{address}"),
        session_path: PathBuf::from("unused"),
    };
    let result = submit_session_snapshot(&config, &test_signing_key(), "op", &[9; 16]).unwrap();
    assert!(matches!(result, SessionWatchResult::WritebackPending));
    server.join().unwrap();
}

fn test_signing_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[7; 32])
}
