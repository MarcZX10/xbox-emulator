#![allow(non_snake_case, clippy::missing_safety_doc)]

use console_dll::{Console, Spec};
use core::ffi::c_void;

pub static CONSOLE: Console = Console::new(Spec {
    name: "dxbc2dxil_xs.dll",
    env: "TOMBOX_DXBC2DXIL_CONSOLE",
    alias: "dxbc2dxil_xs_console.dll",
    probe: "SCDxil_ConvertDxbcToDxil",
    emulated_imports: &[],
});

pub const CONVERT_FAILED: u32 = 1;

#[repr(C)]
pub struct Allocator {
    pub alloc: Option<unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void>,
    pub free: Option<unsafe extern "system" fn(*mut c_void, *mut c_void)>,
    pub ctx: *mut c_void,
}

pub type ConvertFn = unsafe extern "system" fn(*const c_void, usize, *mut *mut c_void, *mut usize, *const Allocator) -> u32;

#[no_mangle]
pub unsafe extern "system" fn SCDxil_ConvertDxbcToDxil(src: *const c_void, size: usize, out: *mut *mut c_void, out_size: *mut usize, alloc: *const Allocator) -> u32 {
    match CONSOLE.proc("SCDxil_ConvertDxbcToDxil") {
        Some(p) => {
            let f: ConvertFn = core::mem::transmute(p);
            f(src, size, out, out_size, alloc)
        }
        None => {
            CONSOLE.missing("SCDxil_ConvertDxbcToDxil", "1");
            CONVERT_FAILED
        }
    }
}
