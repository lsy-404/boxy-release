use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::Zeroize;

use crate::file_tasks;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/rclone_binary.rs"));
}

const RCLONE_VERSION: &str = "1.75.2";
const MOUNT_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_TOKEN_LENGTH: usize = 43;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MountSpec {
    pub(crate) mount_id: String,
    pub(crate) path: String,
    pub(crate) guard_path: String,
    pub(crate) guard_sha256: String,
    pub(crate) guard_size: u64,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) files: Vec<MountFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MountFile {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

impl Drop for MountSpec {
    fn drop(&mut self) {
        if let Some(token) = &mut self.token {
            token.zeroize();
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MountRegistryEntry {
    mount_id: String,
    path: String,
    mount_path: PathBuf,
    guard_path: String,
    guard_sha256: String,
    guard_size: u64,
    identity_hash: String,
    spec_hash: String,
    pid: u32,
    process_start_time: u64,
    executable: PathBuf,
}

struct ActiveMount {
    entry: MountRegistryEntry,
    child: Option<Child>,
}

pub(crate) struct MountManager {
    root: PathBuf,
    protected_path: PathBuf,
    server_url: String,
    binary_path: Option<PathBuf>,
    extracted_binary: Option<tempfile::TempDir>,
    registry_dir: PathBuf,
    active: Vec<ActiveMount>,
}

impl MountManager {
    pub(crate) fn new(root: &Path, protected_path: &Path, server_url: &str) -> Self {
        let registry_dir = dirs::cache_dir()
            .unwrap_or_else(env::temp_dir)
            .join("boxy")
            .join("mounts");
        let mut manager = Self {
            root: root.to_path_buf(),
            protected_path: protected_path.to_path_buf(),
            server_url: server_url.to_string(),
            binary_path: None,
            extracted_binary: None,
            registry_dir,
            active: Vec::new(),
        };
        if platform_mount_available() {
            manager.binary_path = manager.resolve_binary();
        }
        manager.recover_mounts();
        manager
    }

    pub(crate) fn capability_available(&self) -> bool {
        self.binary_path.is_some() && platform_mount_available()
    }

    pub(crate) fn mount(&mut self, spec: &MountSpec) -> Result<PathBuf, &'static str> {
        if !self.capability_available() {
            return Err("mount_unavailable");
        }
        validate_mount_spec(spec, &self.server_url)?;
        validate_guard(&self.root, &self.protected_path, spec)?;
        let path = self.resolve_mount_path(spec)?;
        let identity_hash = identity_fingerprint(spec);
        let spec_hash = spec_fingerprint(spec);
        if let Some(index) = self
            .active
            .iter()
            .position(|mount| mount.entry.mount_id == spec.mount_id)
        {
            let existing = &mut self.active[index];
            if existing.entry.identity_hash == identity_hash {
                let registered_path = resolve_registered_mount(
                    &self.root,
                    &self.protected_path,
                    &existing.entry,
                    true,
                );
                if existing.entry.spec_hash == spec_hash
                    && path_equal(&path, &existing.entry.mount_path)
                    && registered_path
                        .as_ref()
                        .is_ok_and(|registered| path_equal(registered, &path))
                    && active_process_owned(existing)
                {
                    return Ok(path);
                }
                if active_process_owned(existing) {
                    let stoppable_path = resolve_registered_mount(
                        &self.root,
                        &self.protected_path,
                        &existing.entry,
                        false,
                    );
                    if !stoppable_path
                        .as_ref()
                        .is_ok_and(|registered| path_equal(registered, &path))
                    {
                        return Err("mount_state_unknown");
                    }
                    stop_mount(&path, existing)?;
                    let stale = self.active.remove(index);
                    remove_registry(&self.registry_dir, &stale.entry.mount_id);
                } else if is_mounted(&path) {
                    return Err("mount_state_unknown");
                } else {
                    let stale = self.active.remove(index);
                    remove_registry(&self.registry_dir, &stale.entry.mount_id);
                }
            } else {
                return Err("mount_conflict");
            }
        }
        if self
            .active
            .iter()
            .any(|mount| same_path(&mount.entry.path, &spec.path))
        {
            return Err("mount_conflict");
        }
        prepare_mountpoint(&self.root, &self.protected_path, spec)?;
        let binary = self.binary_path.as_ref().ok_or("mount_unavailable")?;
        let mut command = mount_command(binary, spec, &path)?;
        let mut child = match command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env(
                "RCLONE_HTTP_HEADERS",
                format!(
                    "Authorization,Bearer {}",
                    spec.token.as_deref().unwrap_or_default()
                ),
            )
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return Err("mount_failed"),
        };
        let until = Instant::now() + MOUNT_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    unmount_platform(&path);
                    return Err("mount_failed");
                }
                Err(_) => {
                    terminate_unregistered_mount(&path, &mut child);
                    return Err("mount_failed");
                }
                Ok(None) => {}
            }
            if mount_ready(&path) {
                if let Err(error) = verify_remote_entries(&path, &spec.files) {
                    terminate_unregistered_mount(&path, &mut child);
                    return Err(error);
                }
                break;
            }
            if Instant::now() >= until {
                terminate_unregistered_mount(&path, &mut child);
                return Err("mount_timeout");
            }
            thread::sleep(Duration::from_millis(200));
        }
        let executable = match fs::canonicalize(binary) {
            Ok(executable) => executable,
            Err(_) => {
                terminate_unregistered_mount(&path, &mut child);
                return Err("mount_failed");
            }
        };
        let Some(start_time) = process_start_time(child.id()) else {
            terminate_unregistered_mount(&path, &mut child);
            return Err("mount_failed");
        };
        let entry = MountRegistryEntry {
            mount_id: spec.mount_id.clone(),
            path: spec.path.clone(),
            mount_path: path.clone(),
            guard_path: spec.guard_path.clone(),
            guard_sha256: spec.guard_sha256.clone(),
            guard_size: spec.guard_size,
            identity_hash,
            spec_hash,
            pid: child.id(),
            process_start_time: start_time,
            executable,
        };
        if persist_registry(&self.registry_dir, &entry).is_err() {
            terminate_unregistered_mount(&path, &mut child);
            return Err("mount_failed");
        }
        self.active.push(ActiveMount {
            entry,
            child: Some(child),
        });
        Ok(path)
    }

    pub(crate) fn unmount(&mut self, spec: &MountSpec) -> Result<PathBuf, &'static str> {
        validate_mount_id(&spec.mount_id)?;
        let mount_path = self.resolve_mount_path(spec)?;
        if let Some(index) = self
            .active
            .iter()
            .position(|mount| mount.entry.mount_id == spec.mount_id)
        {
            let active = &self.active[index];
            if !same_path(&active.entry.path, &spec.path)
                || !same_path(&active.entry.guard_path, &spec.guard_path)
                || active.entry.guard_sha256 != spec.guard_sha256
                || active.entry.guard_size != spec.guard_size
            {
                return Err("mount_conflict");
            }
            validate_guard(&self.root, &self.protected_path, spec)?;
            let registered_path = resolve_registered_mount(
                &self.root,
                &self.protected_path,
                &self.active[index].entry,
                false,
            )?;
            if !path_equal(&registered_path, &mount_path) {
                return Err("mount_conflict");
            }
            if !active_process_owned(&mut self.active[index]) {
                if unmanaged_mount_present(&mount_path) {
                    return Err("mount_not_owned");
                }
                let active = self.active.remove(index);
                remove_registry(&self.registry_dir, &active.entry.mount_id);
                return Ok(mount_path);
            }
            stop_mount(&mount_path, &mut self.active[index])?;
            let active = self.active.remove(index);
            remove_registry(&self.registry_dir, &active.entry.mount_id);
            return Ok(mount_path);
        }
        if unmanaged_mount_present(&mount_path) {
            return Err("mount_not_owned");
        }
        validate_guard(&self.root, &self.protected_path, spec)?;
        Ok(mount_path)
    }

    fn resolve_binary(&mut self) -> Option<PathBuf> {
        if let Some(path) = env::var_os("BOXY_RCLONE_PATH") {
            let path = PathBuf::from(path);
            return valid_binary(&path).then_some(path);
        }
        let bytes = embedded::EMBEDDED_RCLONE?;
        let directory = tempfile::Builder::new()
            .prefix("boxy-rclone-")
            .tempdir()
            .ok()?;
        let name = if cfg!(windows) {
            "rclone.exe"
        } else {
            "rclone"
        };
        let path = directory.path().join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut file = options.open(&path).ok()?;
        file.write_all(bytes).ok()?;
        file.sync_all().ok()?;
        drop(file);
        let license_path = directory.path().join("rclone-LICENSE.txt");
        let mut license = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&license_path)
            .ok()?;
        license.write_all(embedded::RCLONE_LICENSE).ok()?;
        license.sync_all().ok()?;
        self.extracted_binary = Some(directory);
        valid_binary(&path).then_some(path)
    }

    fn resolve_mount_path(&self, spec: &MountSpec) -> Result<PathBuf, &'static str> {
        let path = file_tasks::safe_mount_path_parent(&self.root, &spec.path)?;
        if file_tasks::targets_protected_path(&self.root, &path, &self.protected_path) {
            return Err("protected_path_forbidden");
        }
        Ok(path)
    }

    fn recover_mounts(&mut self) {
        let Ok(entries) = fs::read_dir(&self.registry_dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(registry) = serde_json::from_slice::<MountRegistryEntry>(&bytes) else {
                continue;
            };
            if !registry_process_matches(&registry) {
                continue;
            }
            let Ok(path) =
                resolve_registered_mount(&self.root, &self.protected_path, &registry, true)
            else {
                continue;
            };
            if !path_equal(&path, &registry.mount_path) {
                continue;
            }
            self.active.push(ActiveMount {
                entry: registry,
                child: None,
            });
        }
    }
}

