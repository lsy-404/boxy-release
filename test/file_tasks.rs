use super::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boxy-file-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_entry(path: &str, body: &[u8], overwrite: bool) -> WriteFileTaskEntry {
    WriteFileTaskEntry {
        path: path.into(),
        content_base64: URL_SAFE_NO_PAD.encode(body),
        sha256: None,
        overwrite,
        expected_existing_sha256: None,
    }
}

#[test]
fn reads_batch_files_with_hash_and_rejects_path_escape() {
    let temp = Temp::new();
    fs::create_dir_all(temp.0.join("cache")).unwrap();
    fs::write(temp.0.join("cache/item.bin"), [0, 1, 2]).unwrap();
    let result = read_files(
        &temp.0,
        &["cache/item.bin".into(), "../outside".into()],
        10,
        20,
    );
    assert_eq!(result.files.len(), 2);
    let read = &result.files[0];
    assert_eq!(read.size, Some(3));
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(read.content_base64.as_ref().unwrap())
            .unwrap(),
        [0, 1, 2]
    );
    assert_eq!(
        read.sha256.as_deref(),
        Some(crate::sha256_base64url(&[0, 1, 2]).as_str())
    );
    assert_eq!(result.files[1].error, Some("unsafe_path"));
}

#[test]
fn list_root_sentinel_recursively_reports_entries_without_mutating_them() {
    let temp = Temp::new();
    fs::create_dir_all(temp.0.join("nested")).unwrap();
    fs::write(temp.0.join("nested/probe.txt"), b"read only").unwrap();

    let result = list_files(&temp.0, &[".".into()], &[]);

    assert!(result
        .files
        .iter()
        .any(|entry| entry.path == "." && entry.kind == "directory"));
    assert!(result
        .files
        .iter()
        .any(|entry| entry.path == "nested/probe.txt" && entry.kind == "file"));
    assert_eq!(
        fs::read(temp.0.join("nested/probe.txt")).unwrap(),
        b"read only"
    );
}

#[test]
fn list_manifest_handles_more_than_four_thousand_entries_without_partial_results() {
    let temp = Temp::new();
    for index in 0..4_100 {
        fs::write(temp.0.join(format!("entry-{index:04}.txt")), b"x").unwrap();
    }

    let result = list_files(&temp.0, &[".".into()], &[]);

    assert_eq!(result.files.len(), 4_101);
    assert!(result.files.iter().all(|entry| entry.error.is_none()));
}

#[cfg(unix)]
#[test]
fn list_reports_symlink_metadata_without_following_the_link() {
    use std::os::unix::fs::symlink;

    let temp = Temp::new();
    let root = temp.0.join("root");
    let outside = temp.0.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"outside scope").unwrap();
    symlink(&outside, root.join("alias")).unwrap();

    let result = list_files(&root, &[".".into()], &[]);
    let link = result
        .files
        .iter()
        .find(|entry| entry.path == "alias")
        .unwrap();
    let nested = list_files(&root, &["alias/secret.txt".into()], &[]);

    assert_eq!(link.kind, "symlink");
    assert_eq!(link.error, None);
    assert!(!result
        .files
        .iter()
        .any(|entry| entry.path == "alias/secret.txt"));
    assert_eq!(nested.files[0].error, Some("unsafe_path"));
}

#[test]
fn enforces_actual_aggregate_read_budget_and_uses_unpadded_base64url() {
    let temp = Temp::new();
    fs::write(temp.0.join("one"), [0, 255]).unwrap();
    fs::write(temp.0.join("two"), [1, 2]).unwrap();
    let paths = ["one".into(), "two".into()];
    let too_large = read_files(&temp.0, &paths, 4, 3);
    assert!(too_large
        .files
        .iter()
        .all(|file| file.error == Some("total_too_large")));
    let result = read_files(&temp.0, &["one".into()], 4, 4);
    assert_eq!(result.files[0].content_base64.as_deref(), Some("AP8"));
}

