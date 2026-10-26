use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const MAX_ITEMS: usize = 64;
const MAX_FILE_BYTES: usize = 512 * 1024;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_STREAM_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 100_000;
const MAX_MANIFEST_PATHS: usize = 16;
const MAX_MANIFEST_FILES: usize = 1024;
const MAX_MANIFEST_ENTRIES: usize = 4096;
const MAX_MANIFEST_DEPTH: usize = 8;
const MAX_MANIFEST_PATH_BYTES: usize = 1024 * 1024;
const MAX_RANGE_ITEMS: usize = 32;
const MAX_RANGE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RANGE_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PATCH_FILES: usize = 32;
const MAX_PATCH_SEGMENTS: usize = 64;
const MAX_PATCH_TOTAL_BYTES: usize = 2 * 1024 * 1024;
const FILE_IO_BUFFER_BYTES: usize = 64 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RangeReadTaskEntry {
    pub(crate) path: String,
    pub(crate) offset: u64,
    pub(crate) length: u64,
    pub(crate) size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RangeReadResult {
    pub(crate) files: Vec<RangeReadResultEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) failed: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RangeReadResultEntry {
    pub(crate) path: String,
    pub(crate) offset: u64,
    pub(crate) length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content_base64: Option<String>,
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<&'static str>,
}

impl Drop for RangeReadResultEntry {
    fn drop(&mut self) {
        if let Some(content) = &mut self.content_base64 {
            zeroize::Zeroize::zeroize(content);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePatchTaskEntry {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) segments: Vec<FilePatchSegment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePatchSegment {
    pub(crate) offset: u64,
    pub(crate) before_base64: String,
    pub(crate) content_base64: String,
}

impl Drop for FilePatchTaskEntry {
    fn drop(&mut self) {
        for segment in &mut self.segments {
            zeroize::Zeroize::zeroize(&mut segment.before_base64);
            zeroize::Zeroize::zeroize(&mut segment.content_base64);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePatchResult {
    pub(crate) files: Vec<FilePatchResultEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) failed: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePatchResultEntry {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sha256: Option<String>,
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) already_applied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<&'static str>,
}

struct DecodedPatchSegment {
    offset: u64,
    before: Zeroizing<Vec<u8>>,
    content: Zeroizing<Vec<u8>>,
}

struct DecodedPatch {
    path: String,
    size: u64,
    sha256: String,
    segments: Vec<DecodedPatchSegment>,
}

struct FileStamp {
    size: u64,
    modified: SystemTime,
    permissions: fs::Permissions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteFileTaskEntry {
    pub(crate) path: String,
    pub(crate) content_base64: String,
    pub(crate) sha256: Option<String>,
    pub(crate) overwrite: bool,
    #[serde(default)]
    pub(crate) expected_existing_sha256: Option<String>,
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

pub(crate) fn read_ranges(
    root: &Path,
    protected_path: &Path,
    ranges: &[RangeReadTaskEntry],
    should_stop: impl Fn() -> bool,
) -> RangeReadResult {
    if ranges.len() > MAX_RANGE_ITEMS {
        return RangeReadResult {
            files: ranges
                .iter()
                .take(MAX_RANGE_ITEMS)
                .map(|range| failed_range_entry(range, "too_many_ranges"))
                .collect(),
            failed: Some(true),
        };
    }
    let total = ranges
        .iter()
        .try_fold(0u64, |total, range| total.checked_add(range.length));
    if total.is_none_or(|total| total > MAX_RANGE_TOTAL_BYTES) {
        return RangeReadResult {
            files: ranges
                .iter()
                .map(|range| failed_range_entry(range, "total_too_large"))
                .collect(),
            failed: Some(true),
        };
    }

    let files = ranges
        .iter()
        .map(|range| read_one_range(root, protected_path, range, &should_stop))
        .collect::<Vec<_>>();
    let failed = files.iter().any(|file| !file.ok).then_some(true);
    RangeReadResult { files, failed }
}

fn failed_range_entry(range: &RangeReadTaskEntry, error: &'static str) -> RangeReadResultEntry {
    RangeReadResultEntry {
        path: range.path.clone(),
        offset: range.offset,
        length: range.length,
        size: None,
        sha256: None,
        content_base64: None,
        ok: false,
        error: Some(error),
    }
}

fn read_one_range(
    root: &Path,
    protected_path: &Path,
    range: &RangeReadTaskEntry,
    should_stop: &impl Fn() -> bool,
) -> RangeReadResultEntry {
    let result = (|| {
        if range.length == 0 {
            return Err("invalid_range");
        }
        if range.length > MAX_RANGE_BYTES {
            return Err("range_too_large");
        }
        if range.size > MAX_STREAM_BYTES {
            return Err("file_too_large");
        }
        let end = range
            .offset
            .checked_add(range.length)
            .ok_or("invalid_range")?;
        if end > range.size {
            return Err("invalid_range");
        }
        let (path, mut file, stamp) =
            open_guarded_file(root, protected_path, &range.path, Some(range.size))?;
        file.seek(std::io::SeekFrom::Start(range.offset))
            .map_err(|_| "read_failed")?;
        let mut bytes = Zeroizing::new(vec![0; range.length as usize]);
        read_exact_cancellable(&mut file, &mut bytes, should_stop)?;
        ensure_file_unchanged(root, protected_path, &range.path, &path, &file, &stamp)?;
        let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes[..]));
        Ok((stamp.size, digest, URL_SAFE_NO_PAD.encode(&bytes[..])))
    })();
    match result {
        Ok((size, sha256, content_base64)) => RangeReadResultEntry {
            path: range.path.clone(),
            offset: range.offset,
            length: range.length,
            size: Some(size),
            sha256: Some(sha256),
            content_base64: Some(content_base64),
            ok: true,
            error: None,
        },
        Err(error) => failed_range_entry(range, error),
    }
}

fn read_exact_cancellable(
    source: &mut File,
    destination: &mut [u8],
    should_stop: &impl Fn() -> bool,
) -> Result<(), &'static str> {
    let mut offset = 0usize;
    while offset < destination.len() {
        if should_stop() {
            return Err("cancelled");
        }
        let end = (offset + FILE_IO_BUFFER_BYTES).min(destination.len());
        let count = source
            .read(&mut destination[offset..end])
            .map_err(|_| "read_failed")?;
        if count == 0 {
            return Err("file_changed");
        }
        offset += count;
    }
    Ok(())
}

pub(crate) fn patch_files(
    root: &Path,
    protected_path: &Path,
    patches: &[FilePatchTaskEntry],
    should_stop: impl Fn() -> bool,
) -> FilePatchResult {
    if patches.len() > MAX_PATCH_FILES {
        return FilePatchResult {
            files: patches
                .iter()
                .take(MAX_PATCH_FILES)
                .map(|patch| failed_patch_entry(&patch.path, "too_many_files"))
                .collect(),
            failed: Some(true),
        };
    }

    let mut path_counts = std::collections::HashMap::new();
    for patch in patches {
        *path_counts.entry(path_key(&patch.path)).or_insert(0usize) += 1;
    }
    let mut decoded = Vec::with_capacity(patches.len());
    let mut total_patch_bytes = 0usize;
    for patch in patches {
        let budget = MAX_PATCH_TOTAL_BYTES.saturating_sub(total_patch_bytes);
        let result = decode_patch(patch, budget);
        if let Ok((_, decoded_bytes)) = &result {
            total_patch_bytes = total_patch_bytes.saturating_add(*decoded_bytes);
        }
        if result
            .as_ref()
            .is_err_and(|error| *error == "patch_too_large")
        {
            return FilePatchResult {
                files: patches
                    .iter()
                    .map(|patch| failed_patch_entry(&patch.path, "patch_too_large"))
                    .collect(),
                failed: Some(true),
            };
        }
        decoded.push(result);
    }
    if total_patch_bytes > MAX_PATCH_TOTAL_BYTES {
        return FilePatchResult {
            files: patches
                .iter()
                .map(|patch| failed_patch_entry(&patch.path, "patch_too_large"))
                .collect(),
            failed: Some(true),
        };
    }

    let files = patches
        .iter()
        .zip(decoded)
        .map(|(input, patch)| {
            if path_counts
                .get(&path_key(&input.path))
                .copied()
                .unwrap_or_default()
                > 1
            {
                return failed_patch_entry(&input.path, "duplicate_path");
            }
            let patch = match patch {
                Ok((patch, _)) => patch,
                Err(error) => return failed_patch_entry(&input.path, error),
            };
            match apply_file_patch(root, protected_path, &patch, &should_stop) {
                Ok((sha256, already_applied)) => FilePatchResultEntry {
                    path: input.path.clone(),
                    size: Some(input.size),
                    sha256: Some(sha256),
                    ok: true,
                    already_applied: Some(already_applied),
                    error: None,
                },
                Err(error) => failed_patch_entry(&input.path, error),
            }
        })
        .collect::<Vec<_>>();
    let failed = files.iter().any(|file| !file.ok).then_some(true);
    FilePatchResult { files, failed }
}

fn failed_patch_entry(path: &str, error: &'static str) -> FilePatchResultEntry {
    FilePatchResultEntry {
        path: path.to_string(),
        size: None,
        sha256: None,
        ok: false,
        already_applied: None,
        error: Some(error),
    }
}

fn decode_patch(
    patch: &FilePatchTaskEntry,
    remaining_budget: usize,
) -> Result<(DecodedPatch, usize), &'static str> {
    if patch.size > MAX_STREAM_BYTES {
        return Err("file_too_large");
    }
    if !valid_sha256(&patch.sha256) {
        return Err("invalid_sha256");
    }
    if patch.segments.is_empty() {
        return Err("invalid_patch");
    }
    if patch.segments.len() > MAX_PATCH_SEGMENTS {
        return Err("too_many_segments");
    }

    let mut segments = Vec::with_capacity(patch.segments.len());
    let mut encoded_total = 0usize;
    let mut decoded_total = 0usize;
    for segment in &patch.segments {
        encoded_total = encoded_total
            .saturating_add(segment.before_base64.len())
            .saturating_add(segment.content_base64.len());
        let max_encoded = remaining_budget
            .saturating_mul(4)
            .div_ceil(3)
            .saturating_add(4);
        if remaining_budget == 0 || encoded_total > max_encoded {
            return Err("patch_too_large");
        }
        let before = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(&segment.before_base64)
                .map_err(|_| "invalid_base64")?,
        );
        let content = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(&segment.content_base64)
                .map_err(|_| "invalid_base64")?,
        );
        if before.is_empty() || before.len() != content.len() {
            return Err("invalid_segment");
        }
        decoded_total = decoded_total
            .saturating_add(before.len())
            .saturating_add(content.len());
        if decoded_total > remaining_budget {
            return Err("patch_too_large");
        }
        let length = before.len() as u64;
        let end = segment
            .offset
            .checked_add(length)
            .ok_or("invalid_segment")?;
        if end > patch.size {
            return Err("invalid_segment");
        }
        segments.push(DecodedPatchSegment {
            offset: segment.offset,
            before,
            content,
        });
    }
    segments.sort_by_key(|segment| segment.offset);
    let mut previous_end = 0u64;
    for segment in &segments {
        if segment.offset < previous_end {
            return Err("overlapping_segments");
        }
        previous_end = segment
            .offset
            .checked_add(segment.before.len() as u64)
            .ok_or("invalid_segment")?;
    }
    Ok((
        DecodedPatch {
            path: patch.path.clone(),
            size: patch.size,
            sha256: patch.sha256.clone(),
            segments,
        },
        decoded_total,
    ))
}

fn valid_sha256(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|digest| digest.len() == 32)
}

fn apply_file_patch(
    root: &Path,
    protected_path: &Path,
    patch: &DecodedPatch,
    should_stop: &impl Fn() -> bool,
) -> Result<(String, bool), &'static str> {
    let (path, mut source, stamp) =
        open_guarded_file(root, protected_path, &patch.path, Some(patch.size))?;
    let mut sink = std::io::sink();
    let (original_sha256, result_sha256, all_after) =
        stream_patch(&mut source, patch, &mut sink, should_stop)?;
    if original_sha256 != patch.sha256 {
        return Err("original_hash_mismatch");
    }
    ensure_file_unchanged(root, protected_path, &patch.path, &path, &source, &stamp)?;
    if should_stop() {
        return Err("cancelled");
    }
    if all_after {
        return Ok((result_sha256, true));
    }

    source.rewind().map_err(|_| "read_failed")?;
    let parent = path.parent().ok_or("unsafe_path")?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".boxy-patch-")
        .tempfile_in(parent)
        .map_err(|_| "write_failed")?;
    let (original_sha256, result_sha256, all_after) =
        stream_patch(&mut source, patch, temporary.as_file_mut(), should_stop)?;
    if original_sha256 != patch.sha256 {
        return Err("original_hash_mismatch");
    }
    ensure_file_unchanged(root, protected_path, &patch.path, &path, &source, &stamp)?;
    if should_stop() {
        return Err("cancelled");
    }
    if all_after {
        return Ok((result_sha256, true));
    }
    temporary
        .as_file()
        .set_permissions(stamp.permissions.clone())
        .map_err(|_| "write_failed")?;
    temporary.as_file().sync_all().map_err(|_| "write_failed")?;
    ensure_file_unchanged(root, protected_path, &patch.path, &path, &source, &stamp)?;
    if should_stop() {
        return Err("cancelled");
    }
    drop(source);
    temporary.persist(&path).map_err(|_| "write_failed")?;
    Ok((result_sha256, false))
}

