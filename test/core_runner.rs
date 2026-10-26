use super::*;
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

#[derive(Default)]
struct TestHost {
    opened: Mutex<Option<Url>>,
    browser_cookies: Mutex<Vec<BrowserCookie>>,
    external_opened: Mutex<Option<Url>>,
}

#[test]
fn list_task_returns_an_empty_result_for_a_missing_absolute_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("missing-database");

    let result = list_task(&root, &[".".into()]).unwrap();
    let unsafe_path = list_task(&root, &["../outside".into()]).unwrap();

    assert!(result["files"].as_array().unwrap().is_empty());
    assert!(!root.exists());
    assert_eq!(unsafe_path["files"][0]["error"], "unsafe_path");
    assert!(matches!(
        list_task(Path::new("relative"), &[]),
        Err(error) if error == "invalid_root"
    ));
}

impl ConnectorHost for TestHost {
    fn open_editor(&self, url: Url, cookies: Vec<BrowserCookie>) -> Result<(), String> {
        *self.opened.lock().unwrap() = Some(url);
        *self.browser_cookies.lock().unwrap() = cookies;
        Ok(())
    }

    fn open_external(&self, url: Url) -> Result<(), String> {
        *self.external_opened.lock().unwrap() = Some(url);
        Ok(())
    }

    fn approve_process(&self, _executable: &str, _args: &[String], _cwd: Option<&Path>) -> bool {
        false
    }
    fn should_stop(&self) -> bool {
        false
    }
}

struct StopAfterChecks {
    checks: AtomicUsize,
    stop_after: usize,
}

impl ConnectorHost for StopAfterChecks {
    fn open_editor(&self, _url: Url, _cookies: Vec<BrowserCookie>) -> Result<(), String> {
        unreachable!()
    }

    fn open_external(&self, _url: Url) -> Result<(), String> {
        unreachable!()
    }

    fn approve_process(&self, _executable: &str, _args: &[String], _cwd: Option<&Path>) -> bool {
        false
    }

    fn should_stop(&self) -> bool {
        self.checks.fetch_add(1, Ordering::SeqCst) >= self.stop_after
    }
}

#[test]
fn hello_is_anonymous_and_task_parameters_are_strict_objects() {
    let root = std::env::temp_dir()
        .join("scope")
        .to_string_lossy()
        .into_owned();
    let hello = hello_body("windows", vec!["fs.read"], "ephemeral").unwrap();
    assert_eq!(
        hello,
        json!({
            "protocolVersion": 4,
            "platform": "windows",
            "capabilities": ["fs.read"],
            "publicKey": "ephemeral"
        })
    );
    assert_eq!(
        validate_task(
            "fs.read",
            &json!({"root":root,"paths":[],"maxFileBytes":1,"maxTotalBytes":1})
        ),
        Ok(())
    );
    assert_eq!(
        validate_task("fs.read", &json!([])),
        Err("invalid_params".into())
    );
    assert_eq!(
        validate_task("fs.read", &json!({"root":"C:/scope"})),
        Err("invalid_params".into())
    );
    assert_eq!(
        validate_task("vendor.action", &json!({})),
        Err("unsupported_task".into())
    );
}

#[test]
fn execute_task_accepts_worker_one_mib_read_limits_for_a_small_file() {
    let temp = tempfile::tempdir().unwrap();
    let contents = b"session data";
    std::fs::write(temp.path().join("session.json"), contents).unwrap();
    let task = RemoteTask {
        id: "read1".into(),
        kind: "fs.read".into(),
        params: json!({
            "root":temp.path().to_string_lossy(),
            "paths":["session.json"],
            "maxFileBytes":1024 * 1024,
            "maxTotalBytes":1024 * 1024,
        }),
    };
    validate_task(&task.kind, &task.params).unwrap();
    let key = SigningKey::from_bytes(&[13; 32]);
    let mut watches = HashMap::new();
    let mut mounts = HashMap::new();

    let result = execute_task(
        &Config {
            server_url: "http://127.0.0.1:1".into(),
        },
        &key,
        "/api/connections/conn",
        &task,
        &TestHost::default(),
        &mut watches,
        &mut mounts,
    )
    .unwrap();

    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(result["files"][0]["contentBase64"].as_str().unwrap())
            .unwrap(),
        contents
    );
}

