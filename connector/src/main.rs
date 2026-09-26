#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod device_ids;
mod device_info;
mod host_block;
mod product_database;
mod service_selection;
mod session_io;

use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

const MAX_SESSION_BYTES: usize = 1024 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_COMMAND_SECONDS: u64 = 3600;
const REQUEST_PATH: &str = "/api/v2/connector/operations";

#[derive(Debug)]
struct Config {
    server_url: String,
    session_path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorStartRequest {
    ciphertext: String,
    machine: MachineReport,
    installed_applications: Vec<String>,
    device_info: device_info::DeviceInfo,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineReport {
    platform: String,
    machine_material: String,
    details: DeviceDetails,
    is_virtual_machine: bool,
    hardware_device_id: String,
    sv_device_hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceDetails {
    operating_system: String,
    architecture: String,
    model: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorStartResponse {
    operation_id: String,
    browser_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorOperationResponse {
    state: String,
    ciphertext: Option<String>,
    #[serde(default)]
    directory_tasks: Vec<DirectoryTask>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryTask {
    id: String,
    kind: String,
    relative_path: Option<String>,
    expected_sha256: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    blocked: Option<bool>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    items: Vec<product_database::FetchItem>,
    #[serde(default)]
    product_id: Option<String>,
}

fn main() {
    if let Some(code) = host_block::run_elevated_host_block_if_requested() {
        std::process::exit(code);
    }
    let exit_code = match run() {
        Ok(code) => code,
        Err(error) => {
            show_message("Boxy", &error, MessageLevel::Error);
            1
        }
    };
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn run() -> Result<i32, String> {
    let launch = LaunchOptions::from_environment_and_args()?;
    service_selection::run_window(&launch.initial_url, move |server_url, status| {
        let config = Config {
            server_url,
            session_path: launch.session_path,
        };
        run_connected(&config, &status)
    })
}

fn run_connected(config: &Config, status: &service_selection::RunningStatus) -> Result<(), String> {
    status.update("Preparing the encrypted local session...");
    let ciphertext = read_session_ciphertext(&config.session_path)?;
    let source_sha256 = sha256_base64url(&ciphertext);
    let machine = machine_report()?;
    status.ensure_active()?;
    let request = ConnectorStartRequest {
        ciphertext: URL_SAFE_NO_PAD.encode(&ciphertext),
        machine,
        installed_applications: installed_applications(),
        device_info: device_info::collect(),
    };
    status.update("Connecting to the selected remote...");
    status.ensure_active()?;
    let response =
        post_json::<_, ConnectorStartResponse>(&config.server_url, REQUEST_PATH, &request)?;
    status.ensure_active()?;
    if response.operation_id.is_empty() || response.browser_url.is_empty() {
        return Err("authorization service returned an incomplete pairing response".to_string());
    }
    let browser_url = validate_browser_url(&config.server_url, &response.browser_url)?;
    status.ensure_active()?;
    let mut stable_editor_url = browser_url.clone();
    stable_editor_url.set_query(None);
    status.update("Opening the remote editor in your browser...");
    webbrowser::open(browser_url.as_str())
        .map_err(|error| format!("cannot open the authorization browser page: {error}"))?;
    status.set_editor_url(stable_editor_url.as_str());
    status.update("Browser opened. Waiting for the selected remote...");
    wait_for_writeback(config, &response.operation_id, &source_sha256, status)?;
    status.update("The remote session was written back.");
    Ok(())
}

struct LaunchOptions {
    initial_url: String,
    session_path: PathBuf,
}

impl LaunchOptions {
    fn from_environment_and_args() -> Result<Self, String> {
        let mut server_url = env::var("BOXY_API_URL").ok().or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|path| service_selection::inferred_service_url(&path))
        });
        let mut session_path = None;
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--server" => {
                    server_url = Some(
                        arguments
                            .next()
                            .ok_or_else(|| "--server requires a URL".to_string())?,
                    )
                }
                "--session" => {
                    session_path = Some(PathBuf::from(
                        arguments
                            .next()
                            .ok_or_else(|| "--session requires a path".to_string())?,
                    ))
                }
                "--help" | "-h" => {
                    return Err("usage: boxy [--server URL] [--session PATH]".to_string())
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        Ok(Self {
            initial_url: server_url.unwrap_or_default(),
            session_path: session_path.unwrap_or_else(session_io::default_session_path),
        })
    }
}

fn wait_for_writeback(
    config: &Config,
    operation_id: &str,
    expected_source_sha256: &str,
    status: &service_selection::RunningStatus,
) -> Result<(), String> {
    let path = format!("/api/v2/connector/operations/{operation_id}");
    loop {
        status.ensure_active()?;
        let response = get_json::<ConnectorOperationResponse>(&config.server_url, &path)?;
        status.ensure_active()?;
        match response.state.as_str() {
            "waiting" | "editor_active" | "temporary_pending_activation" => {
                let pending_status = match response.state.as_str() {
                    "editor_active" => "Remote editor is active. Waiting for changes...",
                    "temporary_pending_activation" => "Remote activation is pending...",
                    _ => "Connected. Waiting for the remote...",
                };
                status.update(pending_status);
                for task in response.directory_tasks {
                    status.ensure_active()?;
                    status.update("Handling a remote operation...");
                    execute_directory_task(config, operation_id, &task)?;
                }
                status.update(pending_status);
                thread::sleep(Duration::from_secs(2));
            }
            "ready_for_writeback" => {
                status.update("Writing the updated session...");
                let ciphertext = response
                    .ciphertext
                    .ok_or_else(|| "writeback response has no ciphertext".to_string())?;
                let ciphertext = URL_SAFE_NO_PAD
                    .decode(ciphertext)
                    .map_err(|_| "writeback ciphertext is invalid".to_string())?;
                replace_session_if_unchanged(
                    &config.session_path,
                    expected_source_sha256,
                    &ciphertext,
                )?;
                status.update("Reporting completion to the remote...");
                let _: serde_json::Value = post_json(
                    &config.server_url,
                    &format!("{path}/receipt"),
                    &serde_json::json!({}),
                )?;
                return Ok(());
            }
            "expired" | "rejected" => {
                return Err("the authorization operation is no longer available".to_string())
            }
            _ => {
                return Err("authorization service returned an unknown operation state".to_string())
            }
        }
    }
}

fn read_session_ciphertext(path: &Path) -> Result<Vec<u8>, String> {
    let ciphertext =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if ciphertext.is_empty()
        || ciphertext.len() > MAX_SESSION_BYTES
        || !ciphertext.len().is_multiple_of(8)
    {
        return Err("session ciphertext has an invalid size".to_string());
    }
    Ok(ciphertext)
}

fn sv2_root(session_path: &Path) -> Result<PathBuf, String> {
    session_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot resolve the SV2 data directory".to_string())
}

fn installed_applications() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let mut applications = Vec::new();
        let locations = [
            Some(PathBuf::from("/Applications")),
            dirs::home_dir().map(|path| path.join("Applications")),
        ];
        for location in locations.into_iter().flatten() {
            if let Ok(entries) = fs::read_dir(location) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(".app") && is_sv2_application(name) {
                            applications.push(name.to_string());
                        }
                    }
                }
            }
        }
        applications.sort();
        applications.dedup();
        return applications;
    }
    #[cfg(windows)]
    {
        use winreg::{
            enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
            RegKey,
        };
        let mut applications = Vec::new();
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let root = RegKey::predef(hive);
            for path in [
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            ] {
                if let Ok(uninstall) = root.open_subkey(path) {
                    for key in uninstall.enum_keys().flatten() {
                        if let Ok(entry) = uninstall.open_subkey(key) {
                            if let Ok(name) = entry.get_value::<String, _>("DisplayName") {
                                if is_sv2_application(&name) {
                                    applications.push(name);
                                }
                            }
                        }
                    }
                }
            }
        }
        applications.sort();
        applications.dedup();
        return applications;
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    Vec::new()
}

