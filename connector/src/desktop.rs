use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use boxy::{Config, ConnectorHost, LaunchOptions};
use tauri::{
    webview::{DownloadEvent, NewWindowFeatures, NewWindowResponse},
    AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use url::Url;

struct RuntimeState {
    options: LaunchOptions,
    started: AtomicBool,
    stopping: AtomicBool,
    finished: AtomicBool,
    next_window: AtomicU64,
    worker: Mutex<Option<thread::JoinHandle<i32>>>,
    #[cfg(windows)]
    profile: std::path::PathBuf,
}

struct DesktopHost {
    app: AppHandle,
    state: Arc<RuntimeState>,
    startup: StartupReply,
}

type StartupReply = Arc<Mutex<Option<mpsc::SyncSender<Result<(), String>>>>>;

impl ConnectorHost for DesktopHost {
    fn open_editor(&self, url: Url) -> Result<(), String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let app = self.app.clone();
        let state = self.state.clone();
        let startup = self.startup.clone();
        self.app
            .run_on_main_thread(move || {
                let result = if state.stopping.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    build_window(&app, &state, "editor", url, None).map(|_| {
                        if let Some(window) = app.get_webview_window("service-selection") {
                            let _ = window.destroy();
                        }
                        if let Some(sender) = startup.lock().unwrap().take() {
                            let _ = sender.send(Ok(()));
                        }
                    })
                };
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv()
            .map_err(|_| "cannot open the cloud editor window".to_string())?
    }

    fn approve_command(&self, command: &str, shell: &str, cwd: &Path) -> bool {
        let description = format!("Shell: {shell}\nDirectory: {}\n\n{command}", cwd.display());
        let (sender, receiver) = mpsc::sync_channel(1);
        let state = self.state.clone();
        if self
            .app
            .run_on_main_thread(move || {
                let approved = !state.stopping.load(Ordering::Acquire)
                    && rfd::MessageDialog::new()
                        .set_title("Boxy — Approve command")
                        .set_description(description)
                        .set_level(rfd::MessageLevel::Warning)
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        == rfd::MessageDialogResult::Yes;
                let _ = sender.send(approved);
            })
            .is_err()
        {
            return false;
        }
        receiver.recv().unwrap_or(false)
    }

    fn should_stop(&self) -> bool {
        self.state.stopping.load(Ordering::Acquire)
    }
}

pub fn run(options: LaunchOptions) -> Result<i32, String> {
    let runtime = tempfile::Builder::new()
        .prefix("boxy-runtime-")
        .tempdir()
        .map_err(|error| format!("cannot create temporary browser data: {error}"))?;
    let state = Arc::new(RuntimeState {
        options,
        started: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        next_window: AtomicU64::new(1),
        worker: Mutex::new(None),
        #[cfg(windows)]
        profile: runtime.path().join("webview"),
    });
    // WebView2 environment overrides would bypass the owned temporary profile.
    std::env::remove_var("WEBVIEW2_USER_DATA_FOLDER");
    let setup_state = state.clone();
    let app = tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![select_service])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            if let Some(server) = setup_state.options.server_url.clone() {
                begin_connection(app.handle(), &setup_state, &server, None)
                    .map_err(std::io::Error::other)?;
            } else {
                build_service_selection(app.handle(), &setup_state)
                    .map_err(std::io::Error::other)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|error| format!("cannot initialize the embedded browser: {error}"))?;
    let events_state = state.clone();
    let code = app.run_return(move |app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            if !events_state.finished.load(Ordering::Acquire) {
                api.prevent_exit();
                request_stop(app, &events_state);
            }
        }
    });
    let joined = if let Some(join) = state.worker.lock().unwrap().take() {
        join.join()
            .map_err(|_| "the local connector stopped unexpectedly".to_string())
    } else {
        Ok(code)
    };
    drop(state);
    let cleaned = cleanup_runtime(runtime);
    let code = joined?;
    cleaned?;
    Ok(code)
}

#[tauri::command]
async fn select_service(
    window: WebviewWindow,
    app: AppHandle,
    state: tauri::State<'_, Arc<RuntimeState>>,
    server_url: String,
) -> Result<(), String> {
    if window.label() != "service-selection"
        || !local_selection_url(&window.url().map_err(|error| error.to_string())?)
    {
        return Err("service selection is only available in the local startup window".to_string());
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let state = state.inner().clone();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(error) = begin_connection(&handle, &state, &server_url, Some(sender.clone())) {
            let _ = sender.send(Err(error));
        }
    })
    .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv()
            .map_err(|_| "connection attempt stopped".to_string())?
    })
    .await
    .map_err(|error| error.to_string())?
}

fn begin_connection(
    app: &AppHandle,
    state: &Arc<RuntimeState>,
    server_url: &str,
    reply: Option<mpsc::SyncSender<Result<(), String>>>,
) -> Result<(), String> {
    if state.stopping.load(Ordering::Acquire) {
        return Err("the connector is closing".to_string());
    }
    if state.started.swap(true, Ordering::AcqRel) {
        return Err("a connection is already in progress".to_string());
    }
    let config = match boxy::resolve_config(&state.options, server_url) {
        Ok(config) => config,
        Err(error) => {
            state.started.store(false, Ordering::Release);
            if state.stopping.load(Ordering::Acquire) {
                finish(app, state, 0);
            }
            return Err(error);
        }
    };
    if state.stopping.load(Ordering::Acquire) {
        finish(app, state, 0);
        return Err("the connector is closing".to_string());
    }
    if let Some(previous) = state.worker.lock().unwrap().take() {
        let _ = previous.join();
    }
    let app = app.clone();
    let worker_state = state.clone();
    *state.worker.lock().unwrap() = Some(thread::spawn(move || {
        run_worker(app, worker_state, config, Arc::new(Mutex::new(reply)))
    }));
    Ok(())
}

