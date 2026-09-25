#[allow(dead_code)]
#[path = "../connector/src/service_selection.rs"]
mod service_selection;

#[test]
fn service_address_must_be_a_clean_origin() {
    assert_eq!(
        service_selection::validate_service_url(" https://service.example.test/ ").as_deref(),
        Ok("https://service.example.test")
    );
    assert_eq!(
        service_selection::validate_service_url("http://localhost:8787").as_deref(),
        Ok("http://localhost:8787")
    );
    for value in [
        "http://service.example.test",
        "https://user@service.example.test",
        "https://service.example.test/path",
        "https://service.example.test/?token=1",
        "https://service.example.test/#fragment",
        "http://localhost.evil.example",
    ] {
        assert!(
            service_selection::validate_service_url(value).is_err(),
            "{value}"
        );
    }
}