fn is_sv2_application(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("synthesizer v studio 2")
        || name.contains("synthv-studio2")
        || name.contains("sv2")
}

fn execute_directory_task(
    config: &Config,
    operation_id: &str,
    task: &DirectoryTask,
) -> Result<(), String> {
    let root = sv2_root(&config.session_path)?;
    let base = format!(
        "/api/v2/connector/operations/{operation_id}/directory-tasks/{}",
        task.id
    );
    match task.kind.as_str() {
        "read" => {
            let path = task_path(
                &root,
                task.relative_path
                    .as_deref()
                    .ok_or_else(|| "read task has no path".to_string())?,
            )?;
            let bytes = fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            put_task_content(config, &format!("{base}/content"), &bytes)?;
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::json!({"sha256": sha256_base64url(&bytes), "size": bytes.len()}),
            )
        }
        "write" => {
            let bytes = get_task_content(config, &format!("{base}/content"))?;
            if task
                .expected_sha256
                .as_deref()
                .is_some_and(|hash| hash != sha256_base64url(&bytes))
            {
                return Err("directory task content hash mismatch".to_string());
            }
            atomic_write(
                &task_path(
                    &root,
                    task.relative_path
                        .as_deref()
                        .ok_or_else(|| "write task has no path".to_string())?,
                )?,
                &bytes,
            )?;
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::json!({"sha256": sha256_base64url(&bytes), "size": bytes.len()}),
            )
        }
        "download" => {
            let bytes = get_task_content(config, &format!("{base}/content"))?;
            let name = task
                .relative_path
                .as_deref()
                .and_then(|path| Path::new(path).file_name())
                .ok_or_else(|| "download task has no file name".to_string())?;
            let target = dirs::download_dir()
                .ok_or_else(|| "cannot resolve Downloads folder".to_string())?
                .join(name);
            atomic_write(&target, &bytes)?;
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::json!({"sha256": sha256_base64url(&bytes), "size": bytes.len()}),
            )
        }
        "machine_material" => complete_task(
            config,
            &format!("{base}/complete"),
            serde_json::to_value(machine_report()?).map_err(|error| error.to_string())?,
        ),
        "command" => execute_command_task(config, &base, task),
        "host_block" => {
            let result = match task.blocked {
                Some(blocked) => host_block::set_blocked(blocked),
                None => host_block::status(),
            };
            let result = match result {
                Ok(status) => serde_json::to_value(status).map_err(|error| error.to_string())?,
                Err(error) => serde_json::json!({"error": error}),
            };
            complete_task(config, &format!("{base}/complete"), result)
        }
        "sv2_action" => {
            let action = task
                .action
                .as_deref()
                .ok_or_else(|| "sv2 task has no action".to_string())?;
            let result = match action {
                "launch" => session_io::launch_sv2()?,
                "close" => session_io::close_sv2()?,
                _ => return Err("sv2 task action is invalid".to_string()),
            };
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::to_value(result).map_err(|error| error.to_string())?,
            )
        }
        "open_folder" => {
            session_io::open_session_folder(&config.session_path)?;
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::json!({ "ok": true }),
            )
        }
        "product_inspect" => complete_task(
            config,
            &format!("{base}/complete"),
            serde_json::json!({ "databases": product_database::inspect() }),
        ),
        "product_fetch" => {
            let mut results = Vec::new();
            for item in &task.items {
                results.push(product_database::fetch(item)?);
            }
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::json!({ "items": results }),
            )
        }
        "product_delete" => {
            let id = task
                .product_id
                .as_deref()
                .ok_or_else(|| "product delete task has no id".to_string())?;
            product_database::delete(id)?;
            complete_task(
                config,
                &format!("{base}/complete"),
                serde_json::json!({ "id": id, "installed": false }),
            )
        }
        _ => Err("unsupported directory task".to_string()),
    }
}

