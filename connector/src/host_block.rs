#[cfg(windows)]
use std::fs;
#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::{
    ffi::CString,
    fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
};

#[cfg(all(not(windows), any(target_os = "macos", test)))]
use std::path::Path;

#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(any(windows, target_os = "macos", test))]
const MARKER: &str = "# Synthesizer V Studio 2 Boxy network block";
#[cfg(any(windows, target_os = "macos", test))]
const PREVIOUS_MARKER: &str = "# Synthesizer V Studio 2 Boxy update block";
#[cfg(any(windows, target_os = "macos", test))]
const LEGACY_MARKER: &str = "# SV2 Smooth update block";
#[cfg(any(windows, target_os = "macos", test))]
const OLDER_MARKER: &str = "# SV2 Session Editor update block";
#[cfg(any(windows, target_os = "macos", test))]
const MAX_HOSTS_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const ELEVATED_ARGUMENT: &str = "--sv2-wfp-block";
#[cfg(windows)]
const WEBVIEW_RUNTIME_DIRECTORY: &str = "WebView2Runtime";
#[cfg(windows)]
const WEBVIEW_EXECUTABLE: &str = "msedgewebview2.exe";
#[cfg(windows)]
const WEBVIEW_LIBRARY: &str = "msedge.dll";
#[cfg(windows)]
const WEBVIEW_POLICY_PATH: &str =
    "SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2\\BrowserExecutableFolder";
#[cfg(windows)]
const WEBVIEW_POLICY_VALUE: &str = "synthv-studio.exe";
#[cfg(windows)]
const POLICY_STATE_PATH: &str = "SOFTWARE\\Boxy\\NetworkBlock";
#[cfg(windows)]
const POLICY_STATE_VALUE: &str = "WebView2BrowserExecutableFolder";
#[cfg(windows)]
const WEBVIEW_ENVIRONMENT_VALUE: &str = "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER";
#[cfg(windows)]
const MACHINE_ENVIRONMENT_PATH: &str =
    "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment";
#[cfg(windows)]
const USER_ENVIRONMENT_PATH: &str = "Environment";
#[cfg(target_os = "macos")]
const MACOS_CLEANUP_ARGUMENT: &str = "--sv2-cleanup-legacy-hosts";

