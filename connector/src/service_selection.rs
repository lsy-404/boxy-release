use url::Url;

const STARTUP_NOTICE: &str = "Boxy is a local bridge to the remote service you choose. That service controls the browser editor and remote operations it requests. Boxy sends it an encrypted SV2 session and device information, including installed applications and a listing of the SV2 data directory. The selected service sees your public IP. Every shell command asks for separate approval.\n\nEnter the service address and choose OK to continue. Cancel exits. Keep Boxy open while assistance is active.";
const SIGNING_KEY_NOTICE: &str = "Enter the selected service's base64url Ed25519 public key. Boxy uses this key to verify signed session writebacks. Only use a key supplied by the service owner you trust.";

pub(crate) fn choose_service_url(initial: &str) -> Option<String> {
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

pub(crate) fn choose_server_public_key(initial: &str) -> Option<String> {
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
