#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
        TOKEN_USER,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

#[cfg(windows)]
pub(crate) fn hardware_uuid(raw: &[u8]) -> Result<String, String> {
    if raw.len() < 8 {
        return Err("SMBIOS data is incomplete".to_string());
    }
    let table_length = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    if table_length != raw.len() - 8 {
        return Err("SMBIOS table length is invalid".to_string());
    }
    let mut offset = 8;
    while offset + 4 <= raw.len() {
        let kind = raw[offset];
        let length = raw[offset + 1] as usize;
        if length < 4 || offset + length > raw.len() {
            return Err("SMBIOS entry is invalid".to_string());
        }
        if kind == 1 && length >= 0x19 {
            let bytes = &raw[offset + 8..offset + 24];
            if bytes.iter().all(|byte| *byte == 0) || bytes.iter().all(|byte| *byte == 255) {
                return Err("SMBIOS device UUID is unavailable".to_string());
            }
            let ordered = [
                bytes[3], bytes[2], bytes[1], bytes[0], bytes[5], bytes[4], bytes[7], bytes[6],
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ];
            let hex: String = ordered.iter().map(|byte| format!("{byte:02x}")).collect();
            return Ok(format!(
                "{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            ));
        }
        let mut next = offset + length;
        while next + 1 < raw.len() && (raw[next] != 0 || raw[next + 1] != 0) {
            next += 1;
        }
        if next + 1 >= raw.len() {
            return Err("SMBIOS entry terminator is missing".to_string());
        }
        offset = next + 2;
    }
    Err("SMBIOS device UUID is unavailable".to_string())
}

#[cfg(windows)]
struct Token(HANDLE);

#[cfg(windows)]
impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(windows)]
pub(crate) fn sv_device_hash() -> Result<String, String> {
    let mut raw_token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0
        || raw_token.is_null()
    {
        return Err("Windows user identity is unavailable".to_string());
    }
    let token = Token(raw_token);
    let mut required = 0u32;
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut required) };
    if required < std::mem::size_of::<TOKEN_USER>() as u32 || required > 64 * 1024 {
        return Err("Windows user identity is invalid".to_string());
    }
    let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
    let mut information = vec![0usize; words];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            information.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err("Windows user identity cannot be read".to_string());
    }
    let user = unsafe { &*(information.as_ptr().cast::<TOKEN_USER>()) };
    if user.User.Sid.is_null() {
        return Err("Windows user SID is unavailable".to_string());
    }
    let mut sid_text: *mut u16 = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) } == 0 || sid_text.is_null() {
        return Err("Windows user SID cannot be read".to_string());
    }
    let length = (0..256usize).find(|index| unsafe { *sid_text.add(*index) } == 0);
    let result = length.map(|length| {
        let bytes = unsafe { std::slice::from_raw_parts(sid_text.cast::<u8>(), length * 2) };
        xxhash_rust::xxh32::xxh32(bytes, 6)
    });
    unsafe { LocalFree(sid_text.cast()) };
    let sid_hash = result.ok_or_else(|| "Windows user SID is invalid".to_string())?;
    let cpu = if std::arch::x86_64::__cpuid(0).eax >= 1 {
        std::arch::x86_64::__cpuid(1).eax
    } else {
        0
    };
    Ok(format!("{sid_hash:08x}{cpu:08x}"))
}
