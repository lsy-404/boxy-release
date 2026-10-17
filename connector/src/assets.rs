use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Read,
    path::Path,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;

const MAX_IMAGE_BYTES: usize = 512 * 1024;
const MAX_TOTAL_IMAGE_BYTES: usize = 512 * 1024;
const MAX_CATALOG_BYTES: usize = 512 * 1024;
const MAX_TOTAL_CATALOG_BYTES: usize = 512 * 1024;
const MAX_TOTAL_CATALOG_JSON_BYTES: usize = 512 * 1024;
const MAX_TOTAL_LOGO_JSON_BYTES: usize = 512 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_LOGOS: usize = 512;
const MAX_CATALOGS: usize = 32;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerTranslationCatalog {
    pub locale: String,
    pub language: String,
    pub entries: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedProductLogo {
    pub id: String,
    pub name: String,
    pub data_url: String,
}

pub fn server_translations(session_path: &Path) -> Vec<ServerTranslationCatalog> {
    let Some(directory) = relative_root(session_path).map(|root| root.join("server-translations"))
    else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry.file_type().ok()?.is_file().then(|| entry.path())
        })
        .filter(|path| path.extension().is_some_and(|value| value == "txt"))
        .collect::<Vec<_>>();
    files.sort();
    let mut catalogs = Vec::new();
    let mut total_catalog_bytes = 0usize;
    let mut total_catalog_json_bytes = 0usize;
    for path in files.into_iter().take(MAX_CATALOGS) {
        let remaining_catalog_bytes = MAX_TOTAL_CATALOG_BYTES.saturating_sub(total_catalog_bytes);
        let Some(text) = read_bounded_file(&path, MAX_CATALOG_BYTES.min(remaining_catalog_bytes))
        else {
            continue;
        };
        let Ok(text) = String::from_utf8(text) else {
            continue;
        };
        let locale = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        if locale.is_empty() || locale.encode_utf16().count() > 64 {
            continue;
        }
        let mut language = locale.clone();
        let mut entries = BTreeMap::new();
        let mut valid_language = true;
        for line in text.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("language:") {
                let value = value.trim();
                if value.is_empty() || value.encode_utf16().count() > 128 {
                    valid_language = false;
                    break;
                }
                language = value.to_string();
            } else if let Some((source, target)) = parse_translation_line(line) {
                if entries.len() < MAX_ENTRIES {
                    entries.insert(source, target);
                }
            }
        }
        if !valid_language {
            continue;
        }
        let catalog = ServerTranslationCatalog {
            locale,
            language,
            entries,
        };
        let Ok(catalog_size) = serde_json::to_vec(&catalog).map(|value| value.len()) else {
            continue;
        };
        let remaining_catalog_json_bytes =
            MAX_TOTAL_CATALOG_JSON_BYTES.saturating_sub(total_catalog_json_bytes);
        if catalog_size > remaining_catalog_json_bytes {
            continue;
        }
        total_catalog_bytes += text.len();
        total_catalog_json_bytes += catalog_size;
        catalogs.push(catalog);
    }
    catalogs
}

