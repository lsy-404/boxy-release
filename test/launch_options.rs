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
