#[cfg(windows)]
#[path = "../../connector/src/session_io.rs"]
mod session_io;

#[allow(dead_code)]
#[path = "../../connector/src/host_block.rs"]
mod host_block;

#[test]
fn status_serializes_the_platform_rule_contract() {
    let status = host_block::HostBlockStatus {
        method: "wfp",
        blocked: false,
        configured: false,
        verified: false,
        managed: false,
        legacy_hosts: false,
        warning: None,
        blocked_rules: 0,
        total_rules: 4,
        artifact_path: None,
        manual_import_required: None,
        instructions: None,
    };
    let value = serde_json::to_value(status).expect("status should serialize");

    assert_eq!(value["method"], "wfp");
    assert!(value.get("configured").is_some());
    assert!(value.get("verified").is_some());
    assert!(value.get("legacyHosts").is_some());
    assert!(value.get("blockedRules").is_some());
    assert!(value.get("totalRules").is_some());
    assert!(value.get("hostsPath").is_none());
    assert!(value.get("blockedHosts").is_none());
}

#[test]
fn lulu_import_rule_scopes_sv2_and_its_children() {
    let executable = std::path::Path::new(
        "/Applications/Synthesizer V Studio 2.app/Contents/MacOS/Synthesizer V Studio 2",
    );
    let document =
        host_block::lulu_import_document(executable, "Synthesizer V Studio 2 (Boxy block)")
            .expect("LuLu JSON should be generated");
    let rules = document[executable.to_str().expect("UTF-8 path")]
        .as_array()
        .expect("path key should contain a rule array");
    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(rule["path"], executable.to_str().unwrap());
    assert_eq!(rule["endpointAddr"], "*");
    assert_eq!(rule["endpointPort"], "*");
    assert_eq!(rule["type"], 3);
    assert_eq!(rule["scope"], 2);
    assert_eq!(rule["action"], 0);
    let uuid = rule["uuid"].as_str().expect("UUID string");
    assert_eq!(uuid.len(), 36);
    assert_eq!(&uuid[14..15], "4");
    assert!(rule["creation"]
        .as_str()
        .expect("creation timestamp")
        .ends_with("+0000"));
}

#[test]
#[cfg(windows)]
fn normalizes_case_and_trailing_separators_for_windows_policy_paths() {
    assert!(host_block::same_windows_path(
        r"C:\Boxy\WebView2Runtime\\",
        r"c:\boxy\webview2runtime"
    ));
}

#[test]
#[cfg(windows)]
fn strips_extended_length_prefix_before_writing_the_webview_policy() {
    let directory =
        host_block::browser_executable_folder(std::path::Path::new(r"\\?\C:\Boxy\WebView2Runtime"))
            .expect("a local extended-length path should be normalized");
    assert_eq!(directory, r"C:\Boxy\WebView2Runtime");
}

#[test]
#[cfg(windows)]
fn removes_only_boxy_marked_legacy_hosts_rules() {
    let original = "0.0.0.0 keep.example # user mapping\n";
    let marked =
        format!("{original}0.0.0.0 old.example # Synthesizer V Studio 2 Boxy network block\n");
    assert_eq!(host_block::remove_legacy_hosts_rules(&marked), original);
}

#[test]
#[cfg(windows)]
fn parses_quoted_display_icons_with_indexes() {
    assert_eq!(
        session_io::display_icon_path(r#""C:\Program Files\SV2\synthv-studio.exe",0"#),
        Some(std::path::PathBuf::from(
            r"C:\Program Files\SV2\synthv-studio.exe"
        ))
    );
}

#[test]
#[cfg(windows)]
fn accepts_only_runtime_paths_under_the_canonical_boxy_root() {
    let root = std::path::Path::new(r"D:\Apps\Boxy");
    assert!(host_block::is_path_within(
        root,
        std::path::Path::new(r"d:\apps\boxy\WebView2Runtime\msedgewebview2.exe")
    ));
    assert!(!host_block::is_path_within(
        root,
        std::path::Path::new(r"D:\Apps\BoxyShared\msedgewebview2.exe")
    ));
    assert!(!host_block::is_path_within(
        root,
        std::path::Path::new(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\msedgewebview2.exe"
        )
    ));
}

#[test]
fn removes_only_exact_boxy_hosts_markers() {
    let original = "0.0.0.0 keep.example # user mapping\n";
    let managed = format!(
        "{original}0.0.0.0 old.example # Synthesizer V Studio 2 Boxy network block\n::1 old.example # SV2 Session Editor update block\n"
    );
    assert_eq!(host_block::remove_legacy_hosts_rules(&managed), original);
    let similar = format!("{original}0.0.0.0 keep.example # Boxy network block\n");
    assert_eq!(host_block::remove_legacy_hosts_rules(&similar), similar);
}
