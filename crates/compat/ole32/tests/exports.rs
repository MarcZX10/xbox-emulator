mod common;

use common::*;

#[test]
fn export_table_matches_exports_txt() {
    check_table("ole32.dll");
}

#[test]
fn both_device_catalog_exports_forward_to_combase() {
    let h = load_built("ole32.dll", "tbx_ole32.dll");
    assert_eq!(check_forwarders(h, &[]), 2);
}

#[test]
fn revoke_of_unknown_cookie_matches_combase() {
    let h = load_built("ole32.dll", "tbx_ole32.dll");
    let cb = system("combase.dll");
    unsafe {
        let ours: extern "system" fn(*mut core::ffi::c_void) -> i32 = core::mem::transmute(proc(h, "CoRevokeDeviceCatalog"));
        let theirs: extern "system" fn(*mut core::ffi::c_void) -> i32 = core::mem::transmute(proc(cb, "CoRevokeDeviceCatalog"));
        assert_eq!(ours(core::ptr::null_mut()), theirs(core::ptr::null_mut()));
    }
}