pub fn cached_product_logos(session_path: &Path) -> Vec<CachedProductLogo> {
    let Some(directory) =
        relative_root(session_path).map(|root| root.join("databases").join("meta"))
    else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry.file_type().ok()?.is_file().then(|| entry.path())
        })
        .filter(|path| {
            path.extension()
                .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        })
        .collect::<Vec<_>>();
    files.sort();
    let mut logos = Vec::new();
    let mut covered = HashSet::new();
    let mut total_image_bytes = 0usize;
    let mut total_logo_json_bytes = 0usize;
    for metadata_path in files {
        if logos.len() >= MAX_LOGOS || total_image_bytes >= MAX_TOTAL_IMAGE_BYTES {
            break;
        }
        let Some(id) = metadata_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if id.is_empty() || !within_schema_limit(id) {
            continue;
        }
        let Some(metadata) = read_bounded_file(&metadata_path, MAX_METADATA_BYTES) else {
            continue;
        };
        let name = serde_json::from_slice::<serde_json::Value>(&metadata)
            .ok()
            .and_then(|metadata| {
                metadata
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .filter(|name| within_schema_limit(name))
            .unwrap_or_default();
        let Some((data_url, image_bytes)) = ["png", "jpg", "jpeg", "gif", "webp"]
            .into_iter()
            .find_map(|extension| {
                read_image(
                    &metadata_path.with_extension(extension),
                    MAX_TOTAL_IMAGE_BYTES.saturating_sub(total_image_bytes),
                )
            })
        else {
            continue;
        };
        let logo = CachedProductLogo {
            id: id.to_string(),
            name,
            data_url,
        };
        let Ok(logo_size) = serde_json::to_vec(&logo).map(|value| value.len()) else {
            continue;
        };
        if logo_size > MAX_TOTAL_LOGO_JSON_BYTES.saturating_sub(total_logo_json_bytes) {
            continue;
        }
        total_image_bytes += image_bytes;
        total_logo_json_bytes += logo_size;
        covered.insert(normalize(id));
        logos.push(logo);
    }
    let Ok(images) = fs::read_dir(&directory) else {
        return logos;
    };
    let mut images = images
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry.file_type().ok()?.is_file().then(|| entry.path())
        })
        .collect::<Vec<_>>();
    images.sort();
    for image_path in images {
        if logos.len() >= MAX_LOGOS || total_image_bytes >= MAX_TOTAL_IMAGE_BYTES {
            break;
        }
        if !image_path.extension().is_some_and(|value| {
            ["png", "jpg", "jpeg", "gif", "webp"]
                .iter()
                .any(|extension| value.eq_ignore_ascii_case(extension))
        }) {
            continue;
        }
        let Some(id) = image_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if id.is_empty() || !within_schema_limit(id) {
            continue;
        }
        if covered.contains(&normalize(id)) {
            continue;
        }
        let Some((data_url, image_bytes)) = read_image(
            &image_path,
            MAX_TOTAL_IMAGE_BYTES.saturating_sub(total_image_bytes),
        ) else {
            continue;
        };
        let logo = CachedProductLogo {
            id: id.to_string(),
            name: String::new(),
            data_url,
        };
        let Ok(logo_size) = serde_json::to_vec(&logo).map(|value| value.len()) else {
            continue;
        };
        if logo_size > MAX_TOTAL_LOGO_JSON_BYTES.saturating_sub(total_logo_json_bytes) {
            continue;
        }
        total_image_bytes += image_bytes;
        total_logo_json_bytes += logo_size;
        covered.insert(normalize(id));
        logos.push(logo);
    }
    logos
}

