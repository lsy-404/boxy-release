use std::{
    iter,
    mem::size_of,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{GetStockObject, COLOR_WINDOW, DEFAULT_GUI_FONT, HBRUSH},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{EnableWindow, SetFocus},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem,
            GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
            IsDialogMessageW, LoadCursorW, LoadIconW, MessageBoxW, PostMessageW, PostQuitMessage,
            RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
            ShowWindow, TranslateMessage, BS_DEFPUSHBUTTON, ES_AUTOHSCROLL, GWLP_USERDATA,
            IDC_ARROW, MB_ICONERROR, MB_OK, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, WM_APP,
            WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSEXW, WS_CAPTION, WS_CHILD,
            WS_EX_APPWINDOW, WS_EX_CLIENTEDGE, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_SYSMENU,
            WS_TABSTOP, WS_VISIBLE,
        },
    },
};

use super::{validate_server_public_key, validate_service_url};

const URL_EDIT: i32 = 101;
const KEY_EDIT: i32 = 102;
const CONNECT_BUTTON: i32 = 103;
const CANCEL_BUTTON: i32 = 104;
const WIDTH: i32 = 700;
const HEIGHT: i32 = 410;
const NOTICE: &str = "Boxy is a local bridge. The remote you choose determines its available capabilities. Boxy sends an encrypted SV2 session and device details, including installed applications and an SV2 directory listing. Every shell command asks for separate approval.";

struct DialogState {
    selected: Option<(String, String)>,
}

pub(super) fn choose_service(
    initial_url: &str,
    initial_key: &str,
) -> Result<Option<(String, String)>, String> {
    unsafe { show_dialog(initial_url, initial_key) }
}

