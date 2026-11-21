use core::ffi::c_void;
use std::sync::Mutex;

pub const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const HR_TIMEOUT: i32 = 0x8007_0102u32 as i32;
pub const XFP_NOT_CONNECTED: i32 = 0xd000_00a3u32 as i32;
pub const XFP_BAD_MODE: i32 = 0xd000_000du32 as i32;
pub const XFP_SHORT_REPLY: i32 = 0x8b08_001au32 as i32;
pub const XFP_REPLY_ERROR: i32 = 0x8b08_001bu32 as i32;
pub const XFP_THREAD_EXITED: i32 = 0x8b08_001cu32 as i32;
pub const XFP_WAIT_FAILED: i32 = 0x8b08_001du32 as i32;
pub const MSG_CONNECT: u32 = 0;
pub const MSG_KEYS: u32 = 3;
pub const MSG_GET_LED: u32 = 5;
pub const MSG_SET_LED: u32 = 6;
pub const MSG_DISPLAY: u32 = 7;
pub const MSG_MODE: u32 = 9;
pub const HEADER: usize = 0x10;
pub const DISPLAY_BYTES: usize = 0x2000;
pub const PIPE_PATH: &str = "\\\\.\\pipe\\hevpipe\\XFP";
pub const CONSOLE_PIPE_PATH: &str = "\\\\.\\hevpipe\\XFP";
pub const CLIENT_NAME: &[u8] = b"GameCore";
const INVALID: isize = -1;
const ERROR_IO_PENDING: u32 = 0x3e5;
const INFINITE: u32 = 0xffff_ffff;

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u64,
    event: isize,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *const c_void, disp: u32, flags: u32, tmpl: isize) -> isize;
    fn CloseHandle(h: isize) -> i32;
    fn GetLastError() -> u32;
    fn SetNamedPipeHandleState(h: isize, mode: *const u32, max: *const u32, timeout: *const u32) -> i32;
    fn WriteFile(h: isize, buf: *const u8, n: u32, written: *mut u32, ov: *mut Overlapped) -> i32;
    fn ReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32, ov: *mut Overlapped) -> i32;
    fn GetOverlappedResult(h: isize, ov: *mut Overlapped, n: *mut u32, wait: i32) -> i32;
    fn CancelIoEx(h: isize, ov: *mut Overlapped) -> i32;
    fn CreateEventW(sa: *const c_void, manual: i32, initial: i32, name: *const u16) -> isize;
    fn SetEvent(h: isize) -> i32;
    fn ResetEvent(h: isize) -> i32;
    fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    fn WaitForMultipleObjects(n: u32, hs: *const isize, all: i32, ms: u32) -> u32;
    fn CreateThread(sa: *const c_void, stack: usize, f: unsafe extern "system" fn(*mut c_void) -> u32, p: *mut c_void, flags: u32, id: *mut u32) -> isize;
    fn GetExitCodeThread(h: isize, code: *mut u32) -> i32;
}

struct State {
    keys: u32,
    led: u16,
    denied: bool,
    pipe: isize,
    thread: isize,
    stop: isize,
    led_event: isize,
}

static STATE: Mutex<State> = Mutex::new(State { keys: 0, led: 0, denied: false, pipe: INVALID, thread: 0, stop: 0, led_event: INVALID });
static PATH: Mutex<Option<String>> = Mutex::new(None);

fn st() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_pipe_path(p: Option<&str>) {
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = p.map(str::to_string);
}