impl Drop for MountManager {
    fn drop(&mut self) {
        let mut retain_helper = false;
        for active in &mut self.active {
            let path =
                resolve_registered_mount(&self.root, &self.protected_path, &active.entry, false);
            let stopped = path.is_ok_and(|path| {
                active_process_owned(active) && stop_mount(&path, active).is_ok()
            });
            if stopped {
                remove_registry(&self.registry_dir, &active.entry.mount_id);
            } else if active
                .child
                .as_mut()
                .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
            {
                retain_helper = true;
            }
        }
        if retain_helper {
            if let Some(directory) = self.extracted_binary.take() {
                std::mem::forget(directory);
            }
        }
    }
}

fn validate_mount_spec(spec: &MountSpec, server_url: &str) -> Result<(), &'static str> {
    validate_mount_id(&spec.mount_id)?;
    if spec.files.is_empty() || spec.files.len() > 1024 || spec.guard_size > 1024 * 1024 {
        return Err("invalid_mount_spec");
    }
    let mut names = std::collections::HashSet::new();
    for file in &spec.files {
        validate_filename(&file.name)?;
        let name = if cfg!(windows) {
            file.name.to_ascii_lowercase()
        } else {
            file.name.clone()
        };
        if !names.insert(name)
            || file.name.len() > 255
            || file.size > file_tasks::MAX_STREAM_BYTES
            || !valid_hash(&file.sha256)
        {
            return Err("invalid_mount_spec");
        }
    }
    let url = spec.url.as_deref().ok_or("invalid_mount_spec")?;
    let token = spec.token.as_deref().ok_or("invalid_mount_spec")?;
    validate_remote_url(url, server_url)?;
    if token.len() != AUTH_TOKEN_LENGTH
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("invalid_mount_spec");
    }
    Ok(())
}

