use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use boxy::{Config, ConnectorHost};
use tauri::{
    webview::{DownloadEvent, NewWindowFeatures, NewWindowResponse},
    AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use url::Url;

struct RuntimeState {
    stopping: AtomicBool,
    finished: AtomicBool,
    next_window: AtomicU64,
    #[cfg(windows)]
    profile: std::path::PathBuf,
}

struct DesktopHost {
    app: AppHandle,
    state: Arc<RuntimeState>,
}

impl ConnectorHost for DesktopHost {
    fn open_editor(&self, url: Url) -> Result<(), String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let app = self.app.clone();
        let state = self.state.clone();
        self.app
            .run_on_main_thread(move || {
                let result = if state.stopping.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    build_window(&app, &state, "editor", url, None).map(|_| ())
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

pub fn run(config: Config) -> Result<i32, String> {
    let runtime = tempfile::Builder::new()
        .prefix("boxy-runtime-")
        .tempdir()
        .map_err(|error| format!("cannot create temporary browser data: {error}"))?;
    let state = Arc::new(RuntimeState {
        stopping: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        next_window: AtomicU64::new(1),
        #[cfg(windows)]
        profile: runtime.path().join("webview"),
    });
    // WebView2 environment overrides would bypass the owned temporary profile.
    std::env::remove_var("WEBVIEW2_USER_DATA_FOLDER");
    let worker = Arc::new(Mutex::new(None));
    let setup_worker = worker.clone();
    let setup_state = state.clone();
    let app = tauri::Builder::default()
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            let handle = app.handle().clone();
            let state = setup_state.clone();
            let join = thread::spawn(move || {
                let host = DesktopHost {
                    app: handle.clone(),
                    state: state.clone(),
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    boxy::run_connected(&config, &host)
                }))
                .unwrap_or_else(|_| Err("the local connector stopped unexpectedly".to_string()));
                let code = if let Err(error) = result {
                    if !state.stopping.load(Ordering::Acquire) {
                        let _ = handle.run_on_main_thread(move || {
                            rfd::MessageDialog::new()
                                .set_title("Boxy")
                                .set_description(error)
                                .set_level(rfd::MessageLevel::Error)
                                .show();
                        });
                        1
                    } else {
                        0
                    }
                } else {
                    0
                };
                state.finished.store(true, Ordering::Release);
                let closing_app = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    let windows = closing_app.webview_windows();
                    if windows.is_empty() {
                        closing_app.exit(code);
                    } else {
                        for window in windows.values() {
                            let _ = window.destroy();
                        }
                    }
                });
                code
            });
            *setup_worker.lock().unwrap() = Some(join);
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
    let joined = if let Some(join) = worker.lock().unwrap().take() {
        join.join()
            .map_err(|_| "the local connector stopped unexpectedly".to_string())
    } else {
        Ok(code)
    };
    drop(worker);
    drop(state);
    let cleaned = cleanup_runtime(runtime);
    let code = joined?;
    cleaned?;
    Ok(code)
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
