use super::*;

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

unsafe extern "C" fn capture(fmt: *const u8) {
    let s = unsafe { std::ffi::CStr::from_ptr(fmt as *const i8) }.to_string_lossy().into_owned();
    SEEN.lock().unwrap().push(s);
}

fn install() {
    let mut apis = [0u8; 0x60];
    apis[0..4].copy_from_slice(&0x60u32.to_le_bytes());
    apis[8..16].copy_from_slice(&(capture as *const () as usize).to_le_bytes());
    unsafe { WinDbgExtensionDllInit(apis.as_ptr() as *const c_void, 0x0f, 0x4a61) };
}

#[test]
fn symsrv_extension_close_and_usage() {
    install();
    SEEN.lock().unwrap().clear();
    unsafe { symsrv(0, 0, 0, 0, b"close\0".as_ptr()) };
    unsafe { symsrv(0, 0, 0, 0, b"\0".as_ptr()) };
    unsafe { symsrv(0, 0, 0, 0, b"now close it\0".as_ptr()) };
    let seen = SEEN.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0], "symbol server client has been closed\n");
    assert_eq!(seen[1], "!symsrv close - closes the symbol server client so it can be updated\n");
    assert_eq!(seen[2], seen[0]);
    assert!(!close_symbol_server());
}

#[test]
fn extension_api_version_and_init() {
    let v = unsafe { &*ExtensionApiVersion() };
    assert_eq!((v.major, v.minor, v.revision, v.reserved), (10, 0, 6, 0));
    install();
    assert_eq!(output_routine(), capture as *const () as usize);
}