fn execute_command_task(config: &Config, base: &str, task: &DirectoryTask) -> Result<(), String> {
    let command = task
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "command task has no command".to_string())?;
    let shell = task.shell.as_deref().unwrap_or(default_shell());
    let root = sv2_root(&config.session_path)?;
    let cwd = match task.cwd.as_deref() {
        Some(path) => task_path(&root, path)?,
        None => root,
    };
    if !approve_command(command, shell, &cwd.to_string_lossy()) {
        return complete_task(
            config,
            &format!("{base}/complete"),
            serde_json::json!({ "denied": true, "command": command }),
        );
    }
    let timeout = Duration::from_secs(
        task.timeout_seconds
            .unwrap_or(300)
            .clamp(1, MAX_COMMAND_SECONDS),
    );
    let outcome = run_shell_command(shell, command, &cwd, timeout);
    let body = match outcome {
        Ok(result) => serde_json::json!({
            "command": command,
            "shell": shell,
            "exitCode": result.exit_code,
            "timedOut": result.timed_out,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }),
        Err(error) => {
            serde_json::json!({ "command": command, "shell": shell, "failed": true, "error": error })
        }
    };
    complete_task(config, &format!("{base}/complete"), body)
}

struct CommandResult {
    exit_code: Option<i32>,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

fn run_shell_command(
    shell: &str,
    command: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut process = shell_process(shell, command);
    process
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process
        .spawn()
        .map_err(|error| format!("cannot start the command: {error}"))?;
    let deadline = SystemTime::now() + timeout;
    let mut timed_out = false;
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(_) => break,
            None if SystemTime::now() >= deadline => {
                let _ = child.kill();
                timed_out = true;
                break;
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot read the command output: {error}"))?;
    Ok(CommandResult {
        exit_code: output.status.code(),
        timed_out,
        stdout: truncate_output(&output.stdout),
        stderr: truncate_output(&output.stderr),
    })
}

fn shell_process(shell: &str, command: &str) -> Command {
    #[cfg(windows)]
    {
        match shell {
            "powershell" => {
                let mut process = Command::new("powershell");
                process.args(["-NoProfile", "-NonInteractive", "-Command", command]);
                process
            }
            _ => {
                let mut process = Command::new("cmd");
                process.args(["/C", command]);
                process
            }
        }
    }
    #[cfg(not(windows))]
    {
        let mut process = Command::new(shell);
        process.args(["-c", command]);
        process
    }
}

fn default_shell() -> &'static str {
    #[cfg(windows)]
    {
        "cmd"
    }
    #[cfg(not(windows))]
    {
        "sh"
    }
}

fn truncate_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_COMMAND_OUTPUT_BYTES {
        return text.to_string();
    }
    let mut boundary = MAX_COMMAND_OUTPUT_BYTES;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}\n[output truncated]", &text[..boundary])
}

