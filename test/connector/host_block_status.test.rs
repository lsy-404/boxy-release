#[path = "../../connector/src/session_io.rs"]
mod session_io;

#[allow(dead_code)]
#[path = "../../connector/src/host_block.rs"]
mod host_block;

#[path = "../../connector/src/network_block.rs"]
mod network_block;

#[cfg(windows)]
#[path = "../../connector/src/windows_firewall.rs"]
mod windows_firewall;

#[cfg(windows)]
#[path = "../../connector/src/windows_process.rs"]
mod windows_process;

fn status_value() -> serde_json::Value {
    let firewall_rule_present = cfg!(windows);
    let status = network_block::NetworkBlockStatus {
        blocked: firewall_rule_present,
        mode: "firewall",
        hostless: true,
        legacy_hosts: false,
        firewall_blocked: firewall_rule_present,
        firewall_rule_present,
        firewall_available: cfg!(windows),
        firewall_manual: cfg!(target_os = "macos"),
        firewall_provider: if cfg!(windows) {
            "Windows Firewall"
        } else {
            "LuLu"
        },
        firewall_error: if cfg!(windows) {
            None
        } else {
            Some("firewall status is unavailable".to_string())
        },
        program_path: None,
        #[cfg(windows)]
        firewall_profiles_enabled: Some(true),
        #[cfg(not(windows))]
        firewall_profiles_enabled: None,
    };
    serde_json::to_value(status).expect("status should serialize")
}

#[test]
fn status_serializes_the_platform_rule_contract() {
    let value = status_value();
    assert_eq!(value["mode"], "firewall");
    assert_eq!(value["hostless"], true);
    assert_eq!(value["legacyHosts"], false);
    assert_eq!(value["blocked"], cfg!(windows));
    assert_eq!(value["firewallBlocked"], cfg!(windows));
    assert_eq!(value["firewallRulePresent"], cfg!(windows));
    assert_eq!(value["firewallAvailable"], cfg!(windows));
    assert!(value.get("firewallManual").is_some());
    assert!(value.get("firewallProvider").is_some());
    assert!(value.get("firewallError").is_some());
    assert!(value.get("programPath").is_some());
    assert!(value.get("firewallProfilesEnabled").is_some());
    assert!(value.get("managed").is_none());
    assert!(value.get("blockedHosts").is_none());
}

#[test]
fn removes_only_boxy_marked_legacy_hosts_rules() {
    let original = "0.0.0.0 keep.example # user mapping\n";
    let marked =
        format!("{original}0.0.0.0 old.example # Synthesizer V Studio 2 Boxy network block\n");

    assert_eq!(host_block::remove_managed_rules(&marked), original);
}

#[test]
fn removes_only_exact_boxy_hosts_markers() {
    let original = "0.0.0.0 keep.example # user mapping\n";
    let managed = format!(
        "{original}0.0.0.0 old.example # Synthesizer V Studio 2 Boxy network block\n::1 old.example # SV2 Session Editor update block\n"
    );
    assert_eq!(host_block::remove_managed_rules(&managed), original);

    let similar = format!("{original}0.0.0.0 keep.example # Boxy network block\n");
    assert_eq!(host_block::remove_managed_rules(&similar), similar);
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