#[test]
fn operation_level_file_errors_produce_failed_receipts() {
    assert!(task_result_succeeded(
        "fs.read",
        &json!({"files":[{"ok":true,"contentBase64":"YQ=="}]})
    ));
    assert!(!task_result_succeeded(
        "fs.read",
        &json!({"files":[{"ok":false,"error":"file_unavailable"}]})
    ));
    assert!(!task_result_succeeded(
        "fs.patch",
        &json!({"files":[],"failed":true})
    ));
    assert!(!task_result_succeeded(
        "http.request",
        &json!({"status":599,"error":"transport"})
    ));
}

#[test]
fn signed_v4_connection_waits_for_ready_before_reading_explicit_files() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("fact.txt"), b"worker requested this").unwrap();
    let root_path = root.path().to_string_lossy().into_owned();
    let server = thread::spawn(move || {
        let (mut stream, request, body) = accept_json(&listener);
        assert_eq!(
            request.split_whitespace().take(2).collect::<Vec<_>>(),
            ["POST", "/api/connect"]
        );
        let hello: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(hello["protocolVersion"], 4);
        assert_eq!(hello["platform"], platform_name());
        let keys: Vec<_> = hello
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["capabilities", "platform", "protocolVersion", "publicKey"]
        );
        let public_key = URL_SAFE_NO_PAD
            .decode(hello["publicKey"].as_str().unwrap())
            .unwrap();
        let verifier = VerifyingKey::from_bytes(&public_key.try_into().unwrap()).unwrap();
        respond(
            &mut stream,
            201,
            &json!({"protocolVersion":4,"connectionId":"conn-1","browserUrl":format!("http://{address}/"),"browserCookies":[{"name":"boxy_connection","value":"test-session"}],"expiresAt":future_expiry_ms()}),
        );

        let (mut stream, request, _) = accept_json(&listener);
        verify_signed(&request, "GET", "/api/connections/conn-1", &verifier);
        respond(
            &mut stream,
            200,
            &json!({"protocolVersion":4,"state":"awaiting_auth","tasks":[]}),
        );

        let (mut stream, request, _) = accept_json(&listener);
        verify_signed(&request, "GET", "/api/connections/conn-1", &verifier);
        respond(
            &mut stream,
            200,
            &json!({"protocolVersion":4,"state":"ready","tasks":[{"id":"work_a","kind":"fs.read","params":{"root":root_path,"paths":["fact.txt"],"maxFileBytes":1024,"maxTotalBytes":2048}}]}),
        );

        let (mut stream, request, body) = accept_json(&listener);
        verify_signed(
            &request,
            "POST",
            "/api/connections/conn-1/tasks/work_a/receipt",
            &verifier,
        );
        let receipt: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(receipt["ok"], true);
        assert_eq!(
            receipt["result"]["files"][0]["contentBase64"],
            URL_SAFE_NO_PAD.encode(b"worker requested this")
        );
        respond(&mut stream, 200, &json!({"accepted":true}));

        let (mut stream, request, _) = accept_json(&listener);
        verify_signed(&request, "GET", "/api/connections/conn-1", &verifier);
        respond(
            &mut stream,
            200,
            &json!({"protocolVersion":4,"state":"closed","tasks":[]}),
        );

        let (mut stream, request, _) = accept_json(&listener);
        verify_signed(&request, "POST", "/api/connections/conn-1/close", &verifier);
        respond(&mut stream, 200, &json!({"closed":true}));
    });
    let host = TestHost::default();
    run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap();
    assert_eq!(host.opened.lock().unwrap().as_ref().unwrap().path(), "/");
    assert_eq!(
        *host.browser_cookies.lock().unwrap(),
        [BrowserCookie {
            name: "boxy_connection".into(),
            value: "test-session".into(),
        }]
    );
    server.join().unwrap();
}

#[test]
fn connection_expiry_rejects_non_numeric_worker_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, request, _) = accept_json(&listener);
        assert_eq!(request_path(&request), "/api/connect");
        respond(
            &mut stream,
            201,
            &json!({"protocolVersion":4,"connectionId":"conn-invalid-expiry","browserUrl":format!("http://{address}/"),"browserCookies":[{"name":"boxy_connection","value":"test-session"}],"expiresAt":"2030-10-25T00:00:00Z"}),
        );
    });
    let host = TestHost::default();
    let error = run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap_err();

    assert!(
        error.contains("invalid type: string"),
        "unexpected error: {error}"
    );
    assert!(host.opened.lock().unwrap().is_none());
    server.join().unwrap();
}

