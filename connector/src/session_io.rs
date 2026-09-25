use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sv2ControlResult {
    pub action: String,
    pub executable: String,
}

pub fn launch_sv2() -> Result<Sv2ControlResult, String> {
    #[cfg(windows)]
    {
        let executable = windows_executable()?;
        std::process::Command::new(&executable)
            .spawn()
            .map_err(|error| format!("cannot launch SV2: {error}"))?;
        return Ok(result("launch", executable));
    }

    #[cfg(target_os = "macos")]
    {
        let executable = macos_application()?;
        std::process::Command::new("/usr/bin/open")
            .arg(&executable)
            .spawn()
            .map_err(|error| format!("cannot launch SV2: {error}"))?;
        return Ok(result("launch", executable));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    Err("SV2 controls are available on Windows and macOS".to_string())
}

pub fn close_sv2() -> Result<Sv2ControlResult, String> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$items = @(Get-Process -Name 'synthv-studio' -ErrorAction SilentlyContinue); if ($items.Count -eq 0) { exit 2 }; $accepted = $false; foreach ($item in $items) { if ($item.CloseMainWindow()) { $accepted = $true } }; if (-not $accepted) { exit 3 }; $deadline = (Get-Date).AddSeconds(12); do { Start-Sleep -Milliseconds 250; $remaining = @(Get-Process -Name 'synthv-studio' -ErrorAction SilentlyContinue) } while ($remaining.Count -gt 0 -and (Get-Date) -lt $deadline); if ($remaining.Count -gt 0) { exit 4 }; exit 0",
            ])
            .output()
            .map_err(|error| format!("cannot request SV2 to close: {error}"))?;
        return match output.status.code() {
            Some(0) => Ok(result("close", windows_executable().unwrap_or_default())),
            Some(2) => Err("SV2 is not running".to_string()),
            Some(3) => Err("SV2 did not accept a close request".to_string()),
            Some(4) => Err("SV2 is still closing; finish closing it before writing".to_string()),
            _ => Err("SV2 could not be closed".to_string()),
        };
    }

    #[cfg(target_os = "macos")]
    {
        let application = macos_application()?;
        let name = application
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "cannot determine the SV2 application name".to_string())?;
        let running = std::process::Command::new("/usr/bin/pgrep")
            .args(["-x", name])
            .status()
            .map_err(|error| format!("cannot check whether SV2 is running: {error}"))?
            .success();
        if !running {
            return Err("SV2 is not running".to_string());
        }
        let script = format!("tell application \"{name}\" to quit");
        let output = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .output()
            .map_err(|error| format!("cannot request SV2 to close: {error}"))?;
        if !output.status.success() {
            return Err("SV2 did not accept a close request".to_string());
        }
        return Ok(result("close", application));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    Err("SV2 controls are available on Windows and macOS".to_string())
}

pub fn default_session_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Dreamtonics")
        .join("Synthesizer V Studio 2")
        .join("license")
        .join("session")
}

pub fn open_session_folder(path: &Path) -> Result<(), String> {
    let directory = session_folder(path)?;
    #[cfg(target_os = "macos")]
    let status = Command::new("/usr/bin/open")
        .arg(&directory)
        .status()
        .map_err(|error| format!("cannot open the session folder: {error}"))?;
    #[cfg(windows)]
    let status = Command::new("explorer.exe")
        .arg(&directory)
        .status()
        .map_err(|error| format!("cannot open the session folder: {error}"))?;
    #[cfg(not(any(target_os = "macos", windows)))]
    return Err("opening the session folder is available on Windows and macOS".to_string());
    #[cfg(any(target_os = "macos", windows))]
    if !status.success() {
        return Err("the operating system could not open the session folder".to_string());
    }
    Ok(())
}

fn session_folder(path: &Path) -> Result<PathBuf, String> {
    let directory = path
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .ok_or_else(|| "session path has no parent folder".to_string())?;
    let directory = std::fs::canonicalize(directory)
        .map_err(|error| format!("cannot locate the session folder: {error}"))?;
    if !directory.is_dir() {
        return Err("the session parent is not a folder".to_string());
    }
    Ok(directory)
}

fn result(action: &str, executable: PathBuf) -> Sv2ControlResult {
    Sv2ControlResult {
        action: action.to_string(),
        executable: executable.to_string_lossy().into_owned(),
    }
}

#[cfg(windows)]
pub(crate) fn windows_executable() -> Result<PathBuf, String> {
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };
    use winreg::RegKey;

    const UNINSTALL_PATH: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    let hives = [
        RegKey::predef(HKEY_CURRENT_USER),
        RegKey::predef(HKEY_LOCAL_MACHINE),
    ];
    let flags = [
        KEY_READ,
        KEY_READ | KEY_WOW64_64KEY,
        KEY_READ | KEY_WOW64_32KEY,
    ];
    for hive in &hives {
        for flags in flags {
            let Ok(uninstall) = hive.open_subkey_with_flags(UNINSTALL_PATH, flags) else {
                continue;
            };
            for subkey_name in uninstall.enum_keys().filter_map(Result::ok) {
                let Ok(subkey) = uninstall.open_subkey(&subkey_name) else {
                    continue;
                };
                let display_name = subkey
                    .get_value::<String, _>("DisplayName")
                    .unwrap_or_default();
                if !display_name.contains("Synthesizer V Studio 2") {
                    continue;
                }
                if let Ok(install_location) = subkey.get_value::<String, _>("InstallLocation") {
                    let candidate = PathBuf::from(install_location).join("synthv-studio.exe");
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
                if let Ok(display_icon) = subkey.get_value::<String, _>("DisplayIcon") {
                    let Some(candidate) = display_icon_path(&display_icon) else {
                        continue;
                    };
                    if candidate.is_file()
                        && candidate
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.eq_ignore_ascii_case("synthv-studio.exe"))
                    {
                        return Ok(candidate);
                    }
                    if let Some(sibling) = candidate
                        .parent()
                        .map(|directory| directory.join("synthv-studio.exe"))
                    {
                        if sibling.is_file() {
                            return Ok(sibling);
                        }
                    }
                }
            }
        }
    }
    Err("cannot locate SV2; install Synthesizer V Studio 2 before launching it".to_string())
}

#[cfg(windows)]
pub(crate) fn display_icon_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if let Some(value) = value.strip_prefix('"') {
        return value.split_once('"').map(|(path, _)| PathBuf::from(path));
    }
    let path = value
        .rsplit_once(',')
        .and_then(|(path, index)| index.trim().parse::<i32>().ok().map(|_| path))
        .unwrap_or(value);
    (!path.is_empty()).then(|| PathBuf::from(path.trim()))
}

#[cfg(target_os = "macos")]
fn macos_application() -> Result<PathBuf, String> {
    [
        "/Applications/Synthesizer V Studio 2.app",
        "/Applications/Synthesizer V Studio 2 Pro.app",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_dir())
    .ok_or_else(|| "cannot locate SV2 in Applications".to_string())
}