pub fn pipe_path() -> String {
    PATH.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .or_else(|| std::env::var("TOMBOX_XFP_PIPE").ok())
        .unwrap_or_else(|| PIPE_PATH.to_string())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn hr_last() -> i32 {
    let e = unsafe { GetLastError() } as i32;
    if e > 0 {
        (e & 0xffff) | 0x8007_0000u32 as i32
    } else {
        e
    }
}

fn open_pipe() -> isize {
    let p = wide(&pipe_path());
    unsafe { CreateFileW(p.as_ptr(), 0xc000_0000, 0, core::ptr::null(), 3, 0x4000_0000, 0) }
}

pub fn header(cmd: u32, len: usize) -> Vec<u8> {
    let mut m = vec![0u8; len];
    m[..4].copy_from_slice(&cmd.to_le_bytes());
    m
}

fn send(pipe: isize, msg: &[u8]) -> i32 {
    let mut n = 0u32;
    if unsafe { WriteFile(pipe, msg.as_ptr(), msg.len() as u32, &mut n, core::ptr::null_mut()) } == 0 {
        return hr_last();
    }
    0
}

pub fn pack_8bpp(src: &[u8]) -> Vec<u8> {
    (0..DISPLAY_BYTES).map(|i| (src[2 * i] & 0xf0) | (src[2 * i + 1] >> 4)).collect()
}

#[no_mangle]
pub extern "system" fn XfpCheckAvailability() -> i32 {
    let s = st();
    if s.denied {
        return E_ACCESSDENIED;
    }
    if s.pipe != INVALID {
        return 0;
    }
    let h = open_pipe();
    if h == INVALID {
        return hr_last();
    }
    unsafe { CloseHandle(h) };
    0
}

#[no_mangle]
pub extern "system" fn XfpCleanup() {
    let thread = {
        let mut s = st();
        core::mem::replace(&mut s.thread, 0)
    };
    if thread != 0 {
        let stop = st().stop;
        unsafe {
            SetEvent(stop);
            CloseHandle(thread);
        }
        return;
    }
    let mut s = st();
    unsafe { CloseHandle(s.stop) };
    s.stop = 0;
    if s.pipe != INVALID {
        unsafe { CloseHandle(s.pipe) };
        s.pipe = INVALID;
    }
}

#[no_mangle]
pub unsafe extern "system" fn XfpGetKeyState(keys: *mut u32) -> i32 {
    *keys = st().keys;
    0
}

#[no_mangle]
pub unsafe extern "system" fn XfpGetLEDState(led: *mut u16) -> i32 {
    *led = 0;
    let (pipe, ev) = {
        let s = st();
        (s.pipe, s.led_event)
    };
    if pipe == INVALID {
        return XFP_NOT_CONNECTED;
    }
    ResetEvent(ev);
    let r = send(pipe, &header(MSG_GET_LED, 0x12));
    if r != 0 {
        return r;
    }
    match WaitForSingleObject(ev, 10_000) {
        0 => {
            let s = st();
            *led = s.led;
            ResetEvent(s.led_event);
            0
        }
        0x102 => HR_TIMEOUT,
        _ => hr_last(),
    }
}

#[no_mangle]
pub unsafe extern "system" fn XfpSetDisplayBuffer(buffer: *const u8, format: u8) -> i32 {
    if buffer.is_null() {
        return E_POINTER;
    }
    let pipe = st().pipe;
    if pipe == INVALID {
        return XFP_NOT_CONNECTED;
    }
    let mut m = header(MSG_DISPLAY, HEADER + DISPLAY_BYTES);
    match format {
        4 => m[HEADER..].copy_from_slice(core::slice::from_raw_parts(buffer, DISPLAY_BYTES)),
        8 => m[HEADER..].copy_from_slice(&pack_8bpp(core::slice::from_raw_parts(buffer, 2 * DISPLAY_BYTES))),
        _ => return E_INVALIDARG,
    }
    send(pipe, &m)
}

#[no_mangle]
pub extern "system" fn XfpSetDisplayMode(mode: i32) -> i32 {
    let pipe = st().pipe;
    if pipe == INVALID {
        return XFP_NOT_CONNECTED;
    }
    if mode >= 0xc {
        return XFP_BAD_MODE;
    }
    let mut m = header(MSG_MODE, 0x14);
    m[HEADER..HEADER + 4].copy_from_slice(&mode.to_le_bytes());
    send(pipe, &m)
}

#[no_mangle]
pub extern "system" fn XfpSetLEDState(led: u16) -> i32 {
    let pipe = st().pipe;
    if pipe == INVALID {
        return XFP_NOT_CONNECTED;
    }
    let mut m = header(MSG_SET_LED, 0x12);
    m[HEADER..HEADER + 2].copy_from_slice(&led.to_le_bytes());
    send(pipe, &m)
}

struct ThreadArgs {
    connected: isize,
    pipe: isize,
}

unsafe fn read_reply(pipe: isize, out: &mut [u8; 0x12]) -> i32 {
    let mut ov = Overlapped::default();
    let mut n = 0u32;
    if ReadFile(pipe, out.as_mut_ptr(), 0x12, &mut n, &mut ov) == 0 {
        let e = GetLastError();
        if e != ERROR_IO_PENDING || GetOverlappedResult(pipe, &mut ov, &mut n, 1) == 0 {
            return hr_last();
        }
    }
    if n < 0x12 {
        return XFP_SHORT_REPLY;
    }
    if u32::from_le_bytes(out[..4].try_into().unwrap()) != 0 {
        return XFP_REPLY_ERROR;
    }
    0
}

unsafe extern "system" fn reader(p: *mut c_void) -> u32 {
    let a = &*(p as *const ThreadArgs);
    let (connected, pipe) = (a.connected, a.pipe);
    let ev = CreateEventW(core::ptr::null(), 1, 0, core::ptr::null());
    let code: i32;
    if ev == 0 {
        code = hr_last();
    } else {
        SetEvent(connected);
        let mut buf = vec![0u8; HEADER + DISPLAY_BYTES];
        code = 'outer: loop {
            let mut ov = Overlapped { event: ev, ..Default::default() };
            let mut n = 0u32;
            if ReadFile(pipe, buf.as_mut_ptr(), buf.len() as u32, &mut n, &mut ov) != 0 {
                SetEvent(ev);
            } else if GetLastError() != ERROR_IO_PENDING {
                break hr_last();
            }
            loop {
                let hs = [st().stop, ev];
                match WaitForMultipleObjects(2, hs.as_ptr(), 0, INFINITE) {
                    0 => {
                        CancelIoEx(pipe, &mut ov);
                        WaitForSingleObject(ev, INFINITE);
                        break 'outer 0;
                    }
                    1 => break,
                    _ => continue,
                }
            }
            if GetOverlappedResult(pipe, &mut ov, &mut n, 0) == 0 {
                break hr_last();
            }
            if u16::from_le_bytes([buf[4], buf[5]]) != 0 {
                continue;
            }
            match u32::from_le_bytes(buf[..4].try_into().unwrap()) {
                MSG_KEYS => st().keys = u32::from_le_bytes(buf[HEADER..HEADER + 4].try_into().unwrap()),
                MSG_GET_LED => {
                    let mut s = st();
                    s.led = u16::from_le_bytes([buf[HEADER], buf[HEADER + 1]]);
                    SetEvent(s.led_event);
                }
                _ => {}
            }
        };
        CloseHandle(ev);
    }
    let mut s = st();
    if s.stop != 0 {
        CloseHandle(s.stop);
        s.stop = 0;
    }
    if code >= 0 && s.pipe != INVALID {
        CloseHandle(s.pipe);
        s.pipe = INVALID;
    }
    code as u32
}