#[test]
fn connection_rejects_invalid_browser_cookie_before_opening_editor() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _, _) = accept_json(&listener);
        respond(
            &mut stream,
            201,
            &json!({"protocolVersion":4,"connectionId":"invalid_cookie","browserUrl":format!("http://{address}/"),"browserCookies":[{"name":"session; Secure","value":"token"}],"expiresAt":future_expiry_ms()}),
        );
    });
    let host = TestHost::default();
    let error = run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap_err();
    assert!(error.contains("invalid browser cookie"));
    assert!(host.opened.lock().unwrap().is_none());
    server.join().unwrap();
}

#[test]
fn connect_response_requires_browser_cookies() {
    let response = json!({
        "protocolVersion": 4,
        "connectionId": "connection",
        "browserUrl": "https://boxy.example.test/",
        "expiresAt": future_expiry_ms(),
    });

    assert!(serde_json::from_value::<ConnectResponse>(response).is_err());
}

#[test]
fn browser_cookie_validation_rejects_value_attribute_injection() {
    let error = validate_browser_cookies(&[BrowserCookie {
        name: "connection".into(),
        value: "token; Domain=attacker.example".into(),
    }])
    .unwrap_err();

    assert!(error.contains("invalid browser cookie"));
}

#[test]
fn browser_cookie_validation_rejects_non_token_names_and_non_octet_values() {
    for cookie in [
        BrowserCookie {
            name: "has space".into(),
            value: "token".into(),
        },
        BrowserCookie {
            name: "name\r\nInjected".into(),
            value: "token".into(),
        },
        BrowserCookie {
            name: "name".into(),
            value: "token\r\nInjected".into(),
        },
        BrowserCookie {
            name: "name".into(),
            value: "token\0injected".into(),
        },
    ] {
        assert!(validate_browser_cookies(&[cookie]).is_err());
    }
}

#[test]
fn upgrade_handshake_opens_safe_external_url_and_stops() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, request, _) = accept_json(&listener);
        assert_eq!(request_path(&request), "/api/connect");
        respond_status(
            &mut stream,
            426,
            &json!({"openExternalUrl":"https://updates.example/releases/latest"}),
        );
    });
    let host = TestHost::default();
    run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap();
    assert_eq!(
        host.external_opened
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .as_str(),
        "https://updates.example/releases/latest"
    );
    assert!(host.opened.lock().unwrap().is_none());
    server.join().unwrap();
}

#[test]
fn upgrade_handshake_rejects_unsafe_urls_and_does_not_open_on_not_found() {
    for value in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "http://updates.example/releases/latest",
        "https://user:pass@updates.example/releases/latest",
        "https://@updates.example/releases/latest",
        "/releases/latest",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let expected = value.to_string();
        let value = value.to_string();
        let server = thread::spawn(move || {
            let (mut stream, request, _) = accept_json(&listener);
            assert_eq!(request_path(&request), "/api/connect");
            respond_status(&mut stream, 426, &json!({"openExternalUrl":value}));
        });
        let host = TestHost::default();
        let error = run_connected(
            &Config {
                server_url: format!("http://{address}"),
            },
            &host,
        )
        .unwrap_err();
        assert!(
            error.contains("invalid external URL"),
            "{expected}: {error}"
        );
        assert!(host.external_opened.lock().unwrap().is_none());
        server.join().unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, request, _) = accept_json(&listener);
        assert_eq!(request_path(&request), "/api/connect");
        respond_status(&mut stream, 404, &json!({"error":"not_found"}));
    });
    let host = TestHost::default();
    let error = run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap_err();
    assert!(error.contains("404"));
    assert!(host.external_opened.lock().unwrap().is_none());
    server.join().unwrap();
}

#[test]
fn upgrade_poll_opens_external_url_and_stops_without_repolling() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _, body) = accept_json(&listener);
        let hello: Value = serde_json::from_slice(&body).unwrap();
        let public_key = URL_SAFE_NO_PAD
            .decode(hello["publicKey"].as_str().unwrap())
            .unwrap();
        let verifier = VerifyingKey::from_bytes(&public_key.try_into().unwrap()).unwrap();
        respond(
            &mut stream,
            201,
            &json!({"protocolVersion":4,"connectionId":"conn-upgrade","browserUrl":format!("http://{address}/"),"browserCookies":[{"name":"boxy_connection","value":"test-session"}],"expiresAt":future_expiry_ms()}),
        );

        let (mut stream, request, _) = accept_json(&listener);
        verify_signed(&request, "GET", "/api/connections/conn-upgrade", &verifier);
        respond_status(
            &mut stream,
            426,
            &json!({"openExternalUrl":"https://updates.example/releases/latest"}),
        );
    });
    let host = TestHost::default();
    run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap();
    assert_eq!(
        host.external_opened
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .as_str(),
        "https://updates.example/releases/latest"
    );
    server.join().unwrap();
}