#[cfg(target_os = "macos")]
const LULU_METHOD: &str = "lulu";
#[cfg(target_os = "macos")]
const LULU_RULE_NAME: &str = "Synthesizer V Studio 2 (Boxy block)";
#[cfg(target_os = "macos")]
const LULU_RULE_TOTAL: usize = 1;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBlockStatus {
    pub method: &'static str,
    pub blocked: bool,
    pub configured: bool,
    pub verified: bool,
    pub managed: bool,
    pub legacy_hosts: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub blocked_rules: usize,
    pub total_rules: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_import_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

pub fn status() -> Result<HostBlockStatus, String> {
    #[cfg(windows)]
    return wfp_status();

    #[cfg(target_os = "macos")]
    {
        let (legacy_hosts, warning) = macos_legacy_hosts_state();
        let mut status = lulu_status(
            None,
            lulu_import_instructions(None, legacy_hosts),
            legacy_hosts,
        );
        status.warning = warning;
        return Ok(status);
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    Err("network blocking is currently available only on Windows".to_string())
}

pub fn set_blocked(blocked: bool) -> Result<HostBlockStatus, String> {
    #[cfg(windows)]
    {
        if blocked {
            let targets = block_targets()?;
            if sv2_is_running(&targets.sv2_executable)? {
                return Err(
                    "close Synthesizer V Studio 2 before enabling the network block".to_string(),
                );
            }
            ensure_no_webview_runtime_override(&targets)?;
        }
        if !is_elevated() {
            elevate_and_wait(blocked)?;
            return status();
        }
        return set_wfp_blocked(blocked);
    }
    #[cfg(target_os = "macos")]
    {
        return if blocked {
            create_lulu_import_rule()
        } else {
            if macos_legacy_hosts_present()? {
                if unsafe { libc::geteuid() } == 0 {
                    cleanup_macos_legacy_hosts()?;
                } else {
                    elevate_and_cleanup_macos_legacy_hosts()?;
                }
            }
            let legacy_hosts = macos_legacy_hosts_present()?;
            let executable = macos_sv2_executable().ok();
            Ok(lulu_status(
                None,
                lulu_removal_instructions(executable.as_deref()),
                legacy_hosts,
            ))
        };
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = blocked;
        Err("network blocking is currently available only on Windows".to_string())
    }
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
        if args.next().is_some() {
            return Some(1);
        }
        return Some(if set_wfp_blocked(blocked).is_ok() {
            0
        } else {
            1
        });
    }
    #[cfg(target_os = "macos")]
    {
        let mut args = std::env::args_os();
        args.next();
        if args.next()?.to_string_lossy() != MACOS_CLEANUP_ARGUMENT {
            return None;
        }
        if args.next().is_some() {
            return Some(1);
        }
        return Some(if cleanup_macos_legacy_hosts().is_ok() {
            0
        } else {
            1
        });
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    None
}

#[cfg(windows)]
fn is_elevated() -> bool {
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
    unsafe { CloseHandle(token) };
    result && elevation.TokenIsElevated != 0
}

#[cfg(windows)]
fn elevate_and_wait(blocked: bool) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate Boxy executable: {error}"))?;
    let verb = wide(OsStr::new("runas"));
    let file = wide(executable.as_os_str());
    let parameters = wide(OsStr::new(if blocked {
        "--sv2-wfp-block enable"
    } else {
        "--sv2-wfp-block disable"
    }));
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
    if unsafe { ShellExecuteExW(&mut execute_info) } == 0 || execute_info.hProcess.is_null() {
        let error = unsafe { GetLastError() };
        return if error == 1223 {
            Err("administrator approval was cancelled".to_string())
        } else {
            Err(format!(
                "cannot start elevated WFP update (Windows error {error})"
            ))
        };
    }
    if unsafe { WaitForSingleObject(execute_info.hProcess, INFINITE) } != WAIT_OBJECT_0 {
        unsafe { CloseHandle(execute_info.hProcess) };
        return Err("elevated WFP update did not complete".to_string());
    }
    let mut exit_code = 1u32;
    let read_code = unsafe { GetExitCodeProcess(execute_info.hProcess, &mut exit_code) } != 0;
    unsafe { CloseHandle(execute_info.hProcess) };
    if !read_code {
        return Err("cannot read elevated WFP update result".to_string());
    }
    if exit_code != 0 {
        return Err(format!(
            "elevated WFP update failed (exit code {exit_code})"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
const WFP_FILTER_V4: windows_sys::core::GUID =
    windows_sys::core::GUID::from_u128(0x9df86dc4_6b4e_4f2a_862f_2e5728c7ced5);
#[cfg(windows)]
const WFP_FILTER_V6: windows_sys::core::GUID =
    windows_sys::core::GUID::from_u128(0x7c2fe28c_39af_4331_ae03_b884501ebd2e);
#[cfg(windows)]
const WFP_FILTER_WEBVIEW_V4: windows_sys::core::GUID =
    windows_sys::core::GUID::from_u128(0x0c50c7fd_f0f7_43e6_befa_6779f7d407d7);
#[cfg(windows)]
const WFP_FILTER_WEBVIEW_V6: windows_sys::core::GUID =
    windows_sys::core::GUID::from_u128(0x2f1a9b8e_0c62_44ac_806d_395164ea97de);
#[cfg(windows)]
const WFP_SUBLAYER: windows_sys::core::GUID =
    windows_sys::core::GUID::from_u128(0x8da48ae4_9a3f_4729_91f4_e6cd86af3095);
#[cfg(windows)]
const WFP_FILTER_COUNT: usize = 4;
#[cfg(windows)]
const FWP_E_FILTER_NOT_FOUND: u32 = windows_sys::Win32::Foundation::FWP_E_FILTER_NOT_FOUND as u32;
#[cfg(windows)]
const FWP_E_SUBLAYER_NOT_FOUND: u32 =
    windows_sys::Win32::Foundation::FWP_E_SUBLAYER_NOT_FOUND as u32;

#[cfg(windows)]
struct BlockTargets {
    sv2_executable: PathBuf,
    webview_policy_directory: String,
    webview_executable: PathBuf,
}

#[cfg(windows)]
struct WfpAppIds {
    sv2: Vec<u8>,
    webview: Vec<u8>,
}

#[cfg(windows)]
struct WfpEngine(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Drop for WfpEngine {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FwpmEngineClose0(
                self.0,
            )
        };
    }
}

#[cfg(windows)]
fn open_wfp_engine() -> Result<WfpEngine, String> {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FwpmEngineOpen0;
    let mut handle = HANDLE::default();
    wfp_result(
        unsafe {
            FwpmEngineOpen0(
                null(),
                windows_sys::Win32::System::Rpc::RPC_C_AUTHN_WINNT,
                null(),
                null(),
                &mut handle,
            )
        },
        "open Windows Filtering Platform",
    )?;
    Ok(WfpEngine(handle))
}

#[cfg(windows)]
fn wfp_result(status: u32, operation: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("cannot {operation} (WFP status 0x{status:08x})"))
    }
}

#[cfg(windows)]
fn wfp_status() -> Result<HostBlockStatus, String> {
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
        FwpmFilterGetByKey0, FwpmFreeMemory0, FwpmSubLayerGetByKey0,
    };
    let engine = open_wfp_engine()?;
    let targets = block_targets().ok();
    let app_ids = targets.as_ref().map(wfp_app_ids).transpose()?;
    let runtime_state = targets
        .as_ref()
        .map(webview_runtime_state)
        .transpose()?
        .unwrap_or_default();
    let legacy_hosts = legacy_hosts_present().unwrap_or(false);
    let mut sublayer = std::ptr::null_mut();
    let sublayer_result = unsafe { FwpmSubLayerGetByKey0(engine.0, &WFP_SUBLAYER, &mut sublayer) };
    let sublayer_valid = if sublayer.is_null() {
        false
    } else {
        let valid = unsafe { wfp_sublayer_is_valid(&*sublayer) };
        unsafe { FwpmFreeMemory0((&mut sublayer).cast()) };
        valid
    };
    let sublayer_present = match sublayer_result {
        0 => true,
        FWP_E_SUBLAYER_NOT_FOUND => false,
        code => {
            return Err(format!(
                "cannot inspect WFP sublayer (WFP status 0x{code:08x})"
            ))
        }
    };
    let expected_rules = app_ids.as_ref().map(|app_ids| [
        (WFP_FILTER_V4, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V4, app_ids.sv2.as_slice()),
        (WFP_FILTER_V6, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V6, app_ids.sv2.as_slice()),
        (WFP_FILTER_WEBVIEW_V4, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V4, app_ids.webview.as_slice()),
        (WFP_FILTER_WEBVIEW_V6, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V6, app_ids.webview.as_slice()),
    ]);
    let mut present_rules = 0usize;
    let blocked_rules = [
        (WFP_FILTER_V4, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V4, 0usize),
        (WFP_FILTER_V6, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V6, 1usize),
        (WFP_FILTER_WEBVIEW_V4, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V4, 2usize),
        (WFP_FILTER_WEBVIEW_V6, windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_LAYER_ALE_AUTH_CONNECT_V6, 3usize),
    ]
        .into_iter()
        .map(|(key, layer, expected_index)| {
            let mut filter = std::ptr::null_mut();
            let result = unsafe { FwpmFilterGetByKey0(engine.0, &key, &mut filter) };
            let valid = if filter.is_null() {
                false
            } else {
                let valid = expected_rules
                    .as_ref()
                    .is_some_and(|rules| unsafe {
                        wfp_filter_is_valid(&*filter, layer, rules[expected_index].2)
                    });
                unsafe { FwpmFreeMemory0((&mut filter).cast()) };
                valid
            };
            match result {
                0 => { present_rules += 1; Ok(valid) },
                FWP_E_FILTER_NOT_FOUND => Ok(false),
                code => Err(format!(
                    "cannot inspect WFP filter (WFP status 0x{code:08x})"
                )),
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|present| *present)
        .count();
    let configured = targets.is_some()
        && app_ids.is_some()
        && runtime_state.configured
        && sublayer_present
        && sublayer_valid
        && blocked_rules == WFP_FILTER_COUNT;
    Ok(HostBlockStatus {
        method: "wfp",
        blocked: configured,
        configured,
        verified: configured && runtime_state.verified,
        managed: sublayer_present || present_rules > 0,
        legacy_hosts,
        warning: runtime_state.warning,
        blocked_rules,
        total_rules: WFP_FILTER_COUNT,
        artifact_path: None,
        manual_import_required: None,
        instructions: None,
    })
}

#[cfg(target_os = "macos")]
fn lulu_status(
    artifact_path: Option<String>,
    instructions: String,
    legacy_hosts: bool,
) -> HostBlockStatus {
    HostBlockStatus {
        method: LULU_METHOD,
        blocked: false,
        configured: false,
        verified: false,
        managed: false,
        legacy_hosts,
        warning: None,
        blocked_rules: 0,
        total_rules: LULU_RULE_TOTAL,
        artifact_path,
        manual_import_required: Some(true),
        instructions: Some(instructions),
    }
}

#[cfg(target_os = "macos")]
fn create_lulu_import_rule() -> Result<HostBlockStatus, String> {
    let executable = macos_sv2_executable()?;
    let rule = lulu_import_document(&executable, LULU_RULE_NAME)?;
    let directory = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot prepare the LuLu rule directory: {error}"))?;
    let file = directory.join(format!("Boxy-SV2-LuLu-block-{}.json", random_uuid_v4()));
    let content = serde_json::to_vec_pretty(&rule)
        .map_err(|error| format!("cannot encode the LuLu import rule: {error}"))?;
    std::fs::write(&file, content).map_err(|error| {
        format!(
            "cannot save the LuLu import rule {}: {error}",
            file.display()
        )
    })?;
    let path = file.to_string_lossy().into_owned();
    let (legacy_hosts, warning) = macos_legacy_hosts_state();
    let mut status = lulu_status(
        Some(path.clone()),
        lulu_import_instructions(Some(&path), legacy_hosts),
        legacy_hosts,
    );
    status.warning = warning;
    Ok(status)
}

#[cfg(target_os = "macos")]
fn lulu_import_instructions(artifact_path: Option<&str>, legacy_hosts: bool) -> String {
    let file = artifact_path
        .map(|path| format!(" Select {path}."))
        .unwrap_or_default();
    let migration = legacy_hosts
        .then_some(
            " Old Boxy hosts entries are still active; use the explicit cleanup action before relying on LuLu.",
        )
        .unwrap_or_default();
    format!(
        "Install LuLu 4.5.0 or newer, then choose LuLu > Rules > Import… and import the generated file.{file} Verify the imported rule is Block +kids. Boxy cannot verify LuLu state until LuLu applies the rule.{migration}"
    )
}

#[cfg(target_os = "macos")]
fn lulu_removal_instructions(executable: Option<&Path>) -> String {
    let target = executable
        .map(|path| format!(" for {}", path.display()))
        .unwrap_or_default();
    format!(
        "Open LuLu > Rules and remove the Boxy SV2 Block rule{target}. Boxy does not modify LuLu's internal rules file or claim that the rule was removed."
    )
}

#[cfg(target_os = "macos")]
fn macos_legacy_hosts_present() -> Result<bool, String> {
    Ok(read_macos_hosts(&macos_hosts_path())?
        .lines()
        .any(is_managed_line))
}

#[cfg(target_os = "macos")]
fn macos_legacy_hosts_state() -> (bool, Option<String>) {
    match macos_legacy_hosts_present() {
        Ok(present) => (present, None),
        Err(error) => (
            false,
            Some(format!("cannot inspect old hosts rules: {error}")),
        ),
    }
}

#[cfg(target_os = "macos")]
fn cleanup_macos_legacy_hosts() -> Result<(), String> {
    let path = macos_hosts_path();
    let current = read_macos_hosts(&path)?;
    let next = remove_legacy_hosts_rules(&current);
    if next != current {
        write_macos_hosts_atomically(&path, &next)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn elevate_and_cleanup_macos_legacy_hosts() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate Boxy executable: {error}"))?;
    let executable = executable
        .to_str()
        .ok_or_else(|| "Boxy executable path is not valid UTF-8".to_string())?;
    if executable.contains(['\n', '\r']) {
        return Err("Boxy executable path cannot contain a line break".to_string());
    }
    let command = format!("{} {MACOS_CLEANUP_ARGUMENT}", shell_quote(executable));
    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_string(&command)
    );
    let output = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|error| format!("cannot request administrator approval: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err("administrator approval was cancelled or legacy hosts cleanup failed".to_string())
    }
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\""))
}

#[cfg(target_os = "macos")]
fn macos_hosts_path() -> PathBuf {
    PathBuf::from("/etc/hosts")
}

#[cfg(target_os = "macos")]
fn read_macos_hosts(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect hosts file {}: {error}", path.display()))?;
    if metadata.len() > MAX_HOSTS_BYTES as u64 {
        return Err("hosts file is too large to edit".to_string());
    }
    fs::read_to_string(path)
        .map_err(|error| format!("cannot read hosts file {}: {error}", path.display()))
}

#[cfg(target_os = "macos")]
fn write_macos_hosts_atomically(path: &Path, text: &str) -> Result<(), String> {
    use std::os::darwin::fs::MetadataExt as DarwinMetadataExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect hosts file {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || DarwinMetadataExt::st_flags(&metadata) != 0 {
        return Err(
            "hosts file is linked or has special flags; remove old Boxy entries manually"
                .to_string(),
        );
    }
    let directory = path
        .parent()
        .ok_or_else(|| "hosts file has no parent directory".to_string())?;
    let temporary = directory.join(format!(".hosts.boxy-{}.tmp", random_uuid_v4()));
    let source_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "hosts path contains a NUL byte".to_string())?;
    let temporary_path = CString::new(temporary.as_os_str().as_bytes())
        .map_err(|_| "hosts replacement path contains a NUL byte".to_string())?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("cannot create hosts replacement: {error}"))?;
        file.write_all(text.as_bytes())
            .map_err(|error| format!("cannot write hosts replacement: {error}"))?;
        if unsafe { libc::chown(temporary_path.as_ptr(), metadata.uid(), metadata.gid()) } != 0 {
            return Err(format!(
                "cannot preserve hosts ownership: {}",
                std::io::Error::last_os_error()
            ));
        }
        fs::set_permissions(&temporary, metadata.permissions())
            .map_err(|error| format!("cannot preserve hosts permissions: {error}"))?;
        if unsafe {
            libc::copyfile(
                source_path.as_ptr(),
                temporary_path.as_ptr(),
                std::ptr::null_mut(),
                libc::COPYFILE_ACL | libc::COPYFILE_XATTR,
            )
        } != 0
        {
            return Err(format!(
                "cannot preserve hosts ACL or extended attributes: {}",
                std::io::Error::last_os_error()
            ));
        }
        file.sync_all()
            .map_err(|error| format!("cannot sync hosts replacement: {error}"))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot replace hosts file {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "macos")]