fn validate_mount_id(value: &str) -> Result<(), &'static str> {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
    {
        return Err("invalid_mount_spec");
    }
    Ok(())
}

fn validate_filename(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value.ends_with(['.', ' '])
        || value.contains(':')
        || {
            let basename = value
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || basename.len() == 4
                    && (basename.starts_with("COM") || basename.starts_with("LPT"))
                    && matches!(basename.as_bytes()[3], b'1'..=b'9')
        }
    {
        return Err("invalid_mount_spec");
    }
    Ok(())
}

fn verify_remote_entries(path: &Path, expected: &[MountFile]) -> Result<(), &'static str> {
    let expected = expected
        .iter()
        .map(|file| (file.name.as_str(), file.size))
        .collect::<std::collections::HashMap<_, _>>();
    let mut found = std::collections::HashSet::new();
    for entry in fs::read_dir(path).map_err(|_| "mount_unavailable")? {
        let entry = entry.map_err(|_| "mount_unavailable")?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(expected_size) = expected.get(name.as_str()) else {
            continue;
        };
        let metadata = entry.metadata().map_err(|_| "mount_unavailable")?;
        if !metadata.is_file() || metadata.len() != *expected_size {
            return Err("mount_content_mismatch");
        }
        found.insert(name);
    }
    if found.len() != expected.len() {
        return Err("mount_content_mismatch");
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == 32)
}

