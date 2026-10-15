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
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);

    fs::write(&path, [3; 16]).unwrap();
    let changed = fs::read(&path).unwrap();
    assert!(replace_session_if_unchanged(&path, &sha256_base64url(&updated), &original,).is_err());
    assert_eq!(fs::read(&path).unwrap(), changed);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
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
fn custom_requests_allow_https_endpoints_and_keep_method_body_bounds() {
    for endpoint in [
        "https://dreamtonics.com/api/client",
        "https://cdn.dreamtonics.com.cn/api/client",
        "https://service.example.test:8443/api/client",
        "https://custom-service.example/api/client",
    ] {
        assert!(is_valid_custom_endpoint(endpoint));
    }
    for endpoint in [
        "http://authr3.dreamtonics.com/api/client",
        "not a URL",
        "https://user:secret@custom-service.example/api",
        "https://custom-service.example/api#fragment",
        &format!("https://service.example.test/{}", "a".repeat(2048)),
    ] {
        assert!(!is_valid_custom_endpoint(endpoint), "accepted {endpoint}");
    }

    let mut task = directory_task("custom_request");
    task.method = Some("DELETE".into());
    task.endpoint = Some("https://authr3.dreamtonics.com/api/client".into());
    assert_eq!(
        dreamtonics_request(&task).error,
        Some("request method is unsupported")
    );

    task.method = Some("POST".into());
    task.body = Some("[]".into());
    assert_eq!(
        dreamtonics_request(&task).error,
        Some("request body is invalid")
    );

    task.body = Some("x".repeat(256 * 1024));
    task.endpoint = Some("https://custom-service.example/api".into());
    assert_eq!(
        dreamtonics_request(&task).error,
        Some("request body is invalid")
    );
}

#[test]
fn quick_login_uses_the_current_device_id_and_current_computer_name() {
    let body: serde_json::Value =
        serde_json::from_str(&quick_login_body(Some("  device-42  "))).unwrap();
    assert_eq!(body["payload"]["device_id"], "device-42");
    assert_eq!(body["payload"]["device_hash"], "");
    assert_eq!(body["payload"]["editor_version"], 0x20201);
    assert_eq!(body["payload"]["kickout_other_sessions"], false);
    assert_ne!(body["payload"]["device_name"], "");

    let no_device: serde_json::Value = serde_json::from_str(&quick_login_body(None)).unwrap();
    assert!(no_device["payload"]["device_id"].is_null());
}

#[test]
fn product_logo_action_matches_local_metadata_then_falls_back_to_licenses() {
    let directory = TestDirectory::new();
    let app_root = directory.0.join("app");
    let session_path = app_root.join("sessions").join("session");
    let metadata_directory = app_root.join("databases").join("meta");
    fs::create_dir_all(&metadata_directory).unwrap();
    fs::create_dir_all(session_path.parent().unwrap()).unwrap();
    fs::write(
        metadata_directory.join("local-product.json"),
        r#"{"name":"Synth One","profile":{"imageUrl":"https://cdn.dreamtonics.com/local.png"}}"#,
    )
    .unwrap();

    let keys = vec!["  synth   one ".to_string(), "license-only".to_string()];
    let mut license_reads = 0;
    let logos = assets::fetch_product_logos_with(
        &session_path,
        &keys,
        || {
            license_reads += 1;
            Ok(serde_json::json!({"data":[{
                "id":"license-cache-id",
                "productId":"license-only",
                "image":{"url":"https://dreamtonics.com/license.gif"}
            }]}))
        },
        |url, _| match url {
            "https://cdn.dreamtonics.com/local.png" => Some(b"\x89PNG\r\n\x1a\nlocal".to_vec()),
            "https://dreamtonics.com/license.gif" => Some(b"GIF89a-license".to_vec()),
            _ => None,
        },
    )
    .unwrap();

    assert_eq!(license_reads, 1);
    assert_eq!(logos.len(), 2);
    assert_eq!(logos[0].id, "local-product");
    assert_eq!(logos[0].name, "Synth One");
    assert!(logos[0].data_url.starts_with("data:image/png;base64,"));
    assert_eq!(logos[1].id, "license-only");
    assert!(logos[1].data_url.starts_with("data:image/gif;base64,"));
    assert!(metadata_directory.join("local-product.png").exists());
    assert!(metadata_directory.join("license-cache-id.gif").exists());
    let cached = assets::cached_product_logos(&session_path);
    assert!(cached.iter().any(|logo| logo.id == "local-product"));
    assert!(cached.iter().any(|logo| {
        logo.id == "license-cache-id" && logo.data_url.starts_with("data:image/gif;base64,")
    }));
}

