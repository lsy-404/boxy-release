use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Security::{GetTokenInformation, TokenIsAppContainer, TOKEN_QUERY},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

const EDITOR_HASH: [u8; 32] = [
    0x37, 0x59, 0x2b, 0x7d, 0x3c, 0x41, 0xff, 0xaf, 0x04, 0x91, 0xb6, 0xb3, 0x35, 0xee, 0x63, 0x0a,
    0x34, 0xf0, 0x34, 0x16, 0xb8, 0x5f, 0xe9, 0x08, 0x10, 0x21, 0xe6, 0x98, 0x1e, 0xb5, 0xc2, 0xd3,
];

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn is_appcontainer(process: HANDLE) -> Result<bool, &'static str> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err("process_identity_unavailable");
    }
    let token = Handle(token);
    let mut value = 0u32;
    let mut len = 0;
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenIsAppContainer,
            (&mut value as *mut u32).cast(),
            size_of::<u32>() as u32,
            &mut len,
        )
    } == 0
    {
        return Err("process_identity_unavailable");
    }
    Ok(value != 0)
}

fn executable(pid: u32) -> Result<PathBuf, &'static str> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err("process_inaccessible");
    }
    let process = Handle(process);
    if !cfg!(feature = "native-session-test-fixture") && is_appcontainer(process.0)? {
        return Err("appcontainer_unsupported");
    }
    #[cfg(target_arch = "x86_64")]
    if !bridge::same_user(process.0).map_err(|_| "process_identity_unavailable")? {
        return Err("process_user_mismatch");
    }
    let mut buf = vec![0u16; 32768];
    let mut size = buf.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.0, 0, buf.as_mut_ptr(), &mut size) } == 0 {
        return Err("process_inaccessible");
    }
    Ok(PathBuf::from(OsString::from_wide(&buf[..size as usize])))
}

fn supported(pid: u32) -> Result<Option<PathBuf>, &'static str> {
    let path = executable(pid)?;
    if !path.file_name().is_some_and(|s| {
        s.to_string_lossy()
            .eq_ignore_ascii_case("synthv-studio.exe")
    }) {
        return Ok(None);
    }
    let mut file = File::open(&path).map_err(|_| "executable_inaccessible")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| "executable_inaccessible")?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok((digest.finalize().as_slice() == EDITOR_HASH).then_some(path))
}

pub(crate) fn status(params: &Value) -> Result<Value, String> {
    let selected = params.get("pid").and_then(Value::as_u64).map(|x| x as u32);
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("process_enumeration_failed".into());
    }
    let snapshot = Handle(snapshot);
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut processes = Vec::new();
    let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while ok != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        let pid = entry.th32ProcessID;
        if name.eq_ignore_ascii_case("synthv-studio.exe") && selected.is_none_or(|v| v == pid) {
            let item = match supported(pid) {
                Ok(Some(path)) => json!({"pid":pid,"supported":true,"version":"2.3.0","executable":path}),
                Ok(None) => json!({"pid":pid,"supported":false,"reason":"unsupported_build"}),
                Err(reason) => json!({"pid":pid,"supported":false,"reason":reason}),
            };
            processes.push(item);
        }
        ok = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(json!({"protocolVersion":1,"processes":processes}))
}

#[cfg(target_arch = "x86_64")]
mod bridge {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand_core::{OsRng, RngCore};
    use std::{
        ffi::{c_void, OsStr},
        io::Write,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
        ptr::{null, null_mut},
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{FreeLibrary, WAIT_OBJECT_0},
        Security::{EqualSid, TokenUser, TOKEN_USER},
        System::{
            Diagnostics::{
                Debug::{ReadProcessMemory, WriteProcessMemory},
                ToolHelp::{
                    Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
                    TH32CS_SNAPMODULE32,
                },
            },
            LibraryLoader::{GetModuleHandleExW, GetModuleHandleW, GetProcAddress, LoadLibraryW},
            Memory::{
                VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
            },
            Threading::{
                CreateMutexW, CreateRemoteThread, GetCurrentProcess, GetExitCodeThread,
                OpenProcessToken, ReleaseMutex, WaitForSingleObject, PROCESS_CREATE_THREAD,
                PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
            },
        },
        UI::Shell::SHGetSpecialFolderPathW,
    };

