mod common;

use common::*;

#[test]
fn export_table_matches_exports_txt() {
    check_table("kernel32.dll");
}

#[test]
fn every_export_forwards_to_its_kernelbase_or_ntdll_function() {
    let h = load_built("kernel32.dll", "tbx_kernel32.dll");
    assert_eq!(check_forwarders(h, &[]), rows().len());
}

#[test]
fn leap_second_information_through_the_forwarder() {
    let h = load_built("kernel32.dll", "tbx_kernel32.dll");
    unsafe {
        let f: unsafe extern "system" fn(*mut i32, *mut u32) -> i32 = core::mem::transmute(proc(h, "GetSystemLeapSecondInformation"));
        let (mut enabled, mut flags) = (-1i32, 0xffff_ffffu32);
        assert_eq!(f(&mut enabled, &mut flags), 1);
        assert!(enabled == 0 || enabled == 1);
        assert_eq!(flags, 0);
    }
}
