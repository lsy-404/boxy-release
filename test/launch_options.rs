#[allow(dead_code)]
#[path = "../connector/src/launch.rs"]
mod launch;

#[test]
fn selected_service_origin_is_normalized() {
    assert_eq!(
        launch::validate_service_url("service.example.test").unwrap(),
        "https://service.example.test"
    );
    assert_eq!(
        launch::validate_service_url("https://service.example.test/").unwrap(),
        "https://service.example.test"
    );
    assert_eq!(
        launch::validate_service_url("http://127.0.0.1:43123").unwrap(),
        "http://127.0.0.1:43123"
    );
}

#[test]
fn invalid_or_non_origin_urls_are_rejected_without_fallback() {
    for value in [
        "not a URL",
        "http://service.example.test",
        "https://user@service.example.test",
        "https://service.example.test/path",
        "https://service.example.test/?token=1",
        "https://service.example.test/#fragment",
    ] {
        assert!(
            launch::validate_service_url(value).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn service_url_is_inferred_from_valid_executable_names() {
    use std::path::Path;

    assert_eq!(
        launch::inferred_service_url(Path::new("service.example.test.exe")),
        Some("https://service.example.test".to_string())
    );
    assert_eq!(
        launch::inferred_service_url(Path::new("boxy.service.example.test (1).exe")),
        Some("https://boxy.service.example.test".to_string())
    );
    assert_eq!(
        launch::inferred_service_url(Path::new("ordinary-boxy-windows-x64.exe")),
        None
    );
    assert_eq!(
        launch::inferred_service_url(Path::new(
            "Boxy.Service.Example.test.app/Contents/MacOS/Boxy"
        )),
        Some("https://boxy.service.example.test".to_string())
    );
}

#[test]
fn invalid_encoded_hostname_tokens_are_not_inferred() {
    use std::path::Path;

    assert_eq!(
        launch::inferred_service_url(Path::new("boxy%2Eservice%2Eexample%2Etest.exe")),
        None
    );
}
