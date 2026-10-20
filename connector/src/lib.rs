mod assets;
mod device_ids;
mod device_info;
mod file_tasks;
mod host_block;
mod http_request;
mod launch;
mod network_block;
mod product_database;
pub(crate) mod request_trace;
mod session_io;
#[cfg(windows)]
mod windows_firewall;
#[cfg(windows)]
mod windows_process;
#[cfg(windows)]
mod windows_tray;

pub use launch::LaunchOptions;

use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use rand_core::{OsRng, RngCore};
use request_trace::UpstreamAttempt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const MAX_SESSION_BYTES: usize = 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 100_000;
const MAX_START_REQUEST_BYTES: usize = (4 * 1024 * 1024 + 2) / 3 + 16 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_COMMAND_SECONDS: u64 = 3600;
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONNECTOR_PROTOCOL_VERSION: u8 = 3;
const REQUEST_PATH: &str = "/api/v2/connector/operations";

#[derive(Debug)]
pub struct Config {
    pub server_url: String,
    pub session_path: PathBuf,
}

pub trait ConnectorHost: Send + Sync {
    fn open_editor(&self, url: Url) -> Result<(), String>;
    fn approve_command(&self, command: &str, shell: &str, cwd: &Path) -> bool;
    fn should_stop(&self) -> bool;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorStartRequest {
    protocol_version: u8,
    ciphertext: String,
    source_sha256: String,
    machine: MachineReport,
    device_public_key: String,
    connector_version: String,
    directory_manifest: Vec<DirectoryEntry>,
    installed_applications: Vec<String>,
    device_name: String,
    device_info: device_info::DeviceInfo,
    server_translations: Vec<assets::ServerTranslationCatalog>,
    session_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryEntry {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorReceipt {
    source_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    written_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_sha256: Option<String>,
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
    protocol_version: Option<u8>,
    operation_id: String,
    browser_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorOperationResponse {
    protocol_version: Option<u8>,
    state: String,
    ciphertext: Option<String>,
    source_sha256: Option<String>,
    expires_at: Option<String>,
    #[serde(default)]
    directory_tasks: Vec<DirectoryTask>,
}

#[derive(Deserialize)]
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
    mode: Option<String>,
    #[serde(default)]
    items: Vec<product_database::FetchItem>,
    #[serde(default)]
    product_id: Option<String>,
    #[serde(default)]
    expected_session_sha256: Option<String>,
    #[serde(default)]
    request: Option<http_request::HttpRequestSpec>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    max_file_bytes: Option<usize>,
    #[serde(default)]
    max_total_bytes: Option<usize>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    files: Vec<file_tasks::WriteFileTaskEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSnapshotAck {
    state: String,
    source_sha256: Option<String>,
}

impl Drop for DirectoryTask {
    fn drop(&mut self) {
        if let Some(request) = self.request.take() {
            drop(request);
        }
        for file in &mut self.files {
            zeroize::Zeroize::zeroize(&mut file.content_base64);
        }
        for item in &mut self.items {
            item.url.zeroize();
            if let Some(files) = &mut item.files {
                for file in files {
                    file.url.zeroize();
                }
            }
        }
    }
}

pub fn run_elevated_if_requested() -> Option<i32> {
    if let Some(code) = host_block::run_elevated_host_block_if_requested() {
        return Some(code);
    }
    #[cfg(windows)]
    if let Some(code) = windows_firewall::run_elevated_if_requested() {
        return Some(code);
    }
    #[cfg(windows)]
    if let Some(code) = network_block::run_elevated_if_requested() {
        return Some(code);
    }
    None
}

pub fn prepare_config() -> Result<LaunchOptions, String> {
    LaunchOptions::from_environment_and_args(session_io::default_session_path())
}

pub fn resolve_config(launch: &LaunchOptions, server_url: &str) -> Result<Config, String> {
    let server_url = launch::validate_selected_service_url(server_url)?;
    let session_path =
        launch::resolve_session_path(launch.session_path.clone(), launch.session_path_explicit)?;
    Ok(Config {
        server_url,
        session_path,
    })
}

pub fn run_connected(config: &Config, host: &dyn ConnectorHost) -> Result<(), String> {
    if host.should_stop() {
        return Ok(());
    }
    let ciphertext = read_session_ciphertext(&config.session_path)?;
    let source_sha256 = sha256_base64url(&ciphertext);
    let machine = machine_report()?;
    let device_key = new_device_signing_key();
    let mut request = ConnectorStartRequest {
        protocol_version: CONNECTOR_PROTOCOL_VERSION,
        ciphertext: URL_SAFE_NO_PAD.encode(&ciphertext),
        source_sha256: source_sha256.clone(),
        machine,
        device_public_key: URL_SAFE_NO_PAD.encode(device_key.verifying_key().as_bytes()),
        connector_version: env!("CARGO_PKG_VERSION").to_string(),
        directory_manifest: directory_manifest(&sv2_root(&config.session_path)?)?,
        installed_applications: installed_applications(),
        device_name: sanitized_device_name(),
        device_info: device_info::collect(),
        server_translations: assets::server_translations(&config.session_path),
        session_path: config.session_path.to_string_lossy().into_owned(),
    };
    let mut request_bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    if request_bytes.len() > MAX_START_REQUEST_BYTES && !request.server_translations.is_empty() {
        request.server_translations.clear();
        request_bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    }
    if request_bytes.len() > MAX_START_REQUEST_BYTES {
        return Err(
            "SV2 data directory metadata exceeds the authorization service request limit; no partial directory manifest was sent"
                .to_string(),
        );
    }
    let response =
        post_json::<_, ConnectorStartResponse>(&config.server_url, REQUEST_PATH, &request)?;
    require_protocol_v3(response.protocol_version)?;
    if response.operation_id.is_empty() || response.browser_url.is_empty() {
        return Err("authorization service returned an incomplete pairing response".to_string());
    }
    let browser_url = validate_browser_url(&config.server_url, &response.browser_url)?;
    host.open_editor(browser_url)?;
    wait_for_writeback(
        config,
        &device_key,
        &response.operation_id,
        &source_sha256,
        host,
    )?;
    Ok(())
}

fn new_device_signing_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

fn require_protocol_v3(version: Option<u8>) -> Result<(), String> {
    if version == Some(CONNECTOR_PROTOCOL_VERSION) {
        Ok(())
    } else {
        Err("authorization service protocol is not supported; update the connector and service together".to_string())
    }
}

fn sanitized_device_name() -> String {
    sanitize_device_name(
        &sysinfo::System::host_name()
            .or_else(|| env::var("COMPUTERNAME").ok())
            .unwrap_or_default(),
    )
}

fn sanitize_device_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

fn wait_for_writeback(
    config: &Config,
    device_key: &SigningKey,
    operation_id: &str,
    expected_source_sha256: &str,
    host: &dyn ConnectorHost,
) -> Result<(), String> {
    let path = format!("/api/v2/connector/operations/{operation_id}");
    let mut last_reported_sha256 = expected_source_sha256.to_string();
    loop {
        if host.should_stop() {
            return Ok(());
        }
        match report_session_change(config, device_key, operation_id, &last_reported_sha256) {
            Ok(SessionWatchResult::Unchanged) | Ok(SessionWatchResult::WritebackPending) => {}
            Ok(SessionWatchResult::Accepted(source_sha256)) => {
                last_reported_sha256 = source_sha256;
            }
            Err(_) => {
                if wait_or_stop(host, Duration::from_secs(2)) {
                    return Ok(());
                }
                continue;
            }
        }
        let (nonce, signature) = request_signature(device_key, "GET", &path);
        let response = get_signed::<ConnectorOperationResponse>(
            &config.server_url,
            &path,
            &nonce,
            &signature,
        )?;
        require_protocol_v3(response.protocol_version)?;
        match response.state.as_str() {
            "waiting" | "editor_active" | "temporary_pending_activation" => {
                if host.should_stop() {
                    return Ok(());
                }
                if let Some(task) = response.directory_tasks.into_iter().next() {
                    if host.should_stop() {
                        return Ok(());
                    }
                    execute_directory_task(
                        config,
                        device_key,
                        operation_id,
                        &task,
                        &mut last_reported_sha256,
                        host,
                    )?;
                    continue;
                }
                if wait_or_stop(host, Duration::from_secs(2)) {
                    return Ok(());
                }
            }
            "ready_for_writeback" => {
                let ciphertext = response
                    .ciphertext
                    .ok_or_else(|| "writeback response has no ciphertext".to_string())?;
                let source_sha256 = response
                    .source_sha256
                    .ok_or_else(|| "writeback response has no source hash".to_string())?;
                let expires_at = response
                    .expires_at
                    .ok_or_else(|| "writeback response has no expiry".to_string())?
                    .parse::<u128>()
                    .map_err(|_| "writeback expiry is invalid".to_string())?;
                if expires_at <= unix_timestamp_millis() {
                    return Err("writeback response has expired".to_string());
                }
                let ciphertext = URL_SAFE_NO_PAD
                    .decode(ciphertext)
                    .map_err(|_| "writeback ciphertext is invalid".to_string())?;
                let current = read_session_ciphertext(&config.session_path)?;
                if sha256_base64url(&current) != source_sha256 {
                    let observed_sha256 = sha256_base64url(&current);
                    let conflict = ConnectorReceipt {
                        source_sha256,
                        written_sha256: None,
                        error: Some("source_changed"),
                        observed_sha256: Some(observed_sha256.clone()),
                    };
                    let (receipt_status, body) = post_signed_json_status(
                        &config.server_url,
                        &format!("{path}/receipt"),
                        device_key,
                        &conflict,
                    )?;
                    let ack: SessionSnapshotAck = serde_json::from_value(body)
                        .map_err(|_| "writeback conflict acknowledgement is invalid".to_string())?;
                    if receipt_status != 200
                        || ack.state != "writeback_conflict"
                        || ack.source_sha256.as_deref() != Some(observed_sha256.as_str())
                    {
                        return Err("writeback conflict was not accepted".to_string());
                    }
                    match submit_session_snapshot(config, device_key, operation_id, &current)? {
                        SessionWatchResult::Accepted(source_sha256) => {
                            last_reported_sha256 = source_sha256;
                        }
                        _ => {}
                    }
                    continue;
                }
                let written_sha256 = match replace_session_if_unchanged(
                    &config.session_path,
                    &source_sha256,
                    &ciphertext,
                ) {
                    Ok(hash) => hash,
                    Err(_) => {
                        let current = read_session_ciphertext(&config.session_path)?;
                        let observed_sha256 = sha256_base64url(&current);
                        if observed_sha256 == source_sha256 {
                            return Err("the updated session could not be written".to_string());
                        }
                        let conflict = ConnectorReceipt {
                            source_sha256,
                            written_sha256: None,
                            error: Some("source_changed"),
                            observed_sha256: Some(observed_sha256.clone()),
                        };
                        let (receipt_status, body) = post_signed_json_status(
                            &config.server_url,
                            &format!("{path}/receipt"),
                            device_key,
                            &conflict,
                        )?;
                        let ack: SessionSnapshotAck =
                            serde_json::from_value(body).map_err(|_| {
                                "writeback conflict acknowledgement is invalid".to_string()
                            })?;
                        if receipt_status != 200
                            || ack.state != "writeback_conflict"
                            || ack.source_sha256.as_deref() != Some(observed_sha256.as_str())
                        {
                            return Err("writeback conflict was not accepted".to_string());
                        }
                        match submit_session_snapshot(config, device_key, operation_id, &current)? {
                            SessionWatchResult::Accepted(source_sha256) => {
                                last_reported_sha256 = source_sha256;
                            }
                            _ => {}
                        }
                        continue;
                    }
                };
                let receipt = ConnectorReceipt {
                    source_sha256,
                    written_sha256: Some(written_sha256.clone()),
                    error: None,
                    observed_sha256: None,
                };
                let (receipt_status, body) = post_signed_json_status(
                    &config.server_url,
                    &format!("{path}/receipt"),
                    device_key,
                    &receipt,
                )?;
                let ack: SessionSnapshotAck = serde_json::from_value(body)
                    .map_err(|_| "writeback acknowledgement is invalid".to_string())?;
                if receipt_status != 200
                    || ack.state != "writeback_completed"
                    || ack.source_sha256.as_deref() != Some(written_sha256.as_str())
                {
                    return Err("writeback completion was not accepted".to_string());
                }
                last_reported_sha256 = written_sha256;
                continue;
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

fn wait_or_stop(host: &dyn ConnectorHost, duration: Duration) -> bool {
    let deadline = std::time::Instant::now() + duration;
    while !host.should_stop() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        thread::sleep(remaining.min(COMMAND_POLL_INTERVAL));
    }
    true
}

enum SessionWatchResult {
    Unchanged,
    Accepted(String),
    WritebackPending,
}

fn report_session_change(
    config: &Config,
    device_key: &SigningKey,
    operation_id: &str,
    last_reported_sha256: &str,
) -> Result<SessionWatchResult, String> {
    let ciphertext = read_session_ciphertext(&config.session_path)?;
    let source_sha256 = sha256_base64url(&ciphertext);
    if source_sha256 == last_reported_sha256 {
        return Ok(SessionWatchResult::Unchanged);
    }
    submit_session_snapshot(config, device_key, operation_id, &ciphertext)
}

fn submit_session_snapshot(
    config: &Config,
    device_key: &SigningKey,
    operation_id: &str,
    ciphertext: &[u8],
) -> Result<SessionWatchResult, String> {
    let source_sha256 = sha256_base64url(ciphertext);
    let body = serde_json::json!({
        "ciphertext": URL_SAFE_NO_PAD.encode(ciphertext),
        "sourceSha256": source_sha256,
    });
    let path = format!("/api/v2/connector/operations/{operation_id}/session-snapshot");
    let (status, value) = post_signed_json_status(&config.server_url, &path, device_key, &body)?;
    if status == 409 {
        return Ok(SessionWatchResult::WritebackPending);
    }
    let ack: SessionSnapshotAck = serde_json::from_value(value)
        .map_err(|_| "session snapshot acknowledgement is invalid".to_string())?;
    if status == 200
        && ack.state == "accepted"
        && ack.source_sha256.as_deref() == Some(source_sha256.as_str())
    {
        return Ok(SessionWatchResult::Accepted(source_sha256));
    }
    Err("session snapshot was not accepted".to_string())
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

fn directory_manifest(root: &Path) -> Result<Vec<DirectoryEntry>, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("cannot access SV2 data directory: {error}"))?;
    let mut entries = Vec::new();
    collect_directory_entries(&root, &root, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn collect_directory_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<DirectoryEntry>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_directory_entries(root, &path, entries)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if entries.len() >= MAX_DIRECTORY_ENTRIES {
            return Err("SV2 data directory contains too many files to report".to_string());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "SV2 path escaped its root".to_string())?;
        let size = entry.metadata().map_err(|error| error.to_string())?.len();
        entries.push(DirectoryEntry {
            path: relative.to_string_lossy().replace('\\', "/"),
            size,
            sha256: sha256_file(&path)?,
        });
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(URL_SAFE_NO_PAD.encode(hash.finalize()))
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
            enums::{
                HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
            },
            RegKey,
        };
        let mut applications = Vec::new();
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let root = RegKey::predef(hive);
            for view in [KEY_READ | KEY_WOW64_64KEY, KEY_READ | KEY_WOW64_32KEY] {
                if let Ok(uninstall) = root.open_subkey_with_flags(
                    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                    view,
                ) {
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
    key: &SigningKey,
    operation_id: &str,
    task: &DirectoryTask,
    last_reported_sha256: &mut String,
    host: &dyn ConnectorHost,
) -> Result<(), String> {
    if host.should_stop() {
        return Ok(());
    }
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
            put_task_content(config, key, &format!("{base}/content"), &bytes)?;
            complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::json!({"sha256": sha256_base64url(&bytes), "size": bytes.len()}),
            )
        }
        "write" => {
            let bytes = get_task_content(config, key, &format!("{base}/content"))?;
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
                key,
                &format!("{base}/complete"),
                serde_json::json!({"sha256": sha256_base64url(&bytes), "size": bytes.len()}),
            )
        }
        "download" => {
            let bytes = get_task_content(config, key, &format!("{base}/content"))?;
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
                key,
                &format!("{base}/complete"),
                serde_json::json!({"sha256": sha256_base64url(&bytes), "size": bytes.len()}),
            )
        }
        "machine_material" => complete_task(
            config,
            key,
            &format!("{base}/complete"),
            serde_json::to_value(machine_report()?).map_err(|error| error.to_string())?,
        ),
        "command" => execute_command_task(config, key, &base, task, host),
        "host_block" => {
            let result = match task.mode.as_deref() {
                Some("status") => Ok(network_block::status()),
                Some("firewall") => task
                    .blocked
                    .ok_or_else(|| "network block task has no blocked state".to_string())
                    .and_then(network_block::set_blocked),
                Some("cleanup") if task.blocked == Some(false) => network_block::cleanup_hosts(),
                _ => Err("network block mode is invalid".to_string()),
            };
            let result = match result {
                Ok(status) => serde_json::to_value(status).map_err(|error| error.to_string())?,
                Err(error) => serde_json::json!({
                    "failed": true,
                    "error": error,
                    "status": network_block::status(),
                }),
            };
            complete_task(config, key, &format!("{base}/complete"), result)
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
                key,
                &format!("{base}/complete"),
                serde_json::to_value(result).map_err(|error| error.to_string())?,
            )
        }
        "open_folder" => {
            session_io::open_session_folder(&config.session_path)?;
            complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::json!({ "ok": true }),
            )
        }
        "product_inspect" => complete_task(
            config,
            key,
            &format!("{base}/complete"),
            serde_json::json!({ "databases": product_database::inspect() }),
        ),
        "product_fetch" => {
            let mut results = Vec::new();
            let mut attempts = Vec::new();
            for item in &task.items {
                match product_database::fetch(item, &mut attempts) {
                    Ok(result) => results
                        .push(serde_json::to_value(result).map_err(|error| error.to_string())?),
                    Err(_) => results
                        .push(serde_json::json!({ "id": item.id, "error": "download_failed" })),
                }
            }
            for (index, attempt) in attempts.iter_mut().enumerate() {
                attempt.sequence = index + 1;
            }
            complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::json!({
                    "action": task.action,
                    "productId": task.product_id,
                    "items": results,
                    "upstreamAttempts": attempts,
                }),
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
                key,
                &format!("{base}/complete"),
                serde_json::json!({ "id": id, "installed": false }),
            )
        }
        "http_request" => {
            if let Some(expected) = task.expected_session_sha256.as_deref() {
                let current = read_session_ciphertext(&config.session_path)?;
                let current_sha256 = sha256_base64url(&current);
                if expected != current_sha256 {
                    match submit_session_snapshot(config, key, operation_id, &current) {
                        Ok(SessionWatchResult::Accepted(source_sha256)) => {
                            *last_reported_sha256 = source_sha256;
                        }
                        _ => return Ok(()),
                    }
                    let result = HttpRequestResult::session_changed();
                    return complete_task(
                        config,
                        key,
                        &format!("{base}/complete"),
                        serde_json::to_value(result).map_err(|error| error.to_string())?,
                    );
                }
            }
            let request = task
                .request
                .as_ref()
                .ok_or_else(|| "HTTP request task has no request".to_string())?;
            let result = execute_http_request(request);
            complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::to_value(result).map_err(|error| error.to_string())?,
            )
        }
        "read_files" => {
            let root = sv2_root(&config.session_path)?;
            let result = file_tasks::read_files(
                &root,
                &task.paths,
                task.max_file_bytes.unwrap_or(0),
                task.max_total_bytes.unwrap_or(0),
            );
            complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::to_value(result).map_err(|error| error.to_string())?,
            )
        }
        "write_files" => {
            let root = sv2_root(&config.session_path)?;
            let result = file_tasks::write_files(&root, &config.session_path, &task.files);
            complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::to_value(result).map_err(|error| error.to_string())?,
            )
        }
        _ => Err("unsupported directory task".to_string()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpRequestResult {
    status: u16,
    response_body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_body_base64: Option<String>,
    response_headers: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
    upstream_attempts: Vec<UpstreamAttempt>,
}

impl HttpRequestResult {
    fn session_changed() -> Self {
        Self {
            status: 599,
            response_body: String::new(),
            response_body_base64: None,
            response_headers: std::collections::BTreeMap::new(),
            error: Some("session_changed"),
            upstream_attempts: Vec::new(),
        }
    }
}

impl Drop for HttpRequestResult {
    fn drop(&mut self) {
        self.response_body.zeroize();
        if let Some(body) = &mut self.response_body_base64 {
            body.zeroize();
        }
        for values in self.response_headers.values_mut() {
            for value in values {
                value.zeroize();
            }
        }
    }
}

fn execute_http_request(request: &http_request::HttpRequestSpec) -> HttpRequestResult {
    let mut response = http_request::execute(request);
    let (response_body, response_body_base64) =
        if request.response_encoding == "base64" || response.error == Some("invalid_response") {
            (String::new(), Some(URL_SAFE_NO_PAD.encode(&response.body)))
        } else {
            (
                std::str::from_utf8(&response.body)
                    .map(str::to_string)
                    .unwrap_or_default(),
                None,
            )
        };
    HttpRequestResult {
        status: response.status,
        response_body,
        response_body_base64,
        response_headers: std::mem::take(&mut response.headers),
        error: response.error,
        upstream_attempts: response.attempt.take().into_iter().collect(),
    }
}
fn execute_command_task(
    config: &Config,
    key: &SigningKey,
    base: &str,
    task: &DirectoryTask,
    host: &dyn ConnectorHost,
) -> Result<(), String> {
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
    if !host.approve_command(command, shell, &cwd) {
        if host.should_stop() {
            return complete_task(
                config,
                key,
                &format!("{base}/complete"),
                serde_json::json!({ "cancelled": true }),
            );
        }
        return complete_task(
            config,
            key,
            &format!("{base}/complete"),
            serde_json::json!({ "denied": true, "command": command }),
        );
    }
    if host.should_stop() {
        return complete_task(
            config,
            key,
            &format!("{base}/complete"),
            serde_json::json!({ "cancelled": true }),
        );
    }
    let timeout = Duration::from_secs(
        task.timeout_seconds
            .unwrap_or(300)
            .clamp(1, MAX_COMMAND_SECONDS),
    );
    let Some(outcome) = run_shell_command(shell, command, &cwd, timeout, host)? else {
        return complete_task(
            config,
            key,
            &format!("{base}/complete"),
            serde_json::json!({ "command": command, "shell": shell, "cancelled": true }),
        );
    };
    let body = serde_json::json!({
        "command": command,
        "shell": shell,
        "exitCode": outcome.exit_code,
        "timedOut": outcome.timed_out,
        "stdout": outcome.stdout,
        "stderr": outcome.stderr,
    });
    complete_task(config, key, &format!("{base}/complete"), body)
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
    host: &dyn ConnectorHost,
) -> Result<Option<CommandResult>, String> {
    let mut process = shell_process(shell, command);
    process
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process
        .spawn()
        .map_err(|error| format!("cannot start the command: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        terminate_process_tree(&mut child);
        let _ = child.wait();
        return Err("command stdout pipe is unavailable".to_string());
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_process_tree(&mut child);
        let _ = child.wait();
        return Err("command stderr pipe is unavailable".to_string());
    };
    let (output_tx, output_rx) = std::sync::mpsc::channel();
    let stdout_tx = output_tx.clone();
    thread::spawn(move || {
        let _ = stdout_tx.send((true, read_bounded_output(stdout)));
    });
    thread::spawn(move || {
        let _ = output_tx.send((false, read_bounded_output(stderr)));
    });
    let deadline = SystemTime::now() + timeout;
    let mut timed_out = false;
    loop {
        if host.should_stop() {
            terminate_process_tree(&mut child);
            let _ = child.wait();
            return Ok(None);
        }
        match child.try_wait() {
            Err(error) => {
                terminate_process_tree(&mut child);
                let _ = child.wait();
                return Err(error.to_string());
            }
            Ok(Some(_)) => break,
            Ok(None) if SystemTime::now() >= deadline => {
                terminate_process_tree(&mut child);
                timed_out = true;
                break;
            }
            Ok(None) => thread::sleep(COMMAND_POLL_INTERVAL),
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for the command: {error}"))?;
    let mut stdout = None;
    let mut stderr = None;
    while stdout.is_none() || stderr.is_none() {
        if host.should_stop() {
            return Ok(None);
        }
        if SystemTime::now() >= deadline {
            timed_out = true;
            break;
        }
        match output_rx.recv_timeout(COMMAND_POLL_INTERVAL) {
            Ok((true, result)) => {
                stdout = Some(
                    result.map_err(|error| format!("cannot read the command output: {error}"))?,
                );
            }
            Ok((false, result)) => {
                stderr = Some(
                    result.map_err(|error| format!("cannot read the command output: {error}"))?,
                );
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("cannot read the command output".to_string());
            }
        }
    }
    Ok(Some(CommandResult {
        exit_code: status.code(),
        timed_out,
        stdout: stdout
            .map(|bytes| truncate_output(&bytes))
            .unwrap_or_default(),
        stderr: stderr
            .map(|bytes| truncate_output(&bytes))
            .unwrap_or_default(),
    }))
}

fn read_bounded_output(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::with_capacity(MAX_COMMAND_OUTPUT_BYTES + 1);
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(captured);
        }
        let remaining = MAX_COMMAND_OUTPUT_BYTES + 1 - captured.len();
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

fn shell_process(shell: &str, command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut process = match shell {
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
        };
        windows_process::configure_background(&mut process);
        process
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "macos")]
        use std::os::unix::process::CommandExt;

        let mut process = Command::new(shell);
        process.args(["-c", command]);
        #[cfg(target_os = "macos")]
        process.process_group(0);
        process
    }
}

fn terminate_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill.exe");
        command.args(["/T", "/F", "/PID", &child.id().to_string()]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        windows_process::configure_background(&mut command);
        let _ = command.status();
    }
    #[cfg(target_os = "macos")]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
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
    let parent = path
        .parent()
        .ok_or_else(|| "target path has no parent folder".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".boxy-")
        .tempfile_in(parent)
        .map_err(|error| format!("cannot create temporary file: {error}"))?;
    temporary
        .write_all(bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| format!("cannot write temporary file: {error}"))?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| format!("cannot replace {}: {}", path.display(), error.error))
}

