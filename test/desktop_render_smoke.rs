include!("../connector/src/desktop.rs");

fn main() {
    if let Err(error) = run_render_smoke() {
        eprintln!("macOS WebView render smoke failed: {error}");
        std::process::exit(1);
    }
    println!("WKWebView chooser rendered; service connected; scoped image loaded; outside image and remote IPC denied; signed receipt verified; normal exit cleared runtime data.");
}

fn run_render_smoke() -> Result<(), String> {
    let service_url = std::env::var("BOXY_RENDER_SMOKE_SERVICE_URL")
        .map_err(|_| "missing loopback fixture service URL".to_string())?;
    let fixture_state = std::env::var("BOXY_RENDER_SMOKE_STATE")
        .map_err(|_| "missing loopback fixture state path".to_string())?;
    let runtime = tempfile::Builder::new()
        .prefix("boxy-render-smoke-")
        .tempdir()
        .map_err(|error| format!("cannot create temporary runtime: {error}"))?;
    let runtime_path = runtime.path().to_path_buf();
    let options = boxy::prepare_config()?;
    if options.server_url.is_some() {
        return Err("the render smoke must start without a configured service".to_string());
    }
    let state = Arc::new(RuntimeState {
        options,
        started: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        next_window: AtomicU64::new(1),
        worker: Mutex::new(None),
        local_image_policy: Mutex::new(None),
        #[cfg(windows)]
        profile: runtime.path().join("webview"),
    });
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let setup_state = state.clone();
    let setup_service_url = service_url.clone();
    let setup_fixture_state = fixture_state.clone();
    let app = application_builder(state.clone())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            build_service_selection(app.handle(), &setup_state).map_err(std::io::Error::other)?;
            let window = app
                .get_webview_window("service-selection")
                .ok_or_else(|| std::io::Error::other("selection window was not created"))?;
            let controller_app = app.handle().clone();
            let controller_state = setup_state.clone();
            let result_sender = result_sender.clone();
            thread::spawn(move || {
                let result = drive_render_smoke(
                    window,
                    controller_app.clone(),
                    setup_service_url,
                    setup_fixture_state,
                );
                let (sender, receiver) = mpsc::sync_channel(1);
                let stop_app = controller_app.clone();
                if controller_app
                    .run_on_main_thread(move || {
                        request_stop(&stop_app, &controller_state);
                        let _ = sender.send(());
                    })
                    .is_ok()
                {
                    let _ = receiver.recv_timeout(Duration::from_secs(5));
                }
                let _ = result_sender.send(result);
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|error| format!("cannot initialize Tauri: {error}"))?;
    let events_state = state.clone();
    let code = app.run_return(move |app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            if !events_state.finished.load(Ordering::Acquire) {
                api.prevent_exit();
                request_stop(app, &events_state);
            }
        }
    });
    let worker_code = if let Some(worker) = state.worker.lock().unwrap().take() {
        Some(
            worker
                .join()
                .map_err(|_| "connector worker panicked".to_string())?,
        )
    } else {
        None
    };
    drop(state);
    cleanup_runtime(runtime)?;
    if runtime_path.exists() {
        return Err("temporary WebView runtime data remained after cleanup".to_string());
    }
    let controller_result = result_receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("render controller did not finish: {error}"))?;
    controller_result?;
    if let Some(code) = worker_code {
        if code != 0 {
            return Err(format!("connector worker exited with status {code}"));
        }
    }
    if code != 0 {
        return Err(format!("Tauri event loop exited with status {code}"));
    }
    Ok(())
}