unsafe fn connect() -> i32 {
    let pipe = open_pipe();
    if pipe == INVALID {
        return hr_last();
    }
    let mut connected = 0isize;
    let mut r = 'body: {
        let mode = 2u32;
        if SetNamedPipeHandleState(pipe, &mode, core::ptr::null(), core::ptr::null()) == 0 {
            break 'body hr_last();
        }
        let mut m = header(MSG_CONNECT, 0x22);
        m[HEADER..HEADER + 2].copy_from_slice(&2u16.to_le_bytes());
        m[HEADER + 2..HEADER + 2 + CLIENT_NAME.len()].copy_from_slice(CLIENT_NAME);
        let w = send(pipe, &m);
        if w != 0 {
            break 'body w;
        }
        let mut reply = [0u8; 0x12];
        let rr = read_reply(pipe, &mut reply);
        if rr < 0 {
            break 'body rr;
        }
        if u16::from_le_bytes([reply[0x10], reply[0x11]]) == 0 {
            st().denied = true;
            break 'body E_ACCESSDENIED;
        }
        st().denied = false;
        let led = CreateEventW(core::ptr::null(), 1, 0, core::ptr::null());
        if led == 0 {
            break 'body hr_last();
        }
        st().led_event = led;
        connected = CreateEventW(core::ptr::null(), 1, 0, core::ptr::null());
        if connected == 0 {
            break 'body hr_last();
        }
        let stop = CreateEventW(core::ptr::null(), 1, 0, core::ptr::null());
        if stop == 0 {
            break 'body hr_last();
        }
        st().stop = stop;
        let args = Box::into_raw(Box::new(ThreadArgs { connected, pipe }));
        let th = CreateThread(core::ptr::null(), 0, reader, args.cast(), 0, core::ptr::null_mut());
        if th == 0 {
            drop(Box::from_raw(args));
            break 'body hr_last();
        }
        st().thread = th;
        let hs = [th, connected];
        let w = WaitForMultipleObjects(2, hs.as_ptr(), 0, INFINITE);
        drop(Box::from_raw(args));
        match w {
            1 => {
                st().pipe = pipe;
                CloseHandle(connected);
                return 0;
            }
            0 => {
                let mut code = 0u32;
                GetExitCodeThread(th, &mut code);
                if (code as i32) < 0 {
                    code as i32
                } else {
                    XFP_THREAD_EXITED
                }
            }
            _ => XFP_WAIT_FAILED,
        }
    };
    CloseHandle(pipe);
    if connected != 0 {
        CloseHandle(connected);
    }
    if r < 0 {
        let mut s = st();
        if s.stop != 0 {
            CloseHandle(s.stop);
            s.stop = 0;
        }
        if s.thread != 0 {
            CloseHandle(s.thread);
            s.thread = 0;
        }
    } else {
        r = 0;
    }
    r
}

#[no_mangle]
pub extern "system" fn XfpTryConnect() -> i32 {
    static CS: Mutex<()> = Mutex::new(());
    let _g = CS.lock().unwrap_or_else(|e| e.into_inner());
    {
        let s = st();
        if s.denied {
            return E_ACCESSDENIED;
        }
        if s.pipe != INVALID {
            return 0;
        }
    }
    unsafe { connect() }
}

#[cfg(test)]
mod tests;
