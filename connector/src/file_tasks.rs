use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const MAX_ITEMS: usize = 64;
const MAX_FILE_BYTES: usize = 512 * 1024;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteFileTaskEntry {
    pub(crate) path: String,
    pub(crate) content_base64: String,
    pub(crate) sha256: Option<String>,
    pub(crate) overwrite: bool,
}

impl Drop for WriteFileTaskEntry {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.content_base64);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTaskResult {
    pub(crate) files: Vec<FileTaskEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTaskEntry {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<&'static str>,
}

struct PlannedWrite<'a> {
    task: &'a WriteFileTaskEntry,
    path: PathBuf,
    bytes: Zeroizing<Vec<u8>>,
}

pub(crate) fn read_files(
    root: &Path,
    paths: &[String],
    max_file_bytes: usize,
    max_total_bytes: usize,
) -> FileTaskResult {
    if paths.len() > MAX_ITEMS {
        return failure(paths, "too_many_files");
    }
    if max_file_bytes == 0
        || max_file_bytes > MAX_FILE_BYTES
        || max_total_bytes == 0
        || max_total_bytes > MAX_TOTAL_BYTES
    {
        return failure(paths, "invalid_limits");
    }

    let mut planned = Vec::with_capacity(paths.len());
    let mut total = 0u64;
    for relative in paths {
        match safe_path(root, relative, false) {
            Ok(path) => match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_file() => {
                    if metadata.len() > max_file_bytes as u64 {
                        planned.push((relative.clone(), None, Some("file_too_large")));
                    } else {
                        total = total.saturating_add(metadata.len());
                        planned.push((relative.clone(), Some(path), None));
                    }
                }
                _ => planned.push((relative.clone(), None, Some("file_unavailable"))),
            },
            Err(error) => planned.push((relative.clone(), None, Some(error))),
        }
    }
    if total > max_total_bytes as u64 {
        return failure(paths, "total_too_large");
    }

    let mut loaded = Vec::with_capacity(planned.len());
    let mut actual_total = 0usize;
    for (relative, path, error) in planned {
        let result = match (path, error) {
            (_, Some(error)) => Err(error),
            (Some(path), None) => read_file(&path, max_file_bytes),
            (None, None) => Err("file_unavailable"),
        };
        if let Ok(bytes) = &result {
            actual_total = actual_total.saturating_add(bytes.len());
            if actual_total > max_total_bytes {
                return failure(paths, "total_too_large");
            }
        }
        loaded.push((relative, result));
    }
    let files = loaded
        .into_iter()
        .map(|(relative, result)| match result {
            Ok(bytes) => FileTaskEntry {
                path: relative,
                size: Some(bytes.len() as u64),
                sha256: Some(crate::sha256_base64url(&bytes)),
                content_base64: Some(URL_SAFE_NO_PAD.encode(&bytes)),
                ok: None,
                error: None,
            },
            Err(error) => failed_entry(relative, error),
        })
        .collect();
    FileTaskResult { files }
}

pub(crate) fn write_files(
    root: &Path,
    session_path: &Path,
    files: &[WriteFileTaskEntry],
) -> FileTaskResult {
    if files.len() > MAX_ITEMS {
        return FileTaskResult {
            files: files
                .iter()
                .take(MAX_ITEMS)
                .map(|item| failed_entry(item.path.clone(), "too_many_files"))
                .collect(),
        };
    }
    let mut planned = Vec::with_capacity(files.len());
    let mut total = 0usize;
    let mut global_error = None;
    let mut seen = std::collections::HashSet::new();
    for task in files {
        let bytes = match URL_SAFE_NO_PAD.decode(&task.content_base64) {
            Ok(bytes) if bytes.len() <= MAX_FILE_BYTES => Zeroizing::new(bytes),
            Ok(_) => {
                global_error = Some("file_too_large");
                planned.push(Err((task.path.clone(), "file_too_large")));
                continue;
            }
            Err(_) => {
                global_error = Some("invalid_base64");
                planned.push(Err((task.path.clone(), "invalid_base64")));
                continue;
            }
        };
        total = total.saturating_add(bytes.len());
        let path = match safe_path(root, &task.path, true) {
            Ok(path) => path,
            Err(error) => {
                global_error = Some(error);
                planned.push(Err((task.path.clone(), error)));
                continue;
            }
        };
        if !seen.insert(path.clone()) {
            global_error = Some("duplicate_path");
        }
        if task
            .sha256
            .as_deref()
            .is_some_and(|expected| expected != crate::sha256_base64url(&bytes))
        {
            global_error = Some("hash_mismatch");
        }
        if path.exists() && !task.overwrite {
            global_error = Some("file_exists");
        }
        if targets_session(root, &path, session_path) {
            global_error = Some("session_path_forbidden");
        }
        planned.push(Ok(PlannedWrite { task, path, bytes }));
    }
    if total > MAX_TOTAL_BYTES {
        global_error = Some("total_too_large");
    }
    if let Some(error) = global_error {
        return FileTaskResult {
            files: planned
                .into_iter()
                .map(|item| match item {
                    Ok(plan) => failed_entry(
                        plan.task.path.clone(),
                        plan.task
                            .sha256
                            .as_deref()
                            .filter(|expected| *expected != crate::sha256_base64url(&plan.bytes))
                            .map(|_| "hash_mismatch")
                            .unwrap_or(error),
                    ),
                    Err((path, item_error)) => failed_entry(path, item_error),
                })
                .collect(),
        };
    }

    let mut results = Vec::with_capacity(planned.len());
    for item in planned {
        match item {
            Err((path, error)) => results.push(failed_entry(path, error)),
            Ok(plan) => match atomic_write(
                root,
                &plan.task.path,
                &plan.path,
                &plan.bytes,
                plan.task.overwrite,
            ) {
                Ok(()) => results.push(FileTaskEntry {
                    path: plan.task.path.clone(),
                    size: Some(plan.bytes.len() as u64),
                    sha256: Some(crate::sha256_base64url(&plan.bytes)),
                    content_base64: None,
                    ok: Some(true),
                    error: None,
                }),
                Err(_) => results.push(FileTaskEntry {
                    path: plan.task.path.clone(),
                    size: None,
                    sha256: None,
                    content_base64: None,
                    ok: Some(false),
                    error: Some("write_failed"),
                }),
            },
        }
    }
    FileTaskResult { files: results }
}

