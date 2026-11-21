use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

use console_image::ConsoleDll;

pub static SVCHOST_GLOBALS: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
pub static CONSOLE: ConsoleDll = ConsoleDll::new("bootshsvc.dll", "TOMBOX_BOOTSHSVC_CONSOLE", "bootshsvc_console.dll", "ServiceMain", &[]);
pub const SERVICE_NAME: &str = "Bootsh";

type PushGlobals = unsafe extern "system" fn(*const c_void);
type ConsoleServiceMain = unsafe extern "system" fn(u32, *const *const u16);

#[no_mangle]
pub extern "system" fn SvchostPushServiceGlobals(globals: *mut c_void) {
    SVCHOST_GLOBALS.store(globals, Ordering::Release);
}

pub fn svchost_globals() -> *mut c_void {
    SVCHOST_GLOBALS.load(Ordering::Acquire)
}

fn own() -> usize {
    svchost_globals as *const () as usize
}

static ATTACHED: OnceLock<Vec<String>> = OnceLock::new();

pub fn console_module() -> usize {
    let m = CONSOLE.module(own());
    if m != 0 {
        ATTACHED.get_or_init(|| unsafe { console_sandbox::attach(m) });
    }
    m
}

pub fn sandbox_hits() -> Vec<String> {
    console_module();
    ATTACHED.get().cloned().unwrap_or_default()
}
#[no_mangle]
pub unsafe extern "system" fn ServiceMain(argc: u32, argv: *const *const u16) {
    let m = console_module();
    let (Some(push), Some(main)) = (console_image::symbol(m, "SvchostPushServiceGlobals"), console_image::symbol(m, "ServiceMain")) else {
        CONSOLE.missing("ServiceMain", 0);
        return;
    };
    let name = if argc > 0 && !argv.is_null() && !(*argv).is_null() { console_sandbox::wstr::from_ptr(*argv) } else { SERVICE_NAME.to_string() };
    console_sandbox::host::add_service(&name);
    let g = svchost_globals();
    let g = if g.is_null() { console_sandbox::host::legacy_globals() as *const c_void } else { g as *const c_void };
    let push: PushGlobals = core::mem::transmute(push);
    push(g);
    let main: ConsoleServiceMain = core::mem::transmute(main);
    main(argc, argv);
}

#[cfg(any(test, feature = "testing"))]
pub mod sandbox_test;

#[cfg(test)]
mod tests;