fn stream_patch(
    source: &mut File,
    patch: &DecodedPatch,
    destination: &mut impl Write,
    should_stop: &impl Fn() -> bool,
) -> Result<(String, String, bool), &'static str> {
    let mut original_hash = Sha256::new();
    let mut result_hash = Sha256::new();
    let mut current_offset = 0u64;
    let mut all_after = true;
    for segment in &patch.segments {
        if should_stop() {
            return Err("cancelled");
        }
        let prefix_length = segment.offset - current_offset;
        copy_unchanged_bytes(
            source,
            destination,
            prefix_length,
            &mut original_hash,
            &mut result_hash,
            should_stop,
        )?;
        current_offset = segment.offset;

        let observed = read_patch_segment(source, segment.before.len(), should_stop)?;
        if observed.as_slice() == segment.before.as_slice() {
            if segment.before.as_slice() != segment.content.as_slice() {
                all_after = false;
            }
        } else if observed.as_slice() != segment.content.as_slice() {
            return Err("content_mismatch");
        }
        original_hash.update(&segment.before[..]);
        result_hash.update(&segment.content[..]);
        destination
            .write_all(&segment.content)
            .map_err(|_| "write_failed")?;
        current_offset = current_offset
            .checked_add(segment.content.len() as u64)
            .ok_or("invalid_segment")?;
    }
    copy_unchanged_bytes(
        source,
        destination,
        patch.size - current_offset,
        &mut original_hash,
        &mut result_hash,
        should_stop,
    )?;
    Ok((
        URL_SAFE_NO_PAD.encode(original_hash.finalize()),
        URL_SAFE_NO_PAD.encode(result_hash.finalize()),
        all_after,
    ))
}