#[test]
fn awaiting_auth_task_is_rejected_before_execution_and_connection_closes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _, _) = accept_json(&listener);
        respond(
            &mut stream,
            201,
            &json!({"protocolVersion":4,"connectionId":"conn-2","browserUrl":format!("http://{address}/"),"browserCookies":[{"name":"boxy_connection","value":"test-session"}],"expiresAt":future_expiry_ms()}),
        );
        let (mut stream, request, _) = accept_json(&listener);
        let path = request_path(&request);
        respond(
            &mut stream,
            200,
            &json!({"protocolVersion":4,"state":"awaiting_auth","tasks":[{"id":"bad","kind":"fs.read","params":{}}]}),
        );
        assert_eq!(path, "/api/connections/conn-2");
        let (mut stream, request, _) = accept_json(&listener);
        assert_eq!(request_path(&request), "/api/connections/conn-2/close");
        respond(&mut stream, 200, &json!({"closed":true}));
    });
    let host = TestHost::default();
    let error = run_connected(
        &Config {
            server_url: format!("http://{address}"),
        },
        &host,
    )
    .unwrap_err();
    assert!(error.contains("before authorization"));
    server.join().unwrap();
}

#[test]
fn upload_reports_signed_progress_and_ignores_progress_endpoint_failures() {
    let temp = tempfile::tempdir().unwrap();
    let contents = b"generic upload";
    std::fs::write(temp.path().join("upload.bin"), contents).unwrap();
    let expected_hash = sha256_base64url(contents);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let key = SigningKey::from_bytes(&[7; 32]);
    let verifier = key.verifying_key();
    let server = thread::spawn(move || {
        let (mut stream, request, body) = accept_json(&listener);
        verify_signed(
            &request,
            "POST",
            "/api/connections/conn/tasks/upload1/progress",
            &verifier,
        );
        let progress: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(progress["phase"], "uploading");
        assert_eq!(progress["completedBytes"], 0);
        assert_eq!(progress["totalBytes"], contents.len());
        respond(&mut stream, 503, &json!({"ignored":true}));

        let (mut stream, request, body) = accept_json(&listener);
        verify_signed(
            &request,
            "POST",
            "/api/connections/conn/tasks/upload1/content",
            &verifier,
        );
        assert_eq!(body, contents);
        respond(&mut stream, 200, &json!({"accepted":true}));

        let (mut stream, request, body) = accept_json(&listener);
        verify_signed(
            &request,
            "POST",
            "/api/connections/conn/tasks/upload1/progress",
            &verifier,
        );
        let progress: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(progress["phase"], "uploading");
        assert_eq!(progress["completedBytes"], contents.len());
        assert_eq!(progress["totalBytes"], contents.len());
        respond(&mut stream, 503, &json!({"ignored":true}));
    });

    let task = RemoteTask {
        id: "upload1".into(),
        kind: "fs.upload".into(),
        params: json!({
            "path":"upload.bin",
            "expectedSha256":expected_hash,
            "expectedSize":contents.len(),
        }),
    };
    let host = TestHost::default();
    let result = transfer_file(
        &Config {
            server_url: format!("http://{address}"),
        },
        &key,
        "/api/connections/conn",
        temp.path(),
        Path::new(".boxy-no-protected-path"),
        &task,
        "fs.upload",
        &host,
    )
    .unwrap();

    assert_eq!(result["size"], contents.len());
    server.join().unwrap();
}

