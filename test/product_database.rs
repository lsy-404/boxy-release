use super::*;

const DATABASE_ID: &str = "1a2b3c4d-5e6f-7081-92a3-b4c5d6e7f809";

fn item(id: &str, target: &str, filename: &str, url: &str) -> FetchItem {
    FetchItem {
        id: id.to_string(),
        url: url.to_string(),
        target: target.to_string(),
        filename: filename.to_string(),
        sha256: None,
        size: None,
        version_name: None,
        files: None,
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn temporary_file(name: &str, bytes: &[u8]) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "boxy-product-{}-{}",
        std::process::id(),
        unix_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn accepts_only_canonical_database_ids() {
    assert!(valid_database_id(DATABASE_ID));
    assert!(validated_id(&format!(" {DATABASE_ID} ")).is_ok());
    assert!(!valid_database_id("1a2b3c4d5e6f708192a3b4c5d6e7f809"));
    assert!(!valid_database_id("1a2b3c4d-5e6f-7081-92a3-b4c5d6e7f80"));
    assert!(!valid_database_id("1a2b3c4d5-e6f-7081-92a3-b4c5d6e7f809"));
    assert!(!valid_database_id("zzzzzzzz-5e6f-7081-92a3-b4c5d6e7f809"));
    assert!(validated_id("../etc/passwd").is_err());
    assert!(validated_id("").is_err());
}

#[test]
fn accepts_and_rejects_download_hosts() {
    assert!(validated_download_url("https://authr3.dreamtonics.com/x/y.svpk", ".svpk").is_ok());
    assert!(validated_download_url("https://cdn.dreamtonics.com.cn/y.dnni", ".dnni").is_ok());
    assert!(validated_download_url("http://authr3.dreamtonics.com/y.svpk", ".svpk").is_err());
    assert!(validated_download_url("https://evil-dreamtonics.com/y.svpk", ".svpk").is_err());
    assert!(
        validated_download_url("https://dreamtonics.com.evil.example/y.svpk", ".svpk").is_err()
    );
    assert!(validated_download_url("https://authr3.dreamtonics.com/y.zip", ".svpk").is_err());
}

#[test]
fn validates_voice_model_and_package_signatures() {
    let model = temporary_file("model.dnni", &DNNI_MAGIC);
    assert!(has_magic(&model, &DNNI_MAGIC));
    assert!(!has_magic(&model, &SVPK_MAGIC));
    let package = temporary_file("package.svpk", b"SVPKpayload");
    assert!(has_magic(&package, &SVPK_MAGIC));
    assert!(!has_magic(&package, &DNNI_MAGIC));
    let _ = fs::remove_dir_all(model.parent().unwrap());
    let _ = fs::remove_dir_all(package.parent().unwrap());
}

#[test]
fn selects_package_destinations() {
    let downloads = Path::new("/tmp/boxy-downloads");
    let (path, magic, extension) = select_destination(
        downloads,
        &item(DATABASE_ID, "downloads", "voice.svpk", "https://x/y.svpk"),
    )
    .unwrap();
    assert_eq!(path, downloads.join("voice.svpk"));
    assert_eq!(magic, &SVPK_MAGIC);
    assert_eq!(extension, ".svpk");
}

#[test]
fn rejects_unsafe_destinations() {
    let base = Path::new("/tmp/boxy");
    assert!(select_destination(
        base,
        &item(
            DATABASE_ID,
            "downloads",
            "../escape.svpk",
            "https://x/y.svpk"
        )
    )
    .is_err());
    assert!(validated_id("not-a-uuid").is_err());
    assert!(validated_id(DATABASE_ID).is_ok());
}