fn copy_unchanged_bytes(
    source: &mut File,
    destination: &mut impl Write,
    length: u64,
    original_hash: &mut Sha256,
    result_hash: &mut Sha256,
    should_stop: &impl Fn() -> bool,
) -> Result<(), &'static str> {
    let mut remaining = length;
    let mut buffer = Zeroizing::new([0u8; FILE_IO_BUFFER_BYTES]);
    while remaining > 0 {
        if should_stop() {
            return Err("cancelled");
        }
        let maximum =
            usize::try_from(remaining.min(buffer.len() as u64)).map_err(|_| "file_too_large")?;
        let count = source
            .read(&mut buffer[..maximum])
            .map_err(|_| "read_failed")?;
        if count == 0 {
            return Err("file_changed");
        }
        original_hash.update(&buffer[..count]);
        result_hash.update(&buffer[..count]);
        destination
            .write_all(&buffer[..count])
            .map_err(|_| "write_failed")?;
        remaining -= count as u64;
    }
    Ok(())
}

fn read_patch_segment(
    source: &mut File,
    length: usize,
    should_stop: &impl Fn() -> bool,
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let mut bytes = Zeroizing::new(vec![0u8; length]);
    let mut offset = 0usize;
    while offset < length {
        if should_stop() {
            return Err("cancelled");
        }
        let end = (offset + FILE_IO_BUFFER_BYTES).min(length);
        let count = source
            .read(&mut bytes[offset..end])
            .map_err(|_| "read_failed")?;
        if count == 0 {
            return Err("file_changed");
        }
        offset += count;
    }
    Ok(bytes)
}

