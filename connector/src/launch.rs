use std::env;

use url::Url;

#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    pub server_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub server_url: String,
}

impl LaunchOptions {
    pub(crate) fn from_environment_and_args() -> Result<Self, String> {
        if env::args().len() > 1 {
            return Err("Boxy accepts the service address in its startup field.".to_string());
        }
        let server_url = env::current_exe()
            .ok()
            .and_then(|executable| inferred_service_url(&executable))
            .map(|value| validate_service_url(&value))
            .transpose()?;
        Ok(Self { server_url })
    }
}

pub(crate) fn inferred_service_url(executable: &std::path::Path) -> Option<String> {
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

pub(crate) fn validate_service_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    let value = if value.contains("://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let url = Url::parse(&value).map_err(|_| "Enter a valid service URL.".to_string())?;
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
