use std::{
    env, fs,
    path::{Path, PathBuf},
};

use rfd::FileDialog;
use url::Url;

const DEFAULT_SERVICE_URL: &str = "https://boxy.voidcarve.com";

pub(crate) struct LaunchOptions {
    pub(crate) server_url: String,
    pub(crate) session_path: PathBuf,
    pub(crate) session_path_explicit: bool,
}

pub(crate) struct LaunchArguments {
    pub(crate) server_url: Option<String>,
    pub(crate) session_path: Option<PathBuf>,
}

impl LaunchOptions {
    pub(crate) fn from_environment_and_args(default_session_path: PathBuf) -> Result<Self, String> {
        let arguments = parse_arguments(env::args().skip(1))?;
        let configured_server = env::var("BOXY_API_URL").ok();
        let executable = env::current_exe().ok();
        let server_url = resolve_server_url(
            arguments.server_url.as_deref(),
            configured_server.as_deref(),
            executable.as_deref(),
        )?;
        let session_path_explicit = arguments.session_path.is_some();
        Ok(Self {
            server_url,
            session_path: arguments.session_path.unwrap_or(default_session_path),
            session_path_explicit,
        })
    }
}

pub(crate) fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<LaunchArguments, String> {
    let mut server_url = None;
    let mut session_path = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--server" => {
                server_url = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--server requires a URL".to_string())?,
                )
            }
            "--session" => {
                session_path = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--session requires a path".to_string())?,
                ))
            }
            "--help" | "-h" => {
                return Err("usage: boxy [--server URL] [--session PATH]".to_string())
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(LaunchArguments {
        server_url,
        session_path,
    })
}

pub(crate) fn resolve_server_url(
    command_line: Option<&str>,
    environment: Option<&str>,
    executable: Option<&Path>,
) -> Result<String, String> {
    let value = command_line
        .or(environment)
        .map(str::to_string)
        .or_else(|| executable.and_then(inferred_service_url))
        .unwrap_or_else(|| DEFAULT_SERVICE_URL.to_string());
    validate_service_url(&value)
}

pub(crate) fn resolve_session_path(path: PathBuf, explicit: bool) -> Result<PathBuf, String> {
    if validate_session_path(&path).is_ok() {
        return Ok(path);
    }
    if explicit {
        return Err("The selected SV2 session path is not an existing file.".to_string());
    }

    let mut picker = FileDialog::new().set_title("Select Synthesizer V session");
    if let Some(parent) = path.parent().filter(|parent| parent.is_dir()) {
        picker = picker.set_directory(parent);
    }
    let selected = picker
        .pick_file()
        .ok_or_else(|| "A Synthesizer V session file must be selected.".to_string())?;
    validate_session_path(&selected)?;
    Ok(selected)
}

fn validate_session_path(path: &Path) -> Result<(), String> {
    if fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err("The selected SV2 session path is not an existing file.".to_string())
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

fn validate_service_url(value: &str) -> Result<String, String> {
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
