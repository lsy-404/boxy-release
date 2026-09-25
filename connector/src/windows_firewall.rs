use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

const ELEVATED_ARGUMENT: &str = "--sv2-firewall-block";

const RULE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$name = 'Boxy.SV2.Outbound'
$group = 'Boxy SV2'
$rule = @(Get-NetFirewallRule -Name $name -PolicyStore PersistentStore -ErrorAction SilentlyContinue)
if ($rule.Count -gt 1) { throw 'Multiple Boxy firewall rules have the same name' }
if ($rule.Count -eq 1 -and $rule[0].Group -ne $group) { throw 'A different firewall rule uses the Boxy name' }
$action = $env:BOXY_FIREWALL_ACTION
if ($action -eq 'enable') {
    $program = $env:BOXY_SV2_PROGRAM
    if (-not $program -or -not (Test-Path -LiteralPath $program -PathType Leaf)) { throw 'SV2 executable is unavailable' }
    if ($rule.Count -eq 0) {
        New-NetFirewallRule -Name $name -DisplayName 'Boxy: Block SV2 outbound' -Group $group -Direction Outbound -Action Block -Enabled True -Profile Any -Program $program -Protocol Any -PolicyStore PersistentStore | Out-Null
    } else {
        $rule[0] | Get-NetFirewallApplicationFilter | Set-NetFirewallApplicationFilter -Program $program
        $rule[0] | Set-NetFirewallRule -Direction Outbound -Action Block -Enabled True -Profile Any
    }
} elseif ($action -eq 'disable') {
    if ($rule.Count -eq 1) { $rule[0] | Remove-NetFirewallRule }
} elseif ($action -ne 'status') {
    throw 'Invalid firewall action'
}
$rule = @(Get-NetFirewallRule -Name $name -PolicyStore PersistentStore -ErrorAction SilentlyContinue)
if ($rule.Count -gt 1) { throw 'Multiple Boxy firewall rules have the same name' }
if ($rule.Count -eq 1 -and $rule[0].Group -ne $group) { throw 'A different firewall rule uses the Boxy name' }
$program = ''
$enabled = $false
if ($rule.Count -eq 1) {
    $program = [string]($rule[0] | Get-NetFirewallApplicationFilter).Program
    $enabled = ($rule[0].Enabled -eq 'True' -and $rule[0].Direction -eq 'Outbound' -and $rule[0].Action -eq 'Block' -and $rule[0].Profile -eq 'Any' -and $env:BOXY_SV2_PROGRAM -and $program -eq $env:BOXY_SV2_PROGRAM)
}
$active = $false
if ($enabled) {
    $effective = @(Get-NetFirewallRule -Name $name -PolicyStore ActiveStore -ErrorAction SilentlyContinue)
    $active = @($effective | Where-Object {
        $_.Group -eq $group -and $_.Enabled -eq 'True' -and $_.Direction -eq 'Outbound' -and $_.Action -eq 'Block' -and
        [string]($_ | Get-NetFirewallApplicationFilter).Program -eq $program
    }).Count -gt 0
}
$profiles = @(Get-NetFirewallProfile -PolicyStore ActiveStore)
$profilesEnabled = ($profiles.Count -ge 3 -and @($profiles | Where-Object { $_.Enabled -ne 'True' }).Count -eq 0)
[pscustomobject]@{ rulePresent = ($rule.Count -eq 1); blocked = ($enabled -and $active -and $profilesEnabled); programPath = $program; profilesEnabled = $profilesEnabled } | ConvertTo-Json -Compress
"#;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    pub rule_present: bool,
    pub blocked: bool,
    pub program_path: String,
    pub profiles_enabled: bool,
}

pub fn status() -> Result<FirewallStatus, String> {
    run_script("status", crate::session_io::windows_executable().ok())
}

pub fn set_blocked(blocked: bool) -> Result<FirewallStatus, String> {
    if !crate::host_block::is_elevated() {
        crate::host_block::elevate_and_wait_windows(ELEVATED_ARGUMENT, blocked)?;
        return status();
    }
    set_blocked_direct(blocked)
}

fn set_blocked_direct(blocked: bool) -> Result<FirewallStatus, String> {
    let program = if blocked {
        Some(crate::session_io::windows_executable()?)
    } else {
        None
    };
    let result = run_script(if blocked { "enable" } else { "disable" }, program)?;
    if result.blocked != blocked {
        return Err("Windows firewall did not apply the requested SV2 rule".to_string());
    }
    Ok(result)
}

fn run_script(action: &str, program: Option<PathBuf>) -> Result<FirewallStatus, String> {
    let executable = crate::host_block::windows_system_directory()?
        .join("WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(executable);
    command.args(["-NoProfile", "-NonInteractive", "-Command", RULE_SCRIPT]);
    command.env("BOXY_FIREWALL_ACTION", action);
    command.env_remove("BOXY_SV2_PROGRAM");
    if let Some(path) = program {
        command.env("BOXY_SV2_PROGRAM", path);
    }
    let output = command
        .output()
        .map_err(|error| format!("cannot inspect Windows firewall: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Windows firewall operation failed: {}",
            detail.trim()
        ));
    }
    serde_json::from_slice::<FirewallStatus>(&output.stdout)
        .map_err(|error| format!("cannot read Windows firewall status: {error}"))
}

pub fn run_elevated_if_requested() -> Option<i32> {
    let mut args = std::env::args_os();
    args.next();
    if args.next()?.to_string_lossy() != ELEVATED_ARGUMENT {
        return None;
    }
    let blocked = match args.next()?.to_string_lossy().as_ref() {
        "enable" => true,
        "disable" => false,
        _ => return Some(1),
    };
    Some(if set_blocked_direct(blocked).is_ok() {
        0
    } else {
        1
    })
}
