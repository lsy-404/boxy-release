#[allow(dead_code)]
#[path = "../connector/src/launch.rs"]
mod launch;

use std::path::Path;

#[test]
fn ordinary_release_names_use_the_current_service() {
    assert_eq!(
        launch::resolve_server_url(None, None, Some(Path::new("boxy-windows-x64.exe"))).as_deref(),
        Ok("https://boxy.voidcarve.com")
    );
}

#[test]
fn service_domains_in_executable_names_are_inferred() {
    assert_eq!(
        launch::resolve_server_url(None, None, Some(Path::new("service.example.test.exe")))
            .as_deref(),
        Ok("https://service.example.test")
    );
    assert_eq!(
        launch::resolve_server_url(
            None,
            None,
            Some(Path::new("boxy.service.example.test (1).exe")),
        )
        .as_deref(),
        Ok("https://boxy.service.example.test")
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
        .as_deref(),
        Ok("https://command.example.test")
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
        .as_deref(),
        Ok("https://environment.example.test")
    );
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
        launch::resolve_server_url(Some("http://127.0.0.1:43123"), None, None).as_deref(),
        Ok("http://127.0.0.1:43123")
    );
}
