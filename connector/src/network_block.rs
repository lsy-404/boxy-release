use serde::Serialize;

#[cfg(windows)]
const ELEVATED_ARGUMENT: &str = "--sv2-network-block";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkBlockStatus {
    pub blocked: bool,
    pub mode: &'static str,
    pub hostless: bool,
    pub legacy_hosts: bool,
    pub firewall_blocked: bool,
    pub firewall_rule_present: bool,
    pub firewall_available: bool,
    pub firewall_manual: bool,
    pub firewall_provider: &'static str,
    pub firewall_error: Option<String>,
    pub program_path: Option<String>,
    pub firewall_profiles_enabled: Option<bool>,
}

pub fn status() -> NetworkBlockStatus {
    let legacy_hosts = crate::host_block::status().managed;
    #[cfg(windows)]
    let firewall = crate::windows_firewall::status();
    #[cfg(not(windows))]
    let firewall: Result<(bool, bool), String> = Err(
        "LuLu does not expose a supported interface for Boxy to change process rules".to_string(),
    );

    #[cfg(windows)]
    let (firewall_blocked, firewall_rule_present) = firewall
        .as_ref()
        .map(|value| (value.blocked, value.rule_present))
        .unwrap_or((false, false));
    #[cfg(not(windows))]
    let (firewall_blocked, firewall_rule_present) =
        firewall.as_ref().copied().unwrap_or((false, false));
    #[cfg(windows)]
    let firewall_profiles_enabled = firewall.as_ref().ok().map(|value| value.profiles_enabled);

    NetworkBlockStatus {
        blocked: firewall_blocked,
        mode: "firewall",
        hostless: true,
        legacy_hosts,
        firewall_blocked,
        firewall_rule_present,
        firewall_available: firewall.is_ok(),
        firewall_manual: cfg!(target_os = "macos"),
        firewall_provider: if cfg!(windows) {
            "Windows Firewall"
        } else {
            "LuLu"
        },
        firewall_error: firewall.err(),
        program_path: program_path(),
        #[cfg(windows)]
        firewall_profiles_enabled,
        #[cfg(not(windows))]
        firewall_profiles_enabled: None,
    }
}

fn program_path() -> Option<String> {
    #[cfg(windows)]
    let path = crate::session_io::windows_executable().ok()?;
    #[cfg(target_os = "macos")]
    let path = crate::session_io::macos_application()
        .ok()?
        .join("Contents/MacOS/synthv-studio");
    #[cfg(not(any(windows, target_os = "macos")))]
    return None;
    Some(path.to_string_lossy().into_owned())
}

pub fn set_blocked(blocked: bool) -> Result<NetworkBlockStatus, String> {
    #[cfg(windows)]
    {
        if !crate::host_block::is_elevated() {
            crate::host_block::elevate_and_wait_windows(ELEVATED_ARGUMENT, blocked)?;
            return verify_status(status(), blocked);
        }
        crate::windows_firewall::set_blocked(blocked)?;
        crate::host_block::cleanup()?;
        return verify_status(status(), blocked);
    }
    #[cfg(target_os = "macos")]
    {
        if blocked {
            return Err(
                "LuLu cannot be switched from Boxy through a supported interface".to_string(),
            );
        }
        crate::host_block::cleanup()?;
        return Ok(status());
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = blocked;
        Err("network blocking is unavailable on this platform".to_string())
    }
}

pub fn cleanup_hosts() -> Result<NetworkBlockStatus, String> {
    crate::host_block::cleanup()?;
    Ok(status())
}

#[cfg(windows)]
fn verify_status(result: NetworkBlockStatus, blocked: bool) -> Result<NetworkBlockStatus, String> {
    if !blocked && !result.firewall_available {
        return Err("Windows Firewall status could not be verified after the change".to_string());
    }
    if result.blocked != blocked || result.legacy_hosts {
        return Err("network rules are incomplete after the change".to_string());
    }
    Ok(result)
}

#[cfg(windows)]
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
    if args.next().is_some() {
        return Some(1);
    }
    Some(if set_blocked(blocked).is_ok() { 0 } else { 1 })
}