#[test]
fn writes_are_atomic_bounded_and_require_explicit_overwrite() {
    let temp = Temp::new();
    let protected = temp.0.join("protecteds/protected");
    fs::create_dir_all(protected.parent().unwrap()).unwrap();
    fs::write(&protected, b"protected").unwrap();

    let files = [write_entry("cache/new.bin", &[4, 5], false)];
    let result = write_files(&temp.0, &protected, &files);
    assert_eq!(result.files[0].ok, Some(true));
    assert_eq!(fs::read(temp.0.join("cache/new.bin")).unwrap(), [4, 5]);

    let overwrite_denied = [write_entry("cache/new.bin", &[6], false)];
    assert_eq!(
        write_files(&temp.0, &protected, &overwrite_denied).files[0].error,
        Some("file_exists")
    );
    let overwrite_allowed = [write_entry("cache/new.bin", &[6], true)];
    assert_eq!(
        write_files(&temp.0, &protected, &overwrite_allowed).files[0].ok,
        Some(true)
    );
    assert_eq!(fs::read(temp.0.join("cache/new.bin")).unwrap(), [6]);

    let protected_write = [write_entry("protecteds/protected", b"changed", true)];
    assert_eq!(
        write_files(&temp.0, &protected, &protected_write).files[0].error,
        Some("protected_path_forbidden")
    );
    assert_eq!(fs::read(&protected).unwrap(), b"protected");
}

#[test]
fn protected_write_requires_a_matching_existing_hash() {
    let temp = Temp::new();
    let protected = temp.0.join("protected");
    fs::write(&protected, b"old-value").unwrap();
    let mut matching = write_entry("protected", b"new-value", true);
    matching.expected_existing_sha256 = Some(crate::sha256_base64url(b"old-value"));
    let result = write_files(&temp.0, &protected, &[matching]);
    assert_eq!(result.files[0].ok, Some(true));
    assert_eq!(fs::read(&protected).unwrap(), b"new-value");

    let mut stale = write_entry("protected", b"stale-write", true);
    stale.expected_existing_sha256 = Some(crate::sha256_base64url(b"old-value"));
    let result = write_files(&temp.0, &protected, &[stale]);
    assert_eq!(result.files[0].error, Some("source_changed"));
    assert_eq!(fs::read(&protected).unwrap(), b"new-value");
}

#[test]
fn rejects_windows_streams_and_reserved_path_components() {
    let root = PathBuf::from("/scope");
    for unsafe_path in [
        "cache/file:stream",
        "CON.txt",
        "LPT1",
        "trailing.",
        "trailing ",
    ] {
        assert_eq!(safe_path(&root, unsafe_path, true), Err("unsafe_path"));
    }
}

#[test]
fn validates_complete_write_batch_before_any_file_is_written() {
    let temp = Temp::new();
    let protected = temp.0.join("protecteds/protected");
    fs::create_dir_all(protected.parent().unwrap()).unwrap();
    fs::write(&protected, b"protected").unwrap();
    let files = [
        write_entry("cache/first.bin", b"first", false),
        write_entry("../escape", b"second", false),
    ];
    let result = write_files(&temp.0, &protected, &files);
    assert_eq!(result.files.len(), 2);
    assert!(!temp.0.join("cache/first.bin").exists());
    assert_eq!(result.files[1].error, Some("unsafe_path"));
}

