use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use serde::Deserialize;
use serde_json::json;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use url::Url;
use wry::{WebView, WebViewBuilder};

enum UiEvent {
    Command(UiCommand),
    Status(String),
    EditorReady(String),
    Finished(Result<(), String>),
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum UiCommand {
    Connect { url: String },
    OpenEditor,
    Stop,
}

#[derive(Clone)]
pub(crate) struct RunningStatus {
    proxy: EventLoopProxy<UiEvent>,
    cancelled: Arc<AtomicBool>,
}

impl RunningStatus {
    pub(crate) fn update(&self, message: &str) {
        let _ = self.proxy.send_event(UiEvent::Status(message.to_string()));
    }

    pub(crate) fn set_editor_url(&self, url: &str) {
        let _ = self.proxy.send_event(UiEvent::EditorReady(url.to_string()));
    }

    pub(crate) fn ensure_active(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err("Boxy was stopped".to_string())
        } else {
            Ok(())
        }
    }
}

pub(crate) fn run_window<F>(initial_url: &str, start: F) -> Result<i32, String>
where
    F: FnOnce(String, RunningStatus) -> Result<(), String> + Send + 'static,
{
    let event_loop = EventLoopBuilder::<UiEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Boxy")
        .with_inner_size(LogicalSize::new(560.0, 410.0))
        .with_resizable(false)
        .build(&event_loop)
        .map_err(|error| format!("cannot create Boxy window: {error}"))?;
    let ipc_proxy = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_html(render_html(initial_url))
        .with_ipc_handler(move |request| {
            if let Ok(command) = serde_json::from_str::<UiCommand>(request.body()) {
                let _ = ipc_proxy.send_event(UiEvent::Command(command));
            }
        })
        .build(&window)
        .map_err(|error| format!("cannot load Boxy interface: {error}"))?;

    let cancelled = Arc::new(AtomicBool::new(false));
    let mut start = Some(start);
    let mut running = false;
    let mut finished = false;
    let mut editor_url: Option<String> = None;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UiEvent::Command(UiCommand::Connect { url })) if !running => {
                match validate_service_url(&url) {
                    Ok(remote) => {
                        let Some(start) = start.take() else { return };
                        running = true;
                        window.set_inner_size(LogicalSize::new(520.0, 310.0));
                        send_ui(&webview, json!({ "kind": "connected", "remote": remote }));
                        let worker_proxy = proxy.clone();
                        let status = RunningStatus {
                            proxy: worker_proxy.clone(),
                            cancelled: Arc::clone(&cancelled),
                        };
                        thread::spawn(move || {
                            let result = start(remote, status);
                            let _ = worker_proxy.send_event(UiEvent::Finished(result));
                        });
                    }
                    Err(error) => send_ui(&webview, json!({ "kind": "error", "message": error })),
                }
            }
            Event::UserEvent(UiEvent::Command(UiCommand::OpenEditor)) => {
                if let Some(url) = &editor_url {
                    if let Err(error) = webbrowser::open(url) {
                        send_ui(&webview, json!({ "kind": "error", "message": format!("Cannot open browser editor: {error}") }));
                    }
                }
            }
            Event::UserEvent(UiEvent::Command(UiCommand::Stop)) => {
                if finished || !running {
                    *control_flow = ControlFlow::Exit;
                } else {
                    cancelled.store(true, Ordering::SeqCst);
                    send_ui(&webview, json!({ "kind": "status", "message": "Stopping Boxy after the current operation..." }));
                }
            }
            Event::UserEvent(UiEvent::Status(message)) => {
                send_ui(&webview, json!({ "kind": "status", "message": message }));
            }
            Event::UserEvent(UiEvent::EditorReady(url)) => {
                editor_url = Some(url);
                send_ui(&webview, json!({ "kind": "editorReady" }));
            }
            Event::UserEvent(UiEvent::Finished(result)) => {
                finished = true;
                if cancelled.load(Ordering::SeqCst) {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                match result {
                    Ok(()) => send_ui(&webview, json!({ "kind": "finished", "message": "The remote session was written back. You can close Boxy." })),
                    Err(error) => send_ui(&webview, json!({ "kind": "failed", "message": format!("Connection failed: {error}") })),
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                cancelled.store(true, Ordering::SeqCst);
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    })
}

fn send_ui(webview: &WebView, payload: serde_json::Value) {
    let _ = webview.evaluate_script(&format!("window.boxyUpdate({payload})"));
}

fn render_html(initial_url: &str) -> String {
    include_str!("../ui/index.html")
        .replace("{{FLUENT_CSS}}", include_str!("../ui/fluent.css"))
        .replace("{{BOXY_CSS}}", include_str!("../ui/styles.css"))
        .replace("{{BOXY_JS}}", include_str!("../ui/app.js"))
        .replace("{{BOXY_LOGO}}", include_str!("../../assets/boxy-pen.svg"))
        .replace("{{INITIAL_URL}}", &escape_html(initial_url))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
