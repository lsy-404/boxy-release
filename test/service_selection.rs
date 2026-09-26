#[allow(dead_code)]
#[path = "../connector/src/service_selection.rs"]
mod service_selection;

use std::path::Path;

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

#[test]
fn filename_only_prefills_a_domain_style_remote() {
    assert_eq!(
        service_selection::inferred_service_url(Path::new("boxy-windows-x64.exe")),
        None
    );
    assert_eq!(
        service_selection::inferred_service_url(Path::new("service.example.test.exe")).as_deref(),
        Some("https://service.example.test")
    );
    assert_eq!(
        service_selection::inferred_service_url(Path::new("boxy.service.example.test (1).exe"))
            .as_deref(),
        Some("https://boxy.service.example.test")
    );
    assert_eq!(
        service_selection::inferred_service_url(Path::new(
            "service.example.test.app/Contents/MacOS/boxy"
        ))
        .as_deref(),
        Some("https://service.example.test")
    );
}