    pub(super) const HELPER: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/session_bridge.dll"));
    const TIMEOUT_MS: u32 = 15000;
    static UNCERTAIN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<u32>>> =
        std::sync::OnceLock::new();
    fn uncertain() -> &'static std::sync::Mutex<std::collections::HashSet<u32>> {
        UNCERTAIN.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
    }
    #[repr(C)]
    #[derive(Default)]
    struct Request {
        nonce: u64,
        generation: u32,
        timeout_ms: u32,
        result: i32,
        ui_thread: u32,
        ready: i32,
        licenses: i32,
        database_refreshed: i32,
        reserved: i32,
    }

    const _: [(); 40] = [(); size_of::<Request>()];

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }
    fn modules(pid: u32, name: &str) -> Result<usize, String> {
        let mut snapshot = INVALID_HANDLE_VALUE;
        for _ in 0..10 {
            snapshot =
                unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
            if snapshot != INVALID_HANDLE_VALUE {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("module_enumeration_failed".into());
        }
        let snapshot = Handle(snapshot);
        let mut entry: MODULEENTRY32W = unsafe { zeroed() };
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        let mut ok = unsafe { Module32FirstW(snapshot.0, &mut entry) };
        while ok != 0 {
            let end = entry
                .szModule
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szModule.len());
            if String::from_utf16_lossy(&entry.szModule[..end]).eq_ignore_ascii_case(name) {
                return Ok(entry.modBaseAddr as usize);
            }
            ok = unsafe { Module32NextW(snapshot.0, &mut entry) };
        }
        Err("module_not_found".into())
    }

    fn local_export(module: *mut c_void, symbol: &[u8]) -> Result<usize, String> {
        unsafe {
            GetProcAddress(module, symbol.as_ptr())
                .map(|f| f as usize)
                .ok_or_else(|| "native_export_missing".to_string())
        }
    }
    fn remote_system_export(pid: u32, symbol: &[u8]) -> Result<usize, String> {
        let kernel = wide(OsStr::new("kernel32.dll"));
        let module = unsafe { GetModuleHandleW(kernel.as_ptr()) };
        if module.is_null() {
            return Err("system_library_missing".into());
        }
        let address = local_export(module, symbol)?;
        let mut owner = null_mut();
        if unsafe { GetModuleHandleExW(6, address as *const u16, &mut owner) } == 0 {
            return Err("system_library_missing".into());
        }
        let owner_name = if owner == module {
            "kernel32.dll"
        } else {
            "kernelbase.dll"
        };
        Ok(modules(pid, owner_name)? + address - owner as usize)
    }
    fn alloc(process: HANDLE, bytes: &[u8]) -> Result<*mut c_void, String> {
        let p = unsafe {
            VirtualAllocEx(
                process,
                null(),
                bytes.len(),
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if p.is_null() {
            return Err("remote_allocation_failed".into());
        }
        let mut written = 0;
        if unsafe {
            WriteProcessMemory(process, p, bytes.as_ptr().cast(), bytes.len(), &mut written)
        } == 0
            || written != bytes.len()
        {
            unsafe {
                VirtualFreeEx(process, p, 0, MEM_RELEASE);
            }
            return Err("remote_write_failed".into());
        }
        Ok(p)
    }
    fn remote_thread(
        process: HANDLE,
        address: usize,
        arg: *mut c_void,
        timeout: u32,
    ) -> Result<u32, String> {
        let start = Some(unsafe {
            std::mem::transmute::<usize, unsafe extern "system" fn(*mut c_void) -> u32>(address)
        });
        let thread = unsafe { CreateRemoteThread(process, null(), 0, start, arg, 0, null_mut()) };
        if thread.is_null() {
            return Err("remote_thread_failed".into());
        }
        let thread = Handle(thread);
        if unsafe { WaitForSingleObject(thread.0, timeout) } != WAIT_OBJECT_0 {
            return Err("native_timeout_unknown".into());
        }
        let mut code = 0;
        if unsafe { GetExitCodeThread(thread.0, &mut code) } == 0 {
            return Err("native_result_unavailable".into());
        }
        Ok(code)
    }
    pub(super) fn same_user(process: HANDLE) -> Result<bool, String> {
        fn token(process: HANDLE) -> Result<(Handle, Vec<u8>), String> {
            let mut token = null_mut();
            if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
                return Err("process_identity_unavailable".into());
            }
            let token = Handle(token);
            let mut len = 0u32;
            unsafe {
                GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len);
            }
            if len == 0 || len > 65536 {
                return Err("process_identity_unavailable".into());
            }
            let mut buf = vec![0u8; len as usize];
            if unsafe {
                GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
            } == 0
            {
                return Err("process_identity_unavailable".into());
            }
            Ok((token, buf))
        }
        let (_a, a) = token(process)?;
        let (_b, b) = token(unsafe { GetCurrentProcess() })?;
        let sid_a = unsafe {
            std::ptr::read_unaligned(a.as_ptr().cast::<TOKEN_USER>())
                .User
                .Sid
        };
        let sid_b = unsafe {
            std::ptr::read_unaligned(b.as_ptr().cast::<TOKEN_USER>())
                .User
                .Sid
        };
        Ok(unsafe { EqualSid(sid_a, sid_b) } != 0)
    }

    fn session_path() -> Result<PathBuf, String> {
        #[cfg(feature = "native-session-test-fixture")]
        if let Some(path) = std::env::var_os("BOXY_TEST_SESSION_PATH") {
            return Ok(PathBuf::from(path));
        }
        let mut path = [0u16; 260];
        if unsafe { SHGetSpecialFolderPathW(null_mut(), path.as_mut_ptr(), 0x1a, 0) } == 0 {
            return Err("session_directory_unavailable".into());
        }
        let end = path
            .iter()
            .position(|&c| c == 0)
            .ok_or("session_directory_unavailable")?;
        Ok(PathBuf::from(OsString::from_wide(&path[..end]))
            .join("Dreamtonics")
            .join("Synthesizer V Studio 2")
            .join("license")
            .join("session"))
    }

    pub(super) fn reload(params: &Value) -> Result<Value, String> {
        let pid = params["pid"].as_u64().ok_or("invalid_params")? as u32;
        if uncertain()
            .lock()
            .map_err(|_| "native_state_unknown")?
            .contains(&pid)
        {
            return Err("native_busy_or_restart_required".into());
        }
        let expected = params["expectedFileSha256"]
            .as_str()
            .ok_or("invalid_params")?;
        if supported(pid).map_err(str::to_string)?.is_none() {
            return Err("unsupported_build".into());
        }
        let mutex_name = wide(OsStr::new(&format!("Local\\BoxyNativeSession-{pid}")));
        let mutex = unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) };
        if mutex.is_null() {
            return Err("native_lock_failed".into());
        }
        let mutex = Handle(mutex);
        if unsafe { WaitForSingleObject(mutex.0, 0) } != WAIT_OBJECT_0 {
            return Err("native_busy".into());
        }
        let result = run_locked(pid, expected);
        if result.as_ref().err().is_some_and(|error| {
            matches!(
                error.as_str(),
                "native_timeout_unknown"
                    | "native_state_unknown"
                    | "native_unload_failed"
                    | "remote_thread_failed"
                    | "native_result_unavailable"
            )
        }) {
            if let Ok(mut pids) = uncertain().lock() {
                pids.insert(pid);
            }
        }
        unsafe {
            ReleaseMutex(mutex.0);
        }
        result
    }

    fn run_locked(pid: u32, expected: &str) -> Result<Value, String> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(session_path()?)
            .map_err(|_| "session_unavailable")?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "session_unavailable")?;
        if bytes.len() > 1024 * 1024 {
            return Err("session_too_large".into());
        }
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes));
        use zeroize::Zeroize;
        bytes.zeroize();
        if hash != expected {
            return Err("session_sha256_mismatch".into());
        }
        let process = unsafe {
            OpenProcess(
                PROCESS_CREATE_THREAD
                    | PROCESS_QUERY_INFORMATION
                    | PROCESS_VM_OPERATION
                    | PROCESS_VM_READ
                    | PROCESS_VM_WRITE,
                0,
                pid,
            )
        };
        if process.is_null() {
            return Err("process_inaccessible".into());
        }
        let process = Handle(process);
        if !same_user(process.0)? {
            return Err("process_user_mismatch".into());
        }
        if supported(pid).map_err(str::to_string)?.is_none() {
            return Err("unsupported_build".into());
        }
        match modules(pid, "session_bridge.dll") {
            Ok(_) => return Err("native_busy_or_restart_required".into()),
            Err(error) if error == "module_not_found" => {}
            Err(error) => return Err(error),
        }
        let mut builder = tempfile::Builder::new();
        builder.prefix("boxy-session-");
        #[cfg(feature = "native-session-test-fixture")]
        let temp = match std::env::var_os("BOXY_TEST_HELPER_DIR") {
            Some(dir) => builder.tempdir_in(dir),
            None => builder.tempdir(),
        };
        #[cfg(not(feature = "native-session-test-fixture"))]
        let temp = builder.tempdir();
        let temp = temp.map_err(|_| "helper_unavailable")?;
        let dll_path = temp.path().join("session_bridge.dll");
        let mut dll = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dll_path)
            .map_err(|_| "helper_unavailable")?;
        dll.write_all(HELPER)
            .and_then(|_| dll.sync_all())
            .map_err(|_| "helper_unavailable")?;
        drop(dll);
        let mut helper_lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&dll_path)
            .map_err(|_| "helper_unavailable")?;
        let mut helper_hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = helper_lock
                .read(&mut buffer)
                .map_err(|_| "helper_unavailable")?;
            if n == 0 {
                break;
            }
            helper_hash.update(&buffer[..n]);
        }
        if helper_hash.finalize().as_slice() != Sha256::digest(HELPER).as_slice() {
            return Err("helper_integrity_failed".into());
        }
        let path = wide(dll_path.as_os_str());
        let local = unsafe { LoadLibraryW(path.as_ptr()) };
        if local.is_null() {
            return Err("helper_unavailable".into());
        }
        let export = local_export(local, b"BridgeReload\0");
        unsafe {
            FreeLibrary(local);
        }
        let offset = export? - local as usize;
        let path_bytes =
            unsafe { std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), path.len() * 2) };
        let loader = remote_system_export(pid, b"LoadLibraryW\0")?;
        let unloader = remote_system_export(pid, b"FreeLibrary\0")?;
        let remote_path = alloc(process.0, path_bytes)?;
        let load_result = remote_thread(process.0, loader, remote_path, TIMEOUT_MS);
        if load_result.is_err() {
            std::mem::forget(helper_lock);
            std::mem::forget(temp);
            return load_result.map(|_| Value::Null);
        }
        unsafe {
            VirtualFreeEx(process.0, remote_path, 0, MEM_RELEASE);
        }
        let module = match modules(pid, "session_bridge.dll") {
            Ok(module) => module,
            Err(_) => {
                std::mem::forget(temp);
                return Err("native_state_unknown".into());
            }
        };
        let mut random = [0u8; 12];
        OsRng.fill_bytes(&mut random);
        let mut request = Request {
            nonce: u64::from_ne_bytes(random[..8].try_into().unwrap()),
            generation: u32::from_ne_bytes(random[8..].try_into().unwrap()),
            timeout_ms: TIMEOUT_MS,
            ..Default::default()
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&request as *const Request).cast::<u8>(),
                size_of::<Request>(),
            )
        };
        let remote = match alloc(process.0, bytes) {
            Ok(remote) => remote,
            Err(error) => {
                if !matches!(remote_thread(process.0, unloader, module as *mut c_void, TIMEOUT_MS), Ok(code) if code != 0)
                {
                    std::mem::forget(temp);
                    return Err("native_state_unknown".into());
                }
                return Err(error);
            }
        };
        let outcome = remote_thread(process.0, module + offset, remote, TIMEOUT_MS + 3000);
        if outcome.is_err() {
            std::mem::forget(temp);
            return outcome.map(|_| Value::Null);
        }
        let code = outcome?;
        if code == 14 {
            std::mem::forget(temp);
            return Err("native_timeout_unknown".into());
        }
        let mut read = 0;
        let read_ok = unsafe {
            ReadProcessMemory(
                process.0,
                remote,
                (&mut request as *mut Request).cast(),
                size_of::<Request>(),
                &mut read,
            )
        } != 0
            && read == size_of::<Request>();
        unsafe {
            VirtualFreeEx(process.0, remote, 0, MEM_RELEASE);
        }
        let unloaded = remote_thread(process.0, unloader, module as *mut c_void, TIMEOUT_MS);
        if !matches!(unloaded, Ok(value) if value != 0) {
            std::mem::forget(temp);
            return Err("native_unload_failed".into());
        }
        drop(helper_lock);
        let mut removed = false;
        for _ in 0..40 {
            match std::fs::remove_file(&dll_path) {
                Ok(()) => {
                    removed = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    removed = true;
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        if !removed || temp.close().is_err() {
            return Err("native_completed_cleanup_failed".into());
        }
        if !read_ok {
            return Err("native_result_unavailable".into());
        }
        if code != 0 || request.result != 1 || request.database_refreshed != 1 || request.ready == 0
        {
            return Err(format!("native_reload_failed_{code}_{}", request.result));
        }
        Ok(
            json!({"protocolVersion":1,"pid":pid,"uiThread":request.ui_thread,"generation":request.generation,"nativeReady":true,"licenseCount":request.licenses,"databaseRefreshed":true,"sessionSha256":hash}),
        )
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn reload(params: &Value) -> Result<Value, String> {
    bridge::reload(params)
}

#[cfg(all(test, target_arch = "x86_64"))]
#[path = "../../test/native_session.rs"]
mod tests;