fn open_guarded_file(
    root: &Path,
    protected_path: &Path,
    relative: &str,
    expected_size: Option<u64>,
) -> Result<(PathBuf, File, FileStamp), &'static str> {
    let path = safe_path(root, relative, false)?;
    if conflicts_with_protected(root, &path, protected_path) {
        return Err("protected_path_forbidden");
    }
    let path_metadata = fs::symlink_metadata(&path).map_err(|_| "file_unavailable")?;
    if !path_metadata.is_file() || is_unsafe_link(&path_metadata) {
        return Err("unsafe_path");
    }
    if path_metadata.len() > MAX_STREAM_BYTES {
        return Err("file_too_large");
    }
    if expected_size.is_some_and(|size| path_metadata.len() != size) {
        return Err("size_mismatch");
    }
    let file = File::open(&path).map_err(|_| "file_unavailable")?;
    let path_metadata = fs::symlink_metadata(&path).map_err(|_| "file_unavailable")?;
    if !path_metadata.is_file() || is_unsafe_link(&path_metadata) {
        return Err("unsafe_path");
    }
    if conflicts_with_protected(root, &path, protected_path) {
        return Err("protected_path_forbidden");
    }
    if !same_file_handle_as_path(&file, &path) {
        return Err("file_changed");
    }
    let metadata = file.metadata().map_err(|_| "file_unavailable")?;
    let stamp = FileStamp {
        size: metadata.len(),
        modified: metadata.modified().map_err(|_| "file_unavailable")?,
        permissions: metadata.permissions(),
    };
    if stamp.size > MAX_STREAM_BYTES || expected_size.is_some_and(|size| stamp.size != size) {
        return Err("size_mismatch");
    }
    if stamp.size != path_metadata.len()
        || stamp.modified != path_metadata.modified().map_err(|_| "file_unavailable")?
    {
        return Err("file_changed");
    }
    Ok((path, file, stamp))
}

fn ensure_file_unchanged(
    root: &Path,
    protected_path: &Path,
    relative: &str,
    path: &Path,
    file: &File,
    stamp: &FileStamp,
) -> Result<(), &'static str> {
    let current = safe_path(root, relative, false)?;
    if current != path || conflicts_with_protected(root, &current, protected_path) {
        return Err("protected_path_forbidden");
    }
    let path_metadata = fs::symlink_metadata(&current).map_err(|_| "file_changed")?;
    if !path_metadata.is_file() || is_unsafe_link(&path_metadata) {
        return Err("unsafe_path");
    }
    let handle_metadata = file.metadata().map_err(|_| "file_changed")?;
    let path_modified = path_metadata.modified().map_err(|_| "file_changed")?;
    let handle_modified = handle_metadata.modified().map_err(|_| "file_changed")?;
    if path_metadata.len() != stamp.size
        || handle_metadata.len() != stamp.size
        || path_modified != stamp.modified
        || handle_modified != stamp.modified
        || !same_file_handle_as_path(file, &current)
    {
        return Err("file_changed");
    }
    Ok(())
}

fn same_file_handle_as_path(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (Ok(opened), Ok(current)) = (file.metadata(), fs::metadata(path)) else {
            return false;
        };
        opened.dev() == current.dev() && opened.ino() == current.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut opened = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut opened) } == 0 {
            return false;
        }
        let Ok(current_file) = File::open(path) else {
            return false;
        };
        let mut current = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(current_file.as_raw_handle(), &mut current) } == 0 {
            return false;
        }
        opened.dwVolumeSerialNumber == current.dwVolumeSerialNumber
            && opened.nFileIndexHigh == current.nFileIndexHigh
            && opened.nFileIndexLow == current.nFileIndexLow
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, path);
        false
    }
}

pub(crate) fn write_files(
    root: &Path,
    protected_path: &Path,
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
        let protected = targets_protected(root, &path, protected_path);
        let expected_existing = task.expected_existing_sha256.as_deref();
        if protected && expected_existing.is_none() {
            global_error = Some("protected_path_forbidden");
        }
        if let Some(expected) = expected_existing {
            if !valid_sha256(expected) || !task.overwrite {
                global_error = Some("invalid_expected_hash");
            } else if hash_path(&path).map_or(true, |(actual, _)| actual != expected) {
                global_error = Some("source_changed");
            }
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
                plan.task.expected_existing_sha256.as_deref(),
            ) {
                Ok(()) => results.push(FileTaskEntry {
                    path: plan.task.path.clone(),
                    size: Some(plan.bytes.len() as u64),
                    sha256: Some(crate::sha256_base64url(&plan.bytes)),
                    content_base64: None,
                    ok: Some(true),
                    error: None,
                }),
                Err(error) => results.push(FileTaskEntry {
                    path: plan.task.path.clone(),
                    size: None,
                    sha256: None,
                    content_base64: None,
                    ok: Some(false),
                    error: Some(if error == "source_changed" {
                        "source_changed"
                    } else {
                        "write_failed"
                    }),
                }),
            },
        }
    }
    FileTaskResult { files: results }
}

