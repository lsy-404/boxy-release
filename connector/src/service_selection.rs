use std::path::Path;

use url::Url;

#[cfg(windows)]
#[path = "windows_service_dialog.rs"]
mod windows_service_dialog;

pub(crate) struct RunningStatus {
    #[cfg(windows)]
    inner: windows_service_dialog::RunningStatus,
}

impl RunningStatus {
    pub(crate) fn open(remote: &str) -> Result<Self, String> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows_service_dialog::RunningStatus::open(remote)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = remote;
            Ok(Self {})
        }
    }

    pub(crate) fn update(&self, message: &str) {
        #[cfg(windows)]
        self.inner.update(message);
        #[cfg(not(windows))]
        let _ = message;
    }

    pub(crate) fn set_editor_url(&self, url: &str) {
        #[cfg(windows)]
        self.inner.set_editor_url(url);
        #[cfg(not(windows))]
        let _ = url;
    }

    pub(crate) fn cancelled(&self) -> bool {
        #[cfg(windows)]
        {
            self.inner.cancelled()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    pub(crate) fn ensure_active(&self) -> Result<(), String> {
        if self.cancelled() {
            Err("Boxy was stopped".to_string())
        } else {
            Ok(())
        }
    }

    #[cfg(windows)]
    pub(crate) fn finish(&self, message: &str) {
        self.inner.finish(message);
    }

    #[cfg(windows)]
    pub(crate) fn wait_for_close(self) {
        self.inner.wait_for_close();
    }
}

#[cfg(not(windows))]
const STARTUP_NOTICE: &str = "Boxy is a local bridge to the remote service you choose. That service controls the browser editor and remote operations it requests. Boxy sends it an encrypted SV2 session and device information, including installed applications and a listing of the SV2 data directory. The selected service sees your public IP. Every shell command asks for separate approval.\n\nEnter the service address and choose OK to continue. Cancel exits. Keep Boxy open while assistance is active.";
#[cfg(not(windows))]
const SIGNING_KEY_NOTICE: &str = "Enter the selected service's base64url Ed25519 public key. Boxy uses this key to verify signed session writebacks. Only use a key supplied by the service owner you trust.";

pub(crate) fn choose_service(
    initial_url: &str,
    initial_key: &str,
) -> Result<Option<(String, String)>, String> {
    #[cfg(windows)]
    {
        windows_service_dialog::choose_service(initial_url, initial_key)
    }
    #[cfg(not(windows))]
    {
        let Some(url) = choose_service_url(initial_url) else {
            return Ok(None);
        };
        let Some(key) = choose_server_public_key(initial_key) else {
            return Ok(None);
        };
        Ok(Some((url, key)))
    }
}

#[cfg(not(windows))]
fn choose_service_url(initial: &str) -> Option<String> {
    if initial.contains('\0') {
        tinyfiledialogs::message_box_ok(
            "Boxy service address",
            "The initial service address contains an invalid character.",
            tinyfiledialogs::MessageBoxIcon::Error,
        );
        return None;
    }
    let mut value = initial.to_string();
    loop {
        let chosen = tinyfiledialogs::input_box("Boxy remote service", STARTUP_NOTICE, &value)?;
        match validate_service_url(&chosen) {
            Ok(url) => return Some(url),
            Err(error) => {
                tinyfiledialogs::message_box_ok(
                    "Invalid service address",
                    &error,
                    tinyfiledialogs::MessageBoxIcon::Error,
                );
                value = chosen;
            }
        }
    }
}

#[cfg(not(windows))]
fn choose_server_public_key(initial: &str) -> Option<String> {
    let mut value = initial.to_string();
    loop {
        let chosen =
            tinyfiledialogs::input_box("Remote service signing key", SIGNING_KEY_NOTICE, &value)?;
        match validate_server_public_key(&chosen) {
            Ok(key) => return Some(key.to_string()),
            Err(error) => {
                tinyfiledialogs::message_box_ok(
                    "Invalid signing key",
                    &error,
                    tinyfiledialogs::MessageBoxIcon::Error,
                );
                value = chosen;
            }
        }
    }
}

pub(crate) fn inferred_service_url(executable: &Path) -> Option<String> {
    let app_name = executable.ancestors().find_map(|path| {
        let name = path.file_name()?.to_str()?;
        name.to_ascii_lowercase()
            .ends_with(".app")
            .then(|| name.to_ascii_lowercase())
    });
    let name = app_name.or_else(|| {
        let name = executable.file_name()?.to_str()?.to_ascii_lowercase();
        name.ends_with(".exe").then_some(name)
    })?;
    let stem = name
        .strip_suffix(".app")
        .or_else(|| name.strip_suffix(".exe"))?;
    let mut hostname = None;
    for candidate in stem.split(|character: char| {
        !character.is_ascii_alphanumeric() && character != '-' && character != '.'
    }) {
        if valid_hostname(candidate)
            && hostname.is_none_or(|current: &str| candidate.len() > current.len())
        {
            hostname = Some(candidate);
        }
    }
    Some(format!("https://{}", hostname?))
}

fn valid_hostname(candidate: &str) -> bool {
    let labels: Vec<_> = candidate.split('.').collect();
    labels.len() >= 2
        && candidate.len() <= 248
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        && labels.last().is_some_and(|suffix| {
            suffix.len() >= 2 && suffix.bytes().all(|byte| byte.is_ascii_alphabetic())
        })
}

pub(crate) fn validate_server_public_key(value: &str) -> Result<&str, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let value = value.trim();
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "Enter a valid base64url service signing key.".to_string())?;
    if decoded.len() != 32 {
        return Err("The service signing key must decode to 32 bytes.".to_string());
    }
    Ok(value)
}

pub(crate) fn validate_service_url(value: &str) -> Result<String, String> {
    let url = Url::parse(value.trim()).map_err(|_| "Enter a valid service URL.".to_string())?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Use HTTPS unless the service is on this computer.".to_string());
    }
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Enter only the service origin, without credentials, a path, query, or fragment."
                .to_string(),
        );
    }
    Ok(url.origin().ascii_serialization())
}
