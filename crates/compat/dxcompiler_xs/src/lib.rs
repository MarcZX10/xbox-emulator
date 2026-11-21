pub mod com;
pub mod guids;
pub mod host;
pub mod native;
pub mod proxy;
pub mod xdxr;

use com::*;
use core::ffi::c_void;
use guids::*;
use host::Host;

const M: &str = "dxcompiler_xs";

pub fn note(func: &str, detail: &str) {
    compat_common::record(M, func, detail);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Host,
    Native,
    Refused,
}

pub unsafe fn create_with(host: Option<&Host>, malloc: *mut c_void, clsid: *const Guid, iid: *const Guid, ppv: *mut *mut c_void) -> (Hr, Route) {
    if ppv.is_null() {
        return (E_POINTER, Route::Refused);
    }
    *ppv = core::ptr::null_mut();
    let (Some(c), Some(i)) = (Guid::read(clsid as *const c_void), Guid::read(iid as *const c_void)) else {
        return (E_POINTER, Route::Refused);
    };
    if let Some((name, Kind::XboxOnly)) = class(&c) {
        note("factory", &format!("{name} {c} is an Xbox-only class (console backend object, no PC DXC equivalent) -> E_NOINTERFACE"));
        return (E_NOINTERFACE, Route::Refused);
    }
    if let Some((name, Kind::XboxOnly)) = interface(&i) {
        note("factory", &format!("{name} {i} is an Xbox-only interface -> E_NOINTERFACE"));
        return (E_NOINTERFACE, Route::Refused);
    }
    if c == CLSID_DXC_COMPILER {
        note("factory", "CLSID_DxcCompiler: the console builds XboxDxcCompiler (HLSL -> Xbox ISA container); the PC host compiler emits plain DXIL");
    }
    if let Some(h) = host {
        let hr = h.create2(malloc, &c, &i, ppv);
        if hr >= 0 && !(*ppv).is_null() {
            *ppv = proxy::wrap_if_compiler(&i, *ppv);
            return (hr, Route::Host);
        }
        if c != CLSID_DXC_UTILS {
            return (hr, Route::Host);
        }
        *ppv = core::ptr::null_mut();
        note("factory", &format!("host refused {} with {:#010x}; using the native IDxcUtils/IDxcLibrary", describe(Some(i), false), hr as u32));
    }
    if c == CLSID_DXC_UTILS {
        return (native::create_utils(&i, ppv), Route::Native);
    }
    (REGDB_E_CLASSNOTREG, Route::Refused)
}

unsafe fn entry(func: &str, malloc: *mut c_void, clsid: *const c_void, iid: *const c_void, ppv: *mut *mut c_void) -> Hr {
    let (hr, route) = create_with(host::global(), malloc, clsid as *const Guid, iid as *const Guid, ppv);
    note(
        func,
        &format!(
            "clsid={} iid={} malloc={:#x} -> {:?} hr={:#010x} obj={:#x}",
            describe(Guid::read(clsid), true),
            describe(Guid::read(iid), false),
            malloc as usize,
            route,
            hr as u32,
            if ppv.is_null() { 0 } else { *ppv as usize }
        ),
    );
    hr
}

#[no_mangle]
pub unsafe extern "system" fn DxcCreateInstance(rclsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> i32 {
    entry("DxcCreateInstance", core::ptr::null_mut(), rclsid, riid, ppv)
}

#[no_mangle]
pub unsafe extern "system" fn DxcCreateInstance2(malloc: *mut c_void, rclsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> i32 {
    entry("DxcCreateInstance2", malloc, rclsid, riid, ppv)
}

pub unsafe fn xdxr_create(malloc: *mut c_void, rclsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> Hr {
    xdxr::create(malloc, rclsid, riid, ppv)
}

#[allow(non_snake_case)]
#[no_mangle]
pub unsafe extern "system" fn CreateXdxrCompiler2(malloc: *mut c_void, rclsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> i32 {
    xdxr_create(malloc, rclsid, riid, ppv)
}

#[allow(non_snake_case)]
#[no_mangle]
pub unsafe extern "system" fn CreateXdxrCompiler(rclsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> i32 {
    xdxr_create(core::ptr::null_mut(), rclsid, riid, ppv)
}

pub const EXPORTS: &str = include_str!("exports.txt");
