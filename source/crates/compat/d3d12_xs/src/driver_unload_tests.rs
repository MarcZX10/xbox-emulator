use crate::driver_state::{adopt_driver_module, owned_driver_module};
use crate::vmem_x::D3D12XboxUnloadDriverModule;
use core::ffi::c_void;

extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetModuleHandleW(p: *const u16) -> *mut c_void;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

#[test]
fn unload_frees_the_owned_driver_module_and_clears_state() {
    let name = wide("dciman32.dll");
    assert!(unsafe { GetModuleHandleW(name.as_ptr()) }.is_null());
    let h = unsafe { LoadLibraryW(name.as_ptr()) };
    assert!(!h.is_null());
    assert_eq!(adopt_driver_module(h), h);
    assert_eq!(owned_driver_module(), h);
    D3D12XboxUnloadDriverModule();
    assert!(owned_driver_module().is_null());
    assert!(unsafe { GetModuleHandleW(name.as_ptr()) }.is_null());
    D3D12XboxUnloadDriverModule();
    assert!(owned_driver_module().is_null());
}