pub(crate) fn safe_path(
    root: &Path,
    relative: &str,
    allow_missing: bool,
) -> Result<PathBuf, &'static str> {
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

pub(crate) fn safe_mount_path_parent(root: &Path, relative: &str) -> Result<PathBuf, &'static str> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative.chars().any(char::is_control)
    {
        return Err("unsafe_path");
    }
    let relative_path = Path::new(relative);
    let components = relative_path.components().collect::<Vec<_>>();
    if components.is_empty()
        || components.iter().any(|part| {
            let Component::Normal(component) = part else {
                return true;
            };
            !safe_windows_component(&component.to_string_lossy())
        })
    {
        return Err("unsafe_path");
    }
    let parent_components = components[..components.len() - 1]
        .iter()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let parent = if parent_components.is_empty() {
        root.canonicalize().map_err(|_| "file_unavailable")?
    } else {
        safe_path(root, &parent_components.join("/"), false)?
    };
    let metadata = fs::symlink_metadata(&parent).map_err(|_| "file_unavailable")?;
    if !metadata.is_dir() || is_unsafe_link(&metadata) {
        return Err("unsafe_path");
    }
    Ok(parent.join(components.last().unwrap().as_os_str()))
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

fn targets_protected(root: &Path, path: &Path, protected_path: &Path) -> bool {
    if !protected_path.exists() {
        return false;
    }
    let Ok(root) = root.canonicalize() else {
        return true;
    };
    let Ok(protected) = protected_path.canonicalize() else {
        return true;
    };
    let Ok(relative) = path.strip_prefix(&root) else {
        return true;
    };
    let target = root.join(relative);
    if same_file_identity(&target, &protected) {
        return true;
    }
    #[cfg(windows)]
    {
        target
            .to_string_lossy()
            .eq_ignore_ascii_case(&protected.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        target == protected
    }
}

pub(crate) fn targets_protected_path(root: &Path, path: &Path, protected_path: &Path) -> bool {
    conflicts_with_protected(root, path, protected_path)
}

fn conflicts_with_protected(root: &Path, path: &Path, protected_path: &Path) -> bool {
    if !protected_path.exists() {
        return false;
    }
    if targets_protected(root, path, protected_path) {
        return true;
    }
    let (Ok(root), Ok(protected)) = (root.canonicalize(), protected_path.canonicalize()) else {
        return true;
    };
    let Ok(relative) = path.strip_prefix(&root) else {
        return true;
    };
    let target = root.join(relative);
    #[cfg(windows)]
    {
        let target = target.to_string_lossy().to_ascii_lowercase();
        let protected = protected.to_string_lossy().to_ascii_lowercase();
        target.starts_with(&format!("{}\\", protected))
            || protected.starts_with(&format!("{}\\", target))
    }
    #[cfg(not(windows))]
    {
        target.starts_with(&protected) || protected.starts_with(&target)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveEntry {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) sha256: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) guard_path: Option<String>,
    pub(crate) guard_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemovalEntry {
    pub(crate) path: String,
    pub(crate) sha256: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) guard_path: Option<String>,
    pub(crate) guard_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationResult {
    pub(crate) files: Vec<OperationEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) failed: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationEntry {
    pub(crate) path: String,
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestResult {
    pub(crate) files: Vec<ManifestEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestEntry {
    pub(crate) path: String,
    pub(crate) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<&'static str>,
}

pub(crate) fn list_files(
    root: &Path,
    paths: &[String],
    skipped_paths: &[PathBuf],
) -> ManifestResult {
    let mut state = ManifestState {
        files: Vec::new(),
        path_bytes: 0,
        file_count: 0,
        entry_count: 0,
        stopped: false,
    };
    if paths.len() > MAX_MANIFEST_PATHS {
        for path in paths.iter().take(MAX_MANIFEST_PATHS) {
            push_manifest_error(&mut state, path, "too_many_paths");
            if state.stopped {
                break;
            }
        }
        return ManifestResult { files: state.files };
    }
    for path in paths {
        collect_manifest_path(root, path, 0, skipped_paths, &mut state);
        if state.stopped {
            break;
        }
    }
    ManifestResult { files: state.files }
}

struct ManifestState {
    files: Vec<ManifestEntry>,
    path_bytes: usize,
    file_count: usize,
    entry_count: usize,
    stopped: bool,
}

fn collect_manifest_path(
    root: &Path,
    relative: &str,
    depth: usize,
    skipped_paths: &[PathBuf],
    state: &mut ManifestState,
) {
    if state.stopped {
        return;
    }
    state.entry_count += 1;
    if state.entry_count > MAX_MANIFEST_ENTRIES {
        push_manifest_error(state, relative, "tree_too_large");
        state.stopped = true;
        return;
    }
    let path = match safe_path(root, relative, false) {
        Ok(path) => path,
        Err(error) => {
            push_manifest_error(state, relative, error);
            return;
        }
    };
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => {
            push_manifest_error(state, relative, "file_unavailable");
            return;
        }
    };
    if skipped_paths
        .iter()
        .any(|skipped| same_or_under(&path, skipped))
    {
        return;
    }
    if is_unsafe_link(&metadata) {
        if add_manifest_path_bytes(state, relative).is_ok() {
            state.files.push(ManifestEntry {
                path: relative.to_string(),
                kind: "symlink",
                size: None,
                sha256: None,
                error: Some("unsafe_path"),
            });
        }
    } else if metadata.is_dir() {
        if depth >= MAX_MANIFEST_DEPTH {
            push_manifest_error(state, relative, "tree_too_deep");
            return;
        }
        if add_manifest_path_bytes(state, relative).is_ok() {
            state.files.push(ManifestEntry {
                path: relative.to_string(),
                kind: "directory",
                size: Some(metadata.len()),
                sha256: None,
                error: None,
            });
        }
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => {
                push_manifest_error(state, relative, "file_unavailable");
                return;
            }
        };
        let mut children = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    if children.len() >= MAX_MANIFEST_ENTRIES {
                        push_manifest_error(state, relative, "tree_too_large");
                        state.stopped = true;
                        return;
                    }
                    children.push(entry.file_name());
                }
                Err(_) => {
                    push_manifest_error(state, relative, "file_unavailable");
                    return;
                }
            }
        }
        children.sort();
        for name in children {
            let child = format!("{relative}/{}", name.to_string_lossy());
            collect_manifest_path(root, &child, depth + 1, skipped_paths, state);
            if state.stopped {
                break;
            }
        }
    } else if metadata.is_file() {
        if state.file_count >= MAX_MANIFEST_FILES {
            push_manifest_error(state, relative, "too_many_files");
            state.stopped = true;
            return;
        }
        match hash_path(&path) {
            Ok((sha256, size)) => {
                if add_manifest_path_bytes(state, relative).is_err() {
                    return;
                }
                state.file_count += 1;
                state.files.push(ManifestEntry {
                    path: relative.to_string(),
                    kind: "file",
                    size: Some(size),
                    sha256: Some(sha256),
                    error: None,
                });
            }
            Err(error) => push_manifest_error(state, relative, error),
        }
    } else {
        push_manifest_error(state, relative, "unsafe_path");
    }
}