fn macos_sv2_executable() -> Result<PathBuf, String> {
    let mut bundles = vec![
        PathBuf::from("/Applications/Synthesizer V Studio 2.app"),
        PathBuf::from("/Applications/Synthesizer V Studio 2 Pro.app"),
    ];
    if let Some(home) = dirs::home_dir() {
        bundles.push(home.join("Applications/Synthesizer V Studio 2.app"));
        bundles.push(home.join("Applications/Synthesizer V Studio 2 Pro.app"));
    }
    for directory in [
        Some(PathBuf::from("/Applications")),
        dirs::home_dir().map(|home| home.join("Applications")),
    ]
    .into_iter()
    .flatten()
    {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if path.is_dir()
                && name.ends_with(".app")
                && name.to_ascii_lowercase().contains("synthesizer v studio 2")
            {
                bundles.push(path);
            }
        }
    }
    bundles.sort();
    bundles.dedup();
    for bundle in bundles {
        if let Ok(executable) = macos_bundle_executable(&bundle) {
            return Ok(executable);
        }
    }
    Err("cannot locate a Synthesizer V Studio 2 application bundle in Applications".to_string())
}

#[cfg(target_os = "macos")]
fn macos_bundle_executable(bundle: &Path) -> Result<PathBuf, String> {
    let info = bundle.join("Contents/Info.plist");
    let output = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleExecutable"])
        .arg(&info)
        .output()
        .map_err(|error| format!("cannot read {}: {error}", info.display()))?;
    if !output.status.success() {
        return Err(format!("{} has no CFBundleExecutable", info.display()));
    }
    let executable_name = String::from_utf8(output.stdout)
        .map_err(|_| format!("{} has a non-UTF-8 CFBundleExecutable", info.display()))?;
    let executable_name = executable_name.trim();
    if executable_name.is_empty() || executable_name.contains(['/', '\\']) {
        return Err(format!(
            "{} has an unsafe CFBundleExecutable",
            info.display()
        ));
    }
    let executable = bundle.join("Contents/MacOS").join(executable_name);
    if !executable.is_file() {
        return Err(format!(
            "{} does not contain its declared executable",
            bundle.display()
        ));
    }
    let bundle_root = std::fs::canonicalize(bundle)
        .map_err(|error| format!("cannot resolve {}: {error}", bundle.display()))?;
    let executable_root = std::fs::canonicalize(&executable)
        .map_err(|error| format!("cannot resolve {}: {error}", executable.display()))?;
    if !executable_root.starts_with(bundle_root) {
        return Err(format!(
            "{} resolves outside its application bundle",
            executable.display()
        ));
    }
    Ok(executable_root)
}

