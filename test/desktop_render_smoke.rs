include!("../connector/src/desktop.rs");

fn main() {
    if let Err(error) = run_render_smoke() {
        eprintln!("Native startup smoke failed: {error}");
        std::process::exit(1);
    }
    println!("Native startup field, Enter submission, authorization gate, scoped file task, and close passed.");
}

fn run_render_smoke() -> Result<(), String> {
    let service_url = std::env::var("BOXY_RENDER_SMOKE_SERVICE_URL")
        .map_err(|_| "missing loopback service URL".to_string())?;
    let fixture_state = std::env::var("BOXY_RENDER_SMOKE_STATE")
        .map_err(|_| "missing loopback fixture state path".to_string())?;
    let runtime = tempfile::Builder::new()
        .prefix("boxy-render-smoke-")
        .tempdir()
        .map_err(|error| format!("cannot create temporary runtime: {error}"))?;
    let runtime_path = runtime.path().to_path_buf();
    let state = Arc::new(RuntimeState {
        options: LaunchOptions,
        started: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        next_window: AtomicU64::new(1),
        worker: Mutex::new(None),
        local_resource_policy: Mutex::new(None),
        #[cfg(windows)]
        profile: runtime.path().join("webview"),
    });
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let setup_state = state.clone();
    let setup_service = service_url.clone();
    let setup_fixture_state = fixture_state.clone();
    let app = application_builder(state.clone())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            build_service_selection(app.handle(), &setup_state).map_err(std::io::Error::other)?;
            let window = app
                .get_webview_window("service-selection")
                .ok_or_else(|| std::io::Error::other("startup window was not created"))?;
            let controller_app = app.handle().clone();
            let controller_state = setup_state.clone();
            let result_sender = result_sender.clone();
            thread::spawn(move || {
                let result = drive_render_smoke(window, setup_service, setup_fixture_state);
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
        .map_err(|error| format!("cannot initialize native WebView: {error}"))?;
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
        return Err("temporary WebView data remained after close".to_string());
    }
    let controller_result = result_receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("WebView controller did not finish: {error}"))?;
    controller_result?;
    if worker_code.is_some_and(|code| code != 0) || code != 0 {
        return Err("the desktop runtime exited with an error".to_string());
    }
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&fixture_state).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if fixture["receiptValid"] != true || fixture["closed"] != true {
        return Err(
            "the mock service did not verify the scoped task and connection close".to_string(),
        );
    }
    Ok(())
}

fn drive_render_smoke(
    window: WebviewWindow,
    service_url: String,
    fixture_state: String,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last_snapshot = serde_json::Value::Null;
    loop {
        if Instant::now() >= deadline {
            return Err(format!(
                "startup form did not render correctly: {last_snapshot}"
            ));
        }
        let snapshot = eval_value(
            &window,
            "(() => { const input=document.getElementById('service-url'); const rect=input?.getBoundingClientRect(); return {ready:document.readyState,title:document.title,inputCount:document.querySelectorAll('input').length,inputType:input?.type??null,inputValue:input?.value??null,visible:!!rect&&rect.width>0&&rect.height>0,headings:document.querySelectorAll('h1,h2,h3,h4,h5,h6').length,buttons:document.querySelectorAll('button').length,selectors:document.querySelectorAll('select,textarea').length,error:!!document.getElementById('error')}; })()".to_string(),
        );
        if let Ok(snapshot) = snapshot {
            last_snapshot = snapshot;
            if last_snapshot["ready"] == "complete"
                && last_snapshot["title"] == ""
                && last_snapshot["inputCount"] == 1
                && last_snapshot["inputType"] == "text"
                && last_snapshot["inputValue"] == ""
                && last_snapshot["visible"] == true
                && last_snapshot["headings"] == 0
                && last_snapshot["buttons"] == 0
                && last_snapshot["selectors"] == 0
                && last_snapshot["error"] == true
            {
                break;
            }
        }
        thread::sleep(Duration::from_millis(150));
    }

    if !local_selection_url(&window.url().map_err(|error| error.to_string())?) {
        return Err("startup form was not loaded from its local entry page".to_string());
    }
    let encoded_url = serde_json::to_string(&service_url).map_err(|error| error.to_string())?;
    let submit = format!(
        "(() => {{ const input=document.getElementById('service-url'); const form=document.getElementById('service-form'); let submitted=false; form.addEventListener('submit',()=>submitted=true,{{once:true}}); input.focus(); input.value={encoded_url}; input.dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',code:'Enter',bubbles:true}})); form.requestSubmit(); return {{submitted,inputValue:input.value}}; }})()"
    );
    let submission = eval_value(&window, submit)?;
    if submission["submitted"] != true || submission["inputValue"] != service_url {
        return Err("the Enter submission did not reach the startup form".to_string());
    }

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if Instant::now() >= deadline {
            return Err("the mock service did not complete the authorized task".to_string());
        }
        let contents = match std::fs::read_to_string(&fixture_state) {
            Ok(contents) => contents,
            Err(_) => {
                thread::sleep(Duration::from_millis(150));
                continue;
            }
        };
        if let Ok(fixture) = serde_json::from_str::<serde_json::Value>(&contents) {
            if fixture["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty())
            {
                return Err(format!(
                    "the mock service rejected a request: {}",
                    fixture["errors"]
                ));
            }
            if fixture["hello"]["protocolVersion"] == 4
                && fixture["polls"][0]["state"] == "awaiting_auth"
                && fixture["polls"][0]["taskCount"] == 0
                && fixture["taskSentAfterReady"] == true
                && fixture["browserLoaded"] == true
                && fixture["receiptValid"] == true
            {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(150));
    }
}

fn eval_value(window: &WebviewWindow, script: String) -> Result<serde_json::Value, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    window
        .eval_with_callback(script, move |value| {
            let _ = sender.send(value);
        })
        .map_err(|error| format!("cannot inspect native WebView: {error}"))?;
    let value = receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("native WebView evaluation timed out: {error}"))?;
    serde_json::from_str(&value).map_err(|error| format!("invalid WebView result: {error}"))
}