fn relative_root(session_path: &Path) -> Option<std::path::PathBuf> {
    session_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn read_image(path: &Path, remaining_bytes: usize) -> Option<(String, usize)> {
    let max_bytes = MAX_IMAGE_BYTES.min(remaining_bytes);
    let bytes = read_bounded_file(path, max_bytes)?;
    let data_url = image_data_url(&bytes)?;
    Some((data_url, bytes.len()))
}

pub(super) fn fetch_product_logos(
    session_path: &Path,
    keys: &[String],
    access_token: &str,
    recorder: &crate::request_trace::UpstreamAttemptRecorder,
) -> Result<Vec<CachedProductLogo>, String> {
    fetch_product_logos_with_request(
        session_path,
        keys,
        access_token,
        recorder,
        |endpoint, token| {
            crate::dreamtonics_http("GET", endpoint, Some(token), None, "application/json")
        },
        |url, max_bytes| download_product_image(url, max_bytes, recorder),
    )
}

pub(super) fn fetch_product_logos_with_request<F, G>(
    session_path: &Path,
    keys: &[String],
    access_token: &str,
    recorder: &crate::request_trace::UpstreamAttemptRecorder,
    mut request: F,
    download: G,
) -> Result<Vec<CachedProductLogo>, String>
where
    F: FnMut(&str, &str) -> Result<(u16, String), &'static str>,
    G: FnMut(&str, usize) -> Option<Vec<u8>>,
{
    fetch_product_logos_with(
        session_path,
        keys,
        || {
            crate::dreamtonics_license_endpoint(access_token)
                .ok_or_else(|| "access token issuer is missing or invalid".to_string())
        },
        |endpoint| {
            let (status, response_body) = crate::request_trace::recorded_dreamtonics_http(
                recorder,
                "logo_licenses",
                "GET",
                &endpoint,
                false,
                || request(&endpoint, access_token),
            )
            .map_err(str::to_string)?;
            if !(200..300).contains(&status) {
                return Err("license request failed".to_string());
            }
            serde_json::from_str(&response_body).map_err(|_| {
                recorder.mark_last_error_class("invalid_response");
                "license response is invalid".to_string()
            })
        },
        download,
    )
}

pub(super) fn fetch_product_logos_with<F, G>(
    session_path: &Path,
    keys: &[String],
    license_endpoint: F,
    load_licenses: G,
    mut download: impl FnMut(&str, usize) -> Option<Vec<u8>>,
) -> Result<Vec<CachedProductLogo>, String>
where
    F: FnOnce() -> Result<String, String>,
    G: FnOnce(&str) -> Result<serde_json::Value, String>,
{
    if keys.len() > MAX_LOGOS {
        return Err("too many product logo keys".to_string());
    }
    let wanted = keys
        .iter()
        .filter(|key| within_schema_limit(key) && !key.chars().any(char::is_control))
        .map(|key| normalize_product_key(key))
        .filter(|key| !key.is_empty())
        .collect::<HashSet<_>>();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let directory = relative_root(session_path)
        .map(|root| root.join("databases").join("meta"))
        .ok_or_else(|| "cannot determine product metadata directory".to_string())?;
    let mut logos = Vec::new();
    let mut covered = HashSet::new();
    let mut total_image_bytes = 0usize;
    let mut total_json_bytes = 0usize;
    if let Ok(entries) = fs::read_dir(&directory) {
        let mut files = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                entry.file_type().ok()?.is_file().then(|| entry.path())
            })
            .take(MAX_LOGOS)
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            })
            .collect::<Vec<_>>();
        files.sort();
        for metadata_path in files.into_iter().take(MAX_LOGOS) {
            if logos.len() == MAX_LOGOS || total_image_bytes >= MAX_TOTAL_IMAGE_BYTES {
                break;
            }
            let Some(id) = metadata_path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if !valid_product_cache_id(id) {
                continue;
            }
            let Some(bytes) = read_bounded_file(&metadata_path, MAX_METADATA_BYTES) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let name = metadata
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let id_key = normalize_product_key(id);
            let name_key = normalize_product_key(name);
            if !wanted.contains(&id_key) && !wanted.contains(&name_key) {
                continue;
            }
            let Some(url) = find_product_image_url(&metadata) else {
                continue;
            };
            let remaining = MAX_TOTAL_IMAGE_BYTES.saturating_sub(total_image_bytes);
            let Some(image) = download(&url, remaining) else {
                continue;
            };
            let Some(data_url) = image_data_url_bounded(&image, remaining) else {
                continue;
            };
            let logo = CachedProductLogo {
                id: id.to_string(),
                name: name.to_string(),
                data_url,
            };
            let Ok(size) = serde_json::to_vec(&logo).map(|value| value.len()) else {
                continue;
            };
            if size > MAX_TOTAL_LOGO_JSON_BYTES.saturating_sub(total_json_bytes) {
                continue;
            }
            cache_remote_product_image(&directory, id, &image);
            total_image_bytes += image.len();
            total_json_bytes += size;
            covered.insert(id_key);
            if !name_key.is_empty() {
                covered.insert(name_key);
            }
            logos.push(logo);
        }
    }
    if wanted.difference(&covered).next().is_none() || logos.len() >= MAX_LOGOS {
        return Ok(logos);
    }
    let endpoint = license_endpoint()?;
    let Ok(licenses) = load_licenses(&endpoint) else {
        return Ok(logos);
    };
    let licenses = licenses.get("data").unwrap_or(&licenses);
    if let Some(rows) = licenses.as_array() {
        for license in rows {
            if logos.len() >= MAX_LOGOS || total_image_bytes >= MAX_TOTAL_IMAGE_BYTES {
                break;
            }
            let remaining_keys = wanted.difference(&covered).cloned().collect::<HashSet<_>>();
            let Some(identity) = matching_license_identity(license, &remaining_keys) else {
                continue;
            };
            let Some(url) = find_product_image_url(license) else {
                continue;
            };
            let remaining = MAX_TOTAL_IMAGE_BYTES.saturating_sub(total_image_bytes);
            let Some(image) = download(&url, remaining) else {
                continue;
            };
            let Some(data_url) = image_data_url_bounded(&image, remaining) else {
                continue;
            };
            let logo = CachedProductLogo {
                id: identity.to_string(),
                name: String::new(),
                data_url,
            };
            let Ok(size) = serde_json::to_vec(&logo).map(|value| value.len()) else {
                continue;
            };
            if size > MAX_TOTAL_LOGO_JSON_BYTES.saturating_sub(total_json_bytes) {
                continue;
            }
            if let Some(cache_id) = license.get("id").and_then(serde_json::Value::as_str) {
                cache_remote_product_image(&directory, cache_id, &image);
            }
            total_image_bytes += image.len();
            total_json_bytes += size;
            covered.insert(normalize_product_key(identity));
            logos.push(logo);
        }
    }
    Ok(logos)
}