#[cfg(any(target_os = "macos", test))]
#[derive(serde::Serialize)]
struct LuluRule<'a> {
    key: String,
    uuid: String,
    path: &'a str,
    name: &'a str,
    #[serde(rename = "endpointAddr")]
    endpoint_addr: &'static str,
    #[serde(rename = "endpointPort")]
    endpoint_port: &'static str,
    #[serde(rename = "isEndpointAddrRegex")]
    endpoint_addr_kind: u8,
    creation: String,
    #[serde(rename = "type")]
    rule_type: u8,
    scope: u8,
    action: u8,
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn lulu_import_document(
    executable: &Path,
    name: &str,
) -> Result<serde_json::Value, String> {
    let path = executable
        .to_str()
        .ok_or_else(|| "the SV2 executable path is not valid UTF-8".to_string())?;
    let creation = time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]+0000"
        ))
        .map_err(|error| format!("cannot format LuLu rule creation time: {error}"))?;
    let rule = LuluRule {
        key: path.to_string(),
        uuid: random_uuid_v4(),
        path,
        name,
        endpoint_addr: "*",
        endpoint_port: "*",
        endpoint_addr_kind: 0,
        creation,
        rule_type: 3,
        scope: 2,
        action: 0,
    };
    let mut document = serde_json::Map::new();
    document.insert(
        rule.key.clone(),
        serde_json::to_value(vec![rule])
            .map_err(|error| format!("cannot encode the LuLu import rule: {error}"))?,
    );
    Ok(serde_json::Value::Object(document))
}

#[cfg(any(target_os = "macos", test))]
fn random_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(windows)]
fn block_targets() -> Result<BlockTargets, String> {
    let sv2_executable = crate::session_io::windows_executable()?;
    let boxy_executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate Boxy executable: {error}"))?;
    let boxy_executable = boxy_executable
        .canonicalize()
        .map_err(|error| format!("cannot resolve the Boxy executable: {error}"))?;
    let boxy_root = boxy_executable
        .parent()
        .ok_or_else(|| "Boxy executable has no parent directory".to_string())?
        .to_path_buf();
    let runtime = boxy_root.join(WEBVIEW_RUNTIME_DIRECTORY);
    let webview_executable = runtime.join(WEBVIEW_EXECUTABLE);
    let webview_library = runtime.join(WEBVIEW_LIBRARY);
    if !webview_executable.is_file() || !webview_library.is_file() {
        return Err(
            "the bundled WebView2 runtime is incomplete; reinstall the Boxy package before enabling the network block"
                .to_string(),
        );
    }
    reject_reparse_point(&runtime, "WebView2 runtime directory")?;
    reject_reparse_point(&webview_executable, "WebView2 executable")?;
    reject_reparse_point(&webview_library, "WebView2 library")?;
    let webview_runtime = runtime.canonicalize().map_err(|error| {
        format!("cannot resolve the bundled WebView2 runtime directory: {error}")
    })?;
    let webview_executable = webview_executable
        .canonicalize()
        .map_err(|error| format!("cannot resolve the bundled WebView2 executable: {error}"))?;
    let webview_library = webview_library
        .canonicalize()
        .map_err(|error| format!("cannot resolve the bundled WebView2 library: {error}"))?;
    if !is_path_within(&boxy_root, &webview_runtime)
        || !is_path_within(&boxy_root, &webview_executable)
        || !is_path_within(&boxy_root, &webview_library)
    {
        return Err(
            "the bundled WebView2 runtime resolves outside the Boxy installation; reinstall the Boxy package"
                .to_string(),
        );
    }
    Ok(BlockTargets {
        sv2_executable,
        webview_policy_directory: browser_executable_folder(&webview_runtime)?,
        webview_executable,
    })
}

#[cfg(windows)]
fn reject_reparse_point(path: &Path, item: &str) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    let path_wide = wide(path.as_os_str());
    let attributes = unsafe { GetFileAttributesW(path_wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(format!("cannot inspect the {item}"));
    }
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "the {item} is a link or junction; reinstall the Boxy package with an unpacked private WebView2 runtime"
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn is_path_within(root: &Path, candidate: &Path) -> bool {
    let root = root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    let candidate = candidate.to_string_lossy();
    candidate
        .get(..root.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&root))
        && candidate
            .as_bytes()
            .get(root.len())
            .is_some_and(|byte| *byte == b'\\' || *byte == b'/')
}

