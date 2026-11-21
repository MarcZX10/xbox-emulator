#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};

include!(concat!(env!("OUT_DIR"), "/forwards.rs"));

pub const PC_DLL: &str = "timebrokerserver_sys.dll";
pub const EX_SIZE: usize = 0xe0;
pub const LEGACY_SIZE: usize = 0xc8;
pub const LEGACY_REGISTER_STOP: usize = 0xc0;
pub const EX_REGISTER_STOP: usize = 0xd0;
pub const EX_UNREGISTER_STOP: usize = 0xd8;
pub const ERROR_INVALID_PARAMETER: u32 = 0x57;
pub const ERROR_MOD_NOT_FOUND: u32 = 0x7e;
pub const ERROR_PROC_NOT_FOUND: u32 = 0x7f;

pub type RegisterStopCallback = unsafe extern "system" fn(*mut *mut c_void, *const u16, *mut c_void, *mut c_void, *mut c_void, u32) -> u32;
pub type UnregisterStopCallback = unsafe extern "system" fn(*mut c_void) -> u32;
type PushEx = unsafe extern "system" fn(*mut c_void);

#[link(name = "kernel32")]
extern "system" {
    fn UnregisterWaitEx(wait: *mut c_void, completion: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, len: u32) -> u32;
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

static LEGACY_REGISTER: AtomicUsize = AtomicUsize::new(0);
static EX_BLOCK: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static PUSH_ERROR: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn register_stop_callback(wait: *mut *mut c_void, service: *const u16, object: *mut c_void, callback: *mut c_void, context: *mut c_void, flags: u32) -> u32 {
    let f = LEGACY_REGISTER.load(Ordering::Acquire);
    if f == 0 {
        return ERROR_INVALID_PARAMETER;
    }
    let f: RegisterStopCallback = std::mem::transmute(f);
    f(wait, service, object, callback, context, flags)
}

unsafe extern "system" fn unregister_stop_callback(wait: *mut c_void) -> u32 {
    if wait.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    UnregisterWaitEx(wait, std::ptr::null_mut());
    0
}

pub unsafe fn build_ex(legacy: *const u8) -> *mut u8 {
    let ex = Box::into_raw(Box::new([0u8; EX_SIZE])) as *mut u8;
    (ex as *mut u32).write_unaligned(1);
    if !legacy.is_null() {
        std::ptr::copy_nonoverlapping(legacy, ex.add(8), LEGACY_SIZE);
        LEGACY_REGISTER.store((legacy.add(LEGACY_REGISTER_STOP) as *const usize).read_unaligned(), Ordering::Release);
    } else {
        LEGACY_REGISTER.store(0, Ordering::Release);
    }
    (ex.add(EX_REGISTER_STOP) as *mut usize).write_unaligned(register_stop_callback as RegisterStopCallback as usize);
    (ex.add(EX_UNREGISTER_STOP) as *mut usize).write_unaligned(unregister_stop_callback as UnregisterStopCallback as usize);
    EX_BLOCK.store(ex, Ordering::Release);
    ex
}

pub fn last_push_error() -> u32 {
    PUSH_ERROR.load(Ordering::Acquire)
}

pub fn last_ex_block() -> *mut u8 {
    EX_BLOCK.load(Ordering::Acquire)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub unsafe fn load_pc() -> Result<*mut c_void, u32> {
    let mut me: *mut c_void = std::ptr::null_mut();
    if GetModuleHandleExW(6, build_ex as *const u16, &mut me) == 0 {
        return Err(GetLastError());
    }
    let mut buf = vec![0u16; 32768];
    let n = GetModuleFileNameW(me, buf.as_mut_ptr(), buf.len() as u32) as usize;
    if n == 0 {
        return Err(GetLastError());
    }
    let path = String::from_utf16_lossy(&buf[..n]);
    let dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let full = wide(&dir.join(PC_DLL).to_string_lossy());
    let h = LoadLibraryExW(full.as_ptr(), std::ptr::null_mut(), 0x1100);
    if h.is_null() {
        let e = GetLastError();
        return Err(if e == 0 { ERROR_MOD_NOT_FOUND } else { e });
    }
    Ok(h)
}

#[no_mangle]
pub unsafe extern "system" fn SvchostPushServiceGlobals(legacy: *const u8) {
    let ex = if legacy.is_null() { std::ptr::null_mut() } else { build_ex(legacy) };
    if legacy.is_null() {
        LEGACY_REGISTER.store(0, Ordering::Release);
        EX_BLOCK.store(std::ptr::null_mut(), Ordering::Release);
    }
    let pc = match load_pc() {
        Ok(pc) => pc,
        Err(e) => {
            PUSH_ERROR.store(e, Ordering::Release);
            return;
        }
    };
    let p = GetProcAddress(pc, b"SvchostPushServiceGlobalsEx\0".as_ptr());
    if p.is_null() {
        PUSH_ERROR.store(ERROR_PROC_NOT_FOUND, Ordering::Release);
        return;
    }
    PUSH_ERROR.store(0, Ordering::Release);
    let push: PushEx = std::mem::transmute(p);
    push(ex as *mut c_void);
}

#[cfg(test)]
mod tests;