fn validate_remote_url(value: &str, server: &str) -> Result<(), &'static str> {
    let url = Url::parse(value).map_err(|_| "invalid_mount_spec")?;
    let server = Url::parse(server).map_err(|_| "invalid_mount_spec")?;
    let local_http = url.scheme() == "http"
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        && server.scheme() == "http";
    if (url.scheme() != "https" && !local_http)
        || url.origin() != server.origin()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err("invalid_mount_spec");
    }
    Ok(())
}

fn validate_guard(
    root: &Path,
    protected_path: &Path,
    spec: &MountSpec,
) -> Result<(), &'static str> {
    validate_guard_values(
        root,
        protected_path,
        &spec.path,
        &spec.guard_path,
        &spec.guard_sha256,
        spec.guard_size,
    )
}

fn validate_guard_values(
    root: &Path,
    protected_path: &Path,
    relative_path: &str,
    relative_guard: &str,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<(), &'static str> {
    let mount = file_tasks::safe_mount_path_parent(root, relative_path)?;
    let guard = file_tasks::safe_path(root, relative_guard, false)?;
    if file_tasks::targets_protected_path(root, &mount, protected_path)
        || file_tasks::targets_protected_path(root, &guard, protected_path)
        || !path_equal(
            mount.parent().ok_or("unsafe_path")?,
            guard.parent().ok_or("unsafe_path")?,
        )
    {
        return Err("protected_path_forbidden");
    }
    let metadata = fs::symlink_metadata(&guard).map_err(|_| "guard_mismatch")?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err("guard_mismatch");
    }
    let (sha, size) = file_tasks::hash_path(&guard)?;
    if sha != expected_sha256 || size != expected_size {
        return Err("guard_mismatch");
    }
    Ok(())
}

fn resolve_registered_mount(
    root: &Path,
    protected_path: &Path,
    entry: &MountRegistryEntry,
    require_mounted: bool,
) -> Result<PathBuf, &'static str> {
    let path = file_tasks::safe_mount_path_parent(root, &entry.path)?;
    if !path_equal(&path, &entry.mount_path)
        || file_tasks::targets_protected_path(root, &path, protected_path)
    {
        return Err("unsafe_path");
    }
    validate_guard_values(
        root,
        protected_path,
        &entry.path,
        &entry.guard_path,
        &entry.guard_sha256,
        entry.guard_size,
    )?;
    if managed_mount_leaf(&path) {
        return Ok(path);
    }
    if require_mounted || !empty_directory_or_missing(&path) {
        return Err("mount_not_owned");
    }
    Ok(path)
}

fn prepare_mountpoint(root: &Path, protected: &Path, spec: &MountSpec) -> Result<(), &'static str> {
    validate_guard(root, protected, spec)?;
    let path = file_tasks::safe_mount_path_parent(root, &spec.path)?;
    let parent = path.parent().ok_or("unsafe_path")?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|_| "unsafe_path")?;
    if !parent_meta.is_dir() || unsafe_link(&parent_meta) {
        return Err("unsafe_path");
    }
    if file_tasks::targets_protected_path(root, &path, protected) {
        return Err("protected_path_forbidden");
    }
    #[cfg(windows)]
    {
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.is_dir()
                || unsafe_link(&metadata)
                || fs::read_dir(&path)
                    .map_err(|_| "mount_not_empty")?
                    .next()
                    .is_some()
            {
                return Err("mount_not_empty");
            }
            fs::remove_dir(&path).map_err(|_| "mount_failed")?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|_| "mount_failed")?;
                fs::symlink_metadata(&path).map_err(|_| "unsafe_path")?
            }
            Err(_) => return Err("unsafe_path"),
        };
        if !metadata.is_dir()
            || unsafe_link(&metadata)
            || fs::read_dir(&path)
                .map_err(|_| "mount_not_empty")?
                .next()
                .is_some()
        {
            return Err("mount_not_empty");
        }
    }
    Ok(())
}