fn approve_command(command: &str, shell: &str, cwd: &str) -> bool {
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Remote command approval")
            .set_description(format!(
                "A remote command is waiting for your approval.\n\nShell: {shell}\nDirectory: {cwd}\n\nCommand:\n{command}\n\nApprove to run it on this computer."
            ))
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes | MessageDialogResult::Ok
    )
}

fn task_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("directory task path is unsafe".to_string());
    }
    Ok(root.join(relative))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension(format!("connector-{}.tmp", std::process::id()));
    write_new_file(&temporary, bytes)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("cannot replace {}: {error}", path.display()))
}

fn replace_session_if_unchanged(
    path: &Path,
    expected_sha256: &str,
    ciphertext: &[u8],
) -> Result<(), String> {
    if ciphertext.is_empty()
        || ciphertext.len() > MAX_SESSION_BYTES
        || !ciphertext.len().is_multiple_of(8)
    {
        return Err("writeback ciphertext has an invalid size".to_string());
    }
    let current = read_session_ciphertext(path)?;
    if sha256_base64url(&current) != expected_sha256 {
        return Err(
            "session changed while authorization was open; no write was performed".to_string(),
        );
    }
    let parent = path
        .parent()
        .ok_or_else(|| "session path has no parent folder".to_string())?;
    let backup = parent.join(format!(
        "session.connector-backup-{}-{}.bin",
        unix_timestamp(),
        std::process::id()
    ));
    write_new_file(&backup, &current)?;
    let temporary = path.with_extension(format!("connector-{}.tmp", std::process::id()));
    write_new_file(&temporary, ciphertext)?;
    #[cfg(windows)]
    fs::remove_file(path).map_err(|error| format!("cannot replace existing session: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("cannot replace session: {error}")
    })?;
    let written = read_session_ciphertext(path)?;
    if written != ciphertext {
        return Err("written session verification failed".to_string());
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("cannot write {}: {error}", path.display()));
    }
    Ok(())
}

