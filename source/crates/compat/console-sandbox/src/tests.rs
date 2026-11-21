use super::*;
use std::sync::{Mutex, Once};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn init() -> std::sync::MutexGuard<'static, ()> {
    static I: Once = Once::new();
    I.call_once(|| {
        let d = std::env::temp_dir().join(format!("tombox-sandbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::env::set_var("TOMBOX_GDK_ROOT", &d);
    });
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn w(s: &str) -> Vec<u16> {
    wstr::wide(s)
}

#[test]
fn paths_stay_inside_the_sandbox() {
    let _g = init();
    let (c, h) = fs::resolve(r"c:\windows\..\..\..\..\x\y.txt").unwrap();
    assert_eq!(c, r"C:\x\y.txt");
    assert!(h.starts_with(fs::fs_root()));
    assert_eq!(fs::resolve(r"boot\a.bsc").unwrap().0, r"C:\Windows\System32\boot\a.bsc");
    assert!(fs::resolve(r"\\server\share\f").is_none());
    assert!(fs::resolve(r"\\.\PhysicalDrive0").is_none());
}

#[test]
fn reg_get_value_reports_sizes_types_and_missing_values() {
    let _g = init();
    let key = r"\Registry\Machine\SOFTWARE\TomBoxSandboxTest";
    registry::set_string(key, "Name", "abc");
    registry::set(key, "Num", registry::REG_DWORD, &7u32.to_le_bytes());
    let mut h = 0usize;
    unsafe {
        assert_eq!(registry::reg_open_key_ex_w(registry::HKEY_LOCAL_MACHINE, w(r"SOFTWARE\TomBoxSandboxTest").as_ptr(), 0, 0x20019, &mut h), 0);
        let mut size = 0u32;
        let mut kind = 0u32;
        assert_eq!(registry::reg_get_value_w(h, core::ptr::null(), w("Name").as_ptr(), 2, &mut kind, core::ptr::null_mut(), &mut size), 0);
        assert_eq!((kind, size), (registry::REG_SZ, 8));
        let mut small = [0u8; 4];
        size = 4;
        assert_eq!(registry::reg_get_value_w(h, core::ptr::null(), w("Name").as_ptr(), 2, &mut kind, small.as_mut_ptr(), &mut size), registry::ERROR_MORE_DATA);
        assert_eq!(size, 8);
        assert_eq!(registry::reg_get_value_w(h, core::ptr::null(), w("Num").as_ptr(), 2, &mut kind, core::ptr::null_mut(), &mut size), registry::ERROR_UNSUPPORTED_TYPE);
        assert_eq!(registry::reg_get_value_w(h, core::ptr::null(), w("Gone").as_ptr(), 0xffff, &mut kind, core::ptr::null_mut(), &mut size), registry::ERROR_FILE_NOT_FOUND);
        let mut name = [0u16; 16];
        let mut nlen = 16u32;
        assert_eq!(registry::reg_enum_value_w(h, 1, name.as_mut_ptr(), &mut nlen, core::ptr::null_mut(), &mut kind, core::ptr::null_mut(), core::ptr::null_mut()), 0);
        assert_eq!(String::from_utf16_lossy(&name[..nlen as usize]), "Num");
        nlen = 16;
        assert_eq!(registry::reg_enum_value_w(h, 2, name.as_mut_ptr(), &mut nlen, core::ptr::null_mut(), &mut kind, core::ptr::null_mut(), core::ptr::null_mut()), registry::ERROR_NO_MORE_ITEMS);
        assert_eq!(registry::reg_close_key(h), 0);
        assert_eq!(registry::reg_close_key(h), registry::ERROR_INVALID_HANDLE);
    }
}

#[test]
fn scm_queries_answer_from_the_in_process_host() {
    let _g = init();
    host::add_service("TomBoxSvc");
    unsafe {
        let scm = host::open_sc_manager_w(core::ptr::null(), core::ptr::null(), 1);
        assert_ne!(scm, 0);
        assert_eq!(host::open_service_w(scm, w("NoSuchSvc").as_ptr(), 4), 0);
        let s = host::open_service_w(scm, w("TomBoxSvc").as_ptr(), 4);
        assert_ne!(s, 0);
        let mut buf = [0u32; 9];
        let mut need = 0u32;
        assert_eq!(host::query_service_status_ex(s, 0, buf.as_mut_ptr() as *mut u8, 8, &mut need), 0);
        assert_eq!(need, 36);
        assert_eq!(host::query_service_status_ex(s, 0, buf.as_mut_ptr() as *mut u8, 36, &mut need), 1);
        assert_eq!(buf[1], host::SERVICE_STOPPED);
        assert_eq!(host::close_service_handle(s), 1);
        assert_eq!(host::close_service_handle(s), 0);
    }
}

#[test]
fn hostname_size_contract_matches_get_computer_name_ex() {
    let _g = init();
    registry::set_string(system::COMPUTER_NAME, "ComputerName", "SANDBOXPC");
    let mut b = [0u16; 4];
    let mut n = 4u32;
    unsafe {
        assert_eq!(system::get_computer_name_ex_w(4, b.as_mut_ptr(), &mut n), 0);
        assert_eq!(n, 10);
        let mut big = [0u16; 16];
        n = 16;
        assert_eq!(system::get_computer_name_ex_w(4, big.as_mut_ptr(), &mut n), 1);
        assert_eq!(String::from_utf16_lossy(&big[..n as usize]), "SANDBOXPC");
        assert_eq!(system::get_computer_name_ex_w(9, big.as_mut_ptr(), &mut n), 0);
    }
}
#[test]
fn import_walk_includes_delay_loads() {
    let _g = init();
    let m = console_image::load_path(std::path::Path::new(r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\netprofmsvc_c.dll"));
    if m == 0 {
        return;
    }
    let imps = unsafe { imports(m) };
    assert!(imps.iter().any(|(d, n)| d.eq_ignore_ascii_case("NSI.dll") && n == "NsiSetAllParameters"), "delay import listed");
}