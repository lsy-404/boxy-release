use std::{fs, path::PathBuf};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01";

fn session_path(root: &std::path::Path) -> PathBuf {
    let session = root
        .join("Dreamtonics")
        .join("SV2")
        .join("license")
        .join("session");
    fs::create_dir_all(session.parent().unwrap()).unwrap();
    fs::write(&session, b"session!").unwrap();
    session
}

#[test]
fn serves_only_image_bytes_from_the_two_product_image_directories() {
    let temp = tempfile::tempdir().unwrap();
    let session = session_path(temp.path());
    let root = session.parent().unwrap().parent().unwrap();
    fs::create_dir_all(root.join("databases/meta")).unwrap();
    fs::create_dir_all(root.join("server-product-logos")).unwrap();
    fs::write(root.join("databases/meta/logo.bin"), PNG).unwrap();
    fs::write(root.join("server-product-logos/brand"), PNG).unwrap();

    let metadata_image = crate::local_images::read(&session, "/databases/meta/logo.bin").unwrap();
    assert_eq!(metadata_image.mime, "image/png");
    assert_eq!(metadata_image.bytes, PNG);
    let product_logo = crate::local_images::read(&session, "/server-product-logos/brand").unwrap();
    assert_eq!(product_logo.mime, "image/png");
}

#[test]
fn rejects_traversal_other_roots_non_images_and_oversized_files() {
    let temp = tempfile::tempdir().unwrap();
    let session = session_path(temp.path());
    let root = session.parent().unwrap().parent().unwrap();
    fs::create_dir_all(root.join("databases/meta")).unwrap();
    fs::create_dir_all(root.join("other")).unwrap();
    fs::write(root.join("databases/meta/not-image.json"), b"{}").unwrap();
    fs::write(root.join("other/logo.png"), PNG).unwrap();
    let oversized = root.join("databases/meta/oversized.png");
    fs::write(&oversized, PNG).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&oversized)
        .unwrap()
        .set_len(crate::local_images::MAX_IMAGE_BYTES + 1)
        .unwrap();

    for path in [
        "/databases/meta/%2e%2e/other/logo.png",
        "/other/logo.png",
        "/databases/meta/not-image.json",
        "/databases/meta/oversized.png",
        "databases/meta/without-leading-slash.png",
    ] {
        assert!(crate::local_images::read(&session, path).is_err(), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_image_paths() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let session = session_path(temp.path());
    let root = session.parent().unwrap().parent().unwrap();
    fs::create_dir_all(root.join("databases/meta")).unwrap();
    fs::write(root.join("outside.png"), PNG).unwrap();
    symlink(
        root.join("outside.png"),
        root.join("databases/meta/link.png"),
    )
    .unwrap();

    assert!(crate::local_images::read(&session, "/databases/meta/link.png").is_err());
}