#[cfg(windows)]
pub(crate) fn browser_executable_folder(runtime: &Path) -> Result<String, String> {
    let runtime = runtime.to_string_lossy();
    if runtime.starts_with(r"\\?\UNC\") {
        return Err(
            "the bundled WebView2 runtime must be installed in an absolute local drive path; reinstall the Boxy package outside a network share"
                .to_string(),
        );
    }
    let runtime = runtime.strip_prefix(r"\\?\").unwrap_or(&runtime);
    if runtime.starts_with(r"\\") || !Path::new(runtime).is_absolute() {
        return Err(
            "the bundled WebView2 runtime must be installed in an absolute local drive path; reinstall the Boxy package outside a network share"
                .to_string(),
        );
    }
    Ok(runtime.trim_end_matches(['\\', '/']).to_string())
}

#[cfg(windows)]
fn wfp_app_ids(targets: &BlockTargets) -> Result<WfpAppIds, String> {
    Ok(WfpAppIds {
        sv2: wfp_app_id(&targets.sv2_executable, "installed SV2")?,
        webview: wfp_app_id(&targets.webview_executable, "bundled WebView2")?,
    })
}

#[cfg(windows)]
fn wfp_app_id(executable: &Path, name: &str) -> Result<Vec<u8>, String> {
    let executable = wide(executable.as_os_str());
    let mut app_id = std::ptr::null_mut();
    wfp_result(
        unsafe {
            windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FwpmGetAppIdFromFileName0(
                executable.as_ptr(),
                &mut app_id,
            )
        },
        &format!("derive the {name} WFP application identifier"),
    )?;
    if app_id.is_null() || unsafe { (*app_id).data.is_null() } {
        if !app_id.is_null() {
            unsafe {
                windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FwpmFreeMemory0(
                    (&mut app_id).cast(),
                )
            };
        }
        return Err(format!(
            "Windows returned an empty {name} WFP application identifier"
        ));
    }
    let bytes =
        unsafe { std::slice::from_raw_parts((*app_id).data, (*app_id).size as usize).to_vec() };
    unsafe {
        windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FwpmFreeMemory0(
            (&mut app_id).cast(),
        )
    };
    Ok(bytes)
}

#[cfg(windows)]
unsafe fn wfp_sublayer_is_valid(
    sublayer: &windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_SUBLAYER0,
) -> bool {
    guid_is(sublayer.subLayerKey, WFP_SUBLAYER)
        && sublayer.flags
            & windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_SUBLAYER_FLAG_PERSISTENT
            != 0
        && sublayer.weight == 0xffff
}

#[cfg(windows)]
unsafe fn wfp_filter_is_valid(
    filter: &windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWPM_FILTER0,
    layer: windows_sys::core::GUID,
    expected_app_id: &[u8],
) -> bool {
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
        FWPM_CONDITION_ALE_APP_ID, FWPM_FILTER_FLAG_PERSISTENT, FWP_ACTION_BLOCK,
        FWP_BYTE_BLOB_TYPE, FWP_MATCH_EQUAL,
    };
    if !guid_is(filter.layerKey, layer)
        || !guid_is(filter.subLayerKey, WFP_SUBLAYER)
        || filter.flags & FWPM_FILTER_FLAG_PERSISTENT == 0
        || filter.action.r#type != FWP_ACTION_BLOCK
        || filter.numFilterConditions != 1
        || filter.filterCondition.is_null()
    {
        return false;
    }
    let condition = &*filter.filterCondition;
    if !guid_is(condition.fieldKey, FWPM_CONDITION_ALE_APP_ID)
        || condition.matchType != FWP_MATCH_EQUAL
        || condition.conditionValue.r#type != FWP_BYTE_BLOB_TYPE
    {
        return false;
    }
    let app_id = condition.conditionValue.Anonymous.byteBlob;
    if app_id.is_null() || (*app_id).data.is_null() || (*app_id).size == 0 {
        return false;
    }
    let actual = std::slice::from_raw_parts((*app_id).data, (*app_id).size as usize);
    actual == expected_app_id
}

#[cfg(windows)]
fn guid_is(left: windows_sys::core::GUID, right: windows_sys::core::GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

#[cfg(windows)]
fn set_wfp_blocked(blocked: bool) -> Result<HostBlockStatus, String> {
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
        FwpmTransactionAbort0, FwpmTransactionBegin0, FwpmTransactionCommit0,
    };
    let targets = blocked.then(block_targets).transpose()?;
    if let Some(targets) = targets.as_ref() {
        if sv2_is_running(&targets.sv2_executable)? {
            return Err(
                "close Synthesizer V Studio 2 before enabling the network block".to_string(),
            );
        }
        ensure_no_webview_runtime_override(targets)?;
        ensure_webview_runtime_access(Path::new(&targets.webview_policy_directory))?;
    }
    let engine = open_wfp_engine()?;
    wfp_result(
        unsafe { FwpmTransactionBegin0(engine.0, 0) },
        "begin WFP transaction",
    )?;
    let policy_change = match if let Some(targets) = targets.as_ref() {
        install_webview_policy(targets)
    } else {
        remove_owned_webview_policy()
    } {
        Ok(change) => change,
        Err(error) => {
            unsafe { FwpmTransactionAbort0(engine.0) };
            return Err(error);
        }
    };
    let changed = if blocked {
        install_wfp_filters(
            &engine,
            targets.as_ref().expect("targets exist when enabling"),
        )
    } else {
        remove_wfp_filters(&engine)
    };
    if let Err(error) = changed {
        unsafe { FwpmTransactionAbort0(engine.0) };
        return Err(rollback_policy_change(policy_change, error));
    }
    if let Err(error) = wfp_result(
        unsafe { FwpmTransactionCommit0(engine.0) },
        "commit WFP transaction",
    ) {
        unsafe { FwpmTransactionAbort0(engine.0) };
        return Err(rollback_policy_change(policy_change, error));
    }
    let cleanup_warning = cleanup_legacy_windows_hosts().err();
    let mut current = wfp_status()?;
    if current.blocked != blocked {
        return Err("WFP filter state could not be verified".to_string());
    }
    if let Some(error) = cleanup_warning {
        let cleanup = format!("WFP updated, but legacy hosts cleanup failed: {error}");
        current.warning = Some(match current.warning {
            Some(warning) => format!("{warning}; {cleanup}"),
            None => cleanup,
        });
    }
    Ok(current)
}

#[cfg(windows)]
fn sv2_is_running(executable: &Path) -> Result<bool, String> {
    let status = std::process::Command::new("powershell.exe")
        .env("BOXY_SV2_EXECUTABLE", executable)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$target = [IO.Path]::GetFullPath($env:BOXY_SV2_EXECUTABLE).TrimEnd('\\'); foreach ($process in @(Get-Process -Name 'synthv-studio' -ErrorAction SilentlyContinue)) { try { $path = $process.Path } catch { exit 0 }; if (-not $path) { exit 0 }; if ([IO.Path]::GetFullPath($path).TrimEnd('\\') -ieq $target) { exit 0 } }; exit 1",
        ])
        .status()
        .map_err(|error| format!("cannot check whether SV2 is running: {error}"))?;
    Ok(status.success())
}

#[cfg(windows)]
#[derive(Default)]
struct WebviewRuntimeState {
    configured: bool,
    verified: bool,
    warning: Option<String>,
}

#[cfg(windows)]
fn webview_runtime_state(targets: &BlockTargets) -> Result<WebviewRuntimeState, String> {
    if !webview_policy_matches(targets)? {
        return Ok(WebviewRuntimeState::default());
    }
    if let Some(source) = webview_runtime_override(&targets.webview_policy_directory)? {
        return Ok(WebviewRuntimeState {
            configured: false,
            verified: false,
            warning: Some(format!(
                "WebView2 runtime selection is overridden by {source}; the private runtime network rules are not active for SV2"
            )),
        });
    }
    if has_unresolved_aumid_policy()? {
        return Ok(WebviewRuntimeState {
            configured: false,
            verified: false,
            warning: Some(
                "a WebView2 AUMID-specific policy may take precedence over the synthv-studio.exe policy; resolve it before enabling the network block"
                    .to_string(),
            ),
        });
    }
    Ok(WebviewRuntimeState {
        configured: true,
        verified: false,
        warning: Some(
            "WFP and the WebView2 policy are configured; a stopped SV2 process cannot reveal an app-set WebView2 environment override or its AUMID"
                .to_string(),
        ),
    })
}

