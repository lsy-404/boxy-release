use std::env;

use url::Url;

#[derive(Clone, Debug, Default)]
pub struct LaunchOptions;

#[derive(Clone, Debug)]
pub struct Config {
    pub server_url: String,
}

impl LaunchOptions {
    pub(crate) fn from_environment_and_args() -> Result<Self, String> {
        if env::args().len() > 1 {
            return Err("Boxy accepts the service address in its startup field.".to_string());
        }
        Ok(Self)
    }
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
