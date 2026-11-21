mod common;

use common::*;

#[test]
fn export_table_matches_exports_txt() {
    check_table("ntdll.dll");
}

#[test]
fn ums_labels_stay_unexported() {
    let h = load_built("ntdll.dll", "tbx_ntdll.dll");
    assert_eq!(check_forwarders(h, &[]), 0);
    for n in ["RtlpExecuteUmsThread", "RtlpUmsExecuteYieldThreadEnd", "RtlpUmsThreadYield"] {
        assert!(proc(h, n).is_null(), "{n}");
    }
}

#[test]
fn rtl_wnf_compare_change_stamp_is_unsigned_three_way() {
    let h = load_built("ntdll.dll", "tbx_ntdll.dll");
    let f: extern "system" fn(u32, u32) -> i32 = unsafe { core::mem::transmute(proc(h, "RtlWnfCompareChangeStamp")) };
    assert_eq!(f(5, 5), 0);
    assert_eq!(f(1, 2), -1);
    assert_eq!(f(2, 1), 1);
    assert_eq!(f(0, 0xffff_ffff), -1);
    assert_eq!(f(0x8000_0000, 1), 1);
}
