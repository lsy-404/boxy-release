use serde::Deserialize;
use serde_json::json;
use std::{
    fs,
    io::Read,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Default)]
pub struct PanelState {
    pub authorized: AtomicBool,
    pub settings: Mutex<Option<PathBuf>>,
    directory: Mutex<Option<PathBuf>>,
    pub mode: Mutex<serde_json::Value>,
    pub verified_at: Mutex<Option<std::time::Instant>>,
    pub account_request: Mutex<Option<serde_json::Value>>,
    pub error: Mutex<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectSettings {
    #[serde(default)]
    http_api_enabled: bool,
    #[serde(default)]
    http_agent_enabled: bool,
    http_api_port: u16,
    http_api_token: String,
}

impl Drop for ConnectSettings {
    fn drop(&mut self) {
        self.http_api_token.zeroize();
    }
}

fn settings(path: &Path) -> Option<ConnectSettings> {
    let mut data = Zeroizing::new(Vec::new());
    fs::File::open(path)
        .ok()?
        .take(65537)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() > 64 * 1024 {
        return None;
    }
    let parsed: ConnectSettings = serde_json::from_slice(&data).ok()?;
    if parsed.http_api_port == 0
        || parsed.http_api_token.is_empty()
        || parsed.http_api_token.contains(char::is_whitespace)
    {
        return None;
    }
    Some(parsed)
}

fn hex(input: &str) -> String {
    input
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn unhex(input: &str) -> Option<String> {
    if input.len() % 2 != 0 || input.len() > 16384 || !input.is_ascii() {
        return None;
    }
    let bytes = (0..input.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&input[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    String::from_utf8(bytes).ok()
}
fn parse_request(data: &str) -> Option<(&str, String)> {
    if data.len() > 17000 {
        return None;
    }
    let mut lines = data.lines();
    if lines.next()? != "version=1" {
        return None;
    }
    let kind = lines.next()?.strip_prefix("kind=")?;
    let payload = unhex(lines.next()?.strip_prefix("payload_hex=")?)?;
    if lines.next().is_some() {
        return None;
    }
    match kind {
        "chat.send" if !payload.is_empty() && payload.len() <= 8192 => Some((kind, payload)),
        "account.source.select" if !payload.is_empty() && payload.len() <= 256 => {
            Some((kind, payload))
        }
        "account.main.restore" if payload.parse::<u32>().is_ok() => Some((kind, payload)),
        _ => None,
    }
}

#[cfg(windows)]
fn create_private(path: &Path) -> Result<(), String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    #[repr(C)]
    struct TokenUser {
        sid: *mut c_void,
        attributes: u32,
    }
    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit: i32,
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn GetTokenInformation(token: *mut c_void, class: u32, info: *mut c_void, len: u32, needed: *mut u32) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            value: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn CreateDirectoryW(path: *const u16, attributes: *const SecurityAttributes) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 {
        return Err("cannot inspect current user".into());
    }
    let mut needed = 0;
    unsafe { GetTokenInformation(token, 1, ptr::null_mut(), 0, &mut needed) };
    let mut buf = vec![0u8; needed as usize];
    let mut sid_text = ptr::null_mut();
    let ok = needed != 0
        && unsafe { GetTokenInformation(token, 1, buf.as_mut_ptr().cast(), needed, &mut needed) } != 0
        && unsafe { ConvertSidToStringSidW((*(buf.as_ptr() as *const TokenUser)).sid, &mut sid_text) } != 0;
    unsafe { CloseHandle(token) };
    if !ok {
        return Err("cannot inspect current user".into());
    }
    let mut len = 0;
    while unsafe { *sid_text.add(len) } != 0 { len += 1; }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_text, len) });
    unsafe { LocalFree(sid_text.cast()) };
    let sddl: Vec<u16> = format!("O:{sid}D:P(A;OICI;GA;;;OW)(A;OICI;GA;;;SY)")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err("cannot secure panel directory".into());
    }
    let attrs = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor,
        inherit: 0,
    };
    let ok = unsafe { CreateDirectoryW(wide.as_ptr(), &attrs) };
    unsafe {
        LocalFree(descriptor);
    }
    if ok == 0 {
        return Err("cannot create private panel directory; close other Boxy instances or remove its stale directory".into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn create_private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|_| "cannot create private panel directory".into())
}

