use super::*;
use crate::host::{Power, TitleVm};
use crate::platform::{install, HostOs, Spec, SYSTEM_TYPE_ANACONDA, VM_TYPE_GAME};
use std::sync::{Arc, Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

fn spec() -> Spec {
    Spec {
        system_type: SYSTEM_TYPE_ANACONDA,
        console_mode: SYSTEM_TYPE_ANACONDA,
        vm_type: VM_TYPE_GAME,
        caps: vec![],
        caps_vm2: vec![],
        caps_vm3: vec![],
        factory: vec![],
        server_type: 0,
        host_os: HostOs { major: 10, minor: 0, build: 22621, revision: 1852, extra: [0, 0] },
        product: platform::PRODUCT_XBOX_GAMEOS,
        os_build: 22621,
        console_id: platform::console_id_of(2533274790395905),
        serial: platform::serial_of(2533274790395905),
        section: true,
        vmctrl: true,
        xconfig: true,
        config_file: None,
        dump_dir: std::env::temp_dir().join(format!("xpal-test-{}", std::process::id())),
    }
}

fn with(f: impl FnOnce(&mut Spec)) -> (MutexGuard<'static, ()>, Arc<Platform>) {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = spec();
    f(&mut s);
    let p = install(Platform::new(s));
    p.host.set_title(TitleVm { id: 0x1234, package_full_name: "Pkg_1.0.0.0_x64__abc".into(), aumid: "Pkg_abc!Game".into(), flags: host::FLAG_CONSTRAINED });
    (g, p)
}

extern "C" fn quiet_iph(_: *const u16, _: *const u16, _: *const u16, _: u32, _: usize) {}

fn with_handler<R>(f: impl FnOnce() -> R) -> R {
    let old = unsafe { set_thread_invalid_parameter_handler(quiet_iph as *const () as usize) };
    let r = f();
    unsafe { set_thread_invalid_parameter_handler(old) };
    r
}

fn fast_fail_child(which: &str) -> u32 {
    let st = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::fast_fail_child_entry", "--nocapture", "--test-threads=1"])
        .env("XPAL_FASTFAIL_CHILD", which)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    st.code().unwrap() as u32
}

#[link(name = "kernel32")]
extern "system" {
    fn SetErrorMode(m: u32) -> u32;
}

#[test]
fn fast_fail_child_entry() {
    let Ok(which) = std::env::var("XPAL_FASTFAIL_CHILD") else { return };
    unsafe { SetErrorMode(0x8003) };
    let _g = with(|_| ());
    let mut b = [0u16; 64];
    let r = if which == "id" { unsafe { XpalGetConsoleIdCch(b.as_mut_ptr(), 10) } } else { unsafe { XpalGetConsoleSerialNumberCch(b.as_mut_ptr(), 5) } };
    std::process::exit(if r { 1 } else { 2 });
}

#[test]
fn title_os_build_from_dump() {
    let root = std::path::Path::new(compat_common::title_os::DUMPS);
    let tm = compat_common::title_os::system32_for_identity(root, "Ubisoft.TrackmaniaNext").unwrap();
    assert_eq!(compat_common::title_os::xvd_version(&tm).map(|v| v[2]), Some(22621));
    let mount = tm.parent().unwrap().parent().unwrap().parent().unwrap().join("Mount");
    assert_eq!(compat_common::title_os::system32_near(&mount), Some(tm));
    let xd = compat_common::title_os::system32_near(&root.join("XDefiant").join("extracted").join("Mount")).unwrap();
    assert_eq!(compat_common::title_os::xvd_version(&xd), Some([10, 0, 22000, 4429]));
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(b: &[u16]) -> String {
    String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
}

#[test]
fn x_get_console_mode() {
    let _g = with(|s| s.console_mode = 0x60);
    assert_eq!(XGetConsoleMode(), 0x60);
    assert_eq!(XGetSystemType(), 0x64);
}

#[test]
fn xpal_get_console_mode() {
    let _g = with(|s| s.console_mode = 0x68);
    assert_eq!(XpalGetConsoleMode(), 0x68);
}

#[test]
fn x_get_system_type() {
    let _g = with(|s| s.system_type = 0x58);
    assert_eq!(XGetSystemType(), 0x58);
}

#[test]
fn xpal_get_system_type() {
    let _g = with(|s| s.system_type = 0x60);
    assert_eq!(XpalGetSystemType(), 0x60);
}

#[test]
fn x_get_generation_type() {
    let _g = with(|s| s.system_type = 0x5f);
    assert_eq!(XGetGenerationType(), 8);
    drop(_g);
    let _g = with(|s| s.system_type = 0x60);
    assert_eq!(XGetGenerationType(), 9);
}

#[test]
fn xpal_get_generation_type() {
    let _g = with(|s| s.system_type = 0x31);
    assert_eq!(XpalGetGenerationType(), 8);
}

#[test]
fn xpal_get_is_gen8() {
    let _g = with(|s| s.system_type = 0x50);
    assert!(XpalGetIsGen8());
    drop(_g);
    let _g = with(|_| ());
    assert!(!XpalGetIsGen8());
}

#[test]
fn x_get_is_scarlett_generation() {
    let _g = with(|_| ());
    assert!(XGetIsScarlettGeneration());
    drop(_g);
    let _g = with(|s| s.system_type = 0x40);
    assert!(!XGetIsScarlettGeneration());
}

#[test]
fn xpal_get_is_gen9() {
    let _g = with(|s| s.system_type = 0x68);
    assert!(XpalGetIsGen9());
}

#[test]
fn x_get_vm_type() {
    let _g = with(|_| ());
    assert_eq!(XGetVmType(), 3);
}

#[test]
fn xpal_get_vm_type() {
    let _g = with(|s| s.vm_type = 2);
    assert_eq!(XpalGetVmType(), 2);
}

#[test]
fn x_get_server_type() {
    let _g = with(|s| s.server_type = 5);
    assert_eq!(XGetServerType(), 0);
    drop(_g);
    let _g = with(|s| {
        s.server_type = 5;
        s.factory = vec![0];
    });
    assert_eq!(XGetServerType(), 5);
}

#[test]
fn xpal_get_server_type() {
    let _g = with(|s| {
        s.server_type = 7;
        s.caps = vec![0x36];
    });
    assert_eq!(XpalGetServerType(), 7);
}

#[test]
fn x_get_host_os_information() {
    let _g = with(|_| ());
    let v = XGetHostOsInformation().to_le_bytes();
    assert_eq!(v[0], 10);
    assert_eq!(v[1], 0);
    assert_eq!(u16::from_le_bytes([v[2], v[3]]), 22621);
    assert_eq!(u16::from_le_bytes([v[4], v[5]]), 1852);
}

#[test]
fn xpal_get_host_os_information() {
    let _g = with(|s| s.host_os = HostOs { major: 10, minor: 1, build: 22000, revision: 4429, extra: [0xAA, 0xBB] });
    let v = XpalGetHostOsInformation().to_le_bytes();
    assert_eq!(v, [10, 1, 0xF0, 0x55, 0x4D, 0x11, 0xAA, 0xBB]);
}

#[test]
fn xpal_get_capabilities() {
    let _g = with(|s| s.caps = vec![3, 33, 127]);
    let mut c = [0u32; 4];
    unsafe { XpalGetCapabilities(c.as_mut_ptr()) };
    assert_eq!(c, [8, 2, 3, 0x8000_0000]);
}

#[test]
fn x_is_capability_enabled() {
    let _g = with(|s| s.caps = vec![0, 0x36, 0x7f]);
    assert!(XIsCapabilityEnabled(0));
    assert!(XIsCapabilityEnabled(0x36));
    assert!(XIsCapabilityEnabled(0x7f));
    assert!(!XIsCapabilityEnabled(1));
    assert!(!XIsCapabilityEnabled(0x80));
    assert!(XIsCapabilityEnabled(0x41));
}

#[test]
fn xpal_is_capability_enabled() {
    let _g = with(|s| s.caps = vec![5]);
    assert!(XpalIsCapabilityEnabled(5));
    assert!(XpalIsCapabilityEnabled(-59));
    assert!(!XpalIsCapabilityEnabled(6));
}

#[test]
fn x_is_capability_enabled_by_vm() {
    let _g = with(|s| {
        s.caps = vec![1];
        s.caps_vm2 = vec![2];
        s.caps_vm3 = vec![4];
    });
    assert!(XIsCapabilityEnabledByVm(1, 0));
    assert!(XIsCapabilityEnabledByVm(1, 1));
    assert!(XIsCapabilityEnabledByVm(2, 2));
    assert!(!XIsCapabilityEnabledByVm(1, 2));
    assert!(XIsCapabilityEnabledByVm(4, 3));
    assert!(!XIsCapabilityEnabledByVm(200, 3));
}

#[test]
fn xpal_is_capability_enabled_by_vm() {
    let _g = with(|s| s.caps_vm2 = vec![100]);
    assert!(XpalIsCapabilityEnabledByVm(100, 2));
    assert!(!XpalIsCapabilityEnabledByVm(100, 0));
}

#[test]
fn x_is_factory_setting_enabled() {
    let _g = with(|s| s.factory = vec![1]);
    assert!(XIsFactorySettingEnabled(1));
    assert!(!XIsFactorySettingEnabled(0));
    assert!(!XIsFactorySettingEnabled(2));
    assert!(!XIsFactorySettingEnabled(3));
}

#[test]
fn xpal_is_factory_setting_enabled() {
    let _g = with(|s| s.caps = vec![0x36]);
    assert!(XpalIsFactorySettingEnabled(0));
    assert!(!XpalIsFactorySettingEnabled(1));
    drop(_g);
    let _g = with(|s| s.factory = vec![63]);
    assert!(XpalIsFactorySettingEnabled(-1));
}

#[test]
fn xpal_is_feature_enabled() {
    let _g = with(|_| ());
    let got: Vec<bool> = (0..17).map(|f| XpalIsFeatureEnabled(f)).collect();
    assert_eq!(got, vec![true, true, false, false, false, false, false, false, true, true, true, true, false, true, true, true, false]);
    drop(_g);
    let _g = with(|s| s.os_build = 22000);
    let got: Vec<bool> = (0..13).map(|f| XpalIsFeatureEnabled(f)).collect();
    assert_eq!(got, vec![true, true, false, false, false, false, false, false, true, true, true, false, false]);
    drop(_g);
    let _g = with(|s| {
        s.system_type = 0x58;
        s.product = PRODUCT_XBOX_KEYSTONE;
    });
    assert!(XpalIsFeatureEnabled(3));
    assert!(!XpalIsFeatureEnabled(1));
    assert!(XpalIsFeatureEnabled(11));
    assert!(!XpalIsFeatureEnabled(6));
    drop(_g);
    let _g = with(|s| {
        s.system_type = 0x40;
        s.factory = vec![2];
    });
    assert!(XpalIsFeatureEnabled(6));
    assert!(XpalIsFeatureEnabled(12));
    assert!(XpalIsFeatureEnabled(5));
    assert!(!XpalIsFeatureEnabled(10));
    drop(_g);
    let _g = with(|s| s.caps = vec![127]);
    assert!(!XpalIsFeatureEnabled(10));
}

#[test]
fn xpal_is_hyper_v() {
    let _g = with(|_| ());
    assert!(!XpalIsHyperV());
}

#[test]
fn xpal_is_internal_devkit() {
    let _g = with(|s| s.caps = vec![3]);
    assert!(XpalIsInternalDevkit());
    assert!(!XpalIsExternalDevkit());
}

#[test]
fn xpal_is_external_devkit() {
    let _g = with(|s| s.caps = vec![127]);
    assert!(XpalIsExternalDevkit());
    assert!(!XpalIsInternalDevkit());
}

#[test]
fn xpal_is_keystone() {
    let _g = with(|_| ());
    assert!(!XpalIsKeystone());
    drop(_g);
    let _g = with(|s| s.product = PRODUCT_XBOX_KEYSTONE);
    assert!(XpalIsKeystone());
}

#[test]
fn xpal_get_console_certificate() {
    let (_g, p) = with(|_| ());
    let c = XpalGetConsoleCertificate();
    assert_eq!(c, p.section.as_ref().unwrap().as_ptr());
    drop(_g);
    let _g = with(|s| s.section = false);
    assert!(XpalGetConsoleCertificate().is_null());
}

#[test]
fn xpal_get_factory_settings() {
    let _g = with(|_| ());
    assert_eq!(XpalGetFactorySettings() as usize, XpalGetConsoleCertificate() as usize + 0x400);
    drop(_g);
    let _g = with(|s| s.section = false);
    assert!(XpalGetFactorySettings().is_null());
}

#[test]
fn xpal_get_console_id_cch() {
    let _g = with(|s| {
        let mut id = [0u8; 17];
        for (i, b) in id.iter_mut().enumerate() {
            *b = i as u8;
        }
        s.console_id = id;
    });
    let mut b = [0u16; 64];
    assert!(unsafe { XpalGetConsoleIdCch(b.as_mut_ptr(), b.len()) });
    assert_eq!(from_wide(&b), "0f0e0d0c.0b0a0908.07060504.03020100.10");
    assert!(with_handler(|| !unsafe { XpalGetConsoleIdCch(b.as_mut_ptr(), 10) }));
    assert_eq!(b[0], 0);
    assert_eq!(fast_fail_child("id"), 0xC000_0409);
    drop(_g);
    let _g = with(|s| s.section = false);
    assert!(unsafe { XpalGetConsoleIdCch(b.as_mut_ptr(), b.len()) });
    assert_eq!(from_wide(&b), CONSOLE_ID_FALLBACK);
}

#[test]
fn xpal_get_console_serial_number_cch() {
    let _g = with(|s| s.serial = *b"012345678901");
    let mut b = [0u16; 13];
    assert!(unsafe { XpalGetConsoleSerialNumberCch(b.as_mut_ptr(), b.len()) });
    assert_eq!(from_wide(&b), "012345678901");
    assert!(with_handler(|| !unsafe { XpalGetConsoleSerialNumberCch(b.as_mut_ptr(), 12) }));
    assert_eq!(b[0], 0);
    assert_eq!(fast_fail_child("serial"), 0xC000_0409);
    drop(_g);
    let _g = with(|s| s.section = false);
    assert!(unsafe { XpalGetConsoleSerialNumberCch(b.as_mut_ptr(), b.len()) });
    assert_eq!(from_wide(&b), SERIAL_FALLBACK);
}

#[test]
fn xpal_get_configuration() {
    let _g = with(|_| ());
    let mut b = [0u8; 8];
    let mut n = 0u32;
    assert_eq!(unsafe { XpalGetConfiguration(7, b.as_mut_ptr(), 8, &mut n) }, STATUS_NOT_FOUND);
    assert_eq!(unsafe { XpalSetConfiguration(7, [1u8, 2, 3, 4].as_ptr(), 4) }, STATUS_SUCCESS);
    assert_eq!(unsafe { XpalGetConfiguration(7, b.as_mut_ptr(), 8, &mut n) }, STATUS_SUCCESS);
    assert_eq!((n, &b[..4]), (4, &[1u8, 2, 3, 4][..]));
    assert_eq!(unsafe { XpalGetConfiguration(7, b.as_mut_ptr(), 2, &mut n) }, STATUS_BUFFER_TOO_SMALL);
    drop(_g);
    let _g = with(|s| s.xconfig = false);
    assert_eq!(unsafe { XpalGetConfiguration(7, b.as_mut_ptr(), 8, &mut n) }, STATUS_OBJECT_NAME_NOT_FOUND);
}

#[test]
fn xpal_set_configuration() {
    let dir = std::env::temp_dir().join(format!("xpal-cfg-{}", std::process::id()));
    let file = dir.join("xconfig.bin");
    let _ = std::fs::remove_file(&file);
    let _g = with(|s| s.config_file = Some(file.clone()));
    assert_eq!(unsafe { XpalSetConfiguration(9, [0xAAu8; 3].as_ptr(), 3) }, STATUS_SUCCESS);
    assert_eq!(unsafe { XpalSetConfiguration(9, core::ptr::null(), 0) }, STATUS_INVALID_PARAMETER);
    drop(_g);
    let _g = with(|s| s.config_file = Some(file.clone()));
    let mut b = [0u8; 3];
    let mut n = 0;
    assert_eq!(unsafe { XpalGetConfiguration(9, b.as_mut_ptr(), 3, &mut n) }, STATUS_SUCCESS);
    assert_eq!(b, [0xAA; 3]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn xpal_request_crash_dump() {
    let (_g, p) = with(|_| ());
    assert_eq!(XpalRequestCrashDump(9, 0, 0, 0, 0), STATUS_INVALID_PARAMETER);
    assert_eq!(XpalRequestCrashDump(4, 8, 0, 0, 0), STATUS_INVALID_PARAMETER);
    assert_eq!(XpalRequestCrashDump(4, 4, 0x1122_3344_5566_7788, 0x99AA_BBCC_DDEE_FF00, 0), STATUS_SUCCESS);
    let d = p.host.state().dumps.last().cloned().unwrap();
    assert!(std::fs::metadata(&d.path).unwrap().len() > 0);
    assert_eq!(d.reason, 4);
    assert_eq!(d.kind, host::MINIDUMP_MINI);
    assert_eq!(d.correlation[..8], 0x1122_3344_5566_7788u64.to_le_bytes());
    assert_eq!(d.correlation[8..], 0x99AA_BBCC_DDEE_FF00u64.to_le_bytes());
    let _ = std::fs::remove_file(&d.path);
    drop(_g);
    let _g = with(|s| s.vmctrl = false);
    assert_eq!(XpalRequestCrashDump(4, 0, 0, 0, 0), STATUS_OBJECT_NAME_NOT_FOUND);
}

#[test]
fn xpal_set_information() {
    let (_g, p) = with(|_| ());
    assert_eq!(unsafe { XpalSetInformation(1, [1u8].as_ptr(), 1) }, STATUS_INVALID_PARAMETER);
    assert_eq!(unsafe { XpalSetInformation(0, [5u8, 6].as_ptr(), 2) }, STATUS_SUCCESS);
    assert_eq!(p.host.state().information, vec![5, 6]);
}

#[test]
fn xpal_query_title_vm_info() {
    let _g = with(|_| ());
    let mut o = [0xCCu8; TITLE_VM_INFO_SIZE];
    assert!(unsafe { XpalQueryTitleVmInfo(o.as_mut_ptr()) });
    assert_eq!(u32::from_le_bytes(o[0..4].try_into().unwrap()), 0x1234);
    assert_eq!(&o[4..7], &[1, 0, 1]);
    let w = |r: core::ops::Range<usize>| from_wide(&o[r].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
    assert_eq!(w(8..0x10E), "Pkg_abc!Game");
    assert_eq!(w(0x10E..0x20E), "Pkg_1.0.0.0_x64__abc");
    drop(_g);
    let _g = with(|s| s.vmctrl = false);
    assert!(!unsafe { XpalQueryTitleVmInfo(o.as_mut_ptr()) });
    assert_eq!(unsafe { GetLastError() }, ERROR_FILE_NOT_FOUND);
    assert!(o.iter().all(|&b| b == 0));
}

#[test]
fn xpal_wait_for_title_vm_termination() {
    let (_g, p) = with(|_| ());
    let name = wide("Pkg_abc!Game");
    assert!(!unsafe { XpalWaitForTitleVmTermination(name.as_ptr(), 10) });
    assert_eq!(unsafe { GetLastError() }, unsafe { RtlNtStatusToDosError(STATUS_IO_TIMEOUT) });
    assert!(!unsafe { XpalWaitForTitleVmTermination(wide("Other").as_ptr(), 10) });
    let p2 = p.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        p2.host.terminate_title();
    });
    assert!(unsafe { XpalWaitForTitleVmTermination(name.as_ptr(), host::INFINITE) });
    t.join().unwrap();
}

#[test]
fn xpal_initiate_system_shutdown() {
    let (_g, p) = with(|_| ());
    assert!(!XpalIsRebootRequested());
    assert!(XpalInitiateSystemShutdown(3, 2));
    assert_eq!(p.host.state().shutdown, Some((3, true)));
    assert!(!p.host.state().title_running);
    assert!(XpalIsRebootRequested());
}

#[test]
fn xpal_initiate_power_state_toggle() {
    let (_g, p) = with(|_| ());
    assert!(XpalInitiatePowerStateToggle(2));
    assert_eq!(p.host.state().power, Power::ConnectedStandby);
    assert!(XpalInitiatePowerStateToggle(1));
    assert_eq!(p.host.state().power, Power::On);
}

#[test]
fn xpal_query_power_toggle_source() {
    let _g = with(|_| ());
    let mut v = 0u32;
    assert!(unsafe { XpalQueryPowerToggleSource(&mut v) });
    assert_eq!(v, 0);
    assert!(XpalInitiatePowerStateToggle(6));
    assert!(unsafe { XpalQueryPowerToggleSource(&mut v) });
    assert_eq!(v, 6);
}

#[test]
fn xpal_background_activity_request() {
    let (_g, p) = with(|_| ());
    assert!(!XpalBackgroundActivityRequest(0));
    assert!(XpalBackgroundActivityRequest(1));
    assert!(XpalBackgroundActivityRequest(2));
    assert!(XpalBackgroundActivityRequest(0));
    assert_eq!(p.host.state().background_requests, 1);
}

#[test]
fn xpal_refresh_power_chime_mode() {
    let (_g, p) = with(|_| ());
    p.host.state().chime_mode = 9;
    assert_eq!(XpalRefreshPowerChimeMode(), 1);
    assert_eq!(p.host.state().chime_mode, compat_common::gdk::setting("xpal_power_chime").and_then(|v| v.parse().ok()).unwrap_or(0));
    drop(_g);
    let _g = with(|s| s.vmctrl = false);
    assert_eq!(XpalRefreshPowerChimeMode(), 0);
}

#[test]
fn xpal_is_reboot_requested() {
    let _g = with(|_| ());
    assert!(!XpalIsRebootRequested());
    assert!(XpalInitiateSystemShutdown(0, 0));
    assert!(!XpalIsRebootRequested());
    assert!(XpalInitiateSystemShutdown(0, 1));
    assert!(XpalIsRebootRequested());
}

#[test]
fn xpal_is_silent_boot_mode() {
    let (_g, p) = with(|_| ());
    assert!(!XpalIsSilentBootMode());
    p.host.set_silent_boot(true);
    assert!(XpalIsSilentBootMode());
}

#[test]
fn xpal_is_transitioning_to_connected_standby() {
    let (_g, p) = with(|_| ());
    assert!(!XpalIsTransitioningToConnectedStandby());
    assert!(XpalInitiatePowerStateToggle(1));
    assert!(XpalIsTransitioningToConnectedStandby());
    p.host.complete_transition();
    assert!(!XpalIsTransitioningToConnectedStandby());
}

#[test]
fn xpal_exit_forced_cs() {
    let (_g, p) = with(|_| ());
    assert!(!XpalExitForcedCs());
    assert_eq!(unsafe { GetLastError() }, unsafe { RtlNtStatusToDosError(STATUS_INVALID_DEVICE_STATE) });
    p.host.enter_forced_cs();
    assert!(XpalExitForcedCs());
    assert_eq!(XpalIsInForcedCs(), 0);
}

#[test]
fn xpal_is_in_forced_cs() {
    let (_g, p) = with(|_| ());
    assert_eq!(XpalIsInForcedCs(), 0);
    p.host.enter_forced_cs();
    assert_eq!(XpalIsInForcedCs(), 1);
}

#[test]
fn xpal_notify_forced_cs_heartbeat() {
    let (_g, p) = with(|_| ());
    assert_eq!(XpalNotifyForcedCsHeartbeat(42), 1);
    assert_eq!(p.host.state().heartbeats, 1);
    assert_eq!(p.host.state().last_heartbeat.map(|h| h.0), Some(42));
}

#[test]
fn xpal_report_uem() {
    let (_g, p) = with(|_| ());
    assert!(XpalReportUem(1, 2, 3));
    assert_eq!(p.host.state().uem, vec![[1, 2, 3]]);
}

#[test]
fn xpal_initiate_dump() {
    let (_g, p) = with(|_| ());
    assert_eq!(XpalInitiateDump(1 << 40), 0);
    assert_eq!(XpalInitiateDump(0), 1);
    let d = p.host.state().dumps.last().cloned().unwrap();
    assert!(std::fs::metadata(&d.path).unwrap().len() > 0);
    let _ = std::fs::remove_file(&d.path);
}

#[test]
fn xpal_set_wakeup_timer() {
    let (_g, p) = with(|_| ());
    assert!(XpalSetWakeupTimer(600));
    assert_eq!(p.host.state().wakeup_timer, Some(600));
    assert!(XpalSetWakeupTimer(0));
    assert_eq!(p.host.state().wakeup_timer, None);
    assert!(XpalInitiatePowerStateToggle(1));
    assert!(XpalSetWakeupTimer(1));
    p.host.state().wake_at = Some(std::time::Instant::now());
    assert_eq!(p.host.state().power, Power::On);
    assert_eq!(p.host.state().woken_by_timer, 1);
    assert!(!XpalIsTransitioningToConnectedStandby());
}

#[test]
fn xpal_set_system_time() {
    let (_g, p) = with(|_| ());
    let target = now_filetime() + 10_000_000 * 3600;
    assert!(unsafe { XpalSetSystemTime(&target) });
    let t = p.host.emulated_system_time();
    assert!(t >= target && t < target + 10_000_000 * 60);
    assert!(!unsafe { XpalSetSystemTime(core::ptr::null()) });
}

#[test]
fn publisher_id_matches_windows() {
    let _g = with(|_| ());
    assert_eq!(host::package_publisher_id("CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US").as_deref(), Some("8wekyb3d8bbwe"));
}

#[test]
fn system32_helpers_ignore_a_same_named_module_already_loaded() {
    let p = String::from_utf16_lossy(&nt::system32_path("dbghelp.dll").unwrap());
    assert!(p.to_ascii_lowercase().ends_with("\\system32\\dbghelp.dll\0"));
    let dir = std::env::temp_dir().join(format!("xpal_dbghelp_shadow_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("dbghelp.dll");
    let win = std::path::PathBuf::from(std::env::var("WINDIR").unwrap()).join("System32");
    std::fs::copy(win.join("version.dll"), &fake).unwrap();
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(n: *const u16) -> *mut core::ffi::c_void;
    }
    let w: Vec<u16> = fake.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    assert!(!unsafe { LoadLibraryW(w.as_ptr()) }.is_null());
    assert_ne!(nt::system32_proc("dbghelp.dll", b"MiniDumpWriteDump\0"), 0);
    assert!(nt::minidump_writer().is_some());
}