fn machine_report() -> Result<MachineReport, String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/sbin/ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .map_err(|error| format!("cannot read the native device identity: {error}"))?;
        if !output.status.success() || output.stdout.len() > 64 * 1024 {
            return Err("native device identity is unavailable".to_string());
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| "native device identity is not UTF-8".to_string())?;
        let uuid = ioreg_property(&text, "IOPlatformUUID")
            .ok_or_else(|| "native device UUID is unavailable".to_string())?;
        let sv_device_hash = ioreg_property(&text, "IOPlatformSerialNumber")
            .filter(|serial| !serial.is_empty())
            .unwrap_or(uuid);
        let model = command_output("/usr/sbin/sysctl", &["-n", "hw.model"]);
        let virtual_machine = ["virtual", "vmware", "parallels", "qemu", "virtualbox"]
            .iter()
            .any(|needle| model.to_ascii_lowercase().contains(needle));
        return Ok(MachineReport {
            platform: "macos".to_string(),
            machine_material: uuid.to_string(),
            details: DeviceDetails {
                operating_system: command_output("/usr/bin/sw_vers", &["-productVersion"]),
                architecture: env::consts::ARCH.to_string(),
                model,
            },
            is_virtual_machine: virtual_machine,
            hardware_device_id: uuid.to_ascii_lowercase(),
            sv_device_hash: sv_device_hash.to_string(),
        });
    }
    #[cfg(windows)]
    {
        let raw = read_raw_smbios()?;
        let printable = String::from_utf8_lossy(&raw).to_ascii_lowercase();
        let is_virtual_machine = [
            "virtualbox",
            "vmware",
            "qemu",
            "kvm",
            "hyper-v",
            "parallels",
        ]
        .iter()
        .any(|needle| printable.contains(needle));
        return Ok(MachineReport {
            platform: "windows".to_string(),
            hardware_device_id: device_ids::hardware_uuid(&raw)?,
            sv_device_hash: device_ids::sv_device_hash()?,
            machine_material: URL_SAFE_NO_PAD.encode(raw),
            details: DeviceDetails {
                operating_system: "Windows".to_string(),
                architecture: env::consts::ARCH.to_string(),
                model: "SMBIOS device".to_string(),
            },
            is_virtual_machine,
        });
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    Err("the connector currently supports macOS and Windows".to_string())
}

#[cfg(target_os = "macos")]
fn ioreg_property<'a>(text: &'a str, property: &str) -> Option<&'a str> {
    let prefix = format!("\"{property}\" = \"");
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix('"'))
    })
}

#[cfg(target_os = "macos")]
fn command_output(command: &str, arguments: &[&str]) -> String {
    Command::new(command)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(windows)]
fn read_raw_smbios() -> Result<Vec<u8>, String> {
    use windows_sys::Win32::System::SystemInformation::GetSystemFirmwareTable;
    const PROVIDER_RSMB: u32 = u32::from_be_bytes(*b"RSMB");
    const TABLE_RSDT: u32 = u32::from_be_bytes(*b"RSDT");
    let required =
        unsafe { GetSystemFirmwareTable(PROVIDER_RSMB, TABLE_RSDT, std::ptr::null_mut(), 0) }
            as usize;
    if !(8..=MAX_SESSION_BYTES).contains(&required) {
        return Err("native SMBIOS data has an invalid size".to_string());
    }
    let mut raw = vec![0u8; required];
    let received = unsafe {
        GetSystemFirmwareTable(
            PROVIDER_RSMB,
            TABLE_RSDT,
            raw.as_mut_ptr(),
            u32::try_from(raw.len()).map_err(|_| "native SMBIOS data is too large".to_string())?,
        )
    } as usize;
    if !(8..=required).contains(&received) {
        return Err("native SMBIOS data is unavailable".to_string());
    }
    raw.truncate(received);
    Ok(raw)
}

fn post_json<T: Serialize, R: for<'de> Deserialize<'de>>(
    server: &str,
    path: &str,
    body: &T,
) -> Result<R, String> {
    let response = ureq::post(&format!("{server}{path}"))
        .timeout(Duration::from_secs(30))
        .set("Content-Type", "application/json")
        .send_json(serde_json::to_value(body).map_err(|error| error.to_string())?);
    parse_response(response)
}

fn get_json<R: for<'de> Deserialize<'de>>(server: &str, path: &str) -> Result<R, String> {
    parse_response(
        ureq::get(&format!("{server}{path}"))
            .timeout(Duration::from_secs(30))
            .call(),
    )
}

fn put_task_content(config: &Config, path: &str, bytes: &[u8]) -> Result<(), String> {
    match ureq::put(&format!("{}{}", config.server_url, path))
        .timeout(Duration::from_secs(30))
        .set("Content-Length", &bytes.len().to_string())
        .send_bytes(bytes)
    {
        Ok(_) => Ok(()),
        Err(error) => Err(format!("cannot upload directory content: {error}")),
    }
}