#[test]
fn product_logo_fetch_skips_license_request_when_local_metadata_covers_keys() {
    let directory = TestDirectory::new();
    let app_root = directory.0.join("app");
    let session_path = app_root.join("sessions").join("session");
    let metadata_directory = app_root.join("databases").join("meta");
    fs::create_dir_all(&metadata_directory).unwrap();
    fs::write(
        metadata_directory.join("local-product.json"),
        r#"{"name":"Synth One","icon":"https://assets.dreamtonics.com/icon.webp"}"#,
    )
    .unwrap();
    let logos = assets::fetch_product_logos_with(
        &session_path,
        &["synth one".to_string()],
        || panic!("licenses should not be fetched when local metadata matches"),
        |_, _| Some(b"RIFF1234WEBPimage".to_vec()),
    )
    .unwrap();
    assert_eq!(logos.len(), 1);
    assert!(logos[0].data_url.starts_with("data:image/webp;base64,"));
}

#[test]
fn product_logo_urls_formats_and_cache_are_bounded_and_trusted() {
    let directory = TestDirectory::new();
    let app_root = directory.0.join("app");
    let session_path = app_root.join("sessions").join("session");
    let metadata_directory = app_root.join("databases").join("meta");
    fs::create_dir_all(&metadata_directory).unwrap();
    fs::write(
        metadata_directory.join("untrusted.json"),
        r#"{"name":"untrusted","imageUrl":"https://evil.example.test/p.png"}"#,
    )
    .unwrap();
    let mut download_count = 0;
    let logos = assets::fetch_product_logos_with(
        &session_path,
        &["untrusted".to_string()],
        || Ok(serde_json::json!({"data":[{"id":"x","productId":"untrusted","logo":"http://dreamtonics.com/image.png"}]})),
        |_, _| {
            download_count += 1;
            Some(b"not an image".to_vec())
        },
    )
    .unwrap();
    assert!(logos.is_empty());
    assert_eq!(download_count, 0);

    let mut too_large = b"\x89PNG\r\n\x1a\n".to_vec();
    too_large.resize(512 * 1024 + 1, b'x');
    let logos = assets::fetch_product_logos_with(
        &session_path,
        &["oversized".to_string()],
        || Ok(serde_json::json!({"data":[{"id":"oversized-cache","productId":"oversized","imageUrl":"https://img.dreamtonics.com/a.png"}]})),
        |_, _| Some(too_large.clone()),
    )
    .unwrap();
    assert!(logos.is_empty());
    assert!(!metadata_directory.join("oversized-cache.png").exists());

    let invalid_keys = vec!["key".to_string(); 513];
    assert!(assets::fetch_product_logos_with(
        &session_path,
        &invalid_keys,
        || panic!("invalid key count must fail before network access"),
        |_, _| None,
    )
    .is_err());
}

#[test]
fn latest_operation_protocol_decodes_directory_tasks_and_snapshot_fields() {
    let response: ConnectorOperationResponse = serde_json::from_value(serde_json::json!({
        "state": "editor_active",
        "directoryTasks": [{
            "id": "task-1",
            "kind": "dreamtonics_request",
            "action": "licenses",
            "expectedSessionSha256": "source-hash",
            "deviceId": "device-42",
            "keys": ["product-a", "Synth One"],
            "accessToken": "access",
            "refreshToken": "refresh"
        }]
    }))
    .unwrap();
    assert_eq!(response.state, "editor_active");
    assert_eq!(response.directory_tasks.len(), 1);
    let task = &response.directory_tasks[0];
    assert_eq!(task.kind, "dreamtonics_request");
    assert_eq!(task.action.as_deref(), Some("licenses"));
    assert_eq!(task.expected_session_sha256.as_deref(), Some("source-hash"));
    assert_eq!(task.device_id.as_deref(), Some("device-42"));
    assert_eq!(task.keys, ["product-a", "Synth One"]);

    let ack: SessionSnapshotAck = serde_json::from_value(serde_json::json!({
        "state": "accepted",
        "sourceSha256": "source-hash"
    }))
    .unwrap();
    assert_eq!(ack.state, "accepted");
    assert_eq!(ack.source_sha256.as_deref(), Some("source-hash"));
}

#[test]
fn session_snapshot_posts_signed_latest_ciphertext_and_recognizes_writeback_pending() {
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
            let request_text = String::from_utf8_lossy(&request);
            if let Some((headers, body)) = request_text.split_once("\r\n\r\n") {
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
        write!(
            stream,
            "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
    });

    let config = Config {
        server_url: format!("http://{address}"),
        session_path: PathBuf::from("unused"),
    };
    let result = submit_session_snapshot(&config, &test_signing_key(), "op", &[9; 16]).unwrap();
    assert!(matches!(result, SessionWatchResult::WritebackPending));
    server.join().unwrap();
}

fn directory_task(action: &str) -> DirectoryTask {
    DirectoryTask {
        id: "task".into(),
        kind: "dreamtonics_request".into(),
        relative_path: None,
        expected_sha256: None,
        command: None,
        shell: None,
        cwd: None,
        timeout_seconds: None,
        blocked: None,
        mode: None,
        action: Some(action.into()),
        items: Vec::new(),
        keys: Vec::new(),
        product_id: None,
        device_id: None,
        access_token: None,
        expected_session_sha256: None,
        refresh_token: None,
        method: None,
        endpoint: None,
        body: None,
        model_version: None,
    }
}

fn test_signing_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[7; 32])
}