#[test]
fn enforces_file_and_total_limits_and_hashes() {
    let temp = Temp::new();
    let protected = temp.0.join("protected");
    fs::write(&protected, b"protected").unwrap();
    let maximum = vec![b'x'; MAX_FILE_BYTES];
    let written = write_files(
        &temp.0,
        &protected,
        &[write_entry("maximum.bin", &maximum, false)],
    );
    assert_eq!(written.files[0].ok, Some(true));
    let read = read_files(
        &temp.0,
        &["maximum.bin".into()],
        MAX_FILE_BYTES,
        MAX_TOTAL_BYTES,
    );
    assert_eq!(read.files[0].size, Some(MAX_FILE_BYTES as u64));

    let oversized = vec![b'x'; MAX_FILE_BYTES + 1];
    assert_eq!(
        write_files(
            &temp.0,
            &protected,
            &[write_entry("big.bin", &oversized, false)]
        )
        .files[0]
            .error,
        Some("file_too_large")
    );
    let mut mismatch = write_entry("wrong.bin", b"bytes", false);
    mismatch.sha256 = Some("incorrect".into());
    assert_eq!(
        write_files(&temp.0, &protected, &[mismatch]).files[0].error,
        Some("hash_mismatch")
    );
    let paths = vec!["large.bin".into()];
    fs::write(temp.0.join("large.bin"), vec![0; 9]).unwrap();
    assert_eq!(
        read_files(&temp.0, &paths, 8, 10).files[0].error,
        Some("file_too_large")
    );
}

#[test]
fn patches_a_staged_file_and_returns_its_updated_hash() {
    let temp = Temp::new();
    let root = temp.0.join("scope");
    let relative = "server-content/install/version/model.dnni";
    let target = root.join(relative);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let mut original = vec![b'a'; 1024];
    original[8] = 0x41;
    fs::write(&target, &original).unwrap();
    let protected = root.join("protected");
    fs::write(&protected, b"protected").unwrap();
    let (sha256, size) = hash_path(&target).unwrap();
    let patch = FilePatchTaskEntry {
        path: relative.into(),
        size,
        sha256,
        segments: vec![FilePatchSegment {
            offset: 8,
            before_base64: URL_SAFE_NO_PAD.encode([0x41]),
            content_base64: URL_SAFE_NO_PAD.encode([0x42]),
        }],
    };

    let result = patch_files(&root, &protected, &[patch], || false);

    assert_eq!(result.failed, None);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, relative);
    assert_eq!(result.files[0].size, Some(size));
    assert_eq!(result.files[0].ok, true);
    assert_eq!(result.files[0].sha256, Some(hash_path(&target).unwrap().0));
    original[8] = 0x42;
    assert_eq!(fs::read(target).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn rejects_symlink_components_for_reads_and_writes() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let outside = temp.0.join("outside");
    let root = temp.0.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(&outside, b"outside").unwrap();
    symlink(&outside, root.join("alias")).unwrap();
    let read = read_files(&root, &["alias".into()], 100, 100);
    assert_eq!(read.files[0].error, Some("unsafe_path"));
    let protected = root.join("protected");
    fs::write(&protected, b"protected").unwrap();
    let write = write_files(&root, &protected, &[write_entry("alias", b"replace", true)]);
    assert_eq!(write.files[0].error, Some("unsafe_path"));
    assert_eq!(fs::read(outside).unwrap(), b"outside");
}

#[test]
fn rejects_hard_link_aliases_of_the_active_protected() {
    let temp = Temp::new();
    let protected = temp.0.join("protected");
    let alias = temp.0.join("alias");
    fs::write(&protected, b"protected").unwrap();
    fs::hard_link(&protected, &alias).unwrap();

    let result = write_files(
        &temp.0,
        &protected,
        &[write_entry("alias", b"replacement", true)],
    );

    assert_eq!(result.files[0].error, Some("protected_path_forbidden"));
    assert_eq!(fs::read(&protected).unwrap(), b"protected");
}

#[cfg(windows)]
#[test]
fn rejects_junction_components_that_escape_the_root() {
    use std::process::Command;

    let temp = Temp::new();
    let root = temp.0.join("root");
    let outside = temp.0.join("outside");
    let junction = root.join("junction");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("file.txt"), b"outside").unwrap();

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "New-Item -ItemType Junction -Path $env:BOXY_JUNCTION_PATH -Target $env:BOXY_JUNCTION_TARGET | Out-Null",
        ])
        .env("BOXY_JUNCTION_PATH", &junction)
        .env("BOXY_JUNCTION_TARGET", &outside)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create junction fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        safe_path(&root, "junction/file.txt", false),
        Err("unsafe_path")
    );
}