fn safe_path(root: &Path, relative: &str, allow_missing: bool) -> Result<PathBuf, &'static str> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative.chars().any(char::is_control)
    {
        return Err("unsafe_path");
    }
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative_path.components().any(|part| {
            let Component::Normal(component) = part else {
                return true;
            };
            !safe_windows_component(&component.to_string_lossy())
        })
    {
        return Err("unsafe_path");
    }
    let root = root.canonicalize().map_err(|_| "file_unavailable")?;
    let path = root.join(relative_path);
    let mut current = root.clone();
    let components = relative_path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_unsafe_link(&metadata) => return Err("unsafe_path"),
            Ok(metadata) if index + 1 < components.len() && !metadata.is_dir() => {
                return Err("unsafe_path")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => break,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && index + 1 < components.len() =>
            {
                break
            }
            Err(_) => return Err("file_unavailable"),
        }
    }
    Ok(path)
}

fn safe_windows_component(value: &str) -> bool {
    if value.is_empty()
        || value.ends_with(['.', ' '])
        || value.contains(':')
        || value.chars().any(char::is_control)
    {
        return false;
    }
    let basename = value
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    !matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(basename.len() == 4
            && (basename.starts_with("COM") || basename.starts_with("LPT"))
            && matches!(basename.as_bytes()[3], b'1'..=b'9'))
}

fn is_unsafe_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    false
}

fn targets_session(root: &Path, path: &Path, session_path: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return true;
    };
    let Ok(session) = session_path.canonicalize() else {
        return true;
    };
    let Ok(relative) = path.strip_prefix(&root) else {
        return true;
    };
    let target = root.join(relative);
    if same_file_identity(&target, &session) {
        return true;
    }
    #[cfg(windows)]
    {
        target
            .to_string_lossy()
            .eq_ignore_ascii_case(&session.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        target == session
    }
}

#[cfg(unix)]
fn same_file_identity(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(windows)]
fn same_file_identity(left: &Path, right: &Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    fn identity(path: &Path) -> Option<(u32, u64)> {
        let file = File::open(path).ok()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
        (success != 0).then_some((
            info.dwVolumeSerialNumber,
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }

    matches!((identity(left), identity(right)), (Some(left), Some(right)) if left == right)
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_left: &Path, _right: &Path) -> bool {
    false
}

fn read_file(path: &Path, max_bytes: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "file_unavailable")?;
    if !metadata.is_file() || is_unsafe_link(&metadata) {
        return Err("unsafe_path");
    }
    if metadata.len() > max_bytes as u64 {
        return Err("file_too_large");
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(metadata.len() as usize));
    File::open(path)
        .and_then(|file| {
            file.take(max_bytes.saturating_add(1) as u64)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| "read_failed")?;
    if bytes.len() > max_bytes {
        return Err("file_too_large");
    }
    Ok(bytes)
}

fn atomic_write(
    root: &Path,
    relative: &str,
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let validated = safe_path(root, relative, true).map_err(str::to_string)?;
    if validated != path {
        return Err("path changed during write".to_string());
    }
    let suffix = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(
        ".boxy-{}-{timestamp}-{suffix}.tmp",
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};
        let source = temporary
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let destination = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let flags = if overwrite {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
            let _ = fs::remove_file(&temporary);
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(not(windows))]
    {
        let result = if overwrite {
            fs::rename(&temporary, path)
        } else {
            fs::hard_link(&temporary, path).and_then(|_| fs::remove_file(&temporary))
        };
        result.map_err(|error| {
            let _ = fs::remove_file(&temporary);
            error.to_string()
        })?;
    }
    Ok(())
}

fn failure(paths: &[String], error: &'static str) -> FileTaskResult {
    FileTaskResult {
        files: paths
            .iter()
            .take(MAX_ITEMS)
            .map(|path| failed_entry(path.clone(), error))
            .collect(),
    }
}

fn failed_entry(path: String, error: &'static str) -> FileTaskEntry {
    FileTaskEntry {
        path,
        size: None,
        sha256: None,
        content_base64: None,
        ok: Some(false),
        error: Some(error),
    }
}

#[cfg(test)]
#[path = "../../test/file_tasks.rs"]
mod tests;
