#[allow(dead_code)]
#[path = "../connector/src/launch.rs"]
mod launch;

use std::path::Path;

use boxy::{resolve_config, LaunchOptions};

#[test]
fn ordinary_release_names_do_not_select_a_service_implicitly() {
    assert_eq!(
        launch::resolve_server_url(None, None, Some(Path::new("boxy-windows-x64.exe"))).unwrap(),
        None
    );
}

#[test]
fn service_domains_in_executable_names_are_inferred() {
    assert_eq!(
        launch::resolve_server_url(None, None, Some(Path::new("service.example.test.exe")))
            .unwrap()
            .as_deref(),
        Some("https://service.example.test")
    );
    assert_eq!(
        launch::resolve_server_url(
            None,
            None,
            Some(Path::new("boxy.service.example.test (1).exe")),
        )
        .unwrap()
        .as_deref(),
        Some("https://boxy.service.example.test")
    );
}

#[test]
fn command_line_server_precedes_environment_and_filename() {
    assert_eq!(
        launch::resolve_server_url(
            Some("https://command.example.test"),
            Some("https://environment.example.test"),
            Some(Path::new("filename.example.test.exe")),
        )
        .unwrap()
        .as_deref(),
        Some("https://command.example.test")
    );
}

#[test]
fn environment_server_precedes_filename_inference() {
    assert_eq!(
        launch::resolve_server_url(
            None,
            Some("https://environment.example.test"),
            Some(Path::new("filename.example.test.exe")),
        )
        .unwrap()
        .as_deref(),
        Some("https://environment.example.test")
    );
}

#[test]
fn invalid_explicit_server_does_not_fall_back_to_environment_or_filename() {
    assert!(launch::resolve_server_url(
        Some("not a URL"),
        Some("https://environment.example.test"),
        Some(Path::new("filename.example.test.exe")),
    )
    .is_err());
    assert!(launch::resolve_server_url(
        None,
        Some("not a URL"),
        Some(Path::new("filename.example.test.exe")),
    )
    .is_err());
}

#[test]
fn server_arguments_preserve_the_session_path() {
    let arguments = launch::parse_arguments([
        "--server".to_string(),
        "https://service.example.test".to_string(),
        "--session".to_string(),
        "C:\\sessions\\test.svp".to_string(),
    ])
    .unwrap();

    assert_eq!(
        arguments.server_url.as_deref(),
        Some("https://service.example.test")
    );
    assert_eq!(
        arguments.session_path.as_deref(),
        Some(Path::new("C:\\sessions\\test.svp"))
    );
}

#[test]
fn malformed_and_non_origin_server_urls_are_rejected() {
    for value in [
        "not a URL",
        "http://service.example.test",
        "https://user@service.example.test",
        "https://service.example.test/path",
        "https://service.example.test/?token=1",
        "https://service.example.test/#fragment",
    ] {
        assert!(
            launch::resolve_server_url(Some(value), None, None).is_err(),
            "accepted {value}"
        );
    }
    assert_eq!(
        launch::resolve_server_url(Some("http://127.0.0.1:43123"), None, None)
            .unwrap()
            .as_deref(),
        Some("http://127.0.0.1:43123")
    );
}

#[test]
fn resolving_a_selected_url_builds_the_config_and_session_path() {
    let session = tempfile::NamedTempFile::new().unwrap();
    let options = LaunchOptions {
        server_url: None,
        session_path: session.path().to_path_buf(),
        session_path_explicit: true,
    };

    let config = resolve_config(&options, "https://service.example.test/").unwrap();
    assert_eq!(config.server_url, "https://service.example.test");
    assert_eq!(config.session_path, session.path());
}

#[test]
fn resolving_a_selected_url_rejects_an_invalid_origin() {
    let options = LaunchOptions {
        server_url: None,
        session_path: Path::new("missing.svp").to_path_buf(),
        session_path_explicit: true,
    };
    assert!(resolve_config(&options, "http://service.example.test").is_err());
}
