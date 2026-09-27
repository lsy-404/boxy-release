use std::{
    collections::BTreeMap,
    ffi::OsString,
    mem::{size_of, MaybeUninit},
    os::windows::ffi::OsStringExt,
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM},
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        Memory::{
            VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
        },
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_VM_OPERATION, PROCESS_VM_READ,
        },
    },
    UI::{
        Controls::{TB_BUTTONCOUNT, TB_GETBUTTON},
        WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetClassNameW, GetParent, GetWindowThreadProcessId,
            SendMessageTimeoutW, SMTO_ABORTIFHUNG,
        },
    },
};

const MAX_TOOLBAR_BUTTONS: usize = 512;
const SEND_TIMEOUT_MS: u32 = 1000;

#[repr(C)]
struct ToolbarButton {
    _bitmap: i32,
    _command: i32,
    _state: u8,
    _style: u8,
    data: usize,
    _string: isize,
}

#[repr(C)]
struct TrayData {
    window: HWND,
    _id: u32,
    _callback_message: u32,
    _reserved: [u32; 2],
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32, access: u32) -> Option<Self> {
        let handle = unsafe { OpenProcess(access, 0, process_id) };
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct RemoteBuffer {
    process: HANDLE,
    address: *mut core::ffi::c_void,
}

impl RemoteBuffer {
    fn new(process: &ProcessHandle) -> Option<Self> {
        let address = unsafe {
            VirtualAllocEx(
                process.0,
                std::ptr::null(),
                size_of::<ToolbarButton>(),
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if address.is_null() {
            return None;
        }
        Some(Self {
            process: process.0,
            address,
        })
    }

    fn keep_if_message_timed_out(&mut self) {
        self.address = null_mut();
    }
}

impl Drop for RemoteBuffer {
    fn drop(&mut self) {
        if !self.address.is_null() {
            unsafe { VirtualFreeEx(self.process, self.address, 0, MEM_RELEASE) };
        }
    }
}

pub fn collect() -> Vec<String> {
    let mut applications = BTreeMap::<String, String>::new();
    let mut process_names = BTreeMap::<u32, Option<String>>::new();

    for toolbar in toolbar_windows() {
        let mut toolbar_process_id = 0;
        unsafe { GetWindowThreadProcessId(toolbar, &mut toolbar_process_id) };
        if toolbar_process_id == 0 {
            continue;
        }
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_READ;
        let Some(process) = ProcessHandle::open(toolbar_process_id, access) else {
            continue;
        };
        let Some(mut buffer) = RemoteBuffer::new(&process) else {
            continue;
        };
        let Some(button_count) = toolbar_message(toolbar, TB_BUTTONCOUNT, 0, 0) else {
            continue;
        };
        if button_count <= 0 {
            continue;
        }

        for index in 0..(button_count as usize).min(MAX_TOOLBAR_BUTTONS) {
            let Some(result) =
                toolbar_message(toolbar, TB_GETBUTTON, index, buffer.address as LPARAM)
            else {
                buffer.keep_if_message_timed_out();
                break;
            };
            if result == 0 {
                continue;
            }
            let Some(button) = read_remote::<ToolbarButton>(process.0, buffer.address) else {
                continue;
            };
            if button.data == 0 {
                continue;
            }
            let tray_data_address = button.data as *const core::ffi::c_void;
            let Some(tray_data) = read_remote::<TrayData>(process.0, tray_data_address) else {
                continue;
            };
            if tray_data.window.is_null() {
                continue;
            }
            let mut process_id = 0;
            unsafe { GetWindowThreadProcessId(tray_data.window, &mut process_id) };
            if process_id == 0 {
                continue;
            }
            let name = process_names
                .entry(process_id)
                .or_insert_with(|| process_name(process_id))
                .clone();
            let Some(name) = name else {
                continue;
            };
            applications
                .entry(name.to_ascii_lowercase())
                .or_insert(name);
            if applications.len() >= MAX_TOOLBAR_BUTTONS {
                return applications.into_values().collect();
            }
        }
    }

    applications.into_values().collect()
}

fn toolbar_windows() -> Vec<HWND> {
    let mut roots = Vec::new();
    unsafe { EnumWindows(Some(collect_taskbar_window), &mut roots as *mut _ as LPARAM) };
    roots.sort_unstable_by_key(|window| *window as usize);
    roots.dedup();

    let mut toolbars = Vec::new();
    for root in roots {
        let overflow = window_class(root).as_deref() == Some("NotifyIconOverflowWindow");
        let mut children = Vec::new();
        unsafe {
            EnumChildWindows(
                root,
                Some(collect_toolbar_window),
                &mut children as *mut _ as LPARAM,
            )
        };
        toolbars.extend(
            children
                .into_iter()
                .filter(|window| overflow || belongs_to_notification_area(*window)),
        );
    }
    toolbars.sort_unstable_by_key(|window| *window as usize);
    toolbars.dedup();
    toolbars
}

unsafe extern "system" fn collect_taskbar_window(window: HWND, context: LPARAM) -> i32 {
    let Some(class) = window_class(window) else {
        return 1;
    };
    if !matches!(
        class.as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow"
    ) {
        return 1;
    }
    let windows = unsafe { &mut *(context as *mut Vec<HWND>) };
    if windows.try_reserve(1).is_err() {
        return 0;
    }
    windows.push(window);
    1
}

unsafe extern "system" fn collect_toolbar_window(window: HWND, context: LPARAM) -> i32 {
    if window_class(window).as_deref() != Some("ToolbarWindow32") {
        return 1;
    }
    let windows = unsafe { &mut *(context as *mut Vec<HWND>) };
    if windows.try_reserve(1).is_err() {
        return 0;
    }
    windows.push(window);
    1
}

fn window_class(window: HWND) -> Option<String> {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    (length > 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]))
}

fn belongs_to_notification_area(window: HWND) -> bool {
    let mut ancestor = unsafe { GetParent(window) };
    for _ in 0..8 {
        if ancestor.is_null() {
            return false;
        }
        if window_class(ancestor).as_deref() == Some("TrayNotifyWnd") {
            return true;
        }
        ancestor = unsafe { GetParent(ancestor) };
    }
    false
}

fn toolbar_message(window: HWND, message: u32, index: usize, data: LPARAM) -> Option<isize> {
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            window,
            message,
            index,
            data,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut result,
        )
    };
    (sent != 0).then_some(result as isize)
}

fn read_remote<T: Copy>(process: HANDLE, address: *const core::ffi::c_void) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut bytes_read = 0;
    let success = unsafe {
        ReadProcessMemory(
            process,
            address,
            value.as_mut_ptr().cast(),
            size_of::<T>(),
            &mut bytes_read,
        )
    };
    if success == 0 || bytes_read != size_of::<T>() {
        return None;
    }
    Some(unsafe { value.assume_init() })
}

fn process_name(process_id: u32) -> Option<String> {
    let process = ProcessHandle::open(process_id, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let success =
        unsafe { QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length) };
    if success == 0 || length == 0 {
        return None;
    }
    let path = std::path::PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    let name = path.file_stem()?.to_string_lossy().trim().to_string();
    (!name.is_empty()).then_some(name)
}
