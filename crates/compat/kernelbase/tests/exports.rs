mod common;

use common::*;

type Get = unsafe extern "system" fn(*const u16, *mut [u32; 2]) -> i32;

fn get() -> Get {
    let h = load_built("kernelbase.dll", "tbx_kernelbase.dll");
    assert_eq!(check_forwarders(h, &[]), 0);
    unsafe { core::mem::transmute(proc(h, "GetPackageInstallTime")) }
}

#[test]
fn export_table_matches_exports_txt() {
    check_table("kernelbase.dll");
}

#[test]
fn family_xref_exports_stay_unexported() {
    let h = load_built("kernelbase.dll", "tbx_kernelbase.dll");
    assert!(proc(h, "AddPackageToFamilyXref").is_null());
    assert!(proc(h, "RemovePackageFromFamilyXref").is_null());
}

#[test]
fn get_package_install_time_console_errors() {
    let f = get();
    let mut ft = [7u32, 7];
    unsafe {
        assert_eq!(f(core::ptr::null(), &mut ft), 0x57);
        assert_eq!(f(wide("A_1.0.0.0_x64__8wekyb3d8bbwe").as_ptr(), core::ptr::null_mut()), 0x57);
        assert_eq!(f(wide("TomBox.Unregistered_1.0.0.0_x64__8wekyb3d8bbwe").as_ptr(), &mut ft), 0x490);
        #[link(name = "kernel32")]
        extern "system" {
            fn PackageFamilyNameFromFullName(full: *const u16, len: *mut u32, family: *mut u16) -> i32;
        }
        let bad = wide("not a package name");
        let mut b = [0u16; 65];
        let mut n = 65u32;
        let pc = PackageFamilyNameFromFullName(bad.as_ptr(), &mut n, b.as_mut_ptr());
        assert_ne!(pc, 0);
        assert_eq!(f(bad.as_ptr(), &mut ft), pc);
    }
    assert_eq!(ft, [7, 7]);
}