struct PanelLease {
    directory: PathBuf,
    _lock: fs::File,
}

fn ordinary(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "cannot inspect panel path")?;
    if metadata.file_type().is_symlink() {
        return Err("panel path is a symbolic link".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("panel path is a reparse point".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn verify_private(path: &Path) -> Result<(), String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    type Handle = *mut c_void;
    #[repr(C)]
    struct AclSize {
        count: u32,
        used: u32,
        free: u32,
    }
    #[repr(C)]
    struct TokenUser {
        sid: Handle,
        attributes: u32,
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn GetNamedSecurityInfoW(
            name: *const u16,
            kind: u32,
            info: u32,
            owner: *mut Handle,
            group: *mut Handle,
            dacl: *mut Handle,
            sacl: *mut Handle,
            sd: *mut Handle,
        ) -> u32;
        fn GetSecurityDescriptorControl(sd: Handle, control: *mut u16, revision: *mut u32) -> i32;
        fn GetAclInformation(acl: Handle, info: *mut c_void, length: u32, class: u32) -> i32;
        fn GetAce(acl: Handle, index: u32, ace: *mut Handle) -> i32;
        fn ConvertStringSidToSidW(text: *const u16, sid: *mut Handle) -> i32;
        fn EqualSid(a: Handle, b: Handle) -> i32;
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: u32,
            info: *mut c_void,
            len: u32,
            needed: *mut u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn LocalFree(memory: Handle) -> Handle;
    }
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    ordinary(path)?;
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut owner = ptr::null_mut();
    let mut acl = ptr::null_mut();
    let mut sd = ptr::null_mut();
    let mut owner_rights = ptr::null_mut();
    let mut system = ptr::null_mut();
    let mut token = ptr::null_mut();
    let result = (|| {
        if unsafe {
            GetNamedSecurityInfoW(
                wide_path.as_ptr(),
                1,
                5,
                &mut owner,
                ptr::null_mut(),
                &mut acl,
                ptr::null_mut(),
                &mut sd,
            )
        } != 0
            || owner.is_null()
            || acl.is_null()
        {
            return Err("cannot inspect panel permissions".into());
        }
        let mut control = 0u16;
        let mut revision = 0u32;
        if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0
            || control & 0x1000 == 0
        {
            return Err("panel permissions are not private".into());
        }
        if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 {
            return Err("cannot inspect current user".into());
        }
        let mut needed = 0u32;
        unsafe { GetTokenInformation(token, 1, ptr::null_mut(), 0, &mut needed) };
        if needed == 0 {
            return Err("cannot inspect current user".into());
        }
        let mut buf = vec![0u8; needed as usize];
        if unsafe { GetTokenInformation(token, 1, buf.as_mut_ptr().cast(), needed, &mut needed) }
            == 0
        {
            return Err("cannot inspect current user".into());
        }
        let user = unsafe { &*(buf.as_ptr() as *const TokenUser) };
        if unsafe { EqualSid(owner, user.sid) } == 0 {
            return Err("panel directory has another owner".into());
        }
        if unsafe { ConvertStringSidToSidW(wide("S-1-3-4").as_ptr(), &mut owner_rights) } == 0
            || unsafe { ConvertStringSidToSidW(wide("S-1-5-18").as_ptr(), &mut system) } == 0
        {
            return Err("cannot inspect panel permissions".into());
        }
        let mut info = AclSize {
            count: 0,
            used: 0,
            free: 0,
        };
        if unsafe {
            GetAclInformation(
                acl,
                (&mut info as *mut AclSize).cast(),
                std::mem::size_of::<AclSize>() as u32,
                2,
            )
        } == 0
            || info.count != 2
        {
            return Err("panel permissions are not private".into());
        }
        let mut seen_owner = false;
        let mut seen_system = false;
        for index in 0..info.count {
            let mut ace = ptr::null_mut();
            if unsafe { GetAce(acl, index, &mut ace) } == 0 || ace.is_null() {
                return Err("cannot inspect panel permissions".into());
            }
            let bytes = ace as *const u8;
            let kind = unsafe { *bytes };
            let flags = unsafe { *bytes.add(1) };
            let mask = unsafe { ptr::read_unaligned(bytes.add(4) as *const u32) };
            let sid = unsafe { bytes.add(8) as Handle };
            if kind != 0 || flags != 3 || (mask != 0x10000000 && mask != 0x001f01ff) {
                return Err("panel permissions are not private".into());
            }
            if unsafe { EqualSid(sid, owner_rights) } != 0 {
                seen_owner = true;
            } else if unsafe { EqualSid(sid, system) } != 0 {
                seen_system = true;
            } else {
                return Err("panel permissions are not private".into());
            }
        }
        if !seen_owner || !seen_system {
            return Err("panel permissions are not private".into());
        }
        Ok(())
    })();
    unsafe {
        if !token.is_null() {
            CloseHandle(token);
        }
        if !owner_rights.is_null() {
            LocalFree(owner_rights);
        }
        if !system.is_null() {
            LocalFree(system);
        }
        if !sd.is_null() {
            LocalFree(sd);
        }
    }
    result
}

#[cfg(not(windows))]
fn verify_private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    ordinary(path)?;
    let metadata = fs::metadata(path).map_err(|_| "cannot inspect panel permissions")?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err("panel permissions are not private".into());
    }
    Ok(())
}

