use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Command;

const ELEVATED_ARGUMENT: &str = "--sv2-firewall-block";
const RULE_DISPLAY_NAME: &str = "Boxy: Block SV2 outbound";

pub(crate) const STATUS_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$displayName = 'Boxy: Block SV2 outbound'
$rules = @(Get-NetFirewallRule -PolicyStore PersistentStore | Where-Object { $_.DisplayName -eq $displayName })
if ($rules.Count -gt 1) { throw 'Multiple firewall rules use the reserved Boxy display name' }
if ($rules.Count -eq 1 -and ($rules[0].Direction -ne 'Outbound' -or $rules[0].Action -ne 'Block')) {
    throw 'A different firewall rule uses the reserved Boxy display name'
}
$program = ''
$enabled = $false
if ($rules.Count -eq 1) {
    $program = [string]($rules[0] | Get-NetFirewallApplicationFilter).Program
    $enabled = ($rules[0].Enabled -eq 'True' -and $rules[0].Direction -eq 'Outbound' -and $rules[0].Action -eq 'Block' -and $rules[0].Profile -eq 'Any' -and $env:BOXY_SV2_PROGRAM -and $program -eq $env:BOXY_SV2_PROGRAM)
}
$active = $false
if ($enabled) {
    $effective = @(Get-NetFirewallRule -PolicyStore ActiveStore | Where-Object { $_.DisplayName -eq $displayName })
    $active = @($effective | Where-Object {
        $_.Enabled -eq 'True' -and $_.Direction -eq 'Outbound' -and $_.Action -eq 'Block' -and
        [string]($_ | Get-NetFirewallApplicationFilter).Program -eq $program
    }).Count -gt 0
}
$profiles = @(Get-NetFirewallProfile -PolicyStore ActiveStore)
$profilesEnabled = ($profiles.Count -ge 3 -and @($profiles | Where-Object { $_.Enabled -ne 'True' }).Count -eq 0)
[pscustomobject]@{ rulePresent = ($rules.Count -eq 1); blocked = ($enabled -and $active -and $profilesEnabled); programPath = $program; profilesEnabled = $profilesEnabled } | ConvertTo-Json -Compress
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
    run_status(crate::session_io::windows_executable().ok())
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
        let path = crate::session_io::windows_executable()?;
        if !path.is_file() {
            return Err("SV2 executable is unavailable".to_string());
        }
        Some(path)
    } else {
        None
    };
    let before = run_status(program.clone())?;
    apply_blocked_with(blocked, before.rule_present, program, run_netsh, run_status)
}

pub(crate) fn apply_blocked_with<R, S>(
    blocked: bool,
    existing_rule: bool,
    program: Option<PathBuf>,
    mut run_command: R,
    mut read_status: S,
) -> Result<FirewallStatus, String>
where
    R: FnMut(&[OsString]) -> Result<(), String>,
    S: FnMut(Option<PathBuf>) -> Result<FirewallStatus, String>,
{
    let args = netsh_args(
        blocked,
        existing_rule,
        program.as_ref().map(|path| path.as_os_str()),
    )?;
    if let Some(args) = args {
        run_command(&args)?;
    }
    let result = read_status(program)?;
    if (blocked && (!result.blocked || !result.rule_present)) || (!blocked && result.rule_present) {
        return Err("Windows firewall did not apply the requested SV2 rule".to_string());
    }
    Ok(result)
}

pub(crate) fn netsh_args(
    blocked: bool,
    existing_rule: bool,
    program: Option<&OsStr>,
) -> Result<Option<Vec<OsString>>, String> {
    if blocked && program.is_none() {
        return Err("SV2 executable is unavailable".to_string());
    }
    if !blocked && !existing_rule {
        return Ok(None);
    }

    let mut args = vec!["advfirewall".into(), "firewall".into()];
    if !blocked {
        args.extend([
            "delete".into(),
            "rule".into(),
            format!("name={RULE_DISPLAY_NAME}").into(),
            "dir=out".into(),
        ]);
        return Ok(Some(args));
    }

    args.extend([
        if existing_rule { "set" } else { "add" }.into(),
        "rule".into(),
        format!("name={RULE_DISPLAY_NAME}").into(),
    ]);
    if existing_rule {
        args.push("new".into());
    }
    args.extend([
        "dir=out".into(),
        "action=block".into(),
        "enable=yes".into(),
        "profile=any".into(),
        "protocol=any".into(),
    ]);
    let mut program_arg = OsString::from("program=");
    program_arg.push(program.unwrap());
    args.push(program_arg);
    Ok(Some(args))
}

fn run_netsh(args: &[OsString]) -> Result<(), String> {
    let executable = crate::host_block::windows_system_directory()?.join("netsh.exe");
    let mut command = Command::new(executable);
    command.args(args);
    crate::windows_process::configure_background(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("cannot run Windows firewall command: {error}"))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Windows firewall command failed ({}): stdout: {}; stderr: {}",
            output.status,
            stdout.trim(),
            stderr.trim()
        ));
    }
    Ok(())
}

fn run_status(program: Option<PathBuf>) -> Result<FirewallStatus, String> {
    let executable = crate::host_block::windows_system_directory()?
        .join("WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(executable);
    command.args(["-NoProfile", "-NonInteractive", "-Command", STATUS_SCRIPT]);
    command.env_remove("BOXY_SV2_PROGRAM");
    if let Some(path) = program {
        command.env("BOXY_SV2_PROGRAM", path);
    }
    crate::windows_process::configure_background(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("cannot inspect Windows firewall: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect Windows firewall: {}",
            String::from_utf8_lossy(&output.stderr).trim()
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
