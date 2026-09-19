use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;

const MAX_IMAGE_BYTES: usize = 512 * 1024;
const MAX_CATALOG_BYTES: usize = 512 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_LOGOS: usize = 512;

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
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "txt"))
        .collect::<Vec<_>>();
    files.sort();
    let mut catalogs = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if text.len() > MAX_CATALOG_BYTES {
            continue;
        }
        let locale = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut language = locale.clone();
        let mut entries = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("language:") {
                language = value.trim().to_string();
            } else if let Some((source, target)) = parse_translation_line(line) {
                if entries.len() < MAX_ENTRIES {
                    entries.insert(source, target);
                }
            }
        }
        catalogs.push(ServerTranslationCatalog {
            locale,
            language,
            entries,
        });
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
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        })
        .collect::<Vec<_>>();
    files.sort();
    let mut logos = Vec::new();
    let mut covered = HashSet::new();
    for metadata_path in files {
        if logos.len() >= MAX_LOGOS {
            break;
        }
        let Some(id) = metadata_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Ok(metadata_text) = fs::read_to_string(&metadata_path) else {
            continue;
        };
        let name = serde_json::from_str::<serde_json::Value>(&metadata_text)
            .ok()
            .and_then(|metadata| {
                metadata
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let Some(data_url) = read_image(&metadata_path.with_extension("png")) else {
            continue;
        };
        covered.insert(normalize(id));
        logos.push(CachedProductLogo {
            id: id.to_string(),
            name,
            data_url,
        });
    }
    let Ok(images) = fs::read_dir(&directory) else {
        return logos;
    };
    for image_path in images.filter_map(Result::ok).map(|entry| entry.path()) {
        if logos.len() >= MAX_LOGOS {
            break;
        }
        if !image_path
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("png"))
        {
            continue;
        }
        let Some(id) = image_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if covered.contains(&normalize(id)) {
            continue;
        }
        let Some(data_url) = read_image(&image_path) else {
            continue;
        };
        covered.insert(normalize(id));
        logos.push(CachedProductLogo {
            id: id.to_string(),
            name: String::new(),
            data_url,
        });
    }
    logos
}

fn relative_root(session_path: &Path) -> Option<std::path::PathBuf> {
    session_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn read_image(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
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
