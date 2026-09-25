use std::fs;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(target_os = "macos")]
use std::io::Write;
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(target_os = "macos")]
use std::process::Stdio;

const BLOCKED_HOSTS: &[&str] = &[
    "authr3.dreamtonics.com",
    "account.dreamtonics.com",
    "authr3-media.r2.dreamtonics.com",
    "authr3-models.r2.dreamtonics.com",
    "resource.dreamtonics.com",
    "store.dreamtonics.com",
    "my.dreamtonics.com",
    "svdocs.dreamtonics.com",
    "dreamtonics.com",
    "dreamtonics.com.cn",
];
const MARKER: &str = "# Synthesizer V Studio 2 Boxy network block";
const PREVIOUS_MARKER: &str = "# Synthesizer V Studio 2 Boxy update block";
const LEGACY_MARKER: &str = "# SV2 Smooth update block";
const OLDER_MARKER: &str = "# SV2 Session Editor update block";
const MAX_HOSTS_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const ELEVATED_ARGUMENT: &str = "--sv2-host-block";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBlockStatus {
    pub blocked: bool,
    pub managed: bool,
    pub blocked_hosts: usize,
    pub total_hosts: usize,
    pub hosts_path: String,
}

pub fn status() -> HostBlockStatus {
    let path = hosts_path();
    let text = read_hosts(&path).unwrap_or_default();
    status_for(&path, &text)
}