fn same_or_under(path: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = path.to_string_lossy().to_ascii_lowercase();
        let parent = parent.to_string_lossy().to_ascii_lowercase();
        path == parent || path.starts_with(&format!("{}\\", parent.trim_end_matches('\\')))
    }
    #[cfg(not(windows))]
    {
        path == parent || path.starts_with(parent)
    }
}

fn add_manifest_path_bytes(state: &mut ManifestState, relative: &str) -> Result<(), ()> {
    if state.stopped {
        return Err(());
    }
    if state.path_bytes.saturating_add(relative.len()) > MAX_MANIFEST_PATH_BYTES {
        state.files.push(ManifestEntry {
            path: String::new(),
            kind: "unknown",
            size: None,
            sha256: None,
            error: Some("path_output_too_large"),
        });
        state.stopped = true;
        return Err(());
    }
    state.path_bytes += relative.len();
    Ok(())
}

fn push_manifest_error(state: &mut ManifestState, relative: &str, error: &'static str) {
    if state.stopped {
        return;
    }
    if add_manifest_path_bytes(state, relative).is_err() {
        return;
    }
    state.files.push(ManifestEntry {
        path: relative.to_string(),
        kind: "unknown",
        size: None,
        sha256: None,
        error: Some(error),
    });
}

pub(crate) fn verify_stream_file(
    root: &Path,
    protected_path: &Path,
    relative: &str,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<(PathBuf, File), &'static str> {
    if expected_size > MAX_STREAM_BYTES {
        return Err("file_too_large");
    }
    let path = safe_path(root, relative, false)?;
    if conflicts_with_protected(root, &path, protected_path) {
        return Err("protected_path_forbidden");
    }
    let metadata = fs::symlink_metadata(&path).map_err(|_| "file_unavailable")?;
    if !metadata.is_file() || is_unsafe_link(&metadata) {
        return Err("unsafe_path");
    }
    if metadata.len() != expected_size {
        return Err("size_mismatch");
    }
    let mut file = File::open(&path).map_err(|_| "file_unavailable")?;
    let (hash, size) = hash_reader(&mut file)?;
    if hash != expected_sha256 || size != expected_size {
        return Err("content_mismatch");
    }
    file.rewind().map_err(|_| "read_failed")?;
    Ok((path, file))
}

pub(crate) fn hash_reader(reader: &mut impl Read) -> Result<(String, u64), &'static str> {
    use sha2::Digest;
    let mut digest = sha2::Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(|_| "read_failed")?;
        if count == 0 {
            break;
        }
        size = size.checked_add(count as u64).ok_or("file_too_large")?;
        if size > MAX_STREAM_BYTES {
            return Err("file_too_large");
        }
        digest.update(&buffer[..count]);
    }
    Ok((URL_SAFE_NO_PAD.encode(digest.finalize()), size))
}

pub(crate) fn install_stream_file(
    root: &Path,
    protected_path: &Path,
    relative: &str,
    temporary: tempfile::NamedTempFile,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<(), &'static str> {
    let target = safe_path(root, relative, true)?;
    if conflicts_with_protected(root, &target, protected_path) {
        return Err("protected_path_forbidden");
    }
    let actual = fs::symlink_metadata(&target);
    if let Ok(metadata) = actual {
        if !metadata.is_file() || is_unsafe_link(&metadata) {
            return Err("unsafe_path");
        }
        let mut existing = File::open(&target).map_err(|_| "file_unavailable")?;
        let (hash, size) = hash_reader(&mut existing)?;
        if hash == expected_sha256 && size == expected_size {
            return Ok(());
        }
        return Err("target_exists");
    }
    let parent = target.parent().ok_or("unsafe_path")?;
    create_safe_directories(root, parent)?;
    let target = safe_path(root, relative, true)?;
    temporary.as_file().sync_all().map_err(|_| "write_failed")?;
    temporary.persist_noclobber(target).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            "target_exists"
        } else {
            "write_failed"
        }
    })?;
    Ok(())
}

