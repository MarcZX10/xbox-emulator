use crate::com::*;
use crate::guids::*;
use console_dll::{Console, Spec};
use core::ffi::c_void;

pub static CONSOLE: Console = Console::new(Spec {
    name: "dxcompiler_xs.dll",
    env: "TOMBOX_DXCXS_CONSOLE",
    alias: "dxcompiler_xs_console.dll",
    probe: "CreateXdxrCompiler2",
    emulated_imports: &[],
});

pub type Create2Fn = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> Hr;

pub unsafe fn create(malloc: *mut c_void, rclsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> Hr {
    if ppv.is_null() {
        return E_POINTER;
    }
    let c = Guid::read(rclsid);
    let hr = match CONSOLE.proc("CreateXdxrCompiler2") {
        Some(p) => {
            let f: Create2Fn = core::mem::transmute(p);
            f(malloc, rclsid, riid, ppv)
        }
        None => {
            *ppv = core::ptr::null_mut();
            CONSOLE.missing("CreateXdxrCompiler2", "REGDB_E_CLASSNOTREG");
            REGDB_E_CLASSNOTREG
        }
    };
    crate::note(
        "CreateXdxrCompiler2",
        &format!(
            "clsid={} iid={} malloc={:#x} -> {:#010x} obj={:#x} via {}",
            describe(c, true),
            describe(Guid::read(riid), false),
            malloc as usize,
            hr as u32,
            *ppv as usize,
            CONSOLE.path().map_or("no console DLL".into(), |p| p.display().to_string())
        ),
    );
    hr
}