fn get_task_content(config: &Config, path: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(&format!("{}{}", config.server_url, path))
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|error| format!("cannot download directory content: {error}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(2 * 1024 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn complete_task(config: &Config, path: &str, body: serde_json::Value) -> Result<(), String> {
    let _: serde_json::Value = post_json(&config.server_url, path, &body)?;
    Ok(())
}

fn parse_response<R: for<'de> Deserialize<'de>>(
    response: Result<ureq::Response, ureq::Error>,
) -> Result<R, String> {
    match response {
        Ok(response) => response
            .into_json()
            .map_err(|error| format!("authorization service returned invalid JSON: {error}")),
        Err(ureq::Error::Status(status, response)) => {
            let detail = response.into_string().unwrap_or_default();
            Err(format!(
                "authorization service rejected the request ({status}): {detail}"
            ))
        }
        Err(error) => Err(format!("authorization service request failed: {error}")),
    }
}

fn sha256_base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn validate_browser_url(server_url: &str, browser_url: &str) -> Result<Url, String> {
    let service =
        Url::parse(server_url).map_err(|_| "authorization service URL is invalid".to_string())?;
    let browser =
        Url::parse(browser_url).map_err(|_| "authorization browser URL is invalid".to_string())?;
    if service.origin() != browser.origin()
        || !browser.username().is_empty()
        || browser.password().is_some()
        || browser.fragment().is_some()
    {
        return Err(
            "authorization browser URL is not issued by the configured service".to_string(),
        );
    }
    let route_id = browser
        .path()
        .strip_prefix("/d_")
        .and_then(|path| path.strip_suffix('/'));
    if !route_id.is_some_and(|id| {
        id.len() == 32
            && id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }) {
        return Err("authorization browser URL has an invalid device route".to_string());
    }
    let mut parameters = browser.query_pairs();
    let Some((name, authorization)) = parameters.next() else {
        return Err("authorization browser URL has no access parameter".to_string());
    };
    if name != "authorization"
        || parameters.next().is_some()
        || !is_capability_id(authorization.as_ref())
    {
        return Err("authorization browser URL has an invalid access parameter".to_string());
    }
    Ok(browser)
}

fn is_capability_id(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn show_message(title: &str, description: &str, level: MessageLevel) {
    let _ = MessageDialog::new()
        .set_level(level)
        .set_title(title)
        .set_description(description)
        .set_buttons(MessageButtons::Ok)
        .show();
}

#[cfg(test)]
mod tests {
    use super::{default_shell, run_shell_command, truncate_output, validate_browser_url};
    use std::time::Duration;

    fn browser_url() -> String {
        format!(
            "https://service.example.test/d_{}/?authorization={}",
            "a".repeat(32),
            "A".repeat(43)
        )
    }

    #[test]
    fn accepts_the_configured_device_route() {
        assert!(validate_browser_url("https://service.example.test", &browser_url()).is_ok());
    }

    #[test]
    fn rejects_another_origin_or_invalid_access_parameter() {
        assert!(validate_browser_url(
            "https://service.example.test",
            &browser_url().replace("service.example.test", "other.example.test")
        )
        .is_err());
        assert!(validate_browser_url(
            "https://service.example.test",
            "https://service.example.test/d_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/?authorization=short"
        )
        .is_err());
        assert!(validate_browser_url(
            "https://service.example.test",
            &format!("{}&other=value", browser_url())
        )
        .is_err());
    }

    #[test]
    fn runs_a_shell_command_and_captures_output() {
        let cwd = std::env::temp_dir();
        let command = if cfg!(windows) {
            "echo hello"
        } else {
            "printf hello"
        };
        let result =
            run_shell_command(default_shell(), command, &cwd, Duration::from_secs(30)).unwrap();
        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn reports_a_nonzero_exit_code() {
        let cwd = std::env::temp_dir();
        let command = if cfg!(windows) { "exit /b 3" } else { "exit 3" };
        let result =
            run_shell_command(default_shell(), command, &cwd, Duration::from_secs(30)).unwrap();
        assert_eq!(result.exit_code, Some(3));
    }

    #[test]
    fn stops_a_command_that_exceeds_its_deadline() {
        let cwd = std::env::temp_dir();
        let command = if cfg!(windows) {
            "ping -n 6 127.0.0.1 > nul"
        } else {
            "sleep 5"
        };
        let result =
            run_shell_command(default_shell(), command, &cwd, Duration::from_secs(1)).unwrap();
        assert!(result.timed_out);
    }

    #[test]
    fn truncates_oversized_output_on_a_character_boundary() {
        let text = "a".repeat(1024);
        assert_eq!(truncate_output(text.as_bytes()), text);
        let oversized = "é".repeat(super::MAX_COMMAND_OUTPUT_BYTES);
        let truncated = truncate_output(oversized.as_bytes());
        let body = truncated.strip_suffix("\n[output truncated]").unwrap();
        assert!(body.len() <= super::MAX_COMMAND_OUTPUT_BYTES);
        assert!(oversized.starts_with(body));
    }
}