fn create_safe_directories(root: &Path, parent: &Path) -> Result<(), &'static str> {
    let root = root.canonicalize().map_err(|_| "file_unavailable")?;
    let relative = parent.strip_prefix(&root).map_err(|_| "unsafe_path")?;
    let mut current = root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("unsafe_path");
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !is_unsafe_link(&metadata) => {}
            Ok(_) => return Err("unsafe_path"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| "write_failed")?;
            }
            Err(_) => return Err("file_unavailable"),
        }
    }
    Ok(())
}

pub(crate) fn move_files(
    root: &Path,
    protected_path: &Path,
    items: &[MoveEntry],
) -> OperationResult {
    if items.len() > MAX_ITEMS {
        return OperationResult {
            files: items
                .iter()
                .take(MAX_ITEMS)
                .map(|item| OperationEntry {
                    path: item.to.clone(),
                    ok: false,
                    error: Some("too_many_files"),
                })
                .collect(),
            failed: Some(true),
        };
    }
    let mut seen_sources = std::collections::HashSet::new();
    let mut seen_targets = std::collections::HashSet::new();
    let preflight = items
        .iter()
        .map(|item| {
            let source = path_key(&item.from);
            let target = path_key(&item.to);
            if !seen_sources.insert(source) || !seen_targets.insert(target) {
                Err("duplicate_path")
            } else {
                preflight_move(root, protected_path, item)
            }
        })
        .collect::<Vec<_>>();
    if preflight.iter().any(Result::is_err) {
        return OperationResult {
            files: items
                .iter()
                .zip(preflight)
                .map(|(item, result)| OperationEntry {
                    path: item.to.clone(),
                    ok: false,
                    error: Some(result.err().unwrap_or("batch_preflight_failed")),
                })
                .collect(),
            failed: Some(true),
        };
    }
    let mut files = Vec::with_capacity(items.len().min(MAX_ITEMS));
    let mut failed = false;
    for item in items {
        let result = move_one(root, protected_path, item);
        failed |= result.is_err();
        files.push(OperationEntry {
            path: item.to.clone(),
            ok: result.is_ok(),
            error: result.err(),
        });
    }
    OperationResult {
        files,
        failed: failed.then_some(true),
    }
}

fn path_key(path: &str) -> String {
    #[cfg(windows)]
    {
        path.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_string()
    }
}

fn preflight_move(
    root: &Path,
    protected_path: &Path,
    item: &MoveEntry,
) -> Result<(), &'static str> {
    let root = root.canonicalize().map_err(|_| "file_unavailable")?;
    let source = safe_path(&root, &item.from, true)?;
    let target = safe_path(&root, &item.to, true)?;
    if source == target || source.starts_with(&target) || target.starts_with(&source) {
        return Err("overlapping_paths");
    }
    if conflicts_with_protected(&root, &source, protected_path)
        || conflicts_with_protected(&root, &target, protected_path)
    {
        return Err("protected_path_forbidden");
    }
    if !source.exists() {
        let metadata = fs::symlink_metadata(&target).map_err(|_| "file_unavailable")?;
        if metadata.is_dir() {
            verify_tree(&target)?;
            return verify_guard(
                &root,
                &item.to,
                item.guard_path.as_deref(),
                item.guard_sha256.as_deref(),
            );
        }
        if metadata.is_file() && !is_unsafe_link(&metadata) {
            let (hash, size) = hash_path(&target)?;
            if item.sha256.as_deref() == Some(hash.as_str()) && item.size == Some(size) {
                return Ok(());
            }
        }
        return Err("content_mismatch");
    }
    let metadata = fs::symlink_metadata(&source).map_err(|_| "file_unavailable")?;
    if metadata.is_dir() {
        verify_tree(&source)?;
        verify_guard(
            &root,
            &item.from,
            item.guard_path.as_deref(),
            item.guard_sha256.as_deref(),
        )?;
        if target.exists() {
            return Err("target_exists");
        }
    } else if metadata.is_file() && !is_unsafe_link(&metadata) {
        let (hash, size) = hash_path(&source)?;
        if item.sha256.as_deref() != Some(hash.as_str()) || item.size != Some(size) {
            return Err("content_mismatch");
        }
        if target.exists() {
            return Err("target_exists");
        }
    } else {
        return Err("unsafe_path");
    }
    Ok(())
}