fn replace_session_if_unchanged(
    path: &Path,
    expected_sha256: &str,
    ciphertext: &[u8],
) -> Result<String, String> {
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
    let mut temporary = tempfile::Builder::new()
        .prefix(".boxy-session-")
        .tempfile_in(parent)
        .map_err(|error| format!("cannot create temporary session file: {error}"))?;
    temporary
        .write_all(ciphertext)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| format!("cannot write temporary session file: {error}"))?;
    let current = read_session_ciphertext(path)?;
    if sha256_base64url(&current) != expected_sha256 {
        return Err(
            "session changed while authorization was open; no write was performed".to_string(),
        );
    }
    temporary
        .persist(path)
        .map_err(|error| format!("cannot replace session: {}", error.error))?;
    let written = read_session_ciphertext(path)?;
    if written != ciphertext {
        return Err("written session verification failed".to_string());
    }
    Ok(sha256_base64url(&written))
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

fn get_signed<R: for<'de> Deserialize<'de>>(
    server: &str,
    path: &str,
    nonce: &str,
    signature: &str,
) -> Result<R, String> {
    parse_response(
        ureq::get(&format!("{server}{path}"))
            .timeout(Duration::from_secs(30))
            .set("X-Connector-Nonce", nonce)
            .set("X-Connector-Signature", signature)
            .call(),
    )
}

