use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Serialize;

const MAX_CATALOG_BYTES: usize = 512 * 1024;
const MAX_TOTAL_CATALOG_BYTES: usize = 512 * 1024;
const MAX_TOTAL_CATALOG_JSON_BYTES: usize = 512 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_CATALOGS: usize = 32;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerTranslationCatalog {
    pub locale: String,
    pub language: String,
    pub entries: BTreeMap<String, String>,
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

fn relative_root(session_path: &Path) -> Option<PathBuf> {
    session_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
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

fn parse_translation_line(line: &str) -> Option<(String, String)> {
    let (source, target) = line.split_once('=')?;
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() || target.is_empty() || source.len() > 512 || target.len() > 512 {
        return None;
    }
    Some((source.to_string(), target.to_string()))
}