fn run_worker(
    app: AppHandle,
    state: Arc<RuntimeState>,
    config: Config,
    startup: StartupReply,
) -> i32 {
    let host = DesktopHost {
        app: app.clone(),
        state: state.clone(),
        startup: startup.clone(),
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        boxy::run_connected(&config, &host)
    }))
    .unwrap_or_else(|_| Err("the local connector stopped unexpectedly".to_string()));
    let retryable = startup.lock().unwrap().is_some() && !state.stopping.load(Ordering::Acquire);
    let code = i32::from(result.is_err() && !state.stopping.load(Ordering::Acquire) && !retryable);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let pending = startup.lock().unwrap().take();
        if let Some(sender) = pending {
            if !state.stopping.load(Ordering::Acquire) {
                state.started.store(false, Ordering::Release);
                let _ = sender.send(Err(result
                    .err()
                    .unwrap_or_else(|| "connection attempt stopped".to_string())));
                return;
            }
            let _ = sender.send(Err("the connector is closing".to_string()));
        }
        if let Err(error) = result {
            if !state.stopping.load(Ordering::Acquire) {
                rfd::MessageDialog::new()
                    .set_title("Boxy")
                    .set_description(error)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
        }
        finish(&handle, &state, code);
    });
    code
}

fn finish(app: &AppHandle, state: &RuntimeState, code: i32) {
    state.finished.store(true, Ordering::Release);
    let windows = app.webview_windows();
    if windows.is_empty() {
        app.exit(code);
    } else {
        for window in windows.values() {
            let _ = window.destroy();
        }
    }
}

fn build_service_selection(app: &AppHandle, state: &Arc<RuntimeState>) -> Result<(), String> {
    let navigation_state = state.clone();
    let mut builder = WebviewWindowBuilder::new(
        app,
        "service-selection",
        WebviewUrl::App("index.html".into()),
    )
    .title("Boxy")
    .inner_size(560.0, 420.0)
    .min_inner_size(440.0, 360.0)
    .incognito(true)
    .on_navigation(move |url| {
        !navigation_state.stopping.load(Ordering::Acquire) && local_selection_url(url)
    })
    .on_new_window(|_, _| NewWindowResponse::Deny);
    #[cfg(windows)]
    {
        builder = builder.data_directory(state.profile.clone());
    }
    let window = builder.build().map_err(|error| error.to_string())?;
    let app = app.clone();
    let state = state.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            request_stop(&app, &state);
        }
    });
    Ok(())
}

fn local_selection_url(url: &Url) -> bool {
    let origin = match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    };
    origin
        && matches!(url.path(), "/" | "/index.html")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn build_window(
    app: &AppHandle,
    state: &Arc<RuntimeState>,
    label: &str,
    url: Url,
    features: Option<NewWindowFeatures>,
) -> Result<WebviewWindow, String> {
    let navigation_state = state.clone();
    let popup_app = app.clone();
    let popup_state = state.clone();
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title("Boxy")
        .inner_size(1180.0, 820.0)
        .min_inner_size(760.0, 540.0)
        .incognito(true)
        .on_navigation(move |url| {
            !navigation_state.stopping.load(Ordering::Acquire) && allowed_navigation(url)
        })
        .on_new_window(move |url, features| {
            if popup_state.stopping.load(Ordering::Acquire) || !allowed_navigation(&url) {
                return NewWindowResponse::Deny;
            }
            let label = format!(
                "popup-{}",
                popup_state.next_window.fetch_add(1, Ordering::Relaxed)
            );
            match build_window(
                &popup_app,
                &popup_state,
                &label,
                Url::parse("about:blank").unwrap(),
                Some(features),
            ) {
                Ok(window) => NewWindowResponse::Create { window },
                Err(_) => NewWindowResponse::Deny,
            }
        })
        .on_download(|_, event| {
            if let DownloadEvent::Requested { destination, .. } = event {
                let suggested = destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("download");
                match rfd::FileDialog::new().set_file_name(suggested).save_file() {
                    Some(path) => *destination = path,
                    None => return false,
                }
            }
            true
        });
    #[cfg(windows)]
    {
        builder = builder.data_directory(state.profile.clone());
    }
    if let Some(features) = features {
        builder = builder.window_features(features);
    }
    let window = builder.build().map_err(|error| error.to_string())?;
    if label == "editor" {
        let app = app.clone();
        let state = state.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                request_stop(&app, &state);
            }
        });
    }
    Ok(window)
}

fn request_stop(app: &AppHandle, state: &RuntimeState) {
    state.stopping.store(true, Ordering::Release);
    if !state.started.load(Ordering::Acquire) {
        finish(app, state, 0);
        return;
    }
    for window in app.webview_windows().values() {
        let _ = window.hide();
    }
}

fn allowed_navigation(url: &Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost")),
        "about" => url.as_str() == "about:blank",
        "blob" => Url::parse(url.path())
            .map(|inner| allowed_navigation(&inner))
            .unwrap_or(false),
        _ => false,
    }
}

fn cleanup_runtime(runtime: tempfile::TempDir) -> Result<(), String> {
    let path = runtime.keep();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match std::fs::remove_dir_all(&path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(format!("cannot remove temporary browser data: {error}")),
        }
    }
}

#[cfg(test)]
#[path = "../../test/desktop.rs"]
mod tests;