#[cfg(windows)]
fn ensure_no_webview_runtime_override(targets: &BlockTargets) -> Result<(), String> {
    if let Some(source) = webview_runtime_override(&targets.webview_policy_directory)? {
        return Err(format!(
            "the WebView2 runtime is overridden by {source}; clear that override before enabling the network block"
        ));
    }
    if has_unresolved_aumid_policy()? {
        return Err(
            "a WebView2 AUMID-specific policy may take precedence over the synthv-studio.exe policy; resolve it before enabling the network block"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn webview_runtime_override(expected: &str) -> Result<Option<&'static str>, String> {
    if std::env::var_os(WEBVIEW_ENVIRONMENT_VALUE).is_some_and(|value| {
        !value.is_empty() && !same_windows_path(&value.to_string_lossy(), &expected)
    }) {
        return Ok(Some("the current process environment"));
    }
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;
    for (source, value) in [
        (
            "the machine environment",
            registry_environment_string(
                &RegKey::predef(HKEY_LOCAL_MACHINE),
                MACHINE_ENVIRONMENT_PATH,
                KEY_READ | KEY_WOW64_64KEY,
            )?,
        ),
        (
            "the user environment",
            registry_environment_string(
                &RegKey::predef(HKEY_CURRENT_USER),
                USER_ENVIRONMENT_PATH,
                KEY_READ,
            )?,
        ),
    ] {
        if value
            .as_deref()
            .is_some_and(|value| !value.is_empty() && !same_windows_path(value, &expected))
        {
            return Ok(Some(source));
        }
    }
    Ok(None)
}

#[cfg(windows)]
fn has_unresolved_aumid_policy() -> Result<bool, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ};
    use winreg::RegKey;

    for hive in [
        (
            RegKey::predef(HKEY_LOCAL_MACHINE),
            KEY_READ | KEY_WOW64_64KEY,
        ),
        (RegKey::predef(HKEY_CURRENT_USER), KEY_READ),
    ] {
        let key = match hive.0.open_subkey_with_flags(WEBVIEW_POLICY_PATH, hive.1) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot inspect WebView2 policy selectors: {error}")),
        };
        for item in key.enum_values() {
            let (name, value) =
                item.map_err(|error| format!("cannot inspect WebView2 policy selectors: {error}"))?;
            if name.eq_ignore_ascii_case(WEBVIEW_POLICY_VALUE)
                || name == "*"
                || name.to_ascii_lowercase().ends_with(".exe")
            {
                continue;
            }
            if value.vtype != REG_SZ {
                return Err("a WebView2 AUMID policy has an invalid registry type".to_string());
            }
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(windows)]
enum PolicyChange {
    Unchanged,
    Added(String),
    Removed(String),
}

#[cfg(windows)]
fn ensure_webview_runtime_access(runtime: &Path) -> Result<(), String> {
    let output = std::process::Command::new("icacls.exe")
        .arg(runtime)
        .args([
            "/grant",
            "*S-1-15-2-2:(OI)(CI)(RX)",
            "/grant",
            "*S-1-15-2-1:(OI)(CI)(RX)",
            "/T",
        ])
        .output()
        .map_err(|error| {
            format!(
                "cannot grant the bundled WebView2 runtime the AppContainer read-and-execute access required on Windows 10: {error}"
            )
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(
            "cannot grant the bundled WebView2 runtime the AppContainer read-and-execute access required on Windows 10; repair the Boxy installation and try again"
                .to_string(),
        )
    }
}

#[cfg(windows)]
fn install_webview_policy(targets: &BlockTargets) -> Result<PolicyChange, String> {
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE,
    };
    use winreg::RegKey;

    let expected = targets.webview_policy_directory.clone();
    let machine_value = registry_string(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        WEBVIEW_POLICY_PATH,
        WEBVIEW_POLICY_VALUE,
        KEY_READ | KEY_WOW64_64KEY,
    )?;
    let user_value = registry_string(
        &RegKey::predef(HKEY_CURRENT_USER),
        WEBVIEW_POLICY_PATH,
        WEBVIEW_POLICY_VALUE,
        KEY_READ,
    )?;
    for (scope, value) in [
        ("HKLM", machine_value.as_deref()),
        ("HKCU", user_value.as_deref()),
    ] {
        if let Some(value) = value {
            if !same_windows_path(value, &expected) {
                return Err(format!(
                    "the {scope} WebView2 BrowserExecutableFolder policy for synthv-studio.exe points to another runtime; resolve that policy before enabling the network block"
                ));
            }
        }
    }
    if machine_value.is_some() {
        return Ok(PolicyChange::Unchanged);
    }
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (policy, _) = machine
        .create_subkey_with_flags(WEBVIEW_POLICY_PATH, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| format!("cannot create the WebView2 policy key: {error}"))?;
    policy
        .set_value(WEBVIEW_POLICY_VALUE, &expected)
        .map_err(|error| format!("cannot configure the WebView2 policy: {error}"))?;
    let (state, _) = machine
        .create_subkey_with_flags(POLICY_STATE_PATH, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| format!("cannot create the Boxy policy state: {error}"))?;
    if let Err(error) = state.set_value(POLICY_STATE_VALUE, &expected) {
        return match policy.delete_value(WEBVIEW_POLICY_VALUE) {
            Ok(()) => Err(format!(
                "cannot record the Boxy WebView2 policy state: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "cannot record the Boxy WebView2 policy state: {error}; policy rollback also failed: {rollback_error}"
            )),
        };
    }
    Ok(PolicyChange::Added(expected))
}

#[cfg(windows)]
fn remove_owned_webview_policy() -> Result<PolicyChange, String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE};
    use winreg::RegKey;

    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let owned = registry_string(
        &machine,
        POLICY_STATE_PATH,
        POLICY_STATE_VALUE,
        KEY_READ | KEY_WOW64_64KEY,
    )?;
    let Some(owned) = owned else {
        return Ok(PolicyChange::Unchanged);
    };
    let current = registry_string(
        &machine,
        WEBVIEW_POLICY_PATH,
        WEBVIEW_POLICY_VALUE,
        KEY_READ | KEY_WOW64_64KEY,
    )?;
    let mut removed = false;
    if current
        .as_deref()
        .is_some_and(|value| same_windows_path(value, &owned))
    {
        let policy = machine
            .open_subkey_with_flags(WEBVIEW_POLICY_PATH, KEY_WRITE | KEY_WOW64_64KEY)
            .map_err(|error| format!("cannot open the WebView2 policy for removal: {error}"))?;
        policy
            .delete_value(WEBVIEW_POLICY_VALUE)
            .map_err(|error| format!("cannot remove the Boxy WebView2 policy: {error}"))?;
        removed = true;
    }
    if let Ok(state) =
        machine.open_subkey_with_flags(POLICY_STATE_PATH, KEY_WRITE | KEY_WOW64_64KEY)
    {
        if let Err(error) = state.delete_value(POLICY_STATE_VALUE) {
            if removed {
                let (policy, _) = machine
                    .create_subkey_with_flags(WEBVIEW_POLICY_PATH, KEY_WRITE | KEY_WOW64_64KEY)
                    .map_err(|restore_error| format!(
                        "cannot clear the Boxy WebView2 policy state: {error}; policy restoration also failed: {restore_error}"
                    ))?;
                policy.set_value(WEBVIEW_POLICY_VALUE, &owned).map_err(|restore_error| format!(
                    "cannot clear the Boxy WebView2 policy state: {error}; policy restoration also failed: {restore_error}"
                ))?;
            }
            return Err(format!(
                "cannot clear the Boxy WebView2 policy state: {error}"
            ));
        }
    }
    Ok(if removed {
        PolicyChange::Removed(owned)
    } else {
        PolicyChange::Unchanged
    })
}

