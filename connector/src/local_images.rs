use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use percent_encoding::percent_decode_str;

pub(crate) const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

pub struct LocalImage {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

pub fn read(session_path: &Path, encoded_path: &str) -> Result<LocalImage, ()> {
    let relative = percent_decode_str(encoded_path)
        .decode_utf8()
        .map_err(|_| ())?;
    let relative = relative.strip_prefix('/').ok_or(())?;
    if relative.is_empty() || relative.starts_with('/') || relative.contains(['\\', '\0']) {
        return Err(());
    }
    if relative
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(());
    }
    let mut components = Path::new(relative).components();
    let first = components.next().ok_or(())?;
    let second = components.next().ok_or(())?;
    let remaining = match (first, second) {
        (Component::Normal(root), Component::Normal(child))
            if root == "databases" && child == "meta" =>
        {
            PathBuf::from("databases").join("meta")
        }
        (Component::Normal(root), Component::Normal(child))
            if root == "server-product-logos" && !child.is_empty() =>
        {
            PathBuf::from("server-product-logos")
        }
        _ => return Err(()),
    };
    if Path::new(relative)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(());
    }
    let root = session_path
        .parent()
        .and_then(Path::parent)
        .ok_or(())?
        .canonicalize()
        .map_err(|_| ())?;
    let target = root.join(relative);
    let allowed_dir = root.join(remaining);
    let canonical_allowed = allowed_dir.canonicalize().map_err(|_| ())?;
    if !canonical_allowed.starts_with(&root) || !canonical_allowed.is_dir() {
        return Err(());
    }
    let mut current = root.clone();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(|_| ())?;
        if metadata.file_type().is_symlink() {
            return Err(());
        }
    }
    let metadata = fs::symlink_metadata(&target).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return Err(());
    }
    let canonical_target = target.canonicalize().map_err(|_| ())?;
    if !canonical_target.starts_with(&canonical_allowed) {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).map_err(|_| ())?);
    fs::File::open(&canonical_target)
        .map_err(|_| ())?
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(());
    }
    let mime = image_mime(&bytes).ok_or(())?;
    Ok(LocalImage { mime, bytes })
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
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
