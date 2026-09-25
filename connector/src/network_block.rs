use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockMode {
    Hosts,
    Firewall,
}

impl BlockMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "hosts" => Ok(Self::Hosts),
            "firewall" => Ok(Self::Firewall),
            _ => Err("network block mode is invalid".to_string()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Hosts => "hosts",
            Self::Firewall => "firewall",
        }
    }
}

#[cfg(windows)]
const ELEVATED_ARGUMENT: &str = "--sv2-network-block";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkBlockStatus {
    pub blocked: bool,
    pub mode: &'static str,
    pub managed: bool,
    pub blocked_hosts: usize,
    pub total_hosts: usize,
    pub hosts_path: String,
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
    let hosts = crate::host_block::status();
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
        blocked: if firewall_rule_present {
            firewall_blocked && hosts.blocked
        } else {
            hosts.blocked
        },
        mode: if firewall_rule_present {
            "firewall"
        } else {
            "hosts"
        },
        managed: hosts.managed,
        blocked_hosts: hosts.blocked_hosts,
        total_hosts: hosts.total_hosts,
        hosts_path: hosts.hosts_path,
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

pub fn set_blocked(blocked: bool, mode: BlockMode) -> Result<NetworkBlockStatus, String> {
    #[cfg(windows)]
    {
        if !crate::host_block::is_elevated() {
            let argument = format!("{ELEVATED_ARGUMENT} {}", mode.as_str());
            crate::host_block::elevate_and_wait_windows(&argument, blocked)?;
            return verify_status(status(), blocked, mode);
        }
        if blocked {
            match mode {
                BlockMode::Hosts => {
                    crate::host_block::set_blocked(true)?;
                    crate::windows_firewall::set_blocked(false)?;
                }
                BlockMode::Firewall => {
                    crate::host_block::set_blocked(true)?;
                    crate::windows_firewall::set_blocked(true)?;
                }
            }
        } else {
            crate::windows_firewall::set_blocked(false)?;
            crate::host_block::set_blocked(false)?;
        }
    }
    #[cfg(not(windows))]
    {
        if mode == BlockMode::Firewall {
            return Err(
                "LuLu cannot be switched from Boxy through a supported interface".to_string(),
            );
        }
        crate::host_block::set_blocked(blocked)?;
    }
    verify_status(status(), blocked, mode)
}

fn verify_status(
    result: NetworkBlockStatus,
    blocked: bool,
    mode: BlockMode,
) -> Result<NetworkBlockStatus, String> {
    if blocked && (!result.blocked || result.mode != mode.as_str()) {
        return Err("network rules are incomplete after the change".to_string());
    }
    if !blocked && (result.firewall_rule_present || result.managed || result.blocked_hosts > 0) {
        return Err("network rules still block known hosts after the change".to_string());
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
    let mode = BlockMode::parse(&args.next()?.to_string_lossy()).ok()?;
    let blocked = match args.next()?.to_string_lossy().as_ref() {
        "enable" => true,
        "disable" => false,
        _ => return Some(1),
    };
    Some(if set_blocked(blocked, mode).is_ok() {
        0
    } else {
        1
    })
}