fn move_one(root: &Path, protected_path: &Path, item: &MoveEntry) -> Result<(), &'static str> {
    preflight_move(root, protected_path, item)?;
    let source = safe_path(root, &item.from, true)?;
    let target = safe_path(root, &item.to, true)?;
    if conflicts_with_protected(root, &source, protected_path)
        || conflicts_with_protected(root, &target, protected_path)
    {
        return Err("protected_path_forbidden");
    }
    if !source.exists() {
        let metadata = fs::symlink_metadata(&target).map_err(|_| "file_unavailable")?;
        if metadata.is_dir() {
            verify_tree(&target)?;
            verify_guard(
                root,
                &item.to,
                item.guard_path.as_deref(),
                item.guard_sha256.as_deref(),
            )?;
            return Ok(());
        }
        if metadata.is_file() && !is_unsafe_link(&metadata) {
            let (hash, size) = hash_path(&target)?;
            if item.sha256.as_deref() == Some(hash.as_str()) && item.size == Some(size) {
                return Ok(());
            }
        }
        return Err("content_mismatch");
    }
    let metadata = fs::symlink_metadata(&source).map_err(|_| "file_unavailable")?;
    if metadata.is_dir() {
        verify_tree(&source)?;
        verify_guard(
            root,
            &item.from,
            item.guard_path.as_deref(),
            item.guard_sha256.as_deref(),
        )?;
        if target.exists() {
            return Err("target_exists");
        }
    } else if metadata.is_file() && !is_unsafe_link(&metadata) {
        let (hash, size) = hash_path(&source)?;
        if item.sha256.as_deref() != Some(hash.as_str()) || item.size != Some(size) {
            return Err("content_mismatch");
        }
    } else {
        return Err("unsafe_path");
    }
    if target.exists() {
        return Err("target_exists");
    }
    create_safe_directories(root, target.parent().ok_or("unsafe_path")?)?;
    if safe_path(root, &item.from, false)? != source || safe_path(root, &item.to, true)? != target {
        return Err("unsafe_path");
    }
    rename_no_replace(&source, &target).map_err(|_| "move_failed")
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let target = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn rename_no_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let target = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn rename_no_replace(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(windows)]
fn rename_no_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub(crate) fn remove_files(
    root: &Path,
    protected_path: &Path,
    items: &[RemovalEntry],
) -> OperationResult {
    if items.len() > MAX_ITEMS {
        return OperationResult {
            files: items
                .iter()
                .take(MAX_ITEMS)
                .map(|item| OperationEntry {
                    path: item.path.clone(),
                    ok: false,
                    error: Some("too_many_files"),
                })
                .collect(),
            failed: Some(true),
        };
    }
    let mut files = Vec::with_capacity(items.len().min(MAX_ITEMS));
    let mut failed = false;
    for item in items {
        let result = remove_one(root, protected_path, item);
        failed |= result.is_err();
        files.push(OperationEntry {
            path: item.path.clone(),
            ok: result.is_ok(),
            error: result.err(),
        });
    }
    OperationResult {
        files,
        failed: failed.then_some(true),
    }
}

fn remove_one(root: &Path, protected_path: &Path, item: &RemovalEntry) -> Result<(), &'static str> {
    let path = match safe_path(root, &item.path, true) {
        Ok(path) => path,
        Err(error) => return Err(error),
    };
    if path == root || conflicts_with_protected(root, &path, protected_path) {
        return Err("protected_path_forbidden");
    }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("file_unavailable"),
    };
    if metadata.is_dir() {
        verify_tree(&path)?;
        verify_guard(
            root,
            &item.path,
            item.guard_path.as_deref(),
            item.guard_sha256.as_deref(),
        )?;
        fs::remove_dir_all(path).map_err(|_| "remove_failed")
    } else if metadata.is_file() && !is_unsafe_link(&metadata) {
        let (hash, size) = hash_path(&path)?;
        if item.sha256.as_deref() != Some(hash.as_str()) || item.size != Some(size) {
            return Err("content_mismatch");
        }
        fs::remove_file(path).map_err(|_| "remove_failed")
    } else {
        Err("unsafe_path")
    }
}

fn verify_guard(
    root: &Path,
    directory: &str,
    guard: Option<&str>,
    expected: Option<&str>,
) -> Result<(), &'static str> {
    let (Some(guard), Some(expected)) = (guard, expected) else {
        return Err("guard_required");
    };
    let root = root.canonicalize().map_err(|_| "file_unavailable")?;
    let directory = safe_path(&root, directory, false)?;
    let marker_rel = format!(
        "{}/{}",
        directory
            .strip_prefix(&root)
            .map_err(|_| "unsafe_path")?
            .to_string_lossy()
            .replace('\\', "/"),
        guard
    );
    let marker = safe_path(&root, &marker_rel, false)?;
    if !marker.starts_with(&directory) {
        return Err("unsafe_path");
    }
    let (hash, _) = hash_path(&marker)?;
    if hash != expected {
        return Err("guard_mismatch");
    }
    Ok(())
}

fn verify_tree(directory: &Path) -> Result<(), &'static str> {
    let mut pending = vec![directory.to_path_buf()];
    let mut visited = 0usize;
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(current).map_err(|_| "file_unavailable")? {
            visited += 1;
            if visited > MAX_TREE_ENTRIES {
                return Err("tree_too_large");
            }
            let entry = entry.map_err(|_| "file_unavailable")?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|_| "file_unavailable")?;
            if is_unsafe_link(&metadata) {
                return Err("unsafe_path");
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if !metadata.is_file() {
                return Err("unsafe_path");
            }
        }
    }
    Ok(())
}

pub(crate) fn hash_path(path: &Path) -> Result<(String, u64), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "file_unavailable")?;
    if !metadata.is_file() || is_unsafe_link(&metadata) {
        return Err("unsafe_path");
    }
    if metadata.len() > MAX_STREAM_BYTES {
        return Err("file_too_large");
    }
    let mut file = File::open(path).map_err(|_| "file_unavailable")?;
    hash_reader(&mut file)
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
    expected_existing_sha256: Option<&str>,
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
    if let Some(expected) = expected_existing_sha256 {
        if hash_path(path).map_or(true, |(actual, _)| actual != expected) {
            let _ = fs::remove_file(&temporary);
            return Err("source_changed".to_string());
        }
    }
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
