use super::*;
use std::{
    cell::RefCell,
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
        || Ok("https://test.invalid/licenses".to_string()),
        |_| {
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
        || panic!("license endpoint should not be resolved when local metadata matches"),
        |_| panic!("licenses should not be fetched when local metadata matches"),
        |_, _| Some(b"RIFF1234WEBPimage".to_vec()),
    )
    .unwrap();
    assert_eq!(logos.len(), 1);
    assert!(logos[0].data_url.starts_with("data:image/webp;base64,"));
}

#[test]
fn product_logo_license_request_uses_the_issuer_selected_endpoint() {
    for (issuer, expected_endpoint) in [
        (
            DREAMTONICS_ISSUER,
            "https://authr3.dreamtonics.com/api/v1/client/my_licenses",
        ),
        (
            DREAMTONICS_CN_ISSUER,
            "https://authr3.dreamtonics.com.cn/api/v1/client/my_licenses",
        ),
    ] {
        let directory = TestDirectory::new();
        let session_path = directory.0.join("app/sessions/session");
        let access_token = refresh_token_claims(issuer, Some("test-client"));
        let captured = RefCell::new(None);
        let recorder = UpstreamAttemptRecorder::new();
        let result = assets::fetch_product_logos_with_request(
            &session_path,
            &["license-only".to_string()],
            &access_token,
            &recorder,
            |endpoint, token| {
                *captured.borrow_mut() = Some((endpoint.to_string(), token.to_string()));
                Ok((200, r#"{"data":[]}"#.to_string()))
            },
            |_, _| None,
        )
        .unwrap();

        assert!(result.is_empty());
        assert_eq!(
            captured.into_inner(),
            Some((expected_endpoint.to_string(), access_token))
        );
    }
}

#[test]
fn product_logo_license_request_rejects_invalid_issuer_before_transport() {
    let directory = TestDirectory::new();
    let session_path = directory.0.join("app/sessions/session");
    let invalid_token = refresh_token_claims(
        "https://account.dreamtonics.com.cn.evil.test/realms/Dreamtonics",
        Some("test-client"),
    );
    let mut requests = 0;
    let recorder = UpstreamAttemptRecorder::new();
    let result = assets::fetch_product_logos_with_request(
        &session_path,
        &["license-only".to_string()],
        &invalid_token,
        &recorder,
        |_, _| {
            requests += 1;
            Ok((200, r#"{"data":[]}"#.to_string()))
        },
        |_, _| None,
    );

    assert_eq!(
        result.unwrap_err(),
        "access token issuer is missing or invalid"
    );
    assert_eq!(requests, 0);
}

#[test]
fn product_logo_local_cache_does_not_require_an_issuer() {
    let directory = TestDirectory::new();
    let app_root = directory.0.join("app");
    let session_path = app_root.join("sessions/session");
    let metadata_directory = app_root.join("databases/meta");
    fs::create_dir_all(&metadata_directory).unwrap();
    fs::write(
        metadata_directory.join("cached-product.json"),
        r#"{"name":"cached product","imageUrl":"https://cdn.dreamtonics.com/product.png"}"#,
    )
    .unwrap();

    let recorder = UpstreamAttemptRecorder::new();
    let result = assets::fetch_product_logos_with_request(
        &session_path,
        &["cached product".to_string()],
        "not-a-jwt",
        &recorder,
        |_, _| panic!("local cache hit must not make a license request"),
        |_, _| Some(b"\x89PNG\r\n\x1a\nimage".to_vec()),
    )
    .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "cached-product");
}

#[test]
fn upstream_attempts_record_china_primary_refresh_and_retry_in_order() {
    let mut task = directory_task("licenses");
    task.access_token = Some(refresh_token_claims(
        DREAMTONICS_CN_ISSUER,
        Some("test-client"),
    ));
    task.refresh_token = Some(refresh_token_claims(
        DREAMTONICS_CN_ISSUER,
        Some("test-client"),
    ));
    let new_access = refresh_token_claims(DREAMTONICS_CN_ISSUER, Some("test-client"));
    let new_refresh = refresh_token_claims(DREAMTONICS_CN_ISSUER, Some("test-client"));
    let mut calls = Vec::new();
    let mut transport = |request: DreamtonicsHttpRequest| {
        calls.push((
            request.method,
            request.endpoint,
            request.body.as_deref().map(|body| body.to_string()),
        ));
        match calls.len() {
            1 => Ok((401, r#"{"error":"expired"}"#.to_string())),
            2 => Ok((
                200,
                serde_json::json!({
                    "access_token": new_access.clone(),
                    "refresh_token": new_refresh.clone(),
                    "id_token": "redact-this-id-token",
                    "expires_in": 3600,
                    "scope": "licenses"
                })
                .to_string(),
            )),
            3 => Ok((200, r#"{"data":["retried"]}"#.to_string())),
            _ => panic!("unexpected upstream call"),
        }
    };

    let result = dreamtonics_request_with_transport(&task, &mut transport);
    assert_eq!(result.status, 200);
    assert_eq!(result.response_body, r#"{"data":["retried"]}"#);
    assert_eq!(
        calls.iter().map(|call| call.1.as_str()).collect::<Vec<_>>(),
        [
            "https://authr3.dreamtonics.com.cn/api/v1/client/my_licenses",
            "https://account.dreamtonics.com.cn/realms/Dreamtonics/protocol/openid-connect/token",
            "https://authr3.dreamtonics.com.cn/api/v1/client/my_licenses",
        ]
    );
    assert_eq!(
        result
            .upstream_attempts
            .iter()
            .map(|attempt| attempt.phase)
            .collect::<Vec<_>>(),
        ["primary", "refresh", "retry"]
    );
    assert_eq!(
        result
            .upstream_attempts
            .iter()
            .map(|attempt| attempt.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let refresh_trace = result.upstream_attempts[1]
        .response_body
        .as_deref()
        .unwrap();
    assert!(refresh_trace.contains("licenses"));
    assert!(!refresh_trace.contains("access_token"));
    assert!(!refresh_trace.contains("refresh_token"));
    assert!(!refresh_trace.contains("redact-this-id-token"));
    assert!(calls[1].2.as_deref().unwrap().contains("refresh_token="));
    assert!(!refresh_trace.contains(calls[1].2.as_deref().unwrap()));
}

#[test]
fn upstream_attempt_records_transport_failure_without_response_status() {
    let mut task = directory_task("licenses");
    task.access_token = Some(refresh_token_claims(
        DREAMTONICS_CN_ISSUER,
        Some("test-client"),
    ));
    let mut transport = |_: DreamtonicsHttpRequest| Err("upstream request failed");

    let result = dreamtonics_request_with_transport(&task, &mut transport);
    let trace = serde_json::to_value(&result).unwrap();
    let attempt = &trace["upstreamAttempts"][0];
    assert_eq!(attempt["phase"], "primary");
    assert_eq!(
        attempt["endpoint"],
        "https://authr3.dreamtonics.com.cn/api/v1/client/my_licenses"
    );
    assert!(attempt["status"].is_null());
    assert_eq!(attempt["errorClass"], "transport");
    assert_eq!(result.status, 599);
}

#[test]
fn upstream_attempt_keeps_refreshed_credentials_when_retry_transport_fails() {
    let mut task = directory_task("licenses");
    task.access_token = Some(refresh_token_claims(
        DREAMTONICS_CN_ISSUER,
        Some("test-client"),
    ));
    task.refresh_token = Some(refresh_token_claims(
        DREAMTONICS_CN_ISSUER,
        Some("test-client"),
    ));
    let new_access = refresh_token_claims(DREAMTONICS_CN_ISSUER, Some("test-client"));
    let new_refresh = refresh_token_claims(DREAMTONICS_CN_ISSUER, Some("test-client"));
    let mut request_count = 0;
    let mut transport = move |_request: DreamtonicsHttpRequest| {
        request_count += 1;
        match request_count {
            1 => Ok((401, "unauthorized".to_string())),
            2 => Ok((
                200,
                serde_json::json!({
                    "access_token": new_access.clone(),
                    "refresh_token": new_refresh.clone(),
                    "expires_in": 3600
                })
                .to_string(),
            )),
            3 => Err("upstream request failed"),
            _ => panic!("unexpected upstream call"),
        }
    };

    let result = dreamtonics_request_with_transport(&task, &mut transport);
    assert_eq!(result.status, 599);
    assert!(result.refreshed_access_token.is_some());
    assert!(result.refreshed_refresh_token.is_some());
    assert_eq!(result.upstream_attempts.len(), 3);
    assert_eq!(result.upstream_attempts[2].phase, "retry");
    assert_eq!(result.upstream_attempts[2].error_class, Some("transport"));
}

#[test]
fn product_logo_license_failure_is_retained_in_attempt_trace() {
    let directory = TestDirectory::new();
    let session_path = directory.0.join("app/sessions/session");
    let mut task = directory_task("product_logos");
    task.keys = vec!["missing-product".to_string()];
    task.access_token = Some(refresh_token_claims(
        DREAMTONICS_CN_ISSUER,
        Some("test-client"),
    ));
    let recorder = UpstreamAttemptRecorder::new();
    let result = assets::fetch_product_logos_with_request(
        &session_path,
        &task.keys,
        task.access_token.as_deref().unwrap(),
        &recorder,
        |_, _| Err("upstream request failed"),
        |_, _| None,
    );
    assert!(result.unwrap().is_empty());
    let attempt = recorder.into_attempts().pop().unwrap();
    assert_eq!(attempt.phase, "logo_licenses");
    assert_eq!(attempt.error_class, Some("transport"));
    assert!(attempt.status.is_none());
}

#[test]
fn binary_attempt_records_failed_image_download_without_body() {
    let recorder = UpstreamAttemptRecorder::new();
    let bytes = request_trace::recorded_binary_get(
        &recorder,
        "image_download",
        "https://cdn.dreamtonics.com/image.png?signature=secret",
        1024,
        || Ok((503, b"binary-error-body".to_vec())),
    );
    assert!(bytes.is_none());
    let trace = serde_json::to_value(recorder.into_attempts()).unwrap();
    let attempt = &trace[0];
    assert_eq!(attempt["bodyKind"], "binary");
    assert!(attempt["responseBody"].is_null());
    assert_eq!(attempt["status"], 503);
    assert_eq!(attempt["errorClass"], "http_status");
    assert_eq!(attempt["endpoint"], "https://cdn.dreamtonics.com/image.png");
    assert!(!trace.to_string().contains("signature=secret"));
    assert!(!trace.to_string().contains("binary-error-body"));
}

#[test]
fn upstream_attempt_bodies_are_utf8_safe_and_within_both_budgets() {
    let recorder = UpstreamAttemptRecorder::new();
    let response_body = "界".repeat(request_trace::MAX_UPSTREAM_ATTEMPT_BODY_BYTES);
    for index in 0..5 {
        recorded_dreamtonics_http(
            &recorder,
            "primary",
            "GET",
            &format!("https://authr3.dreamtonics.com/path/{index}"),
            false,
            || Ok((200, response_body.clone())),
        )
        .unwrap();
    }
    let attempts = recorder.into_attempts();
    assert_eq!(attempts.len(), 5);
    let body_bytes = attempts
        .iter()
        .map(|attempt| attempt.response_body.as_ref().unwrap().len())
        .sum::<usize>();
    assert!(body_bytes <= request_trace::MAX_UPSTREAM_ATTEMPT_TOTAL_BODY_BYTES);
    assert_eq!(attempts[4].response_body.as_deref(), Some("界"));
    assert!(attempts[4].body_truncated);
    for attempt in attempts.iter().take(4) {
        assert_eq!(
            attempt.response_body.as_ref().unwrap().len(),
            request_trace::MAX_UPSTREAM_ATTEMPT_BODY_BYTES - 1
        );
        assert!(attempt.body_truncated);
    }
}

#[test]
fn serialized_completion_payload_fits_a_bounded_large_body_envelope() {
    let recorder = UpstreamAttemptRecorder::new();
    let trace_body = "\0".repeat(request_trace::MAX_UPSTREAM_ATTEMPT_BODY_BYTES);
    for index in 0..4 {
        recorded_dreamtonics_http(
            &recorder,
            "primary",
            "GET",
            &format!("https://authr3.dreamtonics.com/path/{index}"),
            false,
            || Ok((200, trace_body.clone())),
        )
        .unwrap();
    }
    let mut result = DreamtonicsRequestResult::failed("licenses", "request failed");
    result.response_body = "\0".repeat(MAX_DREAMTONICS_RESPONSE_BYTES);
    result.upstream_attempts = recorder.into_attempts();
    let serialized_bytes = serde_json::to_vec(&result).unwrap().len();

    assert!(serialized_bytes > 14_000_000);
    assert!(serialized_bytes < 15_000_000);
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
        || Ok("https://test.invalid/licenses".to_string()),
        |_| Ok(serde_json::json!({"data":[{"id":"x","productId":"untrusted","logo":"http://dreamtonics.com/image.png"}]})),
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
        || Ok("https://test.invalid/licenses".to_string()),
        |_| Ok(serde_json::json!({"data":[{"id":"oversized-cache","productId":"oversized","imageUrl":"https://img.dreamtonics.com/a.png"}]})),
        |_, _| Some(too_large.clone()),
    )
    .unwrap();
    assert!(logos.is_empty());
    assert!(!metadata_directory.join("oversized-cache.png").exists());

    let invalid_keys = vec!["key".to_string(); 513];
    assert!(assets::fetch_product_logos_with(
        &session_path,
        &invalid_keys,
        || panic!("invalid key count must fail before endpoint resolution"),
        |_| panic!("invalid key count must fail before network access"),
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

#[test]
fn refresh_authority_selects_only_the_exact_trusted_regional_issuer() {
    let international = refresh_token_claims(super::DREAMTONICS_ISSUER, Some("test-client"));
    assert_eq!(
        refresh_authority(&international),
        Some(("test-client".to_string(), super::DREAMTONICS_TOKEN_ENDPOINT))
    );

    let regional = refresh_token_claims(super::DREAMTONICS_CN_ISSUER, Some("test-client"));
    assert_eq!(
        refresh_authority(&regional),
        Some((
            "test-client".to_string(),
            super::DREAMTONICS_CN_TOKEN_ENDPOINT
        ))
    );

    for (issuer, client_id) in [
        (
            "https://account.dreamtonics.com.cn.evil.example/realms/Dreamtonics",
            Some("test-client"),
        ),
        (
            "https://account.dreamtonics.example/realms/Dreamtonics",
            Some("test-client"),
        ),
        (
            "http://account.dreamtonics.com.cn/realms/Dreamtonics",
            Some("test-client"),
        ),
        (
            "https://account.dreamtonics.com.cn/realms/Other",
            Some("test-client"),
        ),
        (super::DREAMTONICS_CN_ISSUER, None),
        (super::DREAMTONICS_CN_ISSUER, Some("invalid client")),
    ] {
        assert_eq!(
            refresh_authority(&refresh_token_claims(issuer, client_id)),
            None
        );
    }
}

#[test]
fn access_token_issuer_selects_only_the_fixed_regional_api_bases() {
    let international = refresh_token_claims(super::DREAMTONICS_ISSUER, Some("test-client"));
    assert_eq!(
        dreamtonics_api_base(&international),
        Some(super::DREAMTONICS_API_BASE)
    );

    let china = refresh_token_claims(super::DREAMTONICS_CN_ISSUER, Some("test-client"));
    assert_eq!(
        dreamtonics_api_base(&china),
        Some(super::DREAMTONICS_CN_API_BASE)
    );

    for token in [
        refresh_token_claims(
            "https://account.dreamtonics.com.cn.evil.test/realms/Dreamtonics",
            Some("client"),
        ),
        refresh_token_claims(
            "http://account.dreamtonics.com.cn/realms/Dreamtonics",
            Some("client"),
        ),
        refresh_token_claims(
            "https://account.dreamtonics.com.cn/realms/Other",
            Some("client"),
        ),
        "not-a-jwt".to_string(),
    ] {
        assert_eq!(dreamtonics_api_base(&token), None);
    }
    assert!(requires_regional_api("licenses"));
    assert!(requires_regional_api("quick_licenses"));
    assert!(requires_regional_api("quick_devices"));
    assert!(requires_regional_api("quick_login"));
    assert!(requires_regional_api("offline_activate"));
    assert!(requires_regional_api("offline_deactivate"));
    assert!(requires_regional_api("product_download_url"));
    assert!(requires_regional_api("model_download_url"));
    assert!(!requires_regional_api("custom_request"));
    assert!(!requires_regional_api("refresh"));
    assert!(!requires_regional_api("product_logos"));
}

#[test]
fn fixed_dreamtonics_endpoints_follow_the_selected_api_region() {
    let endpoints = [
        (
            super::DREAMTONICS_LICENSES_ENDPOINT,
            "https://authr3.dreamtonics.com.cn/api/v1/client/my_licenses",
        ),
        (
            "https://authr3.dreamtonics.com/api/v1/client/my_devices?native_product=Synthesizer+V+Studio+2+Pro",
            "https://authr3.dreamtonics.com.cn/api/v1/client/my_devices?native_product=Synthesizer+V+Studio+2+Pro",
        ),
        (
            "https://authr3.dreamtonics.com/api/v1/client/enroll_device",
            "https://authr3.dreamtonics.com.cn/api/v1/client/enroll_device",
        ),
        (
            "https://authr3.dreamtonics.com/api/v1/client/activate_offline_license",
            "https://authr3.dreamtonics.com.cn/api/v1/client/activate_offline_license",
        ),
        (
            "https://authr3.dreamtonics.com/api/v1/client/deactivate_offline_license",
            "https://authr3.dreamtonics.com.cn/api/v1/client/deactivate_offline_license",
        ),
        (
            "https://authr3.dreamtonics.com/api/v1/update/get_download_url?product=sample-product",
            "https://authr3.dreamtonics.com.cn/api/v1/update/get_download_url?product=sample-product",
        ),
        (
            "https://authr3.dreamtonics.com/api/v1/model/request_model_download",
            "https://authr3.dreamtonics.com.cn/api/v1/model/request_model_download",
        ),
    ];
    for (endpoint, expected_china_endpoint) in endpoints {
        let china = replace_dreamtonics_api_base(
            endpoint,
            super::DREAMTONICS_API_BASE,
            super::DREAMTONICS_CN_API_BASE,
        )
        .unwrap();
        assert_eq!(china, expected_china_endpoint);
        assert_eq!(
            replace_dreamtonics_api_base(
                &china,
                super::DREAMTONICS_CN_API_BASE,
                super::DREAMTONICS_API_BASE,
            )
            .unwrap(),
            endpoint
        );
    }
    assert_eq!(
        replace_dreamtonics_api_base(
            "https://service.example.test/api",
            super::DREAMTONICS_API_BASE,
            super::DREAMTONICS_CN_API_BASE,
        ),
        None
    );
}

#[test]
fn fixed_request_rejects_an_untrusted_access_issuer_before_network_io() {
    for issuer in [
        "https://account.dreamtonics.com.cn.evil.test/realms/Dreamtonics",
        "http://account.dreamtonics.com.cn/realms/Dreamtonics",
        "https://account.dreamtonics.com.cn/realms/Other",
    ] {
        let mut task = directory_task("licenses");
        task.access_token = Some(refresh_token_claims(issuer, Some("test-client")));
        let result = dreamtonics_request(&task);
        assert_eq!(result.status, 599);
        assert_eq!(
            result.error,
            Some("access token issuer is missing or invalid")
        );
    }
}

#[test]
fn local_product_logo_action_does_not_require_a_jwt_issuer() {
    let mut task = directory_task("product_logos");
    task.access_token = Some("opaque-present-access-token".into());
    let result = dreamtonics_product_logos(&task, Path::new("unused"));
    assert_eq!(result.status, 200);
    assert_eq!(result.response_body, r#"{"logos":[]}"#);
}

fn refresh_token_claims(issuer: &str, client_id: Option<&str>) -> String {
    let mut claims = serde_json::json!({ "iss": issuer });
    if let Some(client_id) = client_id {
        claims["azp"] = serde_json::Value::String(client_id.to_string());
    }
    format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes())
    )
}
