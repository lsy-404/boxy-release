use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::session_io;

const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const DNNI_MAGIC: [u8; 5] = [0xff, 0x00, 0xca, 0x7f, 0x03];
const SVPK_MAGIC: [u8; 4] = *b"SVPK";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDatabaseStatus {
    pub id: String,
    pub installed: bool,
    pub uninstallable: bool,
    pub dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchItem {
    pub id: String,
    pub url: String,
    pub target: String,
    pub filename: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub version_name: Option<String>,
    pub files: Option<Vec<FetchFile>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchFile {
    pub url: String,
    pub filename: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    pub id: String,
    pub operation: String,
    pub destination_path: Option<String>,
    pub downloaded_bytes: u64,
}

pub fn inspect() -> Vec<ProductDatabaseStatus> {
    let Ok(databases) = databases_directory() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&databases) else {
        return Vec::new();
    };
    let mut statuses = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir() && !file_type.is_symlink())
                .unwrap_or(false)
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|id| valid_database_id(id))
        .map(|id| {
            let dir = databases.join(&id);
            ProductDatabaseStatus {
                installed: installed_database(&dir),
                uninstallable: safe_directory(&dir),
                dir: dir.to_string_lossy().into_owned(),
                id,
            }
        })
        .collect::<Vec<_>>();
    statuses.sort_by(|left, right| left.id.cmp(&right.id));
    statuses
}

pub fn fetch(item: &FetchItem) -> Result<FetchResult, String> {
    if item.target == "sv2" {
        return fetch_voice_models(&databases_directory()?, item);
    }
    if item.target != "downloads" {
        return Err("unknown product fetch target".to_string());
    }
    let base = downloads_directory()?;
    let (destination, expected_magic, extension) = select_destination(&base, item)?;
    let url = validated_download_url(&item.url, extension)?;
    let downloaded_bytes = download_to(&url, &destination, expected_magic, item)?;
    Ok(FetchResult {
        id: item.id.trim().to_string(),
        operation: "svpkDownload".to_string(),
        destination_path: Some(destination.to_string_lossy().into_owned()),
        downloaded_bytes,
    })
}

fn fetch_voice_models(base: &Path, item: &FetchItem) -> Result<FetchResult, String> {
    let id = validated_id(&item.id)?;
    let version = item
        .version_name
        .as_deref()
        .map(safe_filename)
        .transpose()?
        .filter(|value| !value.is_empty())
        .unwrap_or("model");
    let files = match item.files.as_ref().filter(|files| !files.is_empty()) {
        Some(files) => files
            .iter()
            .map(|file| (file.url.clone(), file.filename.clone()))
            .collect::<Vec<_>>(),
        None => vec![(item.url.clone(), item.filename.clone())],
    };
    if files.is_empty() || files.len() > 8 {
        return Err("voice model response has an invalid file count".to_string());
    }
    let staging = base.join(format!(".boxy-{}-{}.part", id, std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("cannot prepare the voice model directory: {error}"))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("cannot prepare the voice model directory: {error}"))?;
    let mut downloaded_bytes = 0u64;
    let result = (|| {
        for (url, filename) in &files {
            let filename = safe_filename(filename)?;
            if !filename.to_ascii_lowercase().ends_with(".dnni") {
                return Err("product model file name is not a DNNI file".to_string());
            }
            let url = validated_download_url(url, ".dnni")?;
            downloaded_bytes += download_to(&url, &staging.join(filename), &DNNI_MAGIC, item)?;
        }
        let active = base.join(&id);
        fs::create_dir_all(&active)
            .map_err(|error| format!("cannot create the voice database directory: {error}"))?;
        let destination = active.join(&version);
        if destination.exists() {
            return Err("this voice database version is already installed".to_string());
        }
        fs::rename(&staging, &destination)
            .map_err(|error| format!("cannot finalize the voice database installation: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => Ok(FetchResult {
            id: id.to_string(),
            operation: "sv2Install".to_string(),
            destination_path: Some(destination.to_string_lossy().into_owned()),
            downloaded_bytes,
        }),
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

pub fn delete(id: &str) -> Result<(), String> {
    let id = validated_id(id)?;
    let target = databases_directory()?.join(id);
    if !safe_directory(&target) {
        return Err("this product database is not installed".to_string());
    }
    fs::remove_dir_all(&target)
        .map_err(|error| format!("cannot remove the product database; close SV2 first: {error}"))
}

fn sv2_root() -> Result<PathBuf, String> {
    session_io::default_session_path()
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot determine the Synthesizer V Studio 2 data directory".to_string())
}

fn databases_directory() -> Result<PathBuf, String> {
    let databases = sv2_root()?.join("databases");
    if !safe_directory(&databases) {
        return Err("the SV2 databases directory is unavailable".to_string());
    }
    Ok(databases)
}

fn downloads_directory() -> Result<PathBuf, String> {
    let directory = dirs::download_dir()
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|path| path.join("Downloads"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join("Downloads"))
        })
        .ok_or_else(|| "cannot determine the user Downloads directory".to_string())?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot prepare the Downloads directory: {error}"))?;
    Ok(directory)
}

fn select_destination(
    base: &Path,
    item: &FetchItem,
) -> Result<(PathBuf, &'static [u8], &'static str), String> {
    let filename = safe_filename(&item.filename)?;
    Ok((base.join(filename), &SVPK_MAGIC, ".svpk"))
}

fn validated_download_url(value: &str, extension: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "product download URL is invalid".to_string())?;
    if url.scheme() != "https" {
        return Err("product download URL must use HTTPS".to_string());
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if !allowed_download_host(&host) {
        return Err("product download URL is not hosted by Dreamtonics".to_string());
    }
    if crate::host_block::is_host_blocked(&host) {
        return Err("Dreamtonics network rules are active; unblock the network first".to_string());
    }
    if !url.path().to_ascii_lowercase().ends_with(extension) {
        return Err("product download URL has an unexpected file type".to_string());
    }
    Ok(url)
}

fn allowed_download_host(host: &str) -> bool {
    host == "dreamtonics.com"
        || host.ends_with(".dreamtonics.com")
        || host == "dreamtonics.com.cn"
        || host.ends_with(".dreamtonics.com.cn")
}

fn safe_filename(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err("product file name is unsafe".to_string());
    }
    Ok(value)
}

