use super::*;
#[test]
fn accepts_only_bounded_panel_requests() {
    assert_eq!(
        parse_request("version=1\nkind=chat.send\npayload_hex=6869\n"),
        Some(("chat.send", "hi".into()))
    );
    assert_eq!(
        parse_request("version=1\nkind=account.main.restore\npayload_hex=313233\n"),
        Some(("account.main.restore", "123".into()))
    );
    assert!(parse_request("version=1\nkind=account.source.select\npayload_hex=\n").is_none());
    assert!(parse_request("version=1\nkind=chat.send\npayload_hex=zz\n").is_none());
    assert!(parse_request("version=1\nkind=other\npayload_hex=61\n").is_none());
}

#[test]
fn creates_an_accessible_private_panel_directory() {
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join("panel");
    create_private(&path).unwrap();
    std::fs::write(path.join("status.txt"), "ok").unwrap();
    assert_eq!(
        std::fs::read_to_string(path.join("status.txt")).unwrap(),
        "ok"
    );
    assert!(create_private(&path).is_err());
}
#[test]
fn calls_existing_agent_and_preserves_conversation() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut data = Vec::new();
        loop {
            let mut chunk = [0u8; 1024];
            let count = socket.read(&mut chunk).unwrap();
            assert!(count > 0);
            data.extend_from_slice(&chunk[..count]);
            if let Some(end) = data.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&data[..end]);
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap();
                if data.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let request = String::from_utf8_lossy(&data);
        assert!(request.starts_with("POST /agent "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-secret"));
        let response = r#"{"conversationId":"conversation-1","messages":[{"role":"assistant","content":"hello"}]}"#;
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
    });
    let settings = ConnectSettings {
        http_api_enabled: false,
        http_agent_enabled: true,
        http_api_port: port,
        http_api_token: "test-secret".into(),
    };
    assert_eq!(
        send_chat(settings, "hi".into(), String::new()).unwrap(),
        ("conversation-1".into(), "hello".into())
    );
    server.join().unwrap();
}
#[test]
fn panel_directory_reuses_after_exit_and_rejects_concurrent_owner() {
    let parent = tempfile::tempdir().unwrap();
    let directory = parent.path().join("panel");
    let first = acquire_panel(&directory).unwrap();
    std::fs::write(directory.join("request.txt"), "stale").unwrap();
    std::fs::write(directory.join("status.txt"), "stale").unwrap();
    assert!(acquire_panel(&directory).is_err());
    drop(first);
    let second = acquire_panel(&directory).unwrap();
    assert!(!directory.join("request.txt").exists());
    assert!(!directory.join("status.txt").exists());
    drop(second);
}

#[test]
fn preferences_store_only_selected_path_and_survive_reopen() {
    let parent = tempfile::tempdir().unwrap();
    let directory = parent.path().join("panel");
    let lease = acquire_panel(&directory).unwrap();
    let settings_file = parent.path().join("runtime-settings.json");
    std::fs::write(
        &settings_file,
        r#"{"httpApiPort":17831,"httpApiToken":"private-test-value"}"#,
    )
    .unwrap();
    let state = PanelState::default();
    *state.directory.lock().unwrap() = Some(directory.clone());
    configure(&state, settings_file.clone()).unwrap();
    let saved = std::fs::read_to_string(directory.join("preferences.json")).unwrap();
    assert!(!saved.contains("private-test-value"));
    drop(lease);
    let _reopened = acquire_panel(&directory).unwrap();
    assert_eq!(preferences(&directory), Some(settings_file));
}
#[test]
fn lists_only_explicitly_supported_processes() {
    let mode = json!({"pid": 12, "processes": [{"pid": 12,"supported":false},{"pid": 20,"supported":true},{"pid": 30}]});
    assert_eq!(process_ids(&mode), vec![20]);
    assert!(process_ids(&json!({"pid": 12})).is_empty());
}
#[test]
fn refuses_preexisting_directory_without_private_permissions() {
    let parent = tempfile::tempdir().unwrap();
    let directory = parent.path().join("panel");
    std::fs::create_dir(&directory).unwrap();
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(acquire_panel(&directory).is_err());
}