fn post_signed_json<T: Serialize>(
    server: &str,
    path: &str,
    key: &SigningKey,
    body: &T,
) -> Result<(), String> {
    let (nonce, signature) = request_signature(key, "POST", path);
    let serialized = Zeroizing::new(serde_json::to_vec(body).map_err(|error| error.to_string())?);
    let response = ureq::post(&format!("{server}{path}"))
        .timeout(Duration::from_secs(30))
        .set("Content-Type", "application/json")
        .set("X-Connector-Nonce", &nonce)
        .set("X-Connector-Signature", &signature)
        .send_bytes(&serialized);
    let _: serde_json::Value = parse_response(response)?;
    Ok(())
}

fn post_signed_json_status<T: Serialize>(
    server: &str,
    path: &str,
    key: &SigningKey,
    body: &T,
) -> Result<(u16, serde_json::Value), String> {
    let (nonce, signature) = request_signature(key, "POST", path);
    let response = ureq::post(&format!("{server}{path}"))
        .timeout(Duration::from_secs(30))
        .set("Content-Type", "application/json")
        .set("X-Connector-Nonce", &nonce)
        .set("X-Connector-Signature", &signature)
        .send_json(serde_json::to_value(body).map_err(|error| error.to_string())?);
    let (status, response) = match response {
        Ok(response) => (response.status(), response),
        Err(ureq::Error::Status(status, response)) => (status, response),
        Err(_) => return Err("authorization service request failed".to_string()),
    };
    let value = response
        .into_json()
        .map_err(|_| "authorization service returned invalid JSON".to_string())?;
    Ok((status, value))
}