fn lock_file(path: &Path) -> Result<fs::File, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => ordinary(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("cannot inspect panel lock".into()),
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x00200000);
    }
    let file = options
        .open(path)
        .map_err(|_| "another Boxy panel is running or the panel lock is unavailable")?;
    let metadata = file.metadata().map_err(|_| "cannot inspect panel lock")?;
    if !metadata.is_file() {
        return Err("panel lock is not a regular file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("panel lock is a reparse point".into());
        }
    }
    #[cfg(not(windows))]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("another Boxy panel is running".into());
        }
    }
    Ok(file)
}

fn acquire_panel(path: &Path) -> Result<PanelLease, String> {
    if !path.exists() {
        create_private(path)?;
    }
    verify_private(path)?;
    let lock = lock_file(&path.join("panel.lock"))?;
    for name in [
        "request.txt",
        "status.txt",
        "status.next",
        "request.txt.tmp",
    ] {
        let child = path.join(name);
        match fs::symlink_metadata(&child) {
            Ok(_) => {
                ordinary(&child)?;
                fs::remove_file(child).map_err(|_| "cannot clear stale panel data")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("cannot inspect stale panel data".into()),
        }
    }
    Ok(PanelLease {
        directory: path.to_path_buf(),
        _lock: lock,
    })
}

fn panel_directory() -> Result<PanelLease, String> {
    #[cfg(windows)]
    let parent = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let parent = std::env::var_os("HOME").map(PathBuf::from);
    let parent = parent
        .ok_or("panel data directory unavailable")?
        .join("Boxy");
    fs::create_dir_all(&parent).map_err(|_| "cannot create panel parent")?;
    ordinary(&parent)?;
    acquire_panel(&parent.join("ConnectPanel"))
}

fn preferences(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("preferences.json");
    ordinary(&path).ok()?;
    let file = fs::File::open(path).ok()?;
    let mut data = String::new();
    file.take(8193).read_to_string(&mut data).ok()?;
    if data.len() > 8192 {
        return None;
    }
    serde_json::from_str::<PathBuf>(&data).ok()
}

pub fn configure(state: &PanelState, path: PathBuf) -> Result<(), String> {
    if settings(&path).is_none() {
        return Err("所选文件不是有效的声梭 Connect 配置".into());
    }
    let dir = state
        .directory
        .lock()
        .map_err(|_| "panel unavailable")?
        .clone()
        .ok_or("panel unavailable")?;
    let target = dir.join("preferences.json");
    match fs::symlink_metadata(&target) {
        Ok(_) => ordinary(&target)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("cannot inspect panel preferences".into()),
    }
    let mut file =
        tempfile::NamedTempFile::new_in(&dir).map_err(|_| "cannot save panel preferences")?;
    use std::io::Write;
    serde_json::to_writer(&mut file, &path).map_err(|_| "cannot save panel preferences")?;
    file.flush().map_err(|_| "cannot save panel preferences")?;
    file.persist(target)
        .map_err(|_| "cannot save panel preferences")?;
    *state.settings.lock().map_err(|_| "panel unavailable")? = Some(path);
    Ok(())
}

pub fn start(
    state: Arc<PanelState>,
    stop: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, String> {
    let lease = panel_directory()?;
    *state.directory.lock().map_err(|_| "panel unavailable")? = Some(lease.directory.clone());
    *state.settings.lock().map_err(|_| "panel unavailable")? = preferences(&lease.directory);
    Ok(thread::spawn(move || run(lease, state, stop)))
}

fn run(lease: PanelLease, state: Arc<PanelState>, stop: Arc<AtomicBool>) {
    let dir = &lease.directory;
    let request = dir.join("request.txt");
    let status = dir.join("status.txt");
    let (sender, receiver) = mpsc::channel::<(u64, Result<(String, String), String>)>();
    let mut pending = false;
    let mut mcp = false;
    let mut agent_valid = false;
    let mut last_mcp = std::time::Instant::now() - Duration::from_secs(5);
    let mut conversation_id = String::new();
    let mut transcript = String::new();
    let mut response_id = 0u64;
    let mut previous_settings: Option<PathBuf> = None;
    let mut settings_generation = 0u64;
    while !stop.load(Ordering::Acquire) {
        let fresh = state
            .verified_at
            .lock()
            .ok()
            .and_then(|value| *value)
            .map(|at| at.elapsed() < Duration::from_secs(8))
            .unwrap_or(false);
        let authorized = state.authorized.load(Ordering::Acquire) && fresh;
        let selected = state.settings.lock().ok().and_then(|path| path.clone());
        if selected != previous_settings {
            previous_settings = selected.clone();
            settings_generation += 1;
            pending = false;
            conversation_id.clear();
            transcript.clear();
            response_id += 1;
            mcp = false;
            agent_valid = false;
            last_mcp = std::time::Instant::now() - Duration::from_secs(5);
        }
        let config = selected.as_deref().and_then(settings);
        let reachable = config
            .as_ref()
            .map(|v| {
                TcpStream::connect_timeout(
                    &SocketAddr::from(([127, 0, 0, 1], v.http_api_port)),
                    Duration::from_millis(250),
                )
                .is_ok()
            })
            .unwrap_or(false);
        let mode = state.mode.lock().map(|m| m.clone()).unwrap_or_default();
        let advanced = authorized && mode["authorized"] == true && mode["advancedAllowed"] == true;
        if last_mcp.elapsed() >= Duration::from_secs(3) {
            mcp = config
                .as_ref()
                .map(|c| c.http_api_enabled && reachable && probe_mcp(c))
                .unwrap_or(false);
            agent_valid = config
                .as_ref()
                .map(|c| c.http_agent_enabled && reachable && probe_agent(c))
                .unwrap_or(false);
            last_mcp = std::time::Instant::now();
        }
        let agent = advanced && agent_valid;
        if let Ok((generation, result)) = receiver.try_recv() {
            if generation != settings_generation {
                continue;
            }
            pending = false;
            response_id += 1;
            match result {
                Ok((id, text)) => {
                    conversation_id = id;
                    transcript.push_str("\n声梭: ");
                    transcript.push_str(&text);
                }
                Err(error) => {
                    transcript.push_str("\n声梭: ");
                    transcript.push_str(&error);
                }
            }
            if transcript.len() > 16000 {
                let start = transcript
                    .char_indices()
                    .find(|(i, _)| *i >= transcript.len() - 16000)
                    .map(|(i, _)| i)
                    .unwrap_or(transcript.len());
                transcript = transcript[start..].to_string();
            }
        }
        if let Ok(data) = fs::read_to_string(&request) {
            let parsed = parse_request(&data).map(|(kind, payload)| (kind.to_string(), payload));
            let _ = fs::remove_file(&request);
            if let Some((kind, message)) = parsed {
                if kind == "chat.send" && agent && !pending {
                    if let Some(config) = config {
                        pending = true;
                        transcript.push_str("\n你: ");
                        transcript.push_str(&message);
                        response_id += 1;
                        let sender = sender.clone();
                        let id = conversation_id.clone();
                        let generation = settings_generation;
                        thread::spawn(move || {
                            let _ = sender.send((generation, send_chat(config, message, id)));
                        });
                    }
                } else if advanced && kind.starts_with("account.") {
                    let (pid_text, selection) = if kind == "account.source.select" {
                        message.split_once('\t').unwrap_or(("", ""))
                    } else {
                        (message.as_str(), "")
                    };
                    let pid = pid_text.parse::<u32>().ok();
                    let allowed = if kind == "account.source.select" {
                        mode["canSelectSource"] == true
                            && mode["selections"]
                                .as_array()
                                .map(|v| v.iter().any(|item| item["id"] == selection))
                                .unwrap_or(false)
                    } else {
                        kind == "account.main.restore" && mode["canRestoreMain"] == true
                    };
                    if let Some(pid) = pid.filter(|pid| allowed && process_ids(&mode).contains(pid))
                    {
                        let mut request = json!({"action": kind, "pid": pid});
                        if kind == "account.source.select" {
                            request["selectionId"] = json!(selection);
                        }
                        if let Ok(mut slot) = state.account_request.lock() {
                            if slot.is_none() {
                                *slot = Some(request);
                            }
                        }
                    }
                } else {
                    response_id += 1;
                    transcript.push_str("\n请求不可用：请检查 Boxy 授权和连接状态。");
                }
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let account =
            advanced && (mode["canSelectSource"] == true || mode["canRestoreMain"] == true);
        let sources = mode["selections"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .take(64)
                    .filter_map(|row| {
                        let id = row["id"].as_str()?;
                        let label = row["displayName"].as_str()?;
                        if id.is_empty() || id.len() > 256 || id.contains(['\t', '\n', '\r']) {
                            return None;
                        }
                        Some(format!(
                            "{}\t{}",
                            id,
                            label
                                .chars()
                                .take(128)
                                .collect::<String>()
                                .replace(['\t', '\n', '\r'], " ")
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let processes = process_ids(&mode)
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let mode_text = mode["mode"]
            .as_str()
            .unwrap_or("unknown")
            .chars()
            .take(48)
            .collect::<String>();
        let phase_text = mode["phase"]
            .as_str()
            .unwrap_or("unknown")
            .chars()
            .take(48)
            .collect::<String>();
        let network_text = if mode["network"]["verified"] == true {
            mode["network"]["mode"].as_str().unwrap_or("unknown")
        } else {
            "unverified"
        };
        let panel_error = state
            .error
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default();
        let content = format!("updated_at={now}\nerror_hex={}\nmode_hex={}\nphase_hex={}\nnetwork_hex={}\nboxy_connected={}\nboxy_authorized={}\naccount_available={}\ncan_select_source={}\ncan_restore_main={}\nagent_available={}\nchat_available={}\nmcp_available={}\nresponse_id={response_id}\nsources_hex={}\nprocesses_hex={}\nconversation_hex={}\n", hex(&panel_error), hex(&mode_text), hex(&phase_text), hex(network_text), u8::from(authorized), u8::from(advanced), u8::from(account), u8::from(advanced && mode["canSelectSource"] == true), u8::from(advanced && mode["canRestoreMain"] == true), u8::from(agent_valid), u8::from(agent && !pending), u8::from(mcp), hex(&sources), hex(&processes), hex(&transcript));
        let temp = dir.join("status.next");
        if fs::write(&temp, content).is_ok() {
            let _ = fs::remove_file(&status);
            let _ = fs::rename(&temp, &status);
        }
        thread::sleep(Duration::from_millis(500));
    }
    let _ = fs::remove_file(dir.join("request.txt"));
    let _ = fs::remove_file(dir.join("status.txt"));
    let _ = fs::remove_file(dir.join("status.next"));
    if let Ok(mut directory) = state.directory.lock() {
        *directory = None;
    }
}

fn process_ids(mode: &serde_json::Value) -> Vec<u32> {
    let mut ids = mode["processes"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item["supported"] == true)
                .filter_map(|item| {
                    item["pid"]
                        .as_u64()
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|value| *value != 0)
                })
                .take(64)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn probe_agent(config: &ConnectSettings) -> bool {
    let client = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout(Duration::from_secs(2))
        .build();
    matches!(
        client
            .post(&format!("http://127.0.0.1:{}/agent", config.http_api_port))
            .set(
                "Authorization",
                &format!("Bearer {}", config.http_api_token)
            )
            .send_json(json!({})),
        Err(ureq::Error::Status(400, _))
    )
}

fn probe_mcp(config: &ConnectSettings) -> bool {
    let client = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout(Duration::from_secs(2))
        .build();
    let result = client
        .post(&format!("http://127.0.0.1:{}/mcp", config.http_api_port))
        .set(
            "Authorization",
            &format!("Bearer {}", config.http_api_token),
        )
        .send_json(json!({"jsonrpc":"2.0", "id":1, "method":"tools/list"}));
    result
        .ok()
        .and_then(|response| response.into_json::<serde_json::Value>().ok())
        .map(|value| value["result"].is_object())
        .unwrap_or(false)
}

fn send_chat(
    config: ConnectSettings,
    message: String,
    id: String,
) -> Result<(String, String), String> {
    let client = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout(Duration::from_secs(120))
        .build();
    let mut body = json!({"input": message});
    if !id.is_empty() {
        body["conversationId"] = json!(id);
    }
    let response = client
        .post(&format!("http://127.0.0.1:{}/agent", config.http_api_port))
        .set(
            "Authorization",
            &format!("Bearer {}", config.http_api_token),
        )
        .send_json(body)
        .map_err(|_| "声梭 Connect 请求失败或未启用".to_string())?;
    let mut text = String::new();
    response
        .into_reader()
        .take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|_| "声梭 Connect 响应无效".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "声梭 Connect 响应无效".to_string())?;
    let conversation_id = value["conversationId"]
        .as_str()
        .filter(|x| !x.is_empty())
        .ok_or("声梭 Connect 不支持继续对话")?;
    let messages = value["messages"]
        .as_array()
        .ok_or("声梭 Connect 响应无效")?;
    let reply = messages
        .iter()
        .rev()
        .find(|m| m["role"] == "assistant")
        .and_then(|m| m["content"].as_str())
        .ok_or("声梭 Connect 没有返回回复")?;
    Ok((conversation_id.to_string(), reply.to_string()))
}

#[cfg(test)]
#[path = "../../test/native_connect_panel.rs"]
mod tests;
