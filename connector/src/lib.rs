mod file_tasks;
mod http_request;
mod launch;
pub mod local_resources;
mod mounts;
pub(crate) mod request_trace;
#[cfg(windows)]
mod windows_process;

pub use launch::{Config, LaunchOptions};

use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use notify::{RecursiveMode, Watcher};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const PROTOCOL_VERSION: u8 = 4;
const CONNECT_PATH: &str = "/api/connect";
const MAX_TASK_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_RECEIPT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROCESS_SECONDS: u64 = 3600;
const POLL_DELAY: Duration = Duration::from_millis(900);
const CONNECTOR_VERSION_HEADER: &str = "X-Boxy-Connector-Version";
const MAX_EXTERNAL_RESPONSE_BYTES: usize = 64 * 1024;

pub trait ConnectorHost: Send + Sync {
    fn open_editor(&self, url: Url, cookies: Vec<BrowserCookie>) -> Result<(), String>;
    fn open_external(&self, url: Url) -> Result<(), String>;
    fn approve_process(&self, executable: &str, args: &[String], cwd: Option<&Path>) -> bool;
    fn should_stop(&self) -> bool;
    fn set_resource_root(&self, _root: &Path) {}
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Hello<'a> {
    protocol_version: u8,
    platform: &'static str,
    capabilities: Vec<&'static str>,
    public_key: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectResponse {
    protocol_version: u8,
    connection_id: String,
    browser_url: String,
    expires_at: i64,
    browser_cookies: Vec<BrowserCookie>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpgradeResponse {
    open_external_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollResponse {
    protocol_version: u8,
    state: String,
    #[serde(default)]
    tasks: Vec<RemoteTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteTask {
    id: String,
    kind: String,
    params: Value,
}

impl Drop for RemoteTask {
    fn drop(&mut self) {
        zeroize_json(&mut self.params);
    }
}

pub fn prepare_config() -> Result<LaunchOptions, String> {
    LaunchOptions::from_environment_and_args()
}

pub fn resolve_config(_launch: &LaunchOptions, server_url: &str) -> Result<Config, String> {
    Ok(Config {
        server_url: launch::validate_service_url(server_url)?,
    })
}

pub fn run_elevated_if_requested() -> Option<i32> {
    #[cfg(windows)]
    {
        let mut arguments = env::args_os();
        arguments.next();
        if arguments.next()?.to_string_lossy() != "--elevated-exec" {
            return None;
        }
        let encoded = arguments.next()?;
        if arguments.next().is_some() {
            return Some(1);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded.to_string_lossy().as_bytes())
            .ok()?;
        let request: ElevatedRequest = serde_json::from_slice(&bytes).ok()?;
        return Some(match run_elevated_child(request) {
            Ok(()) => 0,
            Err(_) => 1,
        });
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn run_connected(config: &Config, host: &dyn ConnectorHost) -> Result<(), String> {
    if host.should_stop() {
        return Ok(());
    }
    let key = SigningKey::generate(&mut OsRng);
    let public_key = URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes());
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
        platform: platform_name(),
        capabilities: capabilities(),
        public_key: &public_key,
    };
    let Some(connected) = post_connect(config, &hello, host)? else {
        return Ok(());
    };
    if connected.protocol_version != PROTOCOL_VERSION
        || connected.connection_id.is_empty()
        || !connected
            .connection_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("service returned an incomplete connection response".to_string());
    }
    let expires_at = time::OffsetDateTime::from_unix_timestamp_nanos(
        i128::from(connected.expires_at) * 1_000_000,
    )
    .map_err(|_| "service returned an invalid connection expiry".to_string())?;
    if expires_at <= time::OffsetDateTime::now_utc() {
        return Err("service connection response has expired".to_string());
    }
    let browser = validate_browser_url(&config.server_url, &connected.browser_url)?;
    validate_browser_cookies(&connected.browser_cookies)?;
    host.open_editor(browser, connected.browser_cookies)?;
    run_connection(config, &connected.connection_id, &key, host)
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else {
        "macos"
    }
}

fn capabilities() -> Vec<&'static str> {
    let mut result = vec![
        "process.exec",
        "fs.read",
        "fs.write",
        "fs.list",
        "fs.read_ranges",
        "fs.patch",
        "fs.move",
        "fs.remove",
        "fs.upload",
        "fs.download",
        "fs.watch",
        "http.request",
    ];
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        result.extend(["fs.mount", "fs.unmount"]);
    }
    result
}

#[cfg(test)]
pub(crate) fn hello_body(
    platform: &'static str,
    capabilities: Vec<&'static str>,
    public_key: &str,
) -> Result<Value, String> {
    serde_json::to_value(Hello {
        protocol_version: PROTOCOL_VERSION,
        platform,
        capabilities,
        public_key,
    })
    .map_err(|error| error.to_string())
}

fn run_connection(
    config: &Config,
    id: &str,
    key: &SigningKey,
    host: &dyn ConnectorHost,
) -> Result<(), String> {
    let base = format!("/api/connections/{id}");
    let mut watches = HashMap::<
        String,
        (
            notify::RecommendedWatcher,
            mpsc::Receiver<notify::Result<notify::Event>>,
            PathBuf,
        ),
    >::new();
    let mut mounts = HashMap::<PathBuf, mounts::MountManager>::new();
    let mut close_connection = true;
    let result = (|| loop {
        if host.should_stop() {
            break Ok(());
        }
        let Some(poll) = get_signed::<PollResponse>(config, &base, key, host)? else {
            close_connection = false;
            break Ok(());
        };
        if poll.protocol_version != PROTOCOL_VERSION {
            break Err("service protocol version is not supported".to_string());
        }
        match poll.state.as_str() {
            "awaiting_auth" => {
                if !poll.tasks.is_empty() {
                    break Err("service sent executor tasks before authorization".to_string());
                }
                if wait_or_stop(host, POLL_DELAY) {
                    break Ok(());
                }
            }
            "ready" => {
                if poll.tasks.len() > 1 {
                    break Err("service returned more than one pending task".to_string());
                }
                send_watch_events(config, &base, key, &mut watches)?;
                if let Some(task) = poll.tasks.into_iter().next() {
                    if task.id.is_empty()
                        || !task
                            .id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    {
                        break Err("service returned an invalid task id".to_string());
                    }
                    let path = format!("{base}/tasks/{}", task.id);
                    let body = if serde_json::to_vec(&task.params)
                        .map_or(true, |bytes| bytes.len() > MAX_TASK_BYTES)
                    {
                        json!({"ok": false, "error": "task_too_large"})
                    } else if let Err(error) = validate_task(&task.kind, &task.params) {
                        json!({"ok": false, "error": error})
                    } else {
                        match execute_task(
                            config,
                            key,
                            &base,
                            &task,
                            host,
                            &mut watches,
                            &mut mounts,
                        ) {
                            Ok(result) if task_result_succeeded(&task.kind, &result) => {
                                json!({"ok": true, "result": result})
                            }
                            Ok(result) => {
                                json!({"ok": false, "error": "operation_failed", "result": result})
                            }
                            Err(error) => json!({"ok": false, "error": error}),
                        }
                    };
                    post_receipt(config, key, &format!("{path}/receipt"), &body, host)?;
                } else if wait_or_stop(host, POLL_DELAY) {
                    break Ok(());
                }
            }
            "denied" if poll.tasks.is_empty() => break Err("service access was denied".to_string()),
            "expired" if poll.tasks.is_empty() => {
                break Err("service connection has expired".to_string())
            }
            "closed" if poll.tasks.is_empty() => break Ok(()),
            _ => break Err("service returned an unknown connection state".to_string()),
        }
    })();
    watches.clear();
    if !close_connection {
        return result;
    }
    let close = post_signed(
        config,
        &format!("{base}/close"),
        key,
        &json!({"closed": true}),
    );
    result.and(close)
}

pub(crate) fn validate_task(kind: &str, params: &Value) -> Result<(), String> {
    let object = params
        .as_object()
        .ok_or_else(|| "invalid_params".to_string())?;
    let required: &[&str] = match kind {
        "process.exec" => &["executable", "args"],
        "fs.read" => &["root", "paths", "maxFileBytes", "maxTotalBytes"],
        "fs.write" => &["root", "files"],
        "fs.list" => &["root", "paths"],
        "fs.read_ranges" => &["root", "ranges"],
        "fs.patch" => &["root", "patches"],
        "fs.move" => &["root", "moves"],
        "fs.remove" => &["root", "removals"],
        "fs.upload" => &["root", "path", "expectedSha256", "expectedSize"],
        "fs.download" => &["root", "path", "maxBytes"],
        "fs.watch" => &["root", "path", "watchId"],
        "fs.mount" | "fs.unmount" => &["root", "mount"],
        "http.request" => &[
            "method",
            "url",
            "responseEncoding",
            "maxResponseBytes",
            "timeoutMs",
        ],
        _ => return Err("unsupported_task".to_string()),
    };
    if required.iter().any(|key| !object.contains_key(*key)) {
        return Err("invalid_params".to_string());
    }
    let allowed: &[&str] = match kind {
        "process.exec" => &[
            "executable",
            "args",
            "cwd",
            "env",
            "timeoutSeconds",
            "maxOutputBytes",
            "elevated",
        ],
        "fs.read" => &[
            "root",
            "protectedPath",
            "paths",
            "maxFileBytes",
            "maxTotalBytes",
        ],
        "fs.write" => &["root", "protectedPath", "files"],
        "fs.list" => &["root", "protectedPath", "paths"],
        "fs.read_ranges" => &["root", "protectedPath", "ranges"],
        "fs.patch" => &["root", "protectedPath", "patches"],
        "fs.move" => &["root", "protectedPath", "moves"],
        "fs.remove" => &["root", "protectedPath", "removals"],
        "fs.upload" => &[
            "root",
            "protectedPath",
            "path",
            "expectedSha256",
            "expectedSize",
        ],
        "fs.download" => &[
            "root",
            "protectedPath",
            "path",
            "maxBytes",
            "expectedSize",
            "expectedSha256",
        ],
        "fs.watch" => &["root", "path", "watchId", "recursive"],
        "fs.mount" | "fs.unmount" => &["root", "protectedPath", "mount"],
        "http.request" => &[
            "method",
            "url",
            "headers",
            "body",
            "responseEncoding",
            "maxResponseBytes",
            "timeoutMs",
        ],
        _ => return Err("unsupported_task".to_string()),
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("invalid_params".to_string());
    }
    if let Some(root) = object.get("root") {
        if !root
            .as_str()
            .is_some_and(|value| Path::new(value).is_absolute())
        {
            return Err("invalid_root".to_string());
        }
    }
    if let Some(protected) = object.get("protectedPath") {
        if !protected.is_null() && !protected.is_string() {
            return Err("invalid_params".to_string());
        }
    }
    if kind == "process.exec" {
        let executable = object["executable"].as_str().ok_or("invalid_params")?;
        let args: Vec<String> =
            serde_json::from_value(object["args"].clone()).map_err(|_| "invalid_params")?;
        if executable.is_empty()
            || executable.contains('\0')
            || args.len() > 256
            || executable
                .len()
                .saturating_add(args.iter().map(String::len).sum::<usize>())
                > 64 * 1024
        {
            return Err("invalid_params".to_string());
        }
        if object
            .get("cwd")
            .is_some_and(|value| !value.is_string() && !value.is_null())
            || object
                .get("elevated")
                .is_some_and(|value| !value.is_boolean())
            || object
                .get("timeoutSeconds")
                .is_some_and(|value| !value.is_u64())
            || object
                .get("maxOutputBytes")
                .is_some_and(|value| !value.is_u64())
        {
            return Err("invalid_params".to_string());
        }
        if let Some(environment) = object.get("env") {
            if !environment.is_object() || environment.as_object().unwrap().len() > 64 {
                return Err("invalid_environment".to_string());
            }
            for (name, value) in environment.as_object().unwrap() {
                if name.is_empty()
                    || name.contains(['=', '\0'])
                    || !value.is_string()
                    || value.as_str().unwrap().contains('\0')
                {
                    return Err("invalid_environment".to_string());
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn task_result_succeeded(kind: &str, result: &Value) -> bool {
    if kind == "http.request" {
        return result.get("error").is_none_or(Value::is_null);
    }
    if kind == "process.exec" {
        return result.get("approved").and_then(Value::as_bool) == Some(true);
    }
    if let Some(files) = result.get("files").and_then(Value::as_array) {
        return result.get("failed").and_then(Value::as_bool) != Some(true)
            && files.iter().all(|file| {
                file.get("error").is_none_or(Value::is_null)
                    && file.get("ok").and_then(Value::as_bool) != Some(false)
            });
    }
    true
}

fn execute_task(
    config: &Config,
    key: &SigningKey,
    connection: &str,
    task: &RemoteTask,
    host: &dyn ConnectorHost,
    watches: &mut HashMap<
        String,
        (
            notify::RecommendedWatcher,
            mpsc::Receiver<notify::Result<notify::Event>>,
            PathBuf,
        ),
    >,
    mounts: &mut HashMap<PathBuf, mounts::MountManager>,
) -> Result<Value, String> {
    let params = &task.params;
    match task.kind.as_str() {
        "process.exec" => execute_process(params, host),
        "http.request" => execute_http(params),
        "fs.watch" => register_watch(params, watches),
        "fs.mount" | "fs.unmount" => execute_mount(config, params, &task.kind, mounts),
        kind if kind.starts_with("fs.") => {
            if let Ok((root, _)) = task_root(params) {
                host.set_resource_root(&root);
            }
            execute_file_task(config, key, connection, task, kind, host)
        }
        _ => {
            let _ = (key, connection);
            Err("unsupported_task".to_string())
        }
    }
}

fn task_root(params: &Value) -> Result<(PathBuf, Option<PathBuf>), String> {
    let root = PathBuf::from(params["root"].as_str().ok_or("invalid_root")?);
    let root = fs::canonicalize(root).map_err(|_| "invalid_root")?;
    if !root.is_dir() {
        return Err("invalid_root".to_string());
    }
    let protected = params
        .get("protectedPath")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    if let Some(path) = &protected {
        let absolute = if path.is_absolute() {
            path.clone()
        } else {
            root.join(path)
        };
        let absolute = fs::canonicalize(absolute).map_err(|_| "invalid_protected_path")?;
        if !absolute.starts_with(&root) {
            return Err("invalid_protected_path".to_string());
        }
        return Ok((root, Some(absolute)));
    }
    Ok((root, None))
}

fn execute_file_task(
    config: &Config,
    key: &SigningKey,
    connection: &str,
    task: &RemoteTask,
    kind: &str,
    host: &dyn ConnectorHost,
) -> Result<Value, String> {
    let params = &task.params;
    if kind == "fs.list" {
        let paths: Vec<String> =
            serde_json::from_value(params["paths"].clone()).map_err(|_| "invalid_params")?;
        let requested_root = PathBuf::from(params["root"].as_str().ok_or("invalid_root")?);
        return list_task(&requested_root, &paths);
    }
    let (root, protected) = task_root(params)?;
    let protected = protected
        .as_deref()
        .unwrap_or(Path::new(".boxy-no-protected-path"));
    match kind {
        "fs.read" => {
            let paths: Vec<String> =
                serde_json::from_value(params["paths"].clone()).map_err(|_| "invalid_params")?;
            let max_file = params["maxFileBytes"].as_u64().ok_or("invalid_params")? as usize;
            let max_total = params["maxTotalBytes"].as_u64().ok_or("invalid_params")? as usize;
            serde_json::to_value(file_tasks::read_files(&root, &paths, max_file, max_total))
                .map_err(|e| e.to_string())
        }
        "fs.read_ranges" => {
            let ranges: Vec<file_tasks::RangeReadTaskEntry> =
                serde_json::from_value(params["ranges"].clone()).map_err(|_| "invalid_params")?;
            serde_json::to_value(file_tasks::read_ranges(&root, protected, &ranges, || {
                host.should_stop()
            }))
            .map_err(|e| e.to_string())
        }
        "fs.patch" => {
            let patches: Vec<file_tasks::FilePatchTaskEntry> =
                serde_json::from_value(params["patches"].clone()).map_err(|_| "invalid_params")?;
            serde_json::to_value(file_tasks::patch_files(&root, protected, &patches, || {
                host.should_stop()
            }))
            .map_err(|e| e.to_string())
        }
        "fs.write" => {
            let files: Vec<file_tasks::WriteFileTaskEntry> =
                serde_json::from_value(params["files"].clone()).map_err(|_| "invalid_params")?;
            serde_json::to_value(file_tasks::write_files(&root, protected, &files))
                .map_err(|e| e.to_string())
        }
        "fs.move" => {
            let moves: Vec<file_tasks::MoveEntry> =
                serde_json::from_value(params["moves"].clone()).map_err(|_| "invalid_params")?;
            serde_json::to_value(file_tasks::move_files(&root, protected, &moves))
                .map_err(|e| e.to_string())
        }
        "fs.remove" => {
            let removals: Vec<file_tasks::RemovalEntry> =
                serde_json::from_value(params["removals"].clone()).map_err(|_| "invalid_params")?;
            serde_json::to_value(file_tasks::remove_files(&root, protected, &removals))
                .map_err(|e| e.to_string())
        }
        "fs.upload" | "fs.download" => {
            transfer_file(config, key, connection, &root, protected, task, kind, host)
        }
        _ => Err("unsupported_task".to_string()),
    }
}

fn list_task(root: &Path, paths: &[String]) -> Result<Value, String> {
    if !root.is_absolute() {
        return Err("invalid_root".to_string());
    }
    let result = match fs::canonicalize(root) {
        Ok(root) if root.is_dir() => Ok(file_tasks::list_files(&root, paths, &[])),
        Ok(_) => Err("invalid_root".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(file_tasks::list_files_for_missing_root(paths))
        }
        Err(_) => Err("invalid_root".to_string()),
    }?;
    let value = serde_json::to_value(result).map_err(|error| error.to_string())?;
    let size = serde_json::to_vec(&json!({
        "ok": false,
        "error": "operation_failed",
        "result": &value,
    }))
    .map_err(|error| error.to_string())?
    .len();
    if size > MAX_RECEIPT_BYTES {
        return Err("manifest_too_large".to_string());
    }
    Ok(value)
}

fn transfer_file(
    config: &Config,
    key: &SigningKey,
    connection: &str,
    root: &Path,
    protected: &Path,
    task: &RemoteTask,
    kind: &str,
    host: &dyn ConnectorHost,
) -> Result<Value, String> {
    use std::io::Seek;
    let relative = task.params["path"].as_str().ok_or("invalid_params")?;
    let content_path = format!("{connection}/tasks/{}/content", task.id);
    let progress_path = format!("{connection}/tasks/{}/progress", task.id);
    if kind == "fs.upload" {
        let expected_hash = task.params["expectedSha256"]
            .as_str()
            .ok_or("invalid_params")?;
        let expected_size = task.params["expectedSize"]
            .as_u64()
            .ok_or("invalid_params")?;
        if expected_size > file_tasks::MAX_STREAM_BYTES {
            return Err("file_too_large".to_string());
        }
        let (_, mut file) =
            file_tasks::verify_stream_file(root, protected, relative, expected_hash, expected_size)
                .map_err(str::to_string)?;
        file.rewind().map_err(|error| error.to_string())?;
        let mut progress = TransferProgress::new(
            config,
            key,
            &progress_path,
            "uploading",
            Some(expected_size),
        );
        progress.report(0, true);
        let (nonce, signature) = request_signature(key, "POST", &content_path);
        let mut reader = ProgressReader {
            reader: file,
            host,
            progress: &mut progress,
            completed: 0,
        };
        let response = ureq::post(&format!("{}{}", config.server_url, content_path))
            .timeout(Duration::from_secs(120))
            .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
            .set("Content-Length", &expected_size.to_string())
            .set("X-Connector-Nonce", &nonce)
            .set("X-Connector-Signature", &signature)
            .send(&mut reader);
        let completed = reader.completed;
        if host.should_stop() {
            return Err("cancelled".to_string());
        }
        match response {
            Ok(_) => {
                progress.finish(completed);
                Ok(json!({"path":relative,"sha256":expected_hash,"size":expected_size}))
            }
            Err(ureq::Error::Status(status, response)) => Err(format!(
                "content upload rejected ({status}): {}",
                response.into_string().unwrap_or_default()
            )),
            Err(error) => Err(format!("content upload failed: {error}")),
        }
    } else {
        let max_bytes = task.params["maxBytes"]
            .as_u64()
            .filter(|value| *value > 0 && *value <= file_tasks::MAX_STREAM_BYTES)
            .ok_or("invalid_max_bytes")?;
        let expected_hash = task.params.get("expectedSha256").and_then(Value::as_str);
        let expected_size = task.params.get("expectedSize").and_then(Value::as_u64);
        if expected_size.is_some_and(|size| size > max_bytes) {
            return Err("invalid_expected_size".to_string());
        }
        if host.should_stop() {
            return Err("cancelled".to_string());
        }
        let mut progress =
            TransferProgress::new(config, key, &progress_path, "downloading", expected_size);
        progress.report(0, true);
        let (nonce, signature) = request_signature(key, "GET", &content_path);
        let response = ureq::get(&format!("{}{}", config.server_url, content_path))
            .timeout(Duration::from_secs(120))
            .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
            .set("X-Connector-Nonce", &nonce)
            .set("X-Connector-Signature", &signature)
            .call()
            .map_err(|error| {
                if host.should_stop() {
                    "cancelled".to_string()
                } else {
                    format!("content download failed: {error}")
                }
            })?;
        let total_was_unknown = progress.total_bytes.is_none();
        progress.total_bytes = progress.total_bytes.or_else(|| {
            response
                .header("Content-Length")
                .and_then(|value| value.parse().ok())
        });
        if total_was_unknown && progress.total_bytes.is_some() {
            progress.report(0, true);
        }
        let mut temporary = tempfile::Builder::new()
            .prefix("boxy-download-")
            .tempfile()
            .map_err(|e| e.to_string())?;
        let mut reader = response.into_reader();
        let mut size = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if host.should_stop() {
                return Err("cancelled".to_string());
            }
            let count = reader
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            size = size.saturating_add(count as u64);
            if size > max_bytes {
                return Err("file_too_large".to_string());
            }
            temporary
                .as_file_mut()
                .write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
            progress.report(size, false);
        }
        progress.finish(size);
        if expected_size.is_some_and(|expected| size != expected) {
            return Err("size_mismatch".to_string());
        }
        temporary
            .as_file_mut()
            .rewind()
            .map_err(|e| e.to_string())?;
        let (hash, size) =
            file_tasks::hash_reader(temporary.as_file_mut()).map_err(str::to_string)?;
        if expected_hash.is_some_and(|expected| hash != expected) {
            return Err("content_mismatch".to_string());
        }
        temporary
            .as_file_mut()
            .rewind()
            .map_err(|e| e.to_string())?;
        file_tasks::install_stream_file(root, protected, relative, temporary, &hash, size)
            .map_err(str::to_string)?;
        Ok(json!({"path":relative,"sha256":hash,"size":size}))
    }
}

struct TransferProgress<'a> {
    config: &'a Config,
    key: &'a SigningKey,
    path: &'a str,
    phase: &'static str,
    total_bytes: Option<u64>,
    last_report: Option<std::time::Instant>,
}

impl<'a> TransferProgress<'a> {
    fn new(
        config: &'a Config,
        key: &'a SigningKey,
        path: &'a str,
        phase: &'static str,
        total_bytes: Option<u64>,
    ) -> Self {
        Self {
            config,
            key,
            path,
            phase,
            total_bytes,
            last_report: None,
        }
    }

    fn report(&mut self, completed_bytes: u64, force: bool) {
        let now = std::time::Instant::now();
        if !force
            && self
                .last_report
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(500))
        {
            return;
        }
        let body = TransferProgressBody {
            completed_bytes,
            total_bytes: self.total_bytes,
            phase: self.phase,
        };
        if let Ok(serialized) = serde_json::to_vec(&body) {
            let (nonce, signature) = request_signature(self.key, "POST", self.path);
            let _ = ureq::post(&format!("{}{}", self.config.server_url, self.path))
                .timeout(Duration::from_secs(2))
                .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
                .set("Content-Type", "application/json")
                .set("X-Connector-Nonce", &nonce)
                .set("X-Connector-Signature", &signature)
                .send_bytes(&serialized);
        }
        self.last_report = Some(std::time::Instant::now());
    }

    fn finish(&mut self, completed_bytes: u64) {
        if self.total_bytes.is_none() {
            self.total_bytes = Some(completed_bytes);
        }
        self.report(completed_bytes, true);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgressBody {
    completed_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    phase: &'static str,
}

struct ProgressReader<'p, 'c, R> {
    reader: R,
    host: &'p dyn ConnectorHost,
    progress: &'p mut TransferProgress<'c>,
    completed: u64,
}

impl<R: Read> Read for ProgressReader<'_, '_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.host.should_stop() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let count = self.reader.read(buffer)?;
        self.completed = self.completed.saturating_add(count as u64);
        self.progress.report(self.completed, false);
        Ok(count)
    }
}

fn execute_http(params: &Value) -> Result<Value, String> {
    let request: http_request::HttpRequestSpec =
        serde_json::from_value(params.clone()).map_err(|_| "invalid_params")?;
    let response = http_request::execute(&request);
    Ok(
        json!({"status":response.status,"headers":response.headers,"bodyBase64":URL_SAFE_NO_PAD.encode(&response.body),"error":response.error,"attempt":response.attempt}),
    )
}

fn execute_process(params: &Value, host: &dyn ConnectorHost) -> Result<Value, String> {
    let executable = params["executable"].as_str().ok_or("invalid_params")?;
    let args: Vec<String> =
        serde_json::from_value(params["args"].clone()).map_err(|_| "invalid_params")?;
    let cwd = params.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    let elevated = params
        .get("elevated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !host.approve_process(executable, &args, cwd.as_deref()) {
        return Ok(json!({"approved":false}));
    }
    let timeout = params
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(60)
        .clamp(1, MAX_PROCESS_SECONDS);
    let output_limit = params
        .get("maxOutputBytes")
        .and_then(Value::as_u64)
        .unwrap_or(MAX_OUTPUT_BYTES as u64)
        .clamp(1, MAX_OUTPUT_BYTES as u64) as usize;
    let mut environment = BTreeMap::new();
    if let Some(envs) = params.get("env").and_then(Value::as_object) {
        if envs.len() > 64 {
            return Err("invalid_environment".to_string());
        }
        let mut total = 0usize;
        for (name, value) in envs {
            let value = value.as_str().ok_or("invalid_environment")?;
            if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
                return Err("invalid_environment".to_string());
            }
            total = total.saturating_add(name.len()).saturating_add(value.len());
            if total > 32 * 1024 {
                return Err("invalid_environment".to_string());
            }
            environment.insert(name.clone(), value.to_string());
        }
    }
    if elevated {
        let (exit_code, timed_out, stdout, stderr, output_truncated) = run_elevated_process(
            executable,
            &args,
            cwd.as_deref(),
            &environment,
            timeout,
            output_limit,
            host,
        )?;
        return Ok(
            json!({"approved":true,"exitCode":exit_code,"timedOut":timed_out,"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"outputTruncated":output_truncated,"elevated":true}),
        );
    }
    let mut command = Command::new(executable);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.envs(&environment);
    #[cfg(windows)]
    windows_process::configure_background(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("process_start:{error}"))?;
    let stdout = child.stdout.take().ok_or("output_unavailable")?;
    let stderr = child.stderr.take().ok_or("output_unavailable")?;
    let stdout_limit = output_limit / 2;
    let stderr_limit = output_limit - stdout_limit;
    let out = thread::spawn(move || read_limited(stdout, stdout_limit));
    let err = thread::spawn(move || read_limited(stderr, stderr_limit));
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout);
    let mut timed_out = false;
    loop {
        if host.should_stop() {
            terminate_process_tree(&mut child);
            return Err("cancelled".to_string());
        }
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            timed_out = true;
            terminate_process_tree(&mut child);
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let (stdout, out_truncated) = out.join().unwrap_or_default();
    let (stderr, err_truncated) = err.join().unwrap_or_default();
    Ok(
        json!({"approved":true,"exitCode":status.code(),"timedOut":timed_out,"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"outputTruncated":out_truncated||err_truncated,"elevated":false}),
    )
}

fn read_limited(mut reader: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let _ = reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes);
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    (bytes, truncated)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedRequest {
    executable: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    env: BTreeMap<String, String>,
    timeout_seconds: u64,
    output_limit: usize,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    result_path: PathBuf,
    cancel_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedResult {
    exit_code: Option<i32>,
    timed_out: bool,
    output_truncated: bool,
    cancelled: bool,
}

fn run_elevated_process(
    executable: &str,
    args: &[String],
    cwd: Option<&Path>,
    environment: &BTreeMap<String, String>,
    timeout: u64,
    output_limit: usize,
    host: &dyn ConnectorHost,
) -> Result<(Option<i32>, bool, Vec<u8>, Vec<u8>, bool), String> {
    if host.should_stop() {
        return Err("cancelled".to_string());
    }
    #[cfg(windows)]
    {
        run_elevated_windows(
            executable,
            args,
            cwd,
            environment,
            timeout,
            output_limit,
            host,
        )
    }
    #[cfg(target_os = "macos")]
    {
        run_elevated_macos(
            executable,
            args,
            cwd,
            environment,
            timeout,
            output_limit,
            host,
        )
    }
}

#[cfg(windows)]
fn run_elevated_windows(
    executable: &str,
    args: &[String],
    cwd: Option<&Path>,
    environment: &BTreeMap<String, String>,
    timeout: u64,
    output_limit: usize,
    host: &dyn ConnectorHost,
) -> Result<(Option<i32>, bool, Vec<u8>, Vec<u8>, bool), String> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0},
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        UI::Shell::{
            ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        },
    };
    let directory = tempfile::Builder::new()
        .prefix("boxy-elevated-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let stdout_path = directory.path().join("stdout");
    let stderr_path = directory.path().join("stderr");
    let result_path = directory.path().join("result.json");
    let cancel_path = directory.path().join("cancel");
    fs::write(&stdout_path, []).map_err(|e| e.to_string())?;
    fs::write(&stderr_path, []).map_err(|e| e.to_string())?;
    let request = ElevatedRequest {
        executable: executable.to_string(),
        args: args.to_vec(),
        cwd: cwd.map(Path::to_path_buf),
        env: environment.clone(),
        timeout_seconds: timeout,
        output_limit,
        stdout_path: stdout_path.clone(),
        stderr_path: stderr_path.clone(),
        result_path: result_path.clone(),
        cancel_path,
    };
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).map_err(|e| e.to_string())?);
    if payload.len() > 24_000 {
        return Err("elevated_request_too_large".to_string());
    }
    let executable = env::current_exe().map_err(|e| e.to_string())?;
    let wide = |value: &OsStr| value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let verb = wide(OsStr::new("runas"));
    let file = wide(executable.as_os_str());
    let parameters = wide(OsStr::new(&format!("--elevated-exec {payload}")));
    let directory_path = executable.parent().map(|path| wide(path.as_os_str()));
    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.lpDirectory = directory_path
        .as_ref()
        .map_or(std::ptr::null(), |path| path.as_ptr());
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 || info.hProcess.is_null() {
        let error = unsafe { GetLastError() };
        return Err(if error == 1223 {
            "system authorization was cancelled".to_string()
        } else {
            format!("elevated process could not start (Windows error {error})")
        });
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout.saturating_add(15));
    let mut cancelled = false;
    loop {
        if host.should_stop() && !cancelled {
            fs::write(&request.cancel_path, []).map_err(|e| e.to_string())?;
            cancelled = true;
        }
        let waited = unsafe { WaitForSingleObject(info.hProcess, 100) };
        if waited == WAIT_OBJECT_0 {
            break;
        }
        if std::time::Instant::now() >= deadline {
            unsafe {
                windows_sys::Win32::System::Threading::TerminateProcess(info.hProcess, 1);
                CloseHandle(info.hProcess);
            }
            return Err("elevated process helper did not complete".to_string());
        }
    }
    let mut helper_code = 1u32;
    let read_code = unsafe { GetExitCodeProcess(info.hProcess, &mut helper_code) } != 0;
    unsafe {
        CloseHandle(info.hProcess);
    }
    if !read_code || helper_code != 0 {
        return Err("elevated process helper failed".to_string());
    }
    let result: ElevatedResult =
        serde_json::from_slice(&fs::read(result_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if cancelled || result.cancelled {
        return Err("cancelled".to_string());
    }
    Ok((
        result.exit_code,
        result.timed_out,
        fs::read(stdout_path).map_err(|e| e.to_string())?,
        fs::read(stderr_path).map_err(|e| e.to_string())?,
        result.output_truncated,
    ))
}

#[cfg(windows)]
fn run_elevated_child(request: ElevatedRequest) -> Result<(), String> {
    let mut command = Command::new(&request.executable);
    command
        .args(&request.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(&request.env);
    if let Some(cwd) = &request.cwd {
        command.current_dir(cwd);
    }
    windows_process::configure_background(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("elevated_stdout_unavailable")?;
    let stderr = child.stderr.take().ok_or("elevated_stderr_unavailable")?;
    let out_limit = request.output_limit / 2;
    let err_limit = request.output_limit - out_limit;
    let out = thread::spawn(move || read_limited(stdout, out_limit));
    let err = thread::spawn(move || read_limited(stderr, err_limit));
    let deadline = std::time::Instant::now() + Duration::from_secs(request.timeout_seconds);
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if request.cancel_path.exists() {
            cancelled = true;
            terminate_process_tree(&mut child);
            break;
        }
        if std::time::Instant::now() >= deadline {
            timed_out = true;
            terminate_process_tree(&mut child);
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let (stdout, out_truncated) = out.join().unwrap_or_default();
    let (stderr, err_truncated) = err.join().unwrap_or_default();
    fs::write(&request.stdout_path, stdout).map_err(|e| e.to_string())?;
    fs::write(&request.stderr_path, stderr).map_err(|e| e.to_string())?;
    let result = ElevatedResult {
        exit_code: status.code(),
        timed_out,
        output_truncated: out_truncated || err_truncated,
        cancelled,
    };
    fs::write(
        request.result_path,
        serde_json::to_vec(&result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn run_elevated_macos(
    executable: &str,
    args: &[String],
    cwd: Option<&Path>,
    environment: &BTreeMap<String, String>,
    timeout: u64,
    output_limit: usize,
    host: &dyn ConnectorHost,
) -> Result<(Option<i32>, bool, Vec<u8>, Vec<u8>, bool), String> {
    use std::os::unix::process::CommandExt;
    let mut command_parts = Vec::new();
    if let Some(cwd) = cwd {
        command_parts.push(format!("cd {} &&", shell_quote(&cwd.to_string_lossy())));
    }
    if !environment.is_empty() {
        command_parts.push("env".to_string());
        for (name, value) in environment {
            command_parts.push(shell_quote(&format!("{name}={value}")));
        }
    }
    command_parts.push(shell_quote(executable));
    command_parts.extend(args.iter().map(|arg| shell_quote(arg)));
    let command_line = format!(
        "{} 2>&1; code=$?; printf '\\n__BOXY_EXIT_CODE:%s' \"$code\"; exit 0",
        command_parts.join(" ")
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(command_line);
    let shell_command = format!("/usr/bin/printf '%s' '{encoded}' | /usr/bin/base64 -D | /bin/sh");
    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_quote(&shell_command)
    );
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    let out_limit = output_limit;
    let stdout = child.stdout.take().unwrap();
    let out = thread::spawn(move || read_limited(stdout, out_limit.saturating_add(64)));
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout + 15);
    let status = loop {
        if host.should_stop() {
            terminate_process_tree(&mut child);
            return Err("cancelled".to_string());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            terminate_process_tree(&mut child);
            break child.wait().map_err(|e| e.to_string())?;
        }
        thread::sleep(Duration::from_millis(40));
    };
    let (mut stdout, truncated) = out.join().unwrap_or_default();
    let stderr = child
        .stderr
        .take()
        .map(|mut stream| {
            let mut bytes = Vec::new();
            let _ = stream.take(output_limit as u64).read_to_end(&mut bytes);
            bytes
        })
        .unwrap_or_default();
    if !status.success() {
        return Err(String::from_utf8_lossy(&stderr).into_owned());
    }
    let output = String::from_utf8_lossy(&stdout).to_string();
    let marker = "__BOXY_EXIT_CODE:";
    let (body, code) = output
        .rsplit_once(marker)
        .ok_or("elevated result is unavailable")?;
    let exit_code = code
        .trim()
        .parse::<i32>()
        .map_err(|_| "elevated result is invalid")?;
    stdout = body.as_bytes().to_vec();
    let was_truncated = truncated || stdout.len() > output_limit;
    stdout.truncate(output_limit);
    Ok((Some(exit_code), false, stdout, Vec::new(), was_truncated))
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn terminate_process_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill.exe");
        command
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        windows_process::configure_background(&mut command);
        let _ = command.status();
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn register_watch(
    params: &Value,
    watches: &mut HashMap<
        String,
        (
            notify::RecommendedWatcher,
            mpsc::Receiver<notify::Result<notify::Event>>,
            PathBuf,
        ),
    >,
) -> Result<Value, String> {
    let (root, _) = task_root(params)?;
    let relative = params["path"].as_str().ok_or("invalid_params")?;
    let path = file_tasks::safe_path(&root, relative, false).map_err(str::to_string)?;
    let id = params["watchId"]
        .as_str()
        .ok_or("invalid_params")?
        .to_string();
    if id.is_empty() || id.len() > 128 || watches.contains_key(&id) {
        return Err("invalid_watch".to_string());
    }
    let recursive = params
        .get("recursive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).map_err(|error| error.to_string())?;
    watcher
        .watch(
            &path,
            if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            },
        )
        .map_err(|error| error.to_string())?;
    watches.insert(id.clone(), (watcher, receiver, root));
    Ok(json!({"watchId":id,"registered":true}))
}

fn send_watch_events(
    config: &Config,
    connection: &str,
    key: &SigningKey,
    watches: &mut HashMap<
        String,
        (
            notify::RecommendedWatcher,
            mpsc::Receiver<notify::Result<notify::Event>>,
            PathBuf,
        ),
    >,
) -> Result<(), String> {
    let mut events = Vec::new();
    for (watch_id, (_, rx, root)) in watches.iter() {
        while let Ok(Ok(event)) = rx.try_recv() {
            for path in event.paths {
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                let Ok((sha256, _)) = file_tasks::hash_path(&path) else {
                    continue;
                };
                events.push(json!({"watchId":watch_id,"path":relative.to_string_lossy().replace('\\',"/"),"sha256":sha256,"kind":"changed"}));
                if events.len() >= 256 {
                    break;
                }
            }
        }
    }
    if !events.is_empty() {
        post_signed(
            config,
            &format!("/api/connections/{connection}/events"),
            key,
            &json!({"events":events}),
        )?;
    }
    Ok(())
}

fn execute_mount(
    config: &Config,
    params: &Value,
    kind: &str,
    managers: &mut HashMap<PathBuf, mounts::MountManager>,
) -> Result<Value, String> {
    let (root, protected) = task_root(params)?;
    let protected = protected.unwrap_or_else(|| root.join(".boxy-protected-placeholder"));
    let manager = managers
        .entry(root.clone())
        .or_insert_with(|| mounts::MountManager::new(&root, &protected, &config.server_url));
    if kind == "fs.mount" {
        let spec: mounts::MountSpec =
            serde_json::from_value(params["mount"].clone()).map_err(|_| "invalid_params")?;
        let path = manager.mount(&spec).map_err(str::to_string)?;
        Ok(json!({"mounted":true,"path":path}))
    } else {
        let spec: mounts::MountSpec =
            serde_json::from_value(params["mount"].clone()).map_err(|_| "invalid_params")?;
        manager.unmount(&spec).map_err(str::to_string)?;
        Ok(json!({"mounted":false}))
    }
}

fn post_connect<T: Serialize>(
    config: &Config,
    body: &T,
    host: &dyn ConnectorHost,
) -> Result<Option<ConnectResponse>, String> {
    let response = ureq::post(&format!("{}{}", config.server_url, CONNECT_PATH))
        .timeout(Duration::from_secs(30))
        .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
        .set("Content-Type", "application/json")
        .send_json(serde_json::to_value(body).map_err(|e| e.to_string())?);
    match response {
        Ok(response) => response.into_json().map(Some).map_err(|e| e.to_string()),
        Err(ureq::Error::Status(426, response)) => {
            open_external_response(response, host)?;
            Ok(None)
        }
        Err(ureq::Error::Status(status, response)) => Err(format!(
            "service rejected connection ({status}): {}",
            response.into_string().unwrap_or_default()
        )),
        Err(error) => Err(error.to_string()),
    }
}

fn validate_external_url(value: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|_| "service returned an invalid external URL".to_string())?;
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
        .unwrap_or_default();
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || authority.contains('@')
    {
        return Err("service returned an invalid external URL".to_string());
    }
    Ok(url)
}

fn open_external_response(
    response: ureq::Response,
    host: &dyn ConnectorHost,
) -> Result<(), String> {
    let mut body = Vec::new();
    response
        .into_reader()
        .take((MAX_EXTERNAL_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() > MAX_EXTERNAL_RESPONSE_BYTES {
        return Err("service external-link response is too large".to_string());
    }
    let upgrade: UpgradeResponse = serde_json::from_slice(&body)
        .map_err(|_| "service returned an invalid external-link response".to_string())?;
    host.open_external(validate_external_url(&upgrade.open_external_url)?)
}

fn get_signed<R: for<'de> Deserialize<'de>>(
    config: &Config,
    path: &str,
    key: &SigningKey,
    host: &dyn ConnectorHost,
) -> Result<Option<R>, String> {
    while !host.should_stop() {
        let (nonce, signature) = request_signature(key, "GET", path);
        match ureq::get(&format!("{}{}", config.server_url, path))
            .timeout(Duration::from_secs(30))
            .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
            .set("X-Connector-Nonce", &nonce)
            .set("X-Connector-Signature", &signature)
            .call()
        {
            Ok(response) => {
                let mut bytes = Vec::new();
                response
                    .into_reader()
                    .take((MAX_TASK_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > MAX_TASK_BYTES {
                    return Err("service poll response is too large".to_string());
                }
                return serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|e| format!("service returned invalid poll JSON: {e}"));
            }
            Err(ureq::Error::Status(426, response)) => {
                open_external_response(response, host)?;
                return Ok(None);
            }
            Err(ureq::Error::Status(408 | 429 | 502 | 503 | 504, _))
            | Err(ureq::Error::Transport(_)) => {
                if wait_or_stop(host, Duration::from_secs(1)) {
                    break;
                }
            }
            Err(ureq::Error::Status(status, response)) => {
                return Err(format!(
                    "service rejected poll ({status}): {}",
                    response.into_string().unwrap_or_default()
                ))
            }
        }
    }
    Err("connection closed".to_string())
}

fn post_signed<T: Serialize>(
    config: &Config,
    path: &str,
    key: &SigningKey,
    body: &T,
) -> Result<(), String> {
    let serialized = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let (nonce, signature) = request_signature(key, "POST", path);
    let response = ureq::post(&format!("{}{}", config.server_url, path))
        .timeout(Duration::from_secs(30))
        .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
        .set("Content-Type", "application/json")
        .set("X-Connector-Nonce", &nonce)
        .set("X-Connector-Signature", &signature)
        .send_bytes(&serialized);
    match response {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(status, response)) => Err(format!(
            "service rejected request ({status}): {}",
            response.into_string().unwrap_or_default()
        )),
        Err(error) => Err(error.to_string()),
    }
}

fn post_receipt(
    config: &Config,
    key: &SigningKey,
    path: &str,
    body: &Value,
    host: &dyn ConnectorHost,
) -> Result<(), String> {
    let serialized = Zeroizing::new(serde_json::to_vec(body).map_err(|e| e.to_string())?);
    let mut retries = 0;
    loop {
        if retries > 0 && host.should_stop() {
            return Err("receipt interrupted".to_string());
        }
        let (nonce, signature) = request_signature(key, "POST", path);
        let response = ureq::post(&format!("{}{}", config.server_url, path))
            .timeout(Duration::from_secs(30))
            .set(CONNECTOR_VERSION_HEADER, env!("CARGO_PKG_VERSION"))
            .set("Content-Type", "application/json")
            .set("X-Connector-Nonce", &nonce)
            .set("X-Connector-Signature", &signature)
            .send_bytes(&serialized);
        match response {
            Ok(response) => {
                let _: Value = response.into_json().map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(ureq::Error::Status(425, _)) | Err(ureq::Error::Transport(_)) if retries < 5 => {
                retries += 1;
                if wait_or_stop(host, Duration::from_millis(250 * retries as u64)) {
                    return Err("receipt interrupted".to_string());
                }
            }
            Err(ureq::Error::Status(status, response)) => {
                return Err(format!(
                    "receipt rejected ({status}): {}",
                    response.into_string().unwrap_or_default()
                ))
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn request_signature(key: &SigningKey, method: &str, path: &str) -> (String, String) {
    let nonce = format!("{}-{}", unix_timestamp(), random_suffix());
    let message = format!("{method}\n{path}\n{nonce}");
    (
        nonce,
        URL_SAFE_NO_PAD.encode(key.sign(message.as_bytes()).to_bytes()),
    )
}

fn random_suffix() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn wait_or_stop(host: &dyn ConnectorHost, duration: Duration) -> bool {
    let deadline = std::time::Instant::now() + duration;
    while !host.should_stop() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return false;
        }
        thread::sleep(left.min(Duration::from_millis(100)));
    }
    true
}

fn validate_browser_url(server: &str, browser: &str) -> Result<Url, String> {
    let service = Url::parse(server).map_err(|_| "invalid service URL")?;
    let browser = Url::parse(browser).map_err(|_| "invalid browser URL")?;
    if service.origin() != browser.origin()
        || !browser.username().is_empty()
        || browser.password().is_some()
        || browser.fragment().is_some()
    {
        return Err("service returned a browser URL outside its origin".to_string());
    }
    Ok(browser)
}

fn validate_browser_cookies(cookies: &[BrowserCookie]) -> Result<(), String> {
    for cookie in cookies {
        if cookie.name.is_empty()
            || !cookie.name.bytes().all(is_cookie_token_byte)
            || !cookie.value.bytes().all(is_cookie_octet)
        {
            return Err("service returned an invalid browser cookie".to_string());
        }
        let pair = format!("{}={}", cookie.name, cookie.value);
        let parsed = cookie::Cookie::parse(pair.as_str())
            .map_err(|_| "service returned an invalid browser cookie".to_string())?;
        if parsed.name() != cookie.name
            || parsed.value() != cookie.value
            || parsed.to_string() != pair
        {
            return Err("service returned an invalid browser cookie".to_string());
        }
    }
    Ok(())
}

fn is_cookie_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_cookie_octet(byte: u8) -> bool {
    matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e)
}

fn zeroize_json(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(zeroize_json),
        Value::Object(values) => {
            for (_, value) in values.iter_mut() {
                zeroize_json(value);
            }
        }
        _ => {}
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sha256_base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

#[cfg(test)]
#[path = "../../test/core_runner.rs"]
mod tests;
