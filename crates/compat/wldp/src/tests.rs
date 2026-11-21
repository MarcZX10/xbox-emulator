use super::*;

type SetDynamicCodeTrust = unsafe extern "system" fn(*mut c_void) -> HResult;

#[test]
fn wldp_check_retail_configuration() {
    unsafe {
        let (mut a, mut b, mut c) = (7u32, 7u32, 7u32);
        assert_eq!(WldpCheckRetailConfiguration(core::ptr::null_mut(), &mut a, &mut b, &mut c), 0);
        assert_eq!((a, b, c), (1, 0, 0));
    }
}

#[test]
fn wldp_query_policy_setting_enabled_internal() {
    unsafe {
        assert_eq!(WldpQueryPolicySettingEnabledInternal(0, core::ptr::null_mut()), E_INVALIDARG_NT);
        for bad in [6, 7, 999, 1001, -1] {
            let mut v = 5i32;
            assert_eq!(WldpQueryPolicySettingEnabledInternal(bad, &mut v), E_INVALIDARG_NT);
            assert_eq!(v, 0);
        }
        let f = query_policy_setting_enabled2().expect("host WldpQueryPolicySettingEnabled2");
        for idx in [0, 1, 2, 3, 4, 5, 1000] {
            let name = wide(policy_setting_name(idx).unwrap());
            let mut want = 9i32;
            let hr_want = f(name.as_ptr(), &mut want);
            let mut got = 9i32;
            assert_eq!(WldpQueryPolicySettingEnabledInternal(idx, &mut got), hr_want);
            assert_eq!(got, want);
        }
        assert_eq!(policy_setting_name(3), Some("BrowserSmartscreenSuppressed"));
        assert_eq!(policy_setting_name(1000), Some("VerifiedAndReputablePerfMode"));
    }
}

#[test]
fn wldp_set_dynamic_code_trust2_matches_pc_v1() {
    unsafe {
        let pc = host_wldp_export(b"WldpSetDynamicCodeTrust\0");
        assert_ne!(pc, 0);
        let pc: SetDynamicCodeTrust = core::mem::transmute(pc);
        let ours = WldpSetDynamicCodeTrust2(core::ptr::null_mut());
        assert!(ours < 0);
        assert_eq!(ours as u32 & 0x1000_0000, 0x1000_0000);
        assert_eq!(ours, pc(core::ptr::null_mut()));
    }
}

#[test]
fn wldp_is_vulnerable_driver_blocklist_disabled() {
    let _g = crate::tests_console::env_lock();
    let dir = crate::tests_console::temp_root("export31");
    std::env::set_var("TOMBOX_GDK_ROOT", &dir);
    let s = store::Store::open();
    assert_eq!(s.root(), dir.join("wldp").as_path());
    unsafe {
        let mut d = 7i32;
        assert_eq!(WldpIsVulnerableDriverBlocklistDisabled(&mut d), 0x8007_0002u32 as i32);
        assert_eq!(d, 0);
        for (v, want) in [(1u32, 0i32), (0, 1), (2, 1)] {
            s.set_blocklist_enable(v).unwrap();
            let mut d = 7i32;
            assert_eq!(WldpIsVulnerableDriverBlocklistDisabled(&mut d), 0);
            assert_eq!(d, want);
        }
    }
    std::env::remove_var("TOMBOX_GDK_ROOT");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wldp_is_vulnerable_driver_blocklist_eligible_to_disable() {
    unsafe {
        let mut e = 7i32;
        assert_eq!(WldpIsVulnerableDriverBlocklistEligibleToDisable(&mut e), 0);
        assert!(e == 0 || e == 1);
        let mut info = [0u8; 16];
        let mut ret = 0u32;
        let st = NtQuerySystemInformation(SYSTEM_CI_UNLOCK_INFORMATION, info.as_mut_ptr() as *mut c_void, 0x10, &mut ret);
        let mut v = 0u32;
        let hr = ci_policy_state(&mut v);
        let want = if st >= 0 && ret == 0x10 && info[0] & 2 != 0 { 0 } else { (hr < 0 || v == 0) as i32 };
        assert_eq!(e, want);
        assert!(hr < 0 || v <= 2);
        assert_eq!(hresult_from_win32(2), 0x8007_0002u32 as i32);
        assert_eq!(nt_to_hresult(0xC000_0008u32 as i32), 0xD000_0008u32 as i32);
    }
}
