use super::*;

#[test]
fn selection_navigation_only_accepts_the_local_entry_page() {
    for url in [
        "tauri://localhost",
        "tauri://localhost/",
        "tauri://localhost/index.html",
        "http://tauri.localhost/index.html",
    ] {
        assert!(local_selection_url(&Url::parse(url).unwrap()), "{url}");
    }
    for url in [
        "https://service.example/index.html",
        "https://tauri.localhost/index.html",
        "http://tauri.localhost:3000/index.html",
        "tauri://localhost/other.html",
        "tauri://localhost/index.html?server=example",
        "tauri://localhost/index.html#remote",
        "tauri://localhost?server=example",
        "tauri://localhost#remote",
        "tauri://user@localhost",
        "tauri://user@localhost/index.html",
        "tauri://remote/index.html",
        "about:blank",
    ] {
        assert!(!local_selection_url(&Url::parse(url).unwrap()), "{url}");
    }
}

#[test]
fn cloud_navigation_accepts_https_and_loopback_without_business_host_rules() {
    for url in [
        "https://service.example/editor",
        "https://identity.example/login",
        "http://127.0.0.1:4567/editor",
        "http://[::1]:4567/editor",
        "blob:https://service.example/download-id",
        "about:blank",
    ] {
        assert!(allowed_navigation(&Url::parse(url).unwrap()), "{url}");
    }
}

#[test]
fn remote_pages_cannot_navigate_to_local_files_or_native_protocols() {
    for url in [
        "file:///C:/private/private-data",
        "tauri://localhost/",
        "asset://localhost/private/private-data",
        "http://public.example/editor",
        "javascript:alert(1)",
        "data:text/html,test",
        "blob:file:///private/private-data",
    ] {
        assert!(!allowed_navigation(&Url::parse(url).unwrap()), "{url}");
    }
}

#[test]
fn cleanup_removes_owned_profile_and_preserves_neighboring_files() {
    let parent = tempfile::tempdir().unwrap();
    let runtime = tempfile::Builder::new()
        .prefix("boxy-runtime-")
        .tempdir_in(parent.path())
        .unwrap();
    let profile = runtime.path().join("webview");
    std::fs::create_dir(&profile).unwrap();
    std::fs::write(profile.join("Cookies"), b"private-data-state").unwrap();
    let output = parent.path().join("saved-private-data.bin");
    std::fs::write(&output, b"user-output").unwrap();
    let runtime_path = runtime.path().to_owned();
    cleanup_runtime(runtime).unwrap();
    assert!(!runtime_path.exists());
    assert_eq!(std::fs::read(output).unwrap(), b"user-output");
}

#[cfg(windows)]
#[test]
fn cleanup_waits_for_a_browser_file_lock_to_release() {
    use std::os::windows::fs::OpenOptionsExt;
    let runtime = tempfile::Builder::new()
        .prefix("boxy-runtime-")
        .tempdir()
        .unwrap();
    let path = runtime.path().to_owned();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(path.join("locked-cache"))
        .unwrap();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        drop(file);
    });
    cleanup_runtime(runtime).unwrap();
    release.join().unwrap();
    assert!(!path.exists());
}