fn download_to(
    url: &Url,
    destination: &Path,
    expected_magic: &[u8],
    item: &FetchItem,
) -> Result<u64, String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot prepare the product directory: {error}"))?;
    }
    let response = ureq::get(url.as_str()).call();
    let (status, response) = match response {
        Ok(response) => (response.status(), response),
        Err(ureq::Error::Status(status, response)) => (status, response),
        Err(ureq::Error::Transport(error)) => {
            return Err(format!("product download failed: {error}"))
        }
    };
    if status != 200 {
        return Err(format!("product download failed ({status})"));
    }
    let declared_length = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|length| *length <= MAX_PACKAGE_BYTES);
    if response.header("Content-Length").is_some() && declared_length.is_none() {
        return Err("product file is too large".to_string());
    }
    let temporary = temporary_path(destination);
    let mut output = create_new_file(&temporary)?;
    let mut input = response.into_reader();
    let copied = match copy_limited(&mut input, &mut output, MAX_PACKAGE_BYTES) {
        Ok(copied) => copied,
        Err(error) => {
            drop(output);
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot download product file: {error}"));
        }
    };
    if let Err(error) = output.sync_all() {
        drop(output);
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot save product file: {error}"));
    }
    drop(output);
    if let Err(error) = validate_download(&temporary, expected_magic, item) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(destination)
            .map_err(|error| format!("cannot replace {}: {error}", destination.display()))?;
    }
    fs::rename(&temporary, destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("cannot finalize {}: {error}", destination.display())
    })?;
    Ok(copied)
}

fn validate_download(path: &Path, expected_magic: &[u8], item: &FetchItem) -> Result<(), String> {
    let length = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if !has_magic(path, expected_magic) {
        return Err("downloaded product file has an invalid file signature".to_string());
    }
    if item.size.is_some_and(|size| size != length) {
        return Err("downloaded product file size does not match the expected size".to_string());
    }
    if let Some(expected) = item
        .sha256
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !sha256_matches(path, expected)? {
            return Err(
                "downloaded product file hash does not match the expected hash".to_string(),
            );
        }
    }
    Ok(())
}

fn sha256_matches(path: &Path, expected: &str) -> Result<bool, String> {
    let digest = sha256_file(path)?;
    if expected.eq_ignore_ascii_case(&hex_lower(&digest)) {
        return Ok(true);
    }
    Ok(URL_SAFE_NO_PAD.encode(&digest) == expected)
}

fn sha256_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hash.finalize().to_vec())
}

fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    value
}

fn copy_limited(
    input: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            return Ok(total);
        }
        total = total.saturating_add(read as u64);
        if total > limit {
            return Err(std::io::Error::other("size limit exceeded"));
        }
        output.write_all(&buffer[..read])?;
    }
}

fn temporary_path(destination: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("download");
    destination.with_file_name(format!(".boxy-{}-{nonce}-{name}.part", std::process::id()))
}

fn create_new_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))
}

fn has_magic(path: &Path, expected: &[u8]) -> bool {
    let mut signature = vec![0u8; expected.len()];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map(|_| signature == expected)
        .unwrap_or(false)
}

fn safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn installed_database(path: &Path) -> bool {
    if !safe_directory(path) {
        return false;
    }
    // Accept both a flat databases/<id> layout and the version subdirectory layout SV2 uses.
    if version_installed(path) {
        return true;
    }
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|version| safe_directory(version))
        .any(|version| version_installed(&version))
}

fn version_installed(directory: &Path) -> bool {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|file| {
            file.extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dnni"))
                && fs::symlink_metadata(file)
                    .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                    .unwrap_or(false)
                && has_magic(file, &DNNI_MAGIC)
        })
        .take(2)
        .count()
        == 2
}

fn validated_id(value: &str) -> Result<&str, String> {
    let value = value.trim();
    valid_database_id(value)
        .then_some(value)
        .ok_or_else(|| "product database ID is invalid".to_string())
}

fn valid_database_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
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
}