fn matching_license_identity<'a>(
    value: &'a serde_json::Value,
    wanted: &HashSet<String>,
) -> Option<&'a str> {
    match value {
        serde_json::Value::Object(fields) => fields.iter().find_map(|(key, value)| {
            let field = normalized_field_name(key);
            if matches!(field.as_str(), "id" | "productid" | "databaseid" | "name") {
                if let Some(value) = value.as_str() {
                    if wanted.contains(&normalize_product_key(value)) {
                        return Some(value);
                    }
                }
            }
            matching_license_identity(value, wanted)
        }),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|value| matching_license_identity(value, wanted)),
        _ => None,
    }
}

fn find_product_image_url(value: &serde_json::Value) -> Option<String> {
    fn scan(value: &serde_json::Value, context: bool) -> Option<String> {
        match value {
            serde_json::Value::Object(fields) => fields.iter().find_map(|(key, value)| {
                let image_context = context || is_image_field(key);
                if is_image_field(key) || (image_context && is_url_field(key)) {
                    if let Some(url) = value.as_str().and_then(trusted_product_image_url) {
                        return Some(url);
                    }
                }
                scan(value, image_context)
            }),
            serde_json::Value::Array(values) => {
                values.iter().find_map(|value| scan(value, context))
            }
            serde_json::Value::String(value) if context => trusted_product_image_url(value),
            _ => None,
        }
    }
    scan(value, false)
}

fn is_image_field(key: &str) -> bool {
    let field = normalized_field_name(key);
    ["image", "icon", "logo", "profile"]
        .iter()
        .any(|needle| field.contains(needle))
}

fn is_url_field(key: &str) -> bool {
    normalized_field_name(key).ends_with("url")
}

fn normalized_field_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn trusted_product_image_url(value: &str) -> Option<String> {
    if value.len() > 2048 {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let trusted = host == "dreamtonics.com"
        || host.ends_with(".dreamtonics.com")
        || host == "dreamtonics.com.cn"
        || host.ends_with(".dreamtonics.com.cn");
    (url.scheme() == "https"
        && trusted
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none())
    .then(|| url.to_string())
}

fn download_product_image(
    url: &str,
    max_bytes: usize,
    recorder: &crate::request_trace::UpstreamAttemptRecorder,
) -> Option<Vec<u8>> {
    trusted_product_image_url(url)?;
    crate::request_trace::recorded_binary_get(recorder, "image_download", url, max_bytes, || {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(std::time::Duration::from_secs(15))
            .build();
        let response = match agent.get(url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(_) => return Err("upstream request failed"),
        };
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "upstream response could not be read")?;
        Ok((status, bytes))
    })
}

fn image_data_url_bounded(bytes: &[u8], max_bytes: usize) -> Option<String> {
    (bytes.len() <= MAX_IMAGE_BYTES.min(max_bytes))
        .then(|| image_data_url(bytes))
        .flatten()
}

fn image_data_url(bytes: &[u8]) -> Option<String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn cache_remote_product_image(directory: &Path, id: &str, bytes: &[u8]) {
    if !valid_product_cache_id(id)
        || bytes.len() > MAX_IMAGE_BYTES
        || image_data_url(bytes).is_none()
    {
        return;
    }
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let extension = match image_data_url(bytes).as_deref() {
        Some(value) if value.starts_with("data:image/png;") => "png",
        Some(value) if value.starts_with("data:image/jpeg;") => "jpg",
        Some(value) if value.starts_with("data:image/gif;") => "gif",
        Some(value) if value.starts_with("data:image/webp;") => "webp",
        _ => return,
    };
    if let Ok(mut file) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(format!("{id}.{extension}")))
    {
        use std::io::Write;
        let _ = file.write_all(bytes);
    }
}

fn valid_product_cache_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn normalize_product_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn read_bounded_file(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    let max_bytes_u64 = u64::try_from(max_bytes).ok()?;
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes_u64 {
        return None;
    }
    let limit = max_bytes_u64.saturating_add(1);
    let capacity = usize::try_from(metadata.len()).ok()?;
    let mut bytes = Vec::with_capacity(capacity);
    fs::File::open(path)
        .ok()?
        .take(limit)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= max_bytes).then_some(bytes)
}

fn within_schema_limit(value: &str) -> bool {
    value.encode_utf16().count() <= 256
}

fn parse_translation_line(line: &str) -> Option<(String, String)> {
    let (source, target) = line.split_once('=')?;
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() || target.is_empty() || source.len() > 512 || target.len() > 512 {
        return None;
    }
    Some((source.to_string(), target.to_string()))
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