#[cfg(windows)]
fn unsafe_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn unsafe_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn mount_command(binary: &Path, spec: &MountSpec, path: &Path) -> Result<Command, &'static str> {
    let mut command = Command::new(binary);
    command.arg("--config").arg("null");
    #[cfg(target_os = "macos")]
    command.arg("nfsmount");
    #[cfg(windows)]
    command.arg("mount");
    command
        .arg("--http-url")
        .arg(spec.url.as_deref().ok_or("invalid_mount_spec")?)
        .arg("--read-only")
        .arg("--vfs-cache-mode")
        .arg("off")
        .arg("--buffer-size")
        .arg("0")
        .arg("--vfs-read-chunk-size")
        .arg("1M")
        .arg("--vfs-read-chunk-size-limit")
        .arg("1M")
        .arg("--vfs-read-ahead")
        .arg("0")
        .arg("--dir-cache-time")
        .arg("0")
        .arg("--poll-interval")
        .arg("0");
    #[cfg(target_os = "macos")]
    command
        .arg("--nfs-cache-type")
        .arg("memory")
        .arg("--attr-timeout")
        .arg("0");
    command.arg(":http:").arg(path);
    #[cfg(not(any(target_os = "macos", windows)))]
    return Err("mount_unavailable");
    Ok(command)
}

#[cfg(target_os = "macos")]
fn platform_mount_available() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(windows)]
fn platform_mount_available() -> bool {
    let candidates = [
        env::var_os("ProgramFiles(x86)")
            .map(PathBuf::from)
            .map(|path| path.join("WinFsp\\bin\\winfsp-x64.dll")),
        env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .map(|path| path.join("WinFsp\\bin\\winfsp-x64.dll")),
    ];
    candidates.into_iter().flatten().any(|path| path.is_file())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_mount_available() -> bool {
    false
}

fn valid_binary(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    Command::new(path)
        .arg("--version")
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .is_some_and(|line| line == format!("rclone v{RCLONE_VERSION}"))
        })
}

fn identity_fingerprint(spec: &MountSpec) -> String {
    let mut hash = Sha256::new();
    hash.update(spec.mount_id.as_bytes());
    hash.update([0]);
    hash.update(spec.path.as_bytes());
    hash.update([0]);
    hash.update(spec.guard_path.as_bytes());
    hash.update([0]);
    hash.update(spec.guard_sha256.as_bytes());
    hash.update(spec.guard_size.to_be_bytes());
    hash.update(spec.url.as_deref().unwrap_or_default().as_bytes());
    URL_SAFE_NO_PAD.encode(hash.finalize())
}

fn spec_fingerprint(spec: &MountSpec) -> String {
    let mut hash = Sha256::new();
    hash.update(identity_fingerprint(spec).as_bytes());
    hash.update(spec.token.as_deref().unwrap_or_default().as_bytes());
    for file in &spec.files {
        hash.update(file.name.as_bytes());
        hash.update(file.size.to_be_bytes());
        hash.update(file.sha256.as_bytes());
    }
    URL_SAFE_NO_PAD.encode(hash.finalize())
}

