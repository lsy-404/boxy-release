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

const MARKER: &str = "# Synthesizer V Studio 2 Boxy network block";
const PREVIOUS_MARKER: &str = "# Synthesizer V Studio 2 Boxy update block";
const LEGACY_MARKER: &str = "# SV2 Smooth update block";
const OLDER_MARKER: &str = "# SV2 Session Editor update block";
const MAX_HOSTS_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const ELEVATED_ARGUMENT: &str = "--sv2-host-cleanup";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCleanupStatus {
    pub managed: bool,
    pub hosts_path: String,
}

pub fn status() -> HostCleanupStatus {
    let path = hosts_path();
    let text = read_hosts(&path).unwrap_or_default();
    status_for(&path, &text)
}

pub fn cleanup() -> Result<HostCleanupStatus, String> {
    #[cfg(windows)]
    {
        if !is_elevated() {
            elevate_and_wait_windows(ELEVATED_ARGUMENT, false)?;
            return Ok(status());
        }
        return cleanup_direct();
    }
    #[cfg(target_os = "macos")]
    {
        elevate_and_wait()?;
        return Ok(status());
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    Err("host cleanup is unavailable on this platform".to_string())
}

#[cfg(windows)]
fn cleanup_direct() -> Result<HostCleanupStatus, String> {
    let path = windows_system_directory()?.join("drivers/etc/hosts");
    let current = read_hosts(&path)?;
    let next = remove_managed_rules(&current);
    if next != current {
        write_hosts(&path, &next)?;
        #[cfg(windows)]
        flush_windows_dns_cache()?;
    }
    Ok(status_for(&path, &read_hosts(&path)?))
}

#[cfg(windows)]
fn flush_windows_dns_cache() -> Result<(), String> {
    let mut command = std::process::Command::new(windows_system_directory()?.join("ipconfig.exe"));
    command.arg("/flushdns");
    crate::windows_process::configure_background(&mut command);
    let result = command.status().map_err(|error| {
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
        if args.next()?.to_string_lossy() != "disable" || args.next().is_some() {
            return Some(1);
        }
        return Some(match cleanup_direct() {
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
fn elevate_and_wait() -> Result<(), String> {
    let path = hosts_path();
    let current = read_hosts(&path)?;
    let next = remove_managed_rules(&current);
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

fn status_for(path: &Path, text: &str) -> HostCleanupStatus {
    let managed = text.lines().any(is_managed_line);
    HostCleanupStatus {
        managed,
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

pub(super) fn remove_managed_rules(current: &str) -> String {
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
    if ends_with_newline && !next.is_empty() {
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
    fn removes_legacy_rules_without_touching_user_mappings() {
        let legacy = format!(
            "0.0.0.0 authr3.dreamtonics.com {OLDER_MARKER}\n::1 authr3.dreamtonics.com {PREVIOUS_MARKER}\n127.0.0.1 keep.example # user mapping\n"
        );
        let cleaned = remove_managed_rules(&legacy);

        assert!(!cleaned.contains(OLDER_MARKER));
        assert!(!cleaned.contains(PREVIOUS_MARKER));
        assert_eq!(cleaned, "127.0.0.1 keep.example # user mapping\n");
        assert!(!status_for(Path::new("hosts"), &cleaned).managed);
    }

    #[test]
    fn cleanup_only_removes_exact_managed_rules() {
        let original = "0.0.0.0 keep.example # user mapping\n";
        let managed = format!("{original}0.0.0.0 old.example {MARKER}\n");
        assert_eq!(remove_managed_rules(&managed), original);
        assert_eq!(remove_managed_rules(original), original);
    }
}
