use std::{
    fs,
    io::Read,
    path::{Component, Path},
};

use percent_encoding::percent_decode_str;

pub(crate) const MAX_RESOURCE_BYTES: u64 = 8 * 1024 * 1024;

pub struct LocalResource {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

pub fn read(root: &Path, encoded_path: &str) -> Result<LocalResource, ()> {
    let relative = percent_decode_str(encoded_path)
        .decode_utf8()
        .map_err(|_| ())?;
    let relative = relative.strip_prefix('/').ok_or(())?;
    if relative.is_empty() || relative.starts_with('/') || relative.contains(['\\', '\0']) {
        return Err(());
    }
    if relative
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(());
    }
    if Path::new(relative)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(());
    }
    let root = root.canonicalize().map_err(|_| ())?;
    let mut target = root.clone();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        target.push(name);
        let metadata = fs::symlink_metadata(&target).map_err(|_| ())?;
        if metadata.file_type().is_symlink() {
            return Err(());
        }
    }
    let metadata = fs::symlink_metadata(&target).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > MAX_RESOURCE_BYTES {
        return Err(());
    }
    let target = target.canonicalize().map_err(|_| ())?;
    if !target.starts_with(&root) {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).map_err(|_| ())?);
    fs::File::open(&target)
        .map_err(|_| ())?
        .take(MAX_RESOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_RESOURCE_BYTES {
        return Err(());
    }
    let mime = mime(&bytes).ok_or(())?;
    Ok(LocalResource { mime, bytes })
}

fn mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}