fn put_task_content(
    config: &Config,
    key: &SigningKey,
    path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let (nonce, signature) = request_signature(key, "PUT", path);
    match ureq::put(&format!("{}{}", config.server_url, path))
        .timeout(Duration::from_secs(30))
        .set("Content-Length", &bytes.len().to_string())
        .set("X-Connector-Nonce", &nonce)
        .set("X-Connector-Signature", &signature)
        .send_bytes(bytes)
    {
        Ok(_) => Ok(()),
        Err(error) => Err(format!("cannot upload directory content: {error}")),
    }
}

fn get_task_content(config: &Config, key: &SigningKey, path: &str) -> Result<Vec<u8>, String> {
    let (nonce, signature) = request_signature(key, "GET", path);
    let response = ureq::get(&format!("{}{}", config.server_url, path))
        .timeout(Duration::from_secs(30))
        .set("X-Connector-Nonce", &nonce)
        .set("X-Connector-Signature", &signature)
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

fn complete_task(
    config: &Config,
    key: &SigningKey,
    path: &str,
    mut body: serde_json::Value,
) -> Result<(), String> {
    let result = post_signed_json(&config.server_url, path, key, &body);
    zeroize_json(&mut body);
    result
}

fn zeroize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => {
            for value in values {
                zeroize_json(value);
            }
        }
        serde_json::Value::Object(values) => {
            for (mut key, mut value) in std::mem::take(values) {
                key.zeroize();
                zeroize_json(&mut value);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn request_signature(key: &SigningKey, method: &str, path: &str) -> (String, String) {
    let nonce = format!("{}-{}", unix_timestamp(), random_suffix());
    let signed = format!("{method}\n{path}\n{nonce}");
    (
        nonce,
        URL_SAFE_NO_PAD.encode(key.sign(signed.as_bytes()).to_bytes()),
    )
}

fn random_suffix() -> String {
    let mut bytes = [0u8; 16];
    let mut rng = OsRng;
    rng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
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

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
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

#[cfg(test)]
#[path = "../../test/connector_regression.rs"]
mod connector_regression;

#[cfg(test)]
#[path = "../../test/core_runner.rs"]
mod core_runner_tests;
