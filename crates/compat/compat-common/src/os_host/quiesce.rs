use core::ffi::c_void;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub const TIMEOUT_VAR: &str = "TOMBOX_SUSPEND_TIMEOUT_MS";
pub const HR_SUSPEND_TIMEOUT: i32 = 0x87e5001b_u32 as i32;

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn CheckRemoteDebuggerPresent(p: *mut c_void, out: *mut i32) -> i32;
    fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, n: u32) -> u32;
    fn GetSystemTime(t: *mut [u16; 8]);
}

pub fn timeout() -> Option<Duration> {
    std::env::var(TIMEOUT_VAR).ok()?.trim().parse::<u64>().ok().map(Duration::from_millis)
}

pub fn debugger_attached() -> bool {
    let mut d = 0i32;
    unsafe { CheckRemoteDebuggerPresent(GetCurrentProcess(), &mut d) != 0 && d != 0 }
}

pub fn module_path() -> Option<Vec<u16>> {
    let mut b = vec![0u16; 0x104];
    let n = unsafe { GetModuleFileNameW(core::ptr::null_mut(), b.as_mut_ptr(), b.len() as u32) } as usize;
    if n == 0 {
        return None;
    }
    b.truncate(n);
    Some(b)
}

pub fn system_time() -> [u16; 8] {
    let mut t = [0u16; 8];
    unsafe { GetSystemTime(&mut t) };
    t
}

pub fn timed_out() -> Option<i32> {
    let module = module_path().unwrap_or_default();
    let r = crate::gdk::launcher::host_quiesce(HR_SUSPEND_TIMEOUT, system_time(), &module);
    crate::record_raw("os_host", "plm", &format!("suspend not completed within {:?}: hr {:#010x}; GameHostService quiesce record -> {}", timeout(), HR_SUSPEND_TIMEOUT, match r {
        Some(hr) => format!("hr {hr:#010x}"),
        None => "no GameHostService in this process".into(),
    }));
    r
}

pub fn watch_with(rx: &Receiver<usize>, limit: Option<Duration>, debugger: bool, on_timeout: impl FnOnce()) -> usize {
    if let (Some(t), false) = (limit, debugger) {
        match rx.recv_timeout(t) {
            Ok(n) => return n,
            Err(RecvTimeoutError::Disconnected) => return 0,
            Err(RecvTimeoutError::Timeout) => on_timeout(),
        }
    }
    rx.recv().unwrap_or(0)
}

pub fn watch(rx: &Receiver<usize>) -> usize {
    watch_with(rx, timeout(), debugger_attached(), || {
        timed_out();
    })
}
