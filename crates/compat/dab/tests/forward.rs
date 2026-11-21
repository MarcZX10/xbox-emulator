use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn SetDllDirectoryW(dir: *const u16) -> i32;
}

fn wide(s: &std::path::Path) -> Vec<u16> {
    s.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect()
}

struct Staged {
    compat: usize,
    pc: usize,
}

fn staged() -> &'static Staged {
    static S: OnceLock<Staged> = OnceLock::new();
    S.get_or_init(|| {
        let exe = std::env::current_exe().unwrap();
        let mut cands: Vec<PathBuf> = vec![exe.parent().unwrap().parent().unwrap().join("dab.dll")];
        if let Ok(rd) = std::fs::read_dir(exe.parent().unwrap().parent().unwrap().parent().unwrap()) {
            cands.extend(rd.filter_map(|e| e.ok()).map(|e| e.path().join("out").join("dab.dll")));
        }
        let built = cands.into_iter().filter(|p| p.is_file()).max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).expect("built dab.dll");
        assert!(built.is_file(), "{} not built", built.display());
        let dir = std::env::temp_dir().join(format!("tombox-dab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join("dab.dll");
        std::fs::copy(&built, dir.join("dab.dll")).unwrap();
        std::fs::copy(&sys, dir.join(dab::PC_DLL)).unwrap();
        unsafe { SetDllDirectoryW(wide(&dir).as_ptr()) };
        let pc = unsafe { LoadLibraryExW(wide(&dir.join(dab::PC_DLL)).as_ptr(), core::ptr::null_mut(), 8) };
        let compat = unsafe { LoadLibraryExW(wide(&dir.join("dab.dll")).as_ptr(), core::ptr::null_mut(), 8) };
        assert!(!pc.is_null() && !compat.is_null());
        Staged { compat: compat as usize, pc: pc as usize }
    })
}

fn proc(m: usize, name: &str) -> usize {
    let c = format!("{name}\0");
    unsafe { GetProcAddress(m as *mut c_void, c.as_ptr()) as usize }
}

fn ordinal(m: usize, ord: u16) -> usize {
    unsafe { GetProcAddress(m as *mut c_void, ord as usize as *const u8) as usize }
}

fn check(name: &str) {
    let s = staged();
    let (ord, _, target) = dab::FORWARDS.iter().find(|f| f.1 == name).copied().unwrap();
    let pc_name = target.split_once('.').unwrap().1;
    let want = proc(s.pc, pc_name);
    assert_ne!(want, 0, "PC dab.dll exports {pc_name}");
    assert_eq!(proc(s.compat, name), want, "{name} resolves to PC {pc_name}");
    assert_eq!(ordinal(s.compat, ord), want, "{name} ordinal {ord}");
}

#[test]
fn dab_initialize_forwards() {
    check("DabInitialize");
}

#[test]
fn dab_power_state_changed_forwards_to_notify_power_event() {
    check("DabPowerStateChanged");
}

#[test]
fn dab_session_state_changed_forwards_to_notify_session_change() {
    check("DabSessionStateChanged");
}

#[test]
fn dab_terminate_forwards() {
    check("DabTerminate");
}

#[test]
fn dab_notify_power_event_forwards() {
    check("DabNotifyPowerEvent");
}

#[test]
fn dab_notify_session_change_forwards() {
    check("DabNotifySessionChange");
}

#[test]
fn dab_notify_time_change_forwards() {
    check("DabNotifyTimeChange");
}

#[test]
fn every_console_export_is_listed() {
    let names: Vec<&str> = dab::FORWARDS.iter().map(|f| f.1).collect();
    for n in ["DabInitialize", "DabPowerStateChanged", "DabSessionStateChanged", "DabTerminate", "DabNotifyPowerEvent", "DabNotifySessionChange", "DabNotifyTimeChange"] {
        assert!(names.contains(&n), "{n}");
    }
}