unsafe fn show_dialog(
    initial_url: &str,
    initial_key: &str,
) -> Result<Option<(String, String)>, String> {
    let instance = GetModuleHandleW(null());
    if instance.is_null() {
        return Err(format!(
            "cannot load Boxy application: {}",
            std::io::Error::last_os_error()
        ));
    }

    let class_name = wide("BoxyRemoteServiceWindow");
    let icon = LoadIconW(instance, 1 as *const u16);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: icon,
        hIconSm: icon,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszClassName: class_name.as_ptr(),
        ..WNDCLASSEXW::default()
    };
    if RegisterClassExW(&window_class) == 0 {
        return Err(format!(
            "cannot register Boxy window: {}",
            std::io::Error::last_os_error()
        ));
    }

    let title = wide("Boxy remote service");
    let x = (GetSystemMetrics(SM_CXSCREEN) - WIDTH).max(0) / 2;
    let y = (GetSystemMetrics(SM_CYSCREEN) - HEIGHT).max(0) / 2;
    let window = CreateWindowExW(
        WS_EX_APPWINDOW,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        x,
        y,
        WIDTH,
        HEIGHT,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if window.is_null() {
        return Err(format!(
            "cannot create Boxy window: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut state = DialogState { selected: None };
    SetWindowLongPtrW(
        window,
        GWLP_USERDATA,
        &mut state as *mut DialogState as isize,
    );
    let url_edit = match populate_window(window, instance, initial_url, initial_key) {
        Ok(edit) => edit,
        Err(error) => {
            DestroyWindow(window);
            return Err(error);
        }
    };

    ShowWindow(window, SW_SHOW);
    SetForegroundWindow(window);
    SetFocus(url_edit);
    let mut message = MSG::default();
    loop {
        let status = GetMessageW(&mut message, null_mut(), 0, 0);
        if status == -1 {
            DestroyWindow(window);
            return Err(format!(
                "Boxy window stopped: {}",
                std::io::Error::last_os_error()
            ));
        }
        if status == 0 {
            break;
        }
        if IsDialogMessageW(window, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(state.selected)
}

unsafe fn populate_window(
    window: HWND,
    instance: HINSTANCE,
    initial_url: &str,
    initial_key: &str,
) -> Result<HWND, String> {
    add_control(
        window,
        instance,
        "STATIC",
        NOTICE,
        [24, 20, 640, 92],
        WS_CHILD | WS_VISIBLE,
        0,
        0,
    )?;
    add_control(
        window,
        instance,
        "STATIC",
        "Remote service URL",
        [24, 122, 640, 22],
        WS_CHILD | WS_VISIBLE,
        0,
        0,
    )?;
    let url_edit = add_control(
        window,
        instance,
        "EDIT",
        initial_url,
        [24, 148, 640, 30],
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        URL_EDIT,
    )?;
    add_control(
        window,
        instance,
        "STATIC",
        "Remote service signing public key (base64url)",
        [24, 195, 640, 22],
        WS_CHILD | WS_VISIBLE,
        0,
        0,
    )?;
    add_control(
        window,
        instance,
        "EDIT",
        initial_key,
        [24, 221, 640, 30],
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        KEY_EDIT,
    )?;
    add_control(window, instance, "STATIC", "The URL may be suggested by this file's name. Confirm the remote and its key before connecting.", [24, 267, 640, 44], WS_CHILD | WS_VISIBLE, 0, 0)?;
    add_control(
        window,
        instance,
        "BUTTON",
        "Connect",
        [452, 326, 100, 32],
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        CONNECT_BUTTON,
    )?;
    add_control(
        window,
        instance,
        "BUTTON",
        "Cancel",
        [564, 326, 100, 32],
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        0,
        CANCEL_BUTTON,
    )?;
    Ok(url_edit)
}

unsafe fn add_control(
    parent: HWND,
    instance: HINSTANCE,
    class_name: &str,
    text: &str,
    bounds: [i32; 4],
    style: u32,
    extended_style: u32,
    id: i32,
) -> Result<HWND, String> {
    let class_name = wide(class_name);
    let text = wide(text);
    let control = CreateWindowExW(
        extended_style,
        class_name.as_ptr(),
        text.as_ptr(),
        style,
        bounds[0],
        bounds[1],
        bounds[2],
        bounds[3],
        parent,
        id as usize as _,
        instance,
        null(),
    );
    if control.is_null() {
        return Err(format!(
            "cannot create Boxy control: {}",
            std::io::Error::last_os_error()
        ));
    }
    let font = GetStockObject(DEFAULT_GUI_FONT);
    if !font.is_null() {
        SendMessageW(control, WM_SETFONT, font as usize, 1);
    }
    Ok(control)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => match (wparam & 0xffff) as i32 {
            CONNECT_BUTTON => {
                let url = read_control(window, URL_EDIT);
                let key = read_control(window, KEY_EDIT);
                let url = match validate_service_url(&url) {
                    Ok(url) => url,
                    Err(error) => {
                        show_error(window, &error);
                        return 0;
                    }
                };
                let key = match validate_server_public_key(&key) {
                    Ok(key) => key.to_string(),
                    Err(error) => {
                        show_error(window, &error);
                        return 0;
                    }
                };
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut DialogState;
                if !state.is_null() {
                    (*state).selected = Some((url, key));
                }
                DestroyWindow(window);
                return 0;
            }
            CANCEL_BUTTON => {
                DestroyWindow(window);
                return 0;
            }
            _ => {}
        },
        WM_CLOSE => {
            DestroyWindow(window);
            return 0;
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(window, message, wparam, lparam)
}

unsafe fn read_control(window: HWND, id: i32) -> String {
    let control = GetDlgItem(window, id);
    let length = GetWindowTextLengthW(control).max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let copied = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&buffer[..copied])
}

unsafe fn show_error(window: HWND, message: &str) {
    let message = wide(message);
    let title = wide("Boxy");
    MessageBoxW(
        window,
        message.as_ptr(),
        title.as_ptr(),
        MB_OK | MB_ICONERROR,
    );
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(iter::once(0)).collect()
}

const STATUS_UPDATE: u32 = WM_APP + 1;
const STATUS_EDITOR_READY: u32 = WM_APP + 2;
const STATUS_CLOSE: u32 = WM_APP + 3;
const EDITOR_BUTTON: i32 = 201;
const STOP_BUTTON: i32 = 202;
const STATUS_WIDTH: i32 = 510;
const STATUS_HEIGHT: i32 = 240;

struct SharedStatus {
    message: Mutex<String>,
    editor_url: Mutex<Option<String>>,
    cancelled: AtomicBool,
    finished: AtomicBool,
}

struct StatusUiState {
    shared: Arc<SharedStatus>,
    message_label: HWND,
    editor_button: HWND,
    stop_button: HWND,
}

pub(super) struct RunningStatus {
    window: isize,
    shared: Arc<SharedStatus>,
    thread: Option<JoinHandle<()>>,
}

impl RunningStatus {
    pub(super) fn open(remote: &str) -> Result<Self, String> {
        let shared = Arc::new(SharedStatus {
            message: Mutex::new("Preparing the encrypted local session...".to_string()),
            editor_url: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        let ui_shared = Arc::clone(&shared);
        let remote = remote.to_string();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let mut state = StatusUiState {
                shared: ui_shared,
                message_label: null_mut(),
                editor_button: null_mut(),
                stop_button: null_mut(),
            };
            unsafe {
                match create_status_window(&remote, &mut state) {
                    Ok(window) => {
                        let _ = ready_tx.send(Ok(window as isize));
                        status_message_loop(window, &state);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            }
        });
        let window = ready_rx
            .recv()
            .map_err(|_| "cannot start Boxy status window".to_string())??;
        Ok(Self {
            window,
            shared,
            thread: Some(thread),
        })
    }

    pub(super) fn update(&self, message: &str) {
        if self.cancelled() && !self.shared.finished.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut current) = self.shared.message.lock() {
            *current = message.to_string();
            unsafe {
                PostMessageW(self.window as HWND, STATUS_UPDATE, 0, 0);
            }
        }
    }

    pub(super) fn set_editor_url(&self, url: &str) {
        if let Ok(mut current) = self.shared.editor_url.lock() {
            *current = Some(url.to_string());
            unsafe {
                PostMessageW(self.window as HWND, STATUS_EDITOR_READY, 0, 0);
            }
        }
    }

    pub(super) fn cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::SeqCst)
    }

    pub(super) fn finish(&self, message: &str) {
        self.shared.finished.store(true, Ordering::SeqCst);
        self.update(message);
    }

    pub(super) fn wait_for_close(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.window = 0;
    }
}

impl Drop for RunningStatus {
    fn drop(&mut self) {
        if self.window != 0 {
            unsafe {
                PostMessageW(self.window as HWND, STATUS_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

unsafe fn create_status_window(remote: &str, state: &mut StatusUiState) -> Result<HWND, String> {
    let instance = GetModuleHandleW(null());
    if instance.is_null() {
        return Err(format!(
            "cannot load Boxy application: {}",
            std::io::Error::last_os_error()
        ));
    }
    let class_name = wide("BoxyConnectionStatusWindow");
    let icon = LoadIconW(instance, 1 as *const u16);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(status_window_proc),
        hInstance: instance,
        hIcon: icon,
        hIconSm: icon,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszClassName: class_name.as_ptr(),
        ..WNDCLASSEXW::default()
    };
    if RegisterClassExW(&window_class) == 0 {
        return Err(format!(
            "cannot register Boxy status window: {}",
            std::io::Error::last_os_error()
        ));
    }
    let title = wide("Boxy connection status");
    let x = (GetSystemMetrics(SM_CXSCREEN) - STATUS_WIDTH - 24).max(0);
    let y = (GetSystemMetrics(SM_CYSCREEN) - STATUS_HEIGHT - 72).max(0);
    let window = CreateWindowExW(
        WS_EX_APPWINDOW | WS_EX_TOPMOST,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        x,
        y,
        STATUS_WIDTH,
        STATUS_HEIGHT,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if window.is_null() {
        return Err(format!(
            "cannot create Boxy status window: {}",
            std::io::Error::last_os_error()
        ));
    }
    SetWindowLongPtrW(window, GWLP_USERDATA, state as *mut StatusUiState as isize);
    let controls = (|| -> Result<(), String> {
        add_control(
            window,
            instance,
            "STATIC",
            &format!("Remote: {remote}"),
            [20, 18, 465, 40],
            WS_CHILD | WS_VISIBLE,
            0,
            0,
        )?;
        state.message_label = add_control(
            window,
            instance,
            "STATIC",
            "Preparing the encrypted local session...",
            [20, 68, 465, 70],
            WS_CHILD | WS_VISIBLE,
            0,
            0,
        )?;
        state.editor_button = add_control(
            window,
            instance,
            "BUTTON",
            "Open browser editor",
            [20, 155, 210, 34],
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            EDITOR_BUTTON,
        )?;
        EnableWindow(state.editor_button, 0);
        state.stop_button = add_control(
            window,
            instance,
            "BUTTON",
            "Stop Boxy",
            [375, 155, 110, 34],
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            STOP_BUTTON,
        )?;
        Ok(())
    })();
    if let Err(error) = controls {
        DestroyWindow(window);
        return Err(error);
    }
    ShowWindow(window, SW_SHOW);
    SetForegroundWindow(window);
    Ok(window)
}

unsafe fn status_message_loop(window: HWND, state: &StatusUiState) {
    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&mut message, null_mut(), 0, 0);
        if result <= 0 {
            if result < 0 {
                state.shared.cancelled.store(true, Ordering::SeqCst);
            }
            break;
        }
        if IsDialogMessageW(window, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn status_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = (GetWindowLongPtrW(window, GWLP_USERDATA) as *const StatusUiState).as_ref();
    match message {
        STATUS_UPDATE => {
            if let Some(state) = state {
                let current = state.shared.message.lock().ok().map(|value| value.clone());
                if let Some(current) = current {
                    SetWindowTextW(state.message_label, wide(&current).as_ptr());
                }
                if state.shared.finished.load(Ordering::SeqCst) {
                    SetWindowTextW(state.stop_button, wide("Close").as_ptr());
                }
                return 0;
            }
        }
        STATUS_EDITOR_READY => {
            if let Some(state) = state {
                EnableWindow(state.editor_button, 1);
                return 0;
            }
        }
        STATUS_CLOSE => {
            DestroyWindow(window);
            return 0;
        }
        WM_COMMAND => {
            if let Some(state) = state {
                match (wparam & 0xffff) as i32 {
                    EDITOR_BUTTON => {
                        let editor = state
                            .shared
                            .editor_url
                            .lock()
                            .ok()
                            .and_then(|url| url.clone());
                        if let Some(url) = editor {
                            if let Err(error) = webbrowser::open(&url) {
                                show_error(window, &format!("Cannot open browser editor: {error}"));
                            }
                        }
                        return 0;
                    }
                    STOP_BUTTON => {
                        stop_or_close(window, state);
                        return 0;
                    }
                    _ => {}
                }
            }
        }
        WM_CLOSE => {
            if let Some(state) = state {
                stop_or_close(window, state);
                return 0;
            }
        }
        WM_DESTROY => {
            if let Some(state) = state {
                if !state.shared.finished.load(Ordering::SeqCst) {
                    state.shared.cancelled.store(true, Ordering::SeqCst);
                }
            }
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(window, message, wparam, lparam)
}

unsafe fn stop_or_close(window: HWND, state: &StatusUiState) {
    if state.shared.finished.load(Ordering::SeqCst) {
        DestroyWindow(window);
    } else {
        state.shared.cancelled.store(true, Ordering::SeqCst);
        SetWindowTextW(
            state.message_label,
            wide("Stopping Boxy after the current operation...").as_ptr(),
        );
        EnableWindow(state.stop_button, 0);
    }
}