#[cfg(windows)]
fn rollback_policy_change(change: PolicyChange, error: String) -> String {
    let rollback = match change {
        PolicyChange::Unchanged => Ok(()),
        PolicyChange::Added(value) => remove_webview_policy_if_owned(&value),
        PolicyChange::Removed(value) => restore_owned_webview_policy(&value),
    };
    match rollback {
        Ok(()) => error,
        Err(rollback_error) => format!("{error}; policy rollback also failed: {rollback_error}"),
    }
}

#[cfg(windows)]
fn remove_webview_policy_if_owned(expected: &str) -> Result<(), String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;

    let owned = registry_string(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        POLICY_STATE_PATH,
        POLICY_STATE_VALUE,
        KEY_READ | KEY_WOW64_64KEY,
    )?;
    if !owned
        .as_deref()
        .is_some_and(|value| same_windows_path(value, expected))
    {
        return Err("the WebView2 policy changed before it could be rolled back".to_string());
    }
    let change = remove_owned_webview_policy()?;
    if matches!(change, PolicyChange::Removed(ref value) if same_windows_path(value, expected)) {
        Ok(())
    } else {
        Err("the WebView2 policy changed before it could be rolled back".to_string())
    }
}

#[cfg(windows)]
fn restore_owned_webview_policy(value: &str) -> Result<(), String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE};
    use winreg::RegKey;

    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Some(current) = registry_string(
        &machine,
        WEBVIEW_POLICY_PATH,
        WEBVIEW_POLICY_VALUE,
        KEY_READ | KEY_WOW64_64KEY,
    )? {
        if !same_windows_path(&current, value) {
            return Err("the WebView2 policy changed before it could be restored".to_string());
        }
    } else {
        let (policy, _) = machine
            .create_subkey_with_flags(WEBVIEW_POLICY_PATH, KEY_WRITE | KEY_WOW64_64KEY)
            .map_err(|error| format!("cannot restore the WebView2 policy key: {error}"))?;
        policy
            .set_value(WEBVIEW_POLICY_VALUE, &value)
            .map_err(|error| format!("cannot restore the WebView2 policy: {error}"))?;
    }
    let (state, _) = machine
        .create_subkey_with_flags(POLICY_STATE_PATH, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| format!("cannot restore the Boxy policy state: {error}"))?;
    state
        .set_value(POLICY_STATE_VALUE, &value)
        .map_err(|error| format!("cannot restore the Boxy policy state: {error}"))
}

#[cfg(windows)]
fn webview_policy_matches(targets: &BlockTargets) -> Result<bool, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;

    let expected = &targets.webview_policy_directory;
    let machine = registry_string(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        WEBVIEW_POLICY_PATH,
        WEBVIEW_POLICY_VALUE,
        KEY_READ | KEY_WOW64_64KEY,
    )?;
    let user = registry_string(
        &RegKey::predef(HKEY_CURRENT_USER),
        WEBVIEW_POLICY_PATH,
        WEBVIEW_POLICY_VALUE,
        KEY_READ,
    )?;
    Ok(machine
        .as_deref()
        .is_some_and(|value| same_windows_path(value, &expected))
        && user
            .as_deref()
            .is_none_or(|value| same_windows_path(value, &expected)))
}

#[cfg(windows)]
fn registry_string(
    hive: &winreg::RegKey,
    path: &str,
    value: &str,
    flags: u32,
) -> Result<Option<String>, String> {
    use std::io::ErrorKind;
    use winreg::enums::REG_SZ;
    use winreg::types::FromRegValue;

    let key = match hive.open_subkey_with_flags(path, flags) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read the WebView2 policy key: {error}")),
    };
    match key.get_raw_value(value) {
        Ok(value) if value.vtype == REG_SZ => String::from_reg_value(&value)
            .map(Some)
            .map_err(|error| format!("cannot read the WebView2 policy value: {error}")),
        Ok(_) => Err("the WebView2 policy value must use the REG_SZ registry type".to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read the WebView2 policy value: {error}")),
    }
}

#[cfg(windows)]
fn registry_environment_string(
    hive: &winreg::RegKey,
    path: &str,
    flags: u32,
) -> Result<Option<String>, String> {
    use std::io::ErrorKind;
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};
    use winreg::types::FromRegValue;

    let key = match hive.open_subkey_with_flags(path, flags) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot read the Windows environment registry key: {error}"
            ))
        }
    };
    match key.get_raw_value(WEBVIEW_ENVIRONMENT_VALUE) {
        Ok(value) if value.vtype == REG_SZ || value.vtype == REG_EXPAND_SZ => {
            String::from_reg_value(&value)
                .map(Some)
                .map_err(|error| format!("cannot read the WebView2 environment override: {error}"))
        }
        Ok(_) => {
            Err("the WebView2 environment override has an unsupported registry type".to_string())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "cannot read the WebView2 environment override: {error}"
        )),
    }
}

#[cfg(windows)]
pub(crate) fn same_windows_path(left: &str, right: &str) -> bool {
    left.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(right.trim_end_matches(['\\', '/']))
}

