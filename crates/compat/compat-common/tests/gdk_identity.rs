use compat_common::gdk::{self, hook, identity};
use core::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetSystemInfo(info: *mut u8);
}

#[test]
fn series_x_identity_and_console_cpu_view() {
    std::env::set_var("TOMBOX_DEVICE_TYPE", "anaconda");
    assert_eq!(identity::device_type_value(), 7);
    assert_eq!(identity::device_form(), "Xbox Series X");
    assert_eq!(gdk::system::device_form(), "Xbox Series X");
    std::env::set_var("TOMBOX_DEVICE_TYPE", "lockhart");
    assert_eq!(identity::device_type_value(), 6);
    assert_eq!(gdk::system::device_form(), "Xbox Series S");
    std::env::set_var("TOMBOX_DEVICE_TYPE", "anaconda");

    let fam = hook::family_of("<Game><!-- TargetDeviceFamily=\"PC\" --><ExecutableList><Executable TargetDeviceFamily=\"Scarlett\"/></ExecutableList></Game>");
    assert_eq!(fam.as_deref(), Some("scarlett"));

    let cpu = identity::cpu();
    let t = compat_common::os_host::affinity::topology();
    assert_eq!(cpu.count as usize, t.cores);
    assert_eq!(cpu.mask, t.full);
    assert_eq!(cpu.count, cpu.mask.count_ones());

    let targets = identity::targets();
    let names: Vec<&str> = targets.iter().map(|t| t.0).collect();
    assert_eq!(names, ["GetSystemInfo", "GetNativeSystemInfo"]);
    unsafe {
        let me = GetModuleHandleW(core::ptr::null()) as *mut u8;
        let n = hook::patch_module(me, &targets);
        assert!(n >= 1, "test exe imports GetSystemInfo");
        let mut info = [0u8; 48];
        GetSystemInfo(info.as_mut_ptr());
        assert_eq!(u32::from_le_bytes(info[32..36].try_into().unwrap()), cpu.count);
        assert_eq!(u64::from_le_bytes(info[24..32].try_into().unwrap()), cpu.mask);
        assert!(identity::is_tombox_module(me as *mut c_void));
    }
}