#[test]
fn download_reports_progress_and_stops_before_installing_a_partial_file() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let key = SigningKey::from_bytes(&[9; 32]);
    let verifier = key.verifying_key();
    let contents = vec![12; 16 * 1024];
    let total_size = contents.len();
    let server = thread::spawn(move || {
        let (mut stream, request, body) = accept_json(&listener);
        verify_signed(
            &request,
            "POST",
            "/api/connections/conn/tasks/download1/progress",
            &verifier,
        );
        let progress: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(progress["phase"], "downloading");
        assert_eq!(progress["completedBytes"], 0);
        assert!(progress.get("totalBytes").is_none());
        respond(&mut stream, 200, &json!({"accepted":true}));

        let (mut stream, request, _) = accept_json(&listener);
        verify_signed(
            &request,
            "GET",
            "/api/connections/conn/tasks/download1/content",
            &verifier,
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {total_size}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.write_all(&contents).unwrap();

        let (mut stream, request, body) = accept_json(&listener);
        verify_signed(
            &request,
            "POST",
            "/api/connections/conn/tasks/download1/progress",
            &verifier,
        );
        let progress: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(progress["phase"], "downloading");
        assert_eq!(progress["completedBytes"], 0);
        assert_eq!(progress["totalBytes"], total_size);
        respond(&mut stream, 200, &json!({"accepted":true}));
    });

    let task = RemoteTask {
        id: "download1".into(),
        kind: "fs.download".into(),
        params: json!({"path":"download.bin","maxBytes":65536}),
    };
    let host = StopAfterChecks {
        checks: AtomicUsize::new(0),
        stop_after: 2,
    };
    let error = transfer_file(
        &Config {
            server_url: format!("http://{address}"),
        },
        &key,
        "/api/connections/conn",
        temp.path(),
        Path::new(".boxy-no-protected-path"),
        &task,
        "fs.download",
        &host,
    )
    .unwrap_err();

    assert_eq!(error, "cancelled");
    assert!(!temp.path().join("download.bin").exists());
    server.join().unwrap();
}

#[test]
fn upload_reader_checks_cancellation_before_reading_the_source() {
    let key = SigningKey::from_bytes(&[11; 32]);
    let config = Config {
        server_url: "http://127.0.0.1:1".into(),
    };
    let host = StopAfterChecks {
        checks: AtomicUsize::new(0),
        stop_after: 0,
    };
    let mut progress = TransferProgress::new(
        &config,
        &key,
        "/api/connections/conn/tasks/upload/progress",
        "uploading",
        Some(6),
    );
    let mut reader = ProgressReader {
        reader: std::io::Cursor::new(b"source"),
        host: &host,
        progress: &mut progress,
        completed: 0,
    };
    let error = reader.read(&mut [0; 6]).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert_eq!(reader.reader.position(), 0);
    assert_eq!(reader.completed, 0);
}

fn accept_json(listener: &TcpListener) -> (TcpStream, String, Vec<u8>) {
    let (mut stream, _) = listener.accept().unwrap();
    let mut request = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        request.extend_from_slice(&buffer[..count]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    assert!(
        headers.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("X-Boxy-Connector-Version")
                    && value.trim() == env!("CARGO_PKG_VERSION")
            })
        }),
        "native control request omitted its connector version"
    );
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        request.extend_from_slice(&buffer[..count]);
    }
    let body = request[header_end..header_end + content_length].to_vec();
    (stream, headers, body)
}

fn future_expiry_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 120_000
}

fn request_path(request: &str) -> &str {
    request.split_whitespace().nth(1).unwrap()
}

fn verify_signed(request: &str, method: &str, path: &str, key: &VerifyingKey) {
    assert_eq!(request.split_whitespace().next(), Some(method));
    assert_eq!(request_path(request), path);
    let nonce = request
        .lines()
        .find_map(|line| {
            line.strip_prefix("X-Connector-Nonce: ")
                .or_else(|| line.strip_prefix("x-connector-nonce: "))
        })
        .unwrap();
    let signature = request
        .lines()
        .find_map(|line| {
            line.strip_prefix("X-Connector-Signature: ")
                .or_else(|| line.strip_prefix("x-connector-signature: "))
        })
        .unwrap();
    let signature = URL_SAFE_NO_PAD.decode(signature).unwrap();
    key.verify_strict(
        format!("{method}\n{path}\n{nonce}").as_bytes(),
        &Signature::from_slice(&signature).unwrap(),
    )
    .unwrap();
}

fn respond(stream: &mut TcpStream, status: u16, value: &Value) {
    respond_status(stream, status, value);
}

fn respond_status(stream: &mut TcpStream, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).unwrap();
    let reason = match status {
        201 => "Created",
        404 => "Not Found",
        426 => "Upgrade Required",
        _ => "OK",
    };
    write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    stream.write_all(&body).unwrap();
}
