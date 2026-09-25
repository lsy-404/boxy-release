use std::path::Path;
use url::Url;

const STARTUP_NOTICE: &str = "Boxy connects to the service you select. It sends an encrypted SV2 session, device information, public IP, installed applications, and a listing of the SV2 data directory. The service can request bounded local file and application actions. Every shell command asks for separate approval.\n\nEnter the service address and choose OK to continue. Cancel exits. Keep Boxy open while assistance is active.";

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