#[cfg(windows)]
fn install_wfp_filters(engine: &WfpEngine, targets: &BlockTargets) -> Result<(), String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
        FwpmFilterAdd0, FwpmFilterDeleteByKey0, FwpmFreeMemory0, FwpmGetAppIdFromFileName0,
        FwpmSubLayerAdd0, FwpmSubLayerDeleteByKey0, FWPM_ACTION0, FWPM_CONDITION_ALE_APP_ID,
        FWPM_DISPLAY_DATA0, FWPM_FILTER0, FWPM_FILTER_CONDITION0, FWPM_FILTER_FLAG_PERSISTENT,
        FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_SUBLAYER0,
        FWPM_SUBLAYER_FLAG_PERSISTENT, FWP_ACTION_BLOCK, FWP_BYTE_BLOB_TYPE, FWP_CONDITION_VALUE0,
        FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL,
    };
    let sv2_executable = wide(targets.sv2_executable.as_os_str());
    let webview_executable = wide(targets.webview_executable.as_os_str());
    let mut sv2_app_id = null_mut();
    let mut webview_app_id = null_mut();
    wfp_result(
        unsafe { FwpmGetAppIdFromFileName0(sv2_executable.as_ptr(), &mut sv2_app_id) },
        "derive the SV2 WFP application identifier",
    )?;
    if let Err(error) = wfp_result(
        unsafe { FwpmGetAppIdFromFileName0(webview_executable.as_ptr(), &mut webview_app_id) },
        "derive the bundled WebView2 WFP application identifier",
    ) {
        unsafe { FwpmFreeMemory0((&mut sv2_app_id).cast()) };
        return Err(error);
    }
    let result = (|| {
        for key in [
            WFP_FILTER_V4,
            WFP_FILTER_V6,
            WFP_FILTER_WEBVIEW_V4,
            WFP_FILTER_WEBVIEW_V6,
        ] {
            let delete = unsafe { FwpmFilterDeleteByKey0(engine.0, &key) };
            if delete != 0 && delete != FWP_E_FILTER_NOT_FOUND {
                wfp_result(delete, "replace existing WFP filter")?;
            }
        }
        let delete = unsafe { FwpmSubLayerDeleteByKey0(engine.0, &WFP_SUBLAYER) };
        if delete != 0 && delete != FWP_E_SUBLAYER_NOT_FOUND {
            wfp_result(delete, "replace existing WFP sublayer")?;
        }
        let mut sublayer_name = wide(OsStr::new("Boxy SV2 network block"));
        let sublayer = FWPM_SUBLAYER0 {
            subLayerKey: WFP_SUBLAYER,
            displayData: FWPM_DISPLAY_DATA0 {
                name: sublayer_name.as_mut_ptr(),
                description: null_mut(),
            },
            flags: FWPM_SUBLAYER_FLAG_PERSISTENT,
            providerKey: null_mut(),
            providerData: Default::default(),
            weight: 0xffff,
        };
        wfp_result(
            unsafe { FwpmSubLayerAdd0(engine.0, &sublayer, null_mut()) },
            "add WFP sublayer",
        )?;
        for (key, layer, name, app_id) in [
            (
                WFP_FILTER_V4,
                FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                "Block SV2 network (IPv4)",
                sv2_app_id,
            ),
            (
                WFP_FILTER_V6,
                FWPM_LAYER_ALE_AUTH_CONNECT_V6,
                "Block SV2 network (IPv6)",
                sv2_app_id,
            ),
            (
                WFP_FILTER_WEBVIEW_V4,
                FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                "Block SV2 WebView2 network (IPv4)",
                webview_app_id,
            ),
            (
                WFP_FILTER_WEBVIEW_V6,
                FWPM_LAYER_ALE_AUTH_CONNECT_V6,
                "Block SV2 WebView2 network (IPv6)",
                webview_app_id,
            ),
        ] {
            let mut name = wide(OsStr::new(name));
            let mut condition = FWPM_FILTER_CONDITION0 {
                fieldKey: FWPM_CONDITION_ALE_APP_ID,
                matchType: FWP_MATCH_EQUAL,
                conditionValue: FWP_CONDITION_VALUE0 {
                    r#type: FWP_BYTE_BLOB_TYPE,
                    Anonymous: FWP_CONDITION_VALUE0_0 { byteBlob: app_id },
                },
            };
            let filter = FWPM_FILTER0 {
                filterKey: key,
                displayData: FWPM_DISPLAY_DATA0 {
                    name: name.as_mut_ptr(),
                    description: null_mut(),
                },
                flags: FWPM_FILTER_FLAG_PERSISTENT,
                providerKey: null_mut(),
                providerData: Default::default(),
                layerKey: layer,
                subLayerKey: WFP_SUBLAYER,
                weight: Default::default(),
                numFilterConditions: 1,
                filterCondition: &mut condition,
                action: FWPM_ACTION0 {
                    r#type: FWP_ACTION_BLOCK,
                    ..Default::default()
                },
                Anonymous: Default::default(),
                reserved: null_mut(),
                filterId: 0,
                effectiveWeight: Default::default(),
            };
            wfp_result(
                unsafe { FwpmFilterAdd0(engine.0, &filter, null_mut(), null_mut()) },
                "add WFP filter",
            )?;
        }
        Ok(())
    })();
    unsafe {
        FwpmFreeMemory0((&mut sv2_app_id).cast());
        FwpmFreeMemory0((&mut webview_app_id).cast());
    };
    result
}

#[cfg(windows)]
fn remove_wfp_filters(engine: &WfpEngine) -> Result<(), String> {
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
        FwpmFilterDeleteByKey0, FwpmSubLayerDeleteByKey0,
    };
    for key in [
        WFP_FILTER_V4,
        WFP_FILTER_V6,
        WFP_FILTER_WEBVIEW_V4,
        WFP_FILTER_WEBVIEW_V6,
    ] {
        let result = unsafe { FwpmFilterDeleteByKey0(engine.0, &key) };
        if result != 0 && result != FWP_E_FILTER_NOT_FOUND {
            wfp_result(result, "remove WFP filter")?;
        }
    }
    let result = unsafe { FwpmSubLayerDeleteByKey0(engine.0, &WFP_SUBLAYER) };
    if result != 0 && result != FWP_E_SUBLAYER_NOT_FOUND {
        wfp_result(result, "remove WFP sublayer")?;
    }
    Ok(())
}

#[cfg(windows)]
fn cleanup_legacy_windows_hosts() -> Result<(), String> {
    let path = hosts_path();
    let current = read_hosts(&path)?;
    let next = remove_legacy_hosts_rules(&current);
    if next != current {
        write_hosts(&path, &next)?;
    }
    Ok(())
}

#[cfg(windows)]
fn legacy_hosts_present() -> Result<bool, String> {
    let path = hosts_path();
    Ok(read_hosts(&path)?.lines().any(is_managed_line))
}

#[cfg(any(windows, target_os = "macos", test))]
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

#[cfg(any(windows, target_os = "macos", test))]
pub(crate) fn remove_legacy_hosts_rules(current: &str) -> String {
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

#[cfg(windows)]
fn read_hosts(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect hosts file {}: {error}", path.display()))?;
    if metadata.len() > MAX_HOSTS_BYTES as u64 {
        return Err("hosts file is too large to edit".to_string());
    }
    fs::read_to_string(path)
        .map_err(|error| format!("cannot read hosts file {}: {error}", path.display()))
}

#[cfg(windows)]
fn write_hosts(path: &Path, text: &str) -> Result<(), String> {
    let original_attributes = clear_read_only(path)?;
    let result = fs::write(path, text);
    if let Some(attributes) = original_attributes {
        restore_attributes(path, attributes)?;
    }
    result.map_err(|error| format!("cannot write hosts file {}: {error}", path.display()))
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
    if unsafe { SetFileAttributesW(wide_path.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY) } == 0
    {
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
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}
