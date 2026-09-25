#[allow(dead_code)]
#[path = "../connector/src/service_selection.rs"]
mod service_selection;

use std::path::Path;

#[test]
fn executable_names_prefill_the_service_origin() {
    for name in ["boxy.a.b.exe", "a.b.exe", "BOXY.A.B.exe"] {
        assert_eq!(
            service_selection::inferred_service_url(Path::new(name)).as_deref(),
            Some("https://boxy.a.b")
        );
    }
    for name in ["boxy.a.b.app", "a.b.app"] {
        let path = format!("/Applications/{name}/Contents/MacOS/boxy");
        assert_eq!(
            service_selection::inferred_service_url(Path::new(&path)).as_deref(),
            Some("https://boxy.a.b")
        );
    }
}

#[test]
fn unrelated_and_invalid_names_do_not_select_a_service() {
    for name in [
        "boxy.exe",
        "boxy-windows-x64.exe",
        "boxy..example.exe",
        "a_b.example.exe",
    ] {
        assert!(service_selection::inferred_service_url(Path::new(name)).is_none());
    }
}

#[test]
fn service_address_must_be_a_clean_origin() {
    assert_eq!(
        service_selection::validate_service_url(" https://boxy.example.com/ ").as_deref(),
        Ok("https://boxy.example.com")
    );
    assert_eq!(
        service_selection::validate_service_url("http://localhost:8787").as_deref(),
        Ok("http://localhost:8787")
    );
    for value in [
        "http://boxy.example.com",
        "https://user@boxy.example.com",
        "https://boxy.example.com/path",
        "https://boxy.example.com/?token=1",
        "https://boxy.example.com/#fragment",
        "http://localhost.evil.example",
    ] {
        assert!(
            service_selection::validate_service_url(value).is_err(),
            "{value}"
        );
    }
}