fn drive_render_smoke(
    window: WebviewWindow,
    app: AppHandle,
    service_url: String,
    fixture_state: String,
) -> Result<(), String> {
    let mut last_snapshot = "no JavaScript callback received".to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut rendered = false;
    while Instant::now() < deadline {
        let (sender, receiver) = mpsc::sync_channel(1);
        window
            .eval_with_callback(
                "(() => { const input=document.getElementById('service-url'); const button=document.getElementById('connect'); const error=document.getElementById('error'); const rect=e=>{const r=e?.getBoundingClientRect();return {width:r?.width??0,height:r?.height??0};}; return {href:location.href,title:document.title,readyState:document.readyState,body:!!document.body,input:!!input,inputValue:input?.value??null,button:!!button,error:!!error,inputRect:rect(input),buttonRect:rect(button),borderRadius:input?getComputedStyle(input).borderRadius:null,bodyText:document.body?.innerText??''}; })()",
                move |value| {
                    let _ = sender.send(value);
                },
            )
            .map_err(|error| format!("cannot inspect WKWebView document: {error}"))?;
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(value) => {
                last_snapshot = value.clone();
                let snapshot: serde_json::Value = match serde_json::from_str(&value) {
                    Ok(snapshot) => snapshot,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(150));
                        continue;
                    }
                };
                rendered = snapshot["readyState"] == "complete"
                    && snapshot["title"] == "Boxy"
                    && snapshot["body"] == true
                    && snapshot["input"] == true
                    && snapshot["inputValue"] == ""
                    && snapshot["button"] == true
                    && snapshot["error"] == true
                    && snapshot["inputRect"]["width"].as_f64().unwrap_or_default() > 0.0
                    && snapshot["inputRect"]["height"].as_f64().unwrap_or_default() > 0.0
                    && snapshot["buttonRect"]["width"].as_f64().unwrap_or_default() > 0.0
                    && snapshot["buttonRect"]["height"]
                        .as_f64()
                        .unwrap_or_default()
                        > 0.0
                    && snapshot["borderRadius"] == "6px";
                if rendered {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        thread::sleep(Duration::from_millis(150));
    }
    if !rendered {
        let current_url = window.url().map(|url| url.to_string()).unwrap_or_default();
        let accepted = window
            .url()
            .map(|url| local_selection_url(&url))
            .unwrap_or(false);
        return Err(format!(
            "selection page did not render; native URL={current_url:?}, accepted={accepted}, WK snapshot={last_snapshot}"
        ));
    }
    let url = window
        .url()
        .map_err(|error| format!("cannot read selection window URL: {error}"))?;
    if !local_selection_url(&url) {
        return Err(format!(
            "rendered selection page has unexpected native URL: {url}"
        ));
    }

    let js_url = serde_json::to_string(&service_url).map_err(|error| error.to_string())?;
    let submit = format!(
        "(() => {{ const input=document.getElementById('service-url'); input.value={js_url}; document.getElementById('service-form').requestSubmit(); return {{submitted:true,inputValue:input.value}}; }})()"
    );
    eval_value(&window, submit)?;

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if Instant::now() >= deadline {
            return Err(
                "loopback fixture did not complete the image and signed task checks".to_string(),
            );
        }
        let state_json = match std::fs::read_to_string(&fixture_state) {
            Ok(value) => value,
            Err(_) => {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        let fixture: serde_json::Value = match serde_json::from_str(&state_json) {
            Ok(value) => value,
            Err(_) => {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        if fixture["requestErrors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(format!(
                "fixture rejected connector traffic: {}",
                fixture["requestErrors"]
            ));
        }
        if fixture["editorLoaded"] == true
            && fixture["readyFetch"] == true
            && fixture["cookieReturned"] == true
            && fixture["nativeIpcDenied"] == true
            && fixture["remoteSelectServiceDenied"] == true
            && fixture["operationStartCount"] == 1
            && fixture["startProtocolVersion"] == 3
            && fixture["signedPolls"].as_u64().unwrap_or_default() > 0
            && fixture["signedTaskReceipts"] == 1
            && fixture["readReceiptValid"] == true
            && fixture["localImageLoaded"] == true
            && fixture["outsideImageDenied"] == true
        {
            if app.get_webview_window("service-selection").is_some()
                || app.get_webview_window("editor").is_none()
            {
                return Err("the local chooser was not replaced by the remote editor".to_string());
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn eval_value(window: &WebviewWindow, script: String) -> Result<serde_json::Value, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    window
        .eval_with_callback(script, move |value| {
            let _ = sender.send(value);
        })
        .map_err(|error| format!("cannot evaluate WebView script: {error}"))?;
    let value = receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("WebView evaluation timed out: {error}"))?;
    serde_json::from_str(&value).map_err(|error| format!("invalid WebView result: {error}"))
}
