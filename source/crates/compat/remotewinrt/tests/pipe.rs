use core::ffi::c_void;
use remotewinrt::*;

#[link(name = "kernel32")]
extern "system" {
    fn CreateNamedPipeW(n: *const u16, open: u32, mode: u32, max: u32, out: u32, inb: u32, t: u32, sa: *mut c_void) -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[test]
fn waits_for_listening_pipe_and_reports_missing() {
    let name = format!(r"\\.\pipe\tombox-svc-remotewinrt-{}", std::process::id());
    let w = wide(&name);
    unsafe {
        assert_eq!(WaitRemotePipe(w.as_ptr(), 50), 0);
        assert_eq!(GetLastError(), 2);
        let h = CreateNamedPipeW(w.as_ptr(), 3, 0, 1, 512, 512, 0, core::ptr::null_mut());
        assert!(h as isize != -1);
        assert_eq!(WaitRemotePipe(w.as_ptr(), 1000), 1);
        assert_eq!(WaitRemotePipe(w.as_ptr(), 0), 1);
        assert_eq!(WaitRemotePipe(w.as_ptr(), u32::MAX), 1);
        CloseHandle(h);
        assert_eq!(WaitRemotePipe(wide(r"\\.\pipe").as_ptr(), 10), 0);
        assert_eq!(GetLastError(), ERROR_INVALID_NAME);
        assert_eq!(WaitRemotePipe(wide(r"\\srv\pipe\x").as_ptr(), 10), 0);
        assert_eq!(GetLastError(), ERROR_INVALID_NAME);
    }
}