fn same_path(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn registry_path(directory: &Path, id: &str) -> PathBuf {
    directory.join(format!("{id}.json"))
}

fn persist_registry(directory: &Path, entry: &MountRegistryEntry) -> Result<(), ()> {
    fs::create_dir_all(directory).map_err(|_| ())?;
    let data = serde_json::to_vec(entry).map_err(|_| ())?;
    let target = registry_path(directory, &entry.mount_id);
    let temp = directory.join(format!(".{}.{}.tmp", entry.mount_id, unix_time()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| ())?;
    file.write_all(&data).map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    fs::rename(&temp, &target).map_err(|_| ())?;
    Ok(())
}

fn remove_registry(directory: &Path, id: &str) {
    let _ = fs::remove_file(registry_path(directory, id));
}

fn unix_time() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn process_start_time(pid: u32) -> Option<u64> {
    let mut system = sysinfo::System::new_all();
    system.refresh_all();
    system
        .process(sysinfo::Pid::from_u32(pid))
        .map(|process| process.start_time())
}

fn registry_process_matches(entry: &MountRegistryEntry) -> bool {
    let mut system = sysinfo::System::new_all();
    system.refresh_all();
    let Some(process) = system.process(sysinfo::Pid::from_u32(entry.pid)) else {
        return false;
    };
    process.start_time() == entry.process_start_time
        && process
            .exe()
            .is_some_and(|path| executable_matches(path, &entry.executable))
        && process
            .cmd()
            .iter()
            .any(|arg| command_mount_path_matches(arg, &entry.mount_path))
}

fn active_process_owned(active: &mut ActiveMount) -> bool {
    if let Some(child) = &mut active.child {
        if child.id() != active.entry.pid || !matches!(child.try_wait(), Ok(None)) {
            return false;
        }
        let mut system = sysinfo::System::new_all();
        system.refresh_all();
        let Some(process) = system.process(sysinfo::Pid::from_u32(active.entry.pid)) else {
            return false;
        };
        process.start_time() == active.entry.process_start_time
            && process
                .exe()
                .is_some_and(|path| executable_matches(path, &active.entry.executable))
    } else {
        registry_process_matches(&active.entry)
    }
}

fn command_mount_path_matches(argument: &std::ffi::OsStr, expected: &Path) -> bool {
    let argument = Path::new(argument);
    if path_equal(argument, expected) {
        return true;
    }
    argument
        .canonicalize()
        .is_ok_and(|canonical| path_equal(&canonical, expected))
}

fn executable_matches(actual: &Path, expected: &Path) -> bool {
    let (Ok(actual), Ok(expected)) = (actual.canonicalize(), expected.canonicalize()) else {
        return false;
    };
    path_equal(&actual, &expected)
}

fn path_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn stop_mount(path: &Path, active: &mut ActiveMount) -> Result<(), &'static str> {
    if !active_process_owned(active) {
        return Err("mount_not_owned");
    }
    unmount_platform(path);
    if let Some(child) = active.child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    } else {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();
        let Some(process) = system.process(sysinfo::Pid::from_u32(active.entry.pid)) else {
            return Ok(());
        };
        if process.start_time() != active.entry.process_start_time
            || !process
                .exe()
                .is_some_and(|path| executable_matches(path, &active.entry.executable))
        {
            return Err("mount_not_owned");
        }
        if !process.kill() {
            return Err("mount_failed");
        }
    }
    let until = SystemTime::now() + Duration::from_secs(5);
    loop {
        if process_start_time(active.entry.pid).is_none() && !unmanaged_mount_present(path) {
            return Ok(());
        }
        if SystemTime::now() >= until {
            return Err("mount_failed");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn terminate_unregistered_mount(path: &Path, child: &mut Child) {
    unmount_platform(path);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "macos")]
fn unmount_platform(path: &Path) {
    let result = Command::new("umount")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !result.is_ok_and(|status| status.success()) {
        let _ = Command::new("umount")
            .arg("-f")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(not(target_os = "macos"))]
fn unmount_platform(_path: &Path) {}

#[cfg(target_os = "macos")]
fn is_mounted(path: &Path) -> bool {
    let expected = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace(' ', "\\040")
        .replace('\t', "\\011");
    Command::new("mount").output().ok().is_some_and(|output| {
        String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            line.rsplit_once(" on ").is_some_and(|(_, mount_path)| {
                mount_path.starts_with(&expected)
                    && mount_path.as_bytes().get(expected.len()) == Some(&b' ')
            })
        })
    })
}

#[cfg(target_os = "macos")]
fn unmanaged_mount_present(path: &Path) -> bool {
    is_mounted(path)
}

#[cfg(target_os = "macos")]
fn mount_ready(path: &Path) -> bool {
    is_mounted(path)
}

#[cfg(windows)]
fn is_mounted(path: &Path) -> bool {
    path.exists() && fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(windows)]
fn unmanaged_mount_present(path: &Path) -> bool {
    path.exists() && !empty_directory(path)
}

#[cfg(windows)]
fn empty_directory(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_dir()
        && !unsafe_link(&metadata)
        && fs::read_dir(path).is_ok_and(|entries| entries.count() == 0)
}

#[cfg(windows)]
fn managed_mount_leaf(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_dir() && unsafe_link(&metadata) && is_mounted(path)
}

#[cfg(target_os = "macos")]
fn managed_mount_leaf(path: &Path) -> bool {
    is_mounted(path)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn managed_mount_leaf(_path: &Path) -> bool {
    false
}

fn empty_directory_or_missing(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Ok(metadata) => {
            metadata.is_dir()
                && !unsafe_link(&metadata)
                && fs::read_dir(path).is_ok_and(|entries| entries.count() == 0)
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
fn mount_ready(path: &Path) -> bool {
    path.exists()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn is_mounted(_path: &Path) -> bool {
    false
}

#[cfg(not(any(target_os = "macos", windows)))]
fn mount_ready(_path: &Path) -> bool {
    false
}
