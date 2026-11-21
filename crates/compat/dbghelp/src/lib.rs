use core::ffi::c_void;
use std::sync::Mutex;

pub const CLOSED_MSG: &[u8] = b"symbol server client has been closed\n\0";
pub const USAGE_MSG: &[u8] = b"!symsrv close - closes the symbol server client so it can be updated\n\0";

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExtApiVersion {
    pub major: u16,
    pub minor: u16,
    pub revision: u16,
    pub reserved: u16,
}

#[no_mangle]
pub static mut DBGHELP_EXT_API_VERSION: ExtApiVersion = ExtApiVersion { major: 10, minor: 0, revision: 6, reserved: 0 };

pub type OutputRoutine = unsafe extern "C" fn(*const u8);

static APIS: Mutex<[u8; 0x60]> = Mutex::new([0; 0x60]);

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(n: *const u16) -> isize;
    fn GetProcAddress(m: isize, n: *const u8) -> usize;
}

fn output_routine() -> usize {
    let a = APIS.lock().unwrap();
    usize::from_le_bytes(a[8..16].try_into().unwrap())
}

pub fn close_symbol_server() -> bool {
    let name: Vec<u16> = "symsrv.dll".encode_utf16().chain([0]).collect();
    let m = unsafe { GetModuleHandleW(name.as_ptr()) };
    if m == 0 {
        return false;
    }
    let f = unsafe { GetProcAddress(m, b"SymbolServerClose\0".as_ptr()) };
    if f == 0 {
        return false;
    }
    let f: unsafe extern "system" fn() -> i32 = unsafe { core::mem::transmute(f) };
    unsafe { f() };
    true
}

#[no_mangle]
pub unsafe extern "system" fn WinDbgExtensionDllInit(apis: *const c_void, _major: u16, _minor: u16) {
    let mut a = APIS.lock().unwrap();
    unsafe { core::ptr::copy_nonoverlapping(apis as *const u8, a.as_mut_ptr(), 0x60) };
}

#[no_mangle]
pub extern "system" fn ExtensionApiVersion() -> *mut ExtApiVersion {
    core::ptr::addr_of_mut!(DBGHELP_EXT_API_VERSION)
}

#[no_mangle]
pub unsafe extern "system" fn symsrv(_process: isize, _thread: isize, _pc: u64, _processor: u32, args: *const u8) {
    let close = !args.is_null() && {
        let s = unsafe { std::ffi::CStr::from_ptr(args as *const i8) }.to_bytes();
        s.windows(5).any(|w| w == b"close")
    };
    let msg = if close {
        close_symbol_server();
        CLOSED_MSG
    } else {
        USAGE_MSG
    };
    let o = output_routine();
    if o != 0 {
        let f: OutputRoutine = unsafe { core::mem::transmute(o) };
        unsafe { f(msg.as_ptr()) };
    }
}

#[cfg(test)]
mod tests;