pub fn set_blocked(blocked: bool) -> Result<HostBlockStatus, String> {
    #[cfg(windows)]
    {
        if !is_elevated() {
            elevate_and_wait_windows(ELEVATED_ARGUMENT, blocked)?;
            return Ok(status());
        }
        return set_blocked_direct(blocked);
    }
    #[cfg(target_os = "macos")]
    {
        elevate_and_wait(blocked)?;
        return Ok(status());
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    set_blocked_direct(blocked)
}

#[cfg(not(target_os = "macos"))]
fn set_blocked_direct(blocked: bool) -> Result<HostBlockStatus, String> {
    #[cfg(windows)]
    let path = windows_system_directory()?.join("drivers/etc/hosts");
    #[cfg(not(windows))]
    let path = hosts_path();
    let current = read_hosts(&path)?;
    let next = rewrite_managed_rules(&current, blocked);
    if next != current {
        write_hosts(&path, &next)?;
        #[cfg(windows)]
        flush_windows_dns_cache()?;
    }
    Ok(status_for(&path, &read_hosts(&path)?))
}

#[cfg(windows)]
fn flush_windows_dns_cache() -> Result<(), String> {
    let result = std::process::Command::new(windows_system_directory()?.join("ipconfig.exe"))
        .arg("/flushdns")
        .status()
        .map_err(|error| {
            format!("hosts file changed, but DNS cache flush could not start: {error}")
        })?;
    if !result.success() {
        return Err(format!(
            "hosts file changed, but DNS cache flush failed ({result})"
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn windows_system_directory() -> Result<PathBuf, String> {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut buffer = vec![0u16; 260];
    let mut length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length as usize >= buffer.len() {
        buffer.resize(length as usize + 1, 0);
        length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    }
    if length == 0 || length as usize >= buffer.len() {
        return Err("cannot locate Windows system directory".to_string());
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}

pub fn run_elevated_host_block_if_requested() -> Option<i32> {
    #[cfg(windows)]
    {
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
        return Some(match set_blocked_direct(blocked) {
            Ok(_) => 0,
            Err(_) => 1,
        });
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn elevate_and_wait(blocked: bool) -> Result<(), String> {
    let path = hosts_path();
    let current = read_hosts(&path)?;
    let next = rewrite_managed_rules(&current, blocked);
    if next == current {
        return Ok(());
    }
    let mut child = std::process::Command::new("/usr/libexec/authopen")
        .arg("-w")
        .arg(&path)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start macOS authorization: {error}"))?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| "cannot open the authorized hosts writer".to_string())?;
    input
        .write_all(next.as_bytes())
        .map_err(|error| format!("cannot send updated hosts rules: {error}"))?;
    drop(input);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot finish macOS authorization: {error}"))?;
    if output.status.success() {
        let flushed = std::process::Command::new("/usr/bin/dscacheutil")
            .arg("-flushcache")
            .status()
            .map_err(|error| {
                format!("hosts file changed, but DNS cache flush could not start: {error}")
            })?;
        if !flushed.success() {
            return Err(format!(
                "hosts file changed, but DNS cache flush failed ({flushed})"
            ));
        }
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.to_ascii_lowercase().contains("cancel") {
        return Err("system authorization was cancelled".to_string());
    }
    Err("system authorization could not update the hosts file".to_string())
}

#[cfg(windows)]
pub(crate) fn is_elevated() -> bool {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    } != 0;
    unsafe {
        CloseHandle(token);
    }
    result && elevation.TokenIsElevated != 0
}

#[cfg(windows)]
pub(crate) fn elevate_and_wait_windows(argument: &str, blocked: bool) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    let executable = std::env::current_exe().map_err(|error| {
        format!("cannot locate Synthesizer V Studio 2 Boxy executable: {error}")
    })?;
    let verb = wide(OsStr::new("runas"));
    let file = wide(executable.as_os_str());
    let parameters = wide(OsStr::new(&format!(
        "{argument} {}",
        if blocked { "enable" } else { "disable" }
    )));
    let directory = executable.parent().map(|path| wide(path.as_os_str()));
    let mut execute_info = SHELLEXECUTEINFOW::default();
    execute_info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    execute_info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    execute_info.lpVerb = verb.as_ptr();
    execute_info.lpFile = file.as_ptr();
    execute_info.lpParameters = parameters.as_ptr();
    execute_info.lpDirectory = directory
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());
    execute_info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut execute_info) } == 0 || execute_info.hProcess.is_null() {
        let error = unsafe { GetLastError() };
        return if error == 1223 {
            Err("administrator approval was cancelled".to_string())
        } else {
            Err(format!(
                "cannot start elevated network update (Windows error {error})"
            ))
        };
    }
    let waited = unsafe { WaitForSingleObject(execute_info.hProcess, INFINITE) };
    if waited != WAIT_OBJECT_0 {
        unsafe {
            CloseHandle(execute_info.hProcess);
        }
        return Err("elevated network update did not complete".to_string());
    }
    let mut exit_code = 1u32;
    let read_code = unsafe { GetExitCodeProcess(execute_info.hProcess, &mut exit_code) } != 0;
    unsafe {
        CloseHandle(execute_info.hProcess);
    }
    if !read_code {
        return Err("cannot read elevated network update result".to_string());
    }
    if exit_code != 0 {
        return Err(format!(
            "elevated network update failed (exit code {exit_code})"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn status_for(path: &Path, text: &str) -> HostBlockStatus {
    let blocked_hosts = BLOCKED_HOSTS
        .iter()
        .filter(|host| is_host_fully_blocked(text, host))
        .count();
    let managed = text.lines().any(is_managed_line);
    HostBlockStatus {
        blocked: blocked_hosts == BLOCKED_HOSTS.len(),
        managed,
        blocked_hosts,
        total_hosts: BLOCKED_HOSTS.len(),
        hosts_path: path.to_string_lossy().into_owned(),
    }
}

fn is_managed_line(line: &str) -> bool {
    let Some((_, comment)) = line.split_once('#') else {
        return false;
    };
    let comment = format!("# {}", comment.trim());
    matches!(
        comment.as_str(),
        MARKER | PREVIOUS_MARKER | LEGACY_MARKER | OLDER_MARKER
    )
}

pub fn is_host_blocked(host: &str) -> bool {
    read_hosts(&hosts_path()).is_ok_and(|text| is_host_fully_blocked(&text, host))
}

fn is_host_fully_blocked(text: &str, host: &str) -> bool {
    has_host_mapping(text, host, &["0.0.0.0", "127.0.0.1"])
        && has_host_mapping(text, host, &["::", "::1"])
}

fn has_host_mapping(text: &str, host: &str, addresses: &[&str]) -> bool {
    text.lines().any(|line| {
        let fields = line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>();
        fields
            .first()
            .is_some_and(|address| addresses.contains(address))
            && fields
                .iter()
                .skip(1)
                .any(|value| value.eq_ignore_ascii_case(host))
    })
}

fn rewrite_managed_rules(current: &str, blocked: bool) -> String {
    let newline = if current.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let ends_with_newline = current.ends_with(['\n', '\r']);
    let mut next = current
        .lines()
        .filter(|line| !is_managed_line(line))
        .collect::<Vec<_>>()
        .join(newline);
    if blocked {
        if !next.is_empty() {
            next.push_str(newline);
        }
        for host in BLOCKED_HOSTS {
            next.push_str(&format!("0.0.0.0 {host} {MARKER}{newline}"));
            next.push_str(&format!("::1 {host} {MARKER}{newline}"));
        }
    } else if ends_with_newline && !next.is_empty() {
        next.push_str(newline);
    }
    next
}

fn read_hosts(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect hosts file {}: {error}", path.display()))?;
    if metadata.len() > MAX_HOSTS_BYTES as u64 {
        return Err("hosts file is too large to edit".to_string());
    }
    fs::read_to_string(path)
        .map_err(|error| format!("cannot read hosts file {}: {error}", path.display()))
}

#[cfg(not(target_os = "macos"))]
fn write_hosts(path: &Path, text: &str) -> Result<(), String> {
    #[cfg(windows)]
    let original_attributes = clear_read_only(path)?;
    let result = fs::write(path, text);
    #[cfg(windows)]
    if let Some(attributes) = original_attributes {
        restore_attributes(path, attributes)?;
    }
    result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "cannot write hosts file {}; administrator access is required",
                path.display()
            )
        } else {
            format!("cannot write hosts file {}: {error}", path.display())
        }
    })
}

#[cfg(windows)]
fn clear_read_only(path: &Path) -> Result<Option<u32>, String> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, INVALID_FILE_ATTRIBUTES,
    };
    let wide_path = wide(path.as_os_str());
    let attributes = unsafe { GetFileAttributesW(wide_path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(format!(
            "cannot inspect hosts attributes {}",
            path.display()
        ));
    }
    if attributes & FILE_ATTRIBUTE_READONLY == 0 {
        return Ok(None);
    }
    let writable = attributes & !FILE_ATTRIBUTE_READONLY;
    if unsafe { SetFileAttributesW(wide_path.as_ptr(), writable) } == 0 {
        return Err(format!(
            "cannot clear hosts read-only attribute {}",
            path.display()
        ));
    }
    Ok(Some(attributes))
}

#[cfg(windows)]
fn restore_attributes(path: &Path, attributes: u32) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
    let wide_path = wide(path.as_os_str());
    if unsafe { SetFileAttributesW(wide_path.as_ptr(), attributes) } == 0 {
        return Err(format!(
            "cannot restore hosts attributes {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn hosts_path() -> PathBuf {
    windows_system_directory()
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\System32"))
        .join("drivers")
        .join("etc")
        .join("hosts")
}

#[cfg(not(windows))]
fn hosts_path() -> PathBuf {
    PathBuf::from("/etc/hosts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn enables_every_known_host_over_both_address_families() {
        let original = "127.0.0.1 localhost\r\n# preserved\r\n";
        let enabled = rewrite_managed_rules(original, true);
        let current = status_for(Path::new("hosts"), &enabled);

        assert!(current.blocked);
        assert!(current.managed);
        assert_eq!(current.blocked_hosts, BLOCKED_HOSTS.len());
        assert_eq!(current.total_hosts, BLOCKED_HOSTS.len());
        assert_eq!(
            enabled.lines().filter(|line| is_managed_line(line)).count(),
            BLOCKED_HOSTS.len() * 2
        );
        assert_eq!(rewrite_managed_rules(&enabled, true), enabled);
        assert_eq!(rewrite_managed_rules(&enabled, false), original);
    }

    #[test]
    fn upgrades_legacy_rules_without_touching_user_mappings() {
        let legacy = format!(
            "0.0.0.0 authr3.dreamtonics.com {OLDER_MARKER}\n::1 authr3.dreamtonics.com {PREVIOUS_MARKER}\n127.0.0.1 keep.example # user mapping\n"
        );
        let enabled = rewrite_managed_rules(&legacy, true);

        assert!(!enabled.contains(OLDER_MARKER));
        assert!(!enabled.contains(PREVIOUS_MARKER));
        assert!(enabled.contains("127.0.0.1 keep.example # user mapping"));
        assert!(status_for(Path::new("hosts"), &enabled).blocked);
    }

    #[test]
    fn incomplete_or_aliased_rules_have_precise_status() {
        let host = BLOCKED_HOSTS[0];
        let partial = format!("0.0.0.0 localhost {host}\n");
        assert!(!is_host_fully_blocked(&partial, host));

        let complete = format!("0.0.0.0 localhost {host}\n::1 localhost {host}\n");
        assert!(is_host_fully_blocked(&complete, host));
        let current = status_for(Path::new("hosts"), &complete);
        assert!(!current.blocked);
        assert_eq!(current.blocked_hosts, 1);
    }

    #[test]
    fn disable_only_removes_exact_managed_rules() {
        let managed = rewrite_managed_rules("0.0.0.0 keep.example # user mapping\n", true);
        let disabled = rewrite_managed_rules(&managed, false);

        assert_eq!(disabled, "0.0.0.0 keep.example # user mapping\n");
        assert!(!disabled.lines().any(is_managed_line));
    }
}
