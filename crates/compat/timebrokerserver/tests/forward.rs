use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn SetDllDirectoryW(dir: *const u16) -> i32;
    fn VirtualQuery(addr: *const c_void, info: *mut [usize; 6], len: usize) -> usize;
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
        let name = "timebrokerserver.dll";
        let mut cands: Vec<PathBuf> = vec![exe.parent().unwrap().parent().unwrap().join(name)];
        if let Ok(rd) = std::fs::read_dir(exe.parent().unwrap().parent().unwrap().parent().unwrap()) {
            cands.extend(rd.filter_map(|e| e.ok()).map(|e| e.path().join("out").join(name)));
        }
        let built = cands.into_iter().filter(|p| p.is_file()).max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).expect("built timebrokerserver.dll");
        let dir = std::env::temp_dir().join(format!("tombox-tbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join("TimeBrokerServer.dll");
        std::fs::copy(&built, dir.join(name)).unwrap();
        std::fs::copy(&sys, dir.join(timebrokerserver::PC_DLL)).unwrap();
        unsafe { SetDllDirectoryW(wide(&dir).as_ptr()) };
        let pc = unsafe { LoadLibraryExW(wide(&dir.join(timebrokerserver::PC_DLL)).as_ptr(), core::ptr::null_mut(), 8) };
        let compat = unsafe { LoadLibraryExW(wide(&dir.join(name)).as_ptr(), core::ptr::null_mut(), 8) };
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
    let (ord, _, target) = timebrokerserver::FORWARDS.iter().find(|f| f.1 == name).copied().unwrap();
    let want = proc(s.pc, target.split_once('.').unwrap().1);
    assert_ne!(want, 0);
    assert_eq!(proc(s.compat, name), want);
    assert_eq!(ordinal(s.compat, ord), want);
}

#[test]
fn service_main_forwards() {
    check("ServiceMain");
}

#[test]
fn svchost_push_service_globals_ex_forwards() {
    check("SvchostPushServiceGlobalsEx");
}

fn region(addr: usize) -> Option<(usize, usize, usize, u32, u32)> {
    let mut info = [0usize; 6];
    if unsafe { VirtualQuery(addr as *const c_void, &mut info, 48) } == 0 {
        return None;
    }
    Some((info[0], info[1], info[3], info[4] as u32, (info[4] >> 32) as u32))
}

fn readable_rw(w: usize, len: usize) -> bool {
    match region(w) {
        Some((b, _, size, state, protect)) => state == 0x1000 && protect == 4 && w + len <= b + size,
        None => false,
    }
}

fn pc_stored_pointers(pc: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut addr = pc;
    while let Some((b, alloc, size, state, protect)) = region(addr) {
        if alloc != pc {
            break;
        }
        if state == 0x1000 && (protect == 4 || protect == 8) {
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(b as *const usize, size / 8) });
        }
        addr = b + size;
    }
    out
}

#[test]
fn svchost_push_service_globals_hands_ex_block_to_pc_service() {
    let s = staged();
    let mut legacy = vec![0u8; timebrokerserver::LEGACY_SIZE];
    legacy[..8].copy_from_slice(&0x5eed_0000_0001usize.to_le_bytes());
    let push = proc(s.compat, "SvchostPushServiceGlobals");
    assert_ne!(push, 0);
    assert_eq!(ordinal(s.compat, 2), push);
    let f: unsafe extern "system" fn(*const u8) = unsafe { std::mem::transmute(push) };
    unsafe { f(legacy.as_ptr()) };
    let stored = pc_stored_pointers(s.pc);
    assert!(!stored.contains(&(legacy.as_ptr() as usize)), "PC must get the Ex block, not the legacy block");
    let ex = stored.into_iter().find(|&w| {
        w % 8 == 0
            && readable_rw(w, timebrokerserver::EX_SIZE)
            && unsafe { (w as *const u32).read() } == 1
            && unsafe { ((w + 8) as *const usize).read() } == 0x5eed_0000_0001
    });
    let ex = ex.expect("PC SvchostPushServiceGlobalsEx stored the adapter's Ex block");
    let reg = unsafe { ((ex + timebrokerserver::EX_REGISTER_STOP) as *const usize).read() };
    let unreg = unsafe { ((ex + timebrokerserver::EX_UNREGISTER_STOP) as *const usize).read() };
    let (_, rm, ..) = region(reg).unwrap();
    let (_, um, ..) = region(unreg).unwrap();
    assert_eq!(rm, s.compat);
    assert_eq!(um, s.compat);
    let u: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(unreg) };
    assert_eq!(unsafe { u(core::ptr::null_mut()) }, 0x57);
}