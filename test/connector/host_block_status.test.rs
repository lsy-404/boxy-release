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

#[cfg(windows)]
mod windows_firewall_regressions {
    use super::windows_firewall::{self, FirewallStatus};
    use base64::Engine;
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;
    use std::process::Command;

    fn status(rule_present: bool, blocked: bool, program: &str) -> FirewallStatus {
        FirewallStatus {
            rule_present,
            blocked,
            program_path: program.to_string(),
            profiles_enabled: true,
        }
    }

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn run_mocked_status(case: &str) -> Result<FirewallStatus, String> {
        let harness = r#"
$ErrorActionPreference = 'Stop'
function New-MockRule($displayName, $direction, $action, $enabled, $profile, $program) {
    [pscustomobject]@{ DisplayName = $displayName; Direction = $direction; Action = $action; Enabled = $enabled; Profile = $profile; Program = $program }
}
function Get-NetFirewallRule {
    param([string]$PolicyStore)
    if ($env:BOXY_TEST_FIREWALL_CASE -eq 'provider_error') { throw 'mock provider failure' }
    if ($PolicyStore -eq 'PersistentStore') {
        switch ($env:BOXY_TEST_FIREWALL_CASE) {
            'case_duplicate' {
                return @(
                    (New-MockRule 'Boxy: Block SV2 outbound' 'Outbound' 'Block' 'True' 'Any' $env:BOXY_SV2_PROGRAM),
                    (New-MockRule 'boxy: block sv2 outbound' 'Outbound' 'Block' 'True' 'Any' $env:BOXY_SV2_PROGRAM)
                )
            }
            'foreign_rule' { return ,(New-MockRule 'Boxy: Block SV2 outbound' 'Inbound' 'Allow' 'True' 'Any' $env:BOXY_SV2_PROGRAM) }
            { $_ -in 'valid', 'profiles_disabled' } { return ,(New-MockRule 'boxy: block sv2 outbound' 'Outbound' 'Block' 'True' 'Any' $env:BOXY_SV2_PROGRAM) }
        }
        return @()
    }
    if ($PolicyStore -eq 'ActiveStore' -and $env:BOXY_TEST_FIREWALL_CASE -in 'valid', 'profiles_disabled') {
        return ,(New-MockRule 'BOXy: Block SV2 outbound' 'Outbound' 'Block' 'True' 'Any' $env:BOXY_SV2_PROGRAM)
    }
    return @()
}
function Get-NetFirewallApplicationFilter {
    process { [pscustomobject]@{ Program = $_.Program } }
}
function Get-NetFirewallProfile {
    param([string]$PolicyStore)
    if ($env:BOXY_TEST_FIREWALL_CASE -eq 'profiles_disabled') {
        return @(
            [pscustomobject]@{ Enabled = 'True' },
            [pscustomobject]@{ Enabled = 'False' },
            [pscustomobject]@{ Enabled = 'True' }
        )
    }
    return @(
        [pscustomobject]@{ Enabled = 'True' },
        [pscustomobject]@{ Enabled = 'True' },
        [pscustomobject]@{ Enabled = 'True' }
    )
}
"#;
        let script = format!("{harness}\n{}", windows_firewall::STATUS_SCRIPT);
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .env("BOXY_TEST_FIREWALL_CASE", case)
            .env("BOXY_SV2_PROGRAM", r"C:\SV2\synthv-studio.exe")
            .output()
            .map_err(|error| format!("cannot run mocked PowerShell status query: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("cannot parse mocked firewall status: {error}"))
    }

    #[test]
    fn adds_absent_rule_and_checks_the_post_change_state() {
        let program = PathBuf::from(r"C:\Program Files\SV2\synthv-studio.exe");
        let expected_program = program.clone();
        let mut command = None;
        let result = windows_firewall::apply_blocked_with(
            true,
            false,
            Some(program),
            |args| {
                command = Some(text(args));
                Ok(())
            },
            |path| {
                assert_eq!(path, Some(expected_program.clone()));
                Ok(status(
                    true,
                    true,
                    r"C:\Program Files\SV2\synthv-studio.exe",
                ))
            },
        )
        .expect("the requested rule should be applied");

        assert!(result.rule_present && result.blocked);
        let command = command.expect("netsh should add the absent rule");
        assert!(command.contains(&"add".to_string()));
        assert!(command.contains(&"dir=out".to_string()));
        assert!(command.contains(&"action=block".to_string()));
        assert!(command.contains(&"enable=yes".to_string()));
        assert!(command.contains(&"profile=any".to_string()));
        assert!(command.contains(&"protocol=any".to_string()));
        assert!(command.contains(&"name=Boxy: Block SV2 outbound".to_string()));
        assert!(command.contains(
            &OsString::from(r"program=C:\Program Files\SV2\synthv-studio.exe")
                .to_string_lossy()
                .into_owned()
        ));
    }

    #[test]
    fn updates_existing_rule_and_keeps_special_path_in_one_argument() {
        let program = PathBuf::from("C:\\SV2 folder\\角色 & [demo] `$(test).exe");
        let program_arg = {
            let mut value = OsString::from("program=");
            value.push(program.as_os_str());
            value
        };
        let result = windows_firewall::netsh_args(true, true, Some(program.as_os_str()))
            .expect("valid update args")
            .expect("existing rule should be updated");

        assert_eq!(result[2].as_os_str(), OsStr::new("set"));
        assert!(result
            .iter()
            .any(|arg| arg.as_os_str() == OsStr::new("new")));
        assert_eq!(result.last(), Some(&program_arg));
        assert_eq!(
            result
                .iter()
                .filter(|arg| arg.as_os_str() == program_arg.as_os_str())
                .count(),
            1
        );
    }

    #[test]
    fn deletes_rule_without_requiring_the_current_executable() {
        let mut command = None;
        let result = windows_firewall::apply_blocked_with(
            false,
            true,
            None,
            |args| {
                command = Some(text(args));
                Ok(())
            },
            |path| {
                assert!(path.is_none());
                Ok(status(false, false, ""))
            },
        )
        .expect("owned rule should be deleted");

        assert!(!result.rule_present);
        assert_eq!(
            command.expect("netsh should delete the owned rule"),
            [
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                "name=Boxy: Block SV2 outbound",
                "dir=out"
            ]
        );
    }

    #[test]
    fn absent_rule_is_not_deleted_or_reported_as_a_command_failure() {
        let mut ran_command = false;
        let result = windows_firewall::apply_blocked_with(
            false,
            false,
            None,
            |_| {
                ran_command = true;
                Ok(())
            },
            |path| {
                assert!(path.is_none());
                Ok(status(false, false, ""))
            },
        )
        .expect("an already-absent rule is the requested state");

        assert!(!ran_command);
        assert!(!result.rule_present);
    }

    #[test]
    fn command_failure_propagates_without_claiming_success_or_reading_status() {
        let mut read_status = false;
        let result = windows_firewall::apply_blocked_with(
            true,
            false,
            Some(PathBuf::from(r"C:\SV2\synthv-studio.exe")),
            |_| Err("netsh failed".to_string()),
            |_| {
                read_status = true;
                Ok(status(true, true, r"C:\SV2\synthv-studio.exe"))
            },
        );

        assert_eq!(result.unwrap_err(), "netsh failed");
        assert!(!read_status);
    }

    #[test]
    fn rejects_stale_status_after_add_or_delete() {
        let add = windows_firewall::apply_blocked_with(
            true,
            false,
            Some(PathBuf::from(r"C:\SV2\synthv-studio.exe")),
            |_| Ok(()),
            |_| Ok(status(true, false, r"C:\SV2\synthv-studio.exe")),
        );
        assert!(add.is_err());

        let delete = windows_firewall::apply_blocked_with(
            false,
            true,
            None,
            |_| Ok(()),
            |_| Ok(status(true, false, r"C:\SV2\synthv-studio.exe")),
        );
        assert!(delete.is_err());
    }

    #[test]
    fn status_query_rejects_case_insensitive_duplicates_and_foreign_collisions() {
        assert!(run_mocked_status("case_duplicate").is_err());
        assert!(run_mocked_status("foreign_rule").is_err());
    }

    #[test]
    fn status_query_surfaces_provider_errors_instead_of_treating_them_as_absence() {
        assert!(run_mocked_status("provider_error").is_err());
    }

    #[test]
    fn status_query_accepts_a_unique_display_name_without_internal_identity_fields() {
        let result = run_mocked_status("valid").expect("the matching rule should be recognized");
        assert!(result.rule_present);
        assert!(result.blocked);
    }

    #[test]
    fn status_query_does_not_report_blocking_when_a_firewall_profile_is_disabled() {
        let result = run_mocked_status("profiles_disabled").expect("status should be readable");
        assert!(result.rule_present);
        assert!(!result.profiles_enabled);
        assert!(!result.blocked);
    }

    #[test]
    fn status_query_reports_an_absent_rule_as_unblocked_and_available() {
        let result = run_mocked_status("absent").expect("status should be readable");
        assert!(!result.rule_present);
        assert!(!result.blocked);
        assert!(result.profiles_enabled);
    }
}
