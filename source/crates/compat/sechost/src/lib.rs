#![allow(non_snake_case)]

use core::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const ERROR_INVALID_HANDLE: u32 = 6;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const ERROR_PROC_NOT_FOUND: u32 = 127;
pub const ERROR_CTX_CLOSE_PENDING: u32 = 7007;
pub const INVALID_PROCESSTRACE_HANDLE: u64 = u64::MAX;
pub const PROCESS_TRACE_MODE_REAL_TIME: u32 = 0x100;
const LOGFILE_PROCESS_TRACE_MODE: usize = 0x1c;
const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;

pub const STATE_IDLE: u32 = 0;
pub const STATE_PROCESSING: u32 = 1;
pub const STATE_CLOSING: u32 = 2;

#[derive(Debug)]
pub struct Consumer {
    pub handle: u64,
    pub state: u32,
    pub mode: u32,
    pub events_lost: u32,
    pub buffers_lost: u32,
}

static TABLE: Mutex<Vec<Box<Consumer>>> = Mutex::new(Vec::new());

pub fn table() -> MutexGuard<'static, Vec<Box<Consumer>>> {
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

fn find(t: &mut [Box<Consumer>], handle: u64) -> Option<&mut Consumer> {
    t.iter_mut().find(|c| c.handle == handle).map(|b| &mut **b)
}

fn bad_handle(handle: u64) -> bool {
    handle.wrapping_sub(1) > (-3i64) as u64
}

pub fn register(handle: u64, mode: u32) {
    let mut t = table();
    t.retain(|c| c.handle != handle);
    t.push(Box::new(Consumer { handle, state: STATE_IDLE, mode, events_lost: 0, buffers_lost: 0 }));
}

pub fn query(handle: u64, events_lost: *mut u32, buffers_lost: *mut u32) -> u32 {
    if bad_handle(handle) {
        return ERROR_INVALID_HANDLE;
    }
    if events_lost.is_null() || buffers_lost.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let mut t = table();
    match find(&mut t, handle) {
        Some(c) if c.mode & PROCESS_TRACE_MODE_REAL_TIME != 0 => {
            unsafe {
                *events_lost = c.events_lost;
                *buffers_lost = c.buffers_lost;
            }
            0
        }
        _ => ERROR_INVALID_HANDLE,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CloseStep {
    Pending,
    Invalid,
    Release,
    Unknown,
}

pub fn close_step(handle: u64) -> CloseStep {
    if bad_handle(handle) {
        return CloseStep::Invalid;
    }
    let mut t = table();
    let Some(pos) = t.iter().position(|c| c.handle == handle) else { return CloseStep::Unknown };
    match t[pos].state {
        STATE_PROCESSING => {
            t[pos].state = STATE_CLOSING;
            CloseStep::Pending
        }
        STATE_CLOSING => CloseStep::Invalid,
        _ => {
            t.remove(pos);
            CloseStep::Release
        }
    }
}

pub fn begin_processing(handles: &[u64]) -> Result<Vec<u64>, u32> {
    let mut t = table();
    let mut known = Vec::new();
    for &h in handles {
        if let Some(c) = find(&mut t, h) {
            if c.state != STATE_IDLE {
                for k in &known {
                    if let Some(c) = find(&mut t, *k) {
                        c.state = STATE_IDLE;
                    }
                }
                return Err(ERROR_INVALID_HANDLE);
            }
            c.state = STATE_PROCESSING;
            known.push(h);
        }
    }
    Ok(known)
}

pub fn end_processing(known: &[u64]) {
    let mut t = table();
    for &h in known {
        if let Some(pos) = t.iter().position(|c| c.handle == h) {
            match t[pos].state {
                STATE_PROCESSING => t[pos].state = STATE_IDLE,
                STATE_CLOSING => {
                    t.remove(pos);
                }
                _ => {}
            }
        }
    }
}

type OpenFn = unsafe extern "system" fn(*mut c_void) -> u64;
type CloseFn = unsafe extern "system" fn(u64) -> u32;
type ProcessFn = unsafe extern "system" fn(*const u64, u32, *const c_void, *const c_void) -> u32;

struct Pc {
    open: Option<OpenFn>,
    close: Option<CloseFn>,
    process: Option<ProcessFn>,
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn SetLastError(code: u32);
}

fn pc() -> &'static Pc {
    static PC: OnceLock<Pc> = OnceLock::new();
    PC.get_or_init(|| unsafe {
        let name: Vec<u16> = "advapi32.dll".encode_utf16().chain(Some(0)).collect();
        let m = LoadLibraryExW(name.as_ptr(), core::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        let get = |n: &[u8]| if m.is_null() { core::ptr::null_mut() } else { GetProcAddress(m, n.as_ptr()) };
        let (o, c, p) = (get(b"OpenTraceW\0"), get(b"CloseTrace\0"), get(b"ProcessTrace\0"));
        Pc {
            open: (!o.is_null()).then(|| core::mem::transmute::<*mut c_void, OpenFn>(o)),
            close: (!c.is_null()).then(|| core::mem::transmute::<*mut c_void, CloseFn>(c)),
            process: (!p.is_null()).then(|| core::mem::transmute::<*mut c_void, ProcessFn>(p)),
        }
    })
}

pub unsafe fn open_with(open: OpenFn, logfile: *mut c_void) -> u64 {
    let mode = if logfile.is_null() { 0 } else { core::ptr::read_unaligned((logfile as *const u8).add(LOGFILE_PROCESS_TRACE_MODE) as *const u32) };
    let h = open(logfile);
    if h != INVALID_PROCESSTRACE_HANDLE {
        register(h, mode);
    }
    h
}

pub unsafe fn close_with(close: CloseFn, handle: u64) -> u32 {
    match close_step(handle) {
        CloseStep::Invalid => ERROR_INVALID_HANDLE,
        CloseStep::Pending => {
            close(handle);
            ERROR_CTX_CLOSE_PENDING
        }
        CloseStep::Release | CloseStep::Unknown => close(handle),
    }
}

pub unsafe fn process_with(process: ProcessFn, handles: *const u64, count: u32, start: *const c_void, end: *const c_void) -> u32 {
    if handles.is_null() || count == 0 {
        return process(handles, count, start, end);
    }
    let list = core::slice::from_raw_parts(handles, count as usize);
    let known = match begin_processing(list) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let r = process(handles, count, start, end);
    end_processing(&known);
    r
}

#[no_mangle]
pub unsafe extern "system" fn EtwQueryRealtimeConsumer(handle: u64, events_lost: *mut u32, buffers_lost: *mut u32) -> u32 {
    query(handle, events_lost, buffers_lost)
}

#[no_mangle]
pub unsafe extern "system" fn OpenTraceW(logfile: *mut c_void) -> u64 {
    match pc().open {
        Some(f) => open_with(f, logfile),
        None => {
            SetLastError(ERROR_PROC_NOT_FOUND);
            INVALID_PROCESSTRACE_HANDLE
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn CloseTrace(handle: u64) -> u32 {
    match pc().close {
        Some(f) => close_with(f, handle),
        None => ERROR_PROC_NOT_FOUND,
    }
}

#[no_mangle]
pub unsafe extern "system" fn ProcessTrace(handles: *const u64, count: u32, start: *const c_void, end: *const c_void) -> u32 {
    match pc().process {
        Some(f) => process_with(f, handles, count, start, end),
        None => ERROR_PROC_NOT_FOUND,
    }
}

#[cfg(test)]
mod tests;
