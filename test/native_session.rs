use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
};

#[test]
fn helper_refuses_to_run_inside_an_unrecognized_executable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session_bridge.dll");
    std::fs::write(&path, super::bridge::HELPER).unwrap();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let library = unsafe { LoadLibraryW(wide.as_ptr()) };
    assert!(!library.is_null());
    let proc = unsafe { GetProcAddress(library, b"BridgeReload\0".as_ptr()) }.unwrap();
    let reload: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
        unsafe { std::mem::transmute(proc) };
    let mut request = [0u64; 5];
    let result = unsafe { reload(request.as_mut_ptr().cast()) };
    unsafe {
        FreeLibrary(library);
    }
    assert_eq!(result, 10);
}
