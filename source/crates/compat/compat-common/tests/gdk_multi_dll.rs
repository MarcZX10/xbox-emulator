use core::ffi::c_void;
use std::path::PathBuf;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, addr: *const c_void, out: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, n: u32) -> u32;
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> *mut c_void;
    fn WinHttpConnect(s: *mut c_void, server: *const u16, port: u16, r: u32) -> *mut c_void;
    fn WinHttpOpenRequest(c: *mut c_void, verb: *const u16, obj: *const u16, ver: *const u16, refr: *const u16, acc: *const *const u16, flags: u32) -> *mut c_void;
    fn WinHttpCloseHandle(h: *mut c_void) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid(u32, u16, u16, [u8; 8]);

const XUSER: Guid = Guid(0x01acd177, 0x91f9, 0x4763, [0xa3, 0x8e, 0xcc, 0xbb, 0x55, 0xce, 0x32, 0xe0]);
const XUSER_V2: Guid = Guid(0x1bf2f8c5, 0xd507, 0x4e52, [0xbb, 0x05, 0xf7, 0x26, 0xd0, 0xe7, 0x11, 0x61]);
const XNET: Guid = Guid(0x37e56907, 0x2f10, 0x41e8, [0xb7, 0x2f, 0x36, 0xed, 0xb1, 0x85, 0x33, 0x1a]);
const XASYNC: Guid = Guid(0x073b7dcb, 0x1fcf, 0x4030, [0x94, 0xbe, 0xe3, 0xc9, 0xeb, 0x62, 0x34, 0x28]);
const XSTORE: Guid = Guid(0x0dd112ac, 0x7c24, 0x448c, [0xb9, 0x2b, 0x39, 0x60, 0xfb, 0x5b, 0xd3, 0x0c]);
const XSTORE_V1: Guid = Guid(0xde3dbdd4, 0x0b37, 0x4bdb, [0xa1, 0x0e, 0xac, 0xf3, 0xa3, 0x54, 0xd0, 0x6a]);

const FORTNITE_ORDER: &[&str] = &["mfplat", "XAudio2_9", "xmem", "d3d12_xs", "xcrdapi", "dxcompiler_xs", "acphal", "AudioHALWrapper"];
const REQUIRED: &[&str] = &["mfplat", "xmem", "d3d12_xs", "acphal"];

#[repr(C)]
struct Block {
    queue: usize,
    context: usize,
    callback: usize,
    internal: [u8; 32],
}

static QUERY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COUNT_HR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(99);
static STATUS_IN_CB: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

unsafe extern "system" fn on_updates(b: *mut Block) {
    use std::sync::atomic::Ordering::SeqCst;
    let q: Query = core::mem::transmute(QUERY.load(SeqCst));
    let (a, status) = method(q, XASYNC, XASYNC, 3);
    let status: unsafe extern "system" fn(*mut c_void, *mut Block, bool) -> i32 = core::mem::transmute(status);
    STATUS_IN_CB.store(status(a, b, false), SeqCst);
    let (s, count) = method(q, XSTORE, XSTORE_V1, 56);
    let count: unsafe extern "system" fn(*mut c_void, *mut Block, *mut u32) -> i32 = core::mem::transmute(count);
    let mut n = 0u32;
    COUNT_HR.store(count(s, b, &mut n), SeqCst);
    COUNT.store(n, SeqCst);
}

unsafe fn privilege_and_package_updates(q: Query, user: usize, queue: usize, label: &str) {
    use std::sync::atomic::Ordering::SeqCst;
    COUNT_HR.store(1, SeqCst);
    COUNT.store(99, SeqCst);
    STATUS_IN_CB.store(1, SeqCst);
    let (u, check) = method(q, XUSER, XUSER_V2, 20);
    let check: unsafe extern "system" fn(*mut c_void, usize, u32, u32, *mut bool, *mut u32) -> i32 = core::mem::transmute(check);
    let (mut has, mut why) = (false, 0u32);
    assert_eq!(check(u, user, 0, 254, &mut has, &mut why), 0, "{label}: XUserCheckPrivilege");
    assert!(has, "{label}: multiplayer privilege denied, reason {why}");
    let (s, ctx) = method(q, XSTORE, XSTORE_V1, 3);
    let ctx: unsafe extern "system" fn(*mut c_void, usize, *mut usize) -> i32 = core::mem::transmute(ctx);
    let mut sc = 0usize;
    assert_eq!(ctx(s, user, &mut sc), 0, "{label}: XStoreCreateContext");
    let (_, upd) = method(q, XSTORE, XSTORE_V1, 55);
    let upd: unsafe extern "system" fn(*mut c_void, usize, *mut Block) -> i32 = core::mem::transmute(upd);
    let mut b = Box::new(Block { queue, context: 0, callback: on_updates as usize, internal: [0; 32] });
    assert_eq!(upd(s, sc, &mut *b), 0, "{label}: XStoreQueryGameAndDlcPackageUpdatesAsync");
    let (a, status) = method(q, XASYNC, XASYNC, 3);
    let status: unsafe extern "system" fn(*mut c_void, *mut Block, bool) -> i32 = core::mem::transmute(status);
    assert_eq!(status(a, &mut *b, true), 0, "{label}: XAsyncGetStatus(wait)");
    for _ in 0..400 {
        if COUNT_HR.load(SeqCst) != 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(STATUS_IN_CB.load(SeqCst), 0, "{label}: XAsyncGetStatus in completion callback");
    assert_eq!(COUNT_HR.load(SeqCst), 0, "{label}: XStoreQueryGameAndDlcPackageUpdatesResultCount");
    assert_eq!(COUNT.load(SeqCst), 0, "{label}: package update count");
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[path = "support/built_dlls.rs"]
mod built_dlls;

fn built_dir() -> PathBuf {
    built_dlls::ensure(&[
        ("mfplat", "mfplat.dll"),
        ("xaudio2_9", "XAudio2_9.dll"),
        ("xmem", "xmem.dll"),
        ("d3d12_xs", "d3d12_xs.dll"),
        ("xcrdapi", "xcrdapi.dll"),
        ("dxcompiler_xs", "dxcompiler_xs.dll"),
        ("acphal", "acphal.dll"),
        ("audiohalwrapper", "AudioHALWrapper.dll"),
    ])
}

type Query = unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32;

unsafe fn method(q: Query, c: Guid, i: Guid, slot: usize) -> (*mut c_void, usize) {
    let mut o = core::ptr::null_mut();
    assert_eq!(q(&c, &i, &mut o), 0, "QueryApiImpl {:08x}/{:08x}", c.0, i.0);
    (o, *(*(o as *const *const usize)).add(slot))
}

unsafe fn module_of(p: *const c_void) -> String {
    let mut m = core::ptr::null_mut();
    assert_ne!(GetModuleHandleExW(6, p, &mut m), 0);
    let mut b = [0u16; 520];
    let n = GetModuleFileNameW(m, b.as_mut_ptr(), b.len() as u32);
    String::from_utf16_lossy(&b[..n as usize])
}

#[test]
fn full_dll_set_in_fortnite_order_with_default_runtime() {
    assert!(std::env::var("TOMBOX_GDK_RUNTIME").is_err() || std::env::var("TOMBOX_GDK_RUNTIME").as_deref() == Ok("1"));
    let src = built_dir();
    let stage = std::env::temp_dir().join(format!("tombox_multi_dll_{}", std::process::id()));
    std::fs::create_dir_all(&stage).unwrap();
    std::env::set_var("TOMBOX_GDK_ROOT", stage.join("gdk"));
    std::fs::copy(std::env::var("WINDIR").unwrap() + "\\System32\\mfplat.dll", stage.join("mfplat_sys.dll")).unwrap();
    let mut staged = Vec::new();
    for n in FORTNITE_ORDER {
        let s = src.join(format!("{n}.dll"));
        if s.exists() {
            std::fs::copy(&s, stage.join(format!("{n}.dll"))).unwrap();
            staged.push(*n);
        } else {
            assert!(!REQUIRED.contains(n), "{n}.dll missing from {}", src.display());
        }
    }
    eprintln!("staged {staged:?} from {}", src.display());
    unsafe {
        for n in &staged {
            let p = w(&stage.join(format!("{n}.dll")).to_string_lossy());
            let h = LoadLibraryExW(p.as_ptr(), core::ptr::null_mut(), 8);
            assert!(!h.is_null(), "LoadLibrary {n}.dll failed: {}", std::io::Error::last_os_error());
        }
        assert_eq!(std::env::var("TOMBOX_GDK_OWNER").ok(), Some(GetCurrentProcessId().to_string()), "exactly one GDK owner per process");

        let rt = LoadLibraryW(w("xgameruntime.dll").as_ptr());
        assert!(!rt.is_null(), "xgameruntime.dll not loadable");
        let q1 = GetProcAddress(rt, b"QueryApiImpl\0".as_ptr());
        let q2 = GetProcAddress(rt, b"QueryApiImpl\0".as_ptr());
        assert!(!q1.is_null());
        assert_eq!(q1, q2);
        let owner = module_of(q1);
        eprintln!("QueryApiImpl served by {owner}");
        assert!(owner.to_ascii_lowercase().starts_with(&stage.to_string_lossy().to_ascii_lowercase()), "QueryApiImpl not interposed: {owner}");
        let first_gdk = staged.iter().find(|n| **n != "xcrdapi").unwrap();
        assert!(owner.to_ascii_lowercase().ends_with(&format!("{}.dll", first_gdk.to_ascii_lowercase())), "owner {owner} is not the first loaded GDK DLL {first_gdk}");
        let q: Query = core::mem::transmute(q1);

        let (a, create) = method(q, XASYNC, XASYNC, 12);
        let create: unsafe extern "system" fn(*mut c_void, u32, u32, *mut usize) -> i32 = core::mem::transmute(create);
        let (_, status) = method(q, XASYNC, XASYNC, 3);
        let status: unsafe extern "system" fn(*mut c_void, *mut Block, bool) -> i32 = core::mem::transmute(status);
        let (_, close_q) = method(q, XASYNC, XASYNC, 17);
        let close_q: unsafe extern "system" fn(*mut c_void, usize) = core::mem::transmute(close_q);
        let mut queue = 0usize;
        assert_eq!(create(a, 1, 1, &mut queue), 0);
        assert_ne!(queue, 0);

        let (u, add) = method(q, XUSER, XUSER_V2, 7);
        let add: unsafe extern "system" fn(*mut c_void, u32, *mut Block) -> i32 = core::mem::transmute(add);
        let (_, add_result) = method(q, XUSER, XUSER_V2, 8);
        let add_result: unsafe extern "system" fn(*mut c_void, *mut Block, *mut usize) -> i32 = core::mem::transmute(add_result);
        let (_, get_id) = method(q, XUSER, XUSER_V2, 11);
        let get_id: unsafe extern "system" fn(*mut c_void, usize, *mut u64) -> i32 = core::mem::transmute(get_id);
        let mut b = Box::new(Block { queue, context: 0, callback: 0, internal: [0; 32] });
        assert_eq!(add(u, 1, &mut *b), 0);
        assert_eq!(status(a, &mut *b, true), 0);
        let mut user = 0usize;
        assert_eq!(add_result(u, &mut *b, &mut user), 0);
        assert_ne!(user, 0);
        let mut xuid = 0u64;
        assert_eq!(get_id(u, user, &mut xuid), 0);
        assert_ne!(xuid, 0);

        let (n, verify) = method(q, XNET, XNET, 14);
        let verify: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u8) -> i32 = core::mem::transmute(verify);
        assert!(verify(n, core::ptr::null_mut(), core::ptr::null()) < 0);
        let s = WinHttpOpen(w("tombox-test").as_ptr(), 1, core::ptr::null(), core::ptr::null(), 0);
        let c = WinHttpConnect(s, w("account-public-service-prod.ol.epicgames.com").as_ptr(), 443, 0);
        let r = WinHttpOpenRequest(c, w("POST").as_ptr(), w("/account/api/oauth/token").as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null(), 0x0080_0000);
        assert!(!r.is_null());
        let hr = verify(n, r, core::ptr::null());
        eprintln!("XNetworkingVerifyServerCertificate on unsent login request -> {hr:#010x}");
        WinHttpCloseHandle(r);
        WinHttpCloseHandle(c);
        WinHttpCloseHandle(s);
        QUERY.store(q1 as usize, std::sync::atomic::Ordering::SeqCst);
        privilege_and_package_updates(q, user, 0, "process queue");
        let (_, composite) = method(q, XASYNC, XASYNC, 13);
        let composite: unsafe extern "system" fn(*mut c_void, usize, usize, *mut usize) -> i32 = core::mem::transmute(composite);
        let (_, get_port) = method(q, XASYNC, XASYNC, 14);
        let get_port: unsafe extern "system" fn(*mut c_void, usize, u32, *mut usize) -> i32 = core::mem::transmute(get_port);
        let (_, dup) = method(q, XASYNC, XASYNC, 15);
        let dup: unsafe extern "system" fn(*mut c_void, usize, *mut usize) -> i32 = core::mem::transmute(dup);
        let (_, terminate) = method(q, XASYNC, XASYNC, 22);
        let terminate: unsafe extern "system" fn(*mut c_void, usize, bool, *mut c_void, usize) -> i32 = core::mem::transmute(terminate);
        let mut base = 0usize;
        assert_eq!(create(a, 1, 1, &mut base), 0);
        let (mut wp, mut cp) = (0usize, 0usize);
        assert_eq!(get_port(a, base, 0, &mut wp), 0);
        assert_eq!(get_port(a, base, 1, &mut cp), 0);
        let (mut online, mut chat, mut eos) = (0usize, 0usize, 0usize);
        assert_eq!(composite(a, wp, cp, &mut online), 0);
        assert_eq!(composite(a, wp, cp, &mut chat), 0);
        assert_eq!(composite(a, wp, cp, &mut eos), 0);
        let mut online2 = 0usize;
        assert_eq!(dup(a, online, &mut online2), 0);
        assert_eq!(terminate(a, chat, true, core::ptr::null_mut(), 0), 0);
        close_q(a, chat);
        close_q(a, eos);
        close_q(a, base);
        privilege_and_package_updates(q, user, online2, "composite queue after sibling composite terminate/close and base close");
        close_q(a, online2);
        privilege_and_package_updates(q, user, online, "composite after closing its duplicate");
        close_q(a, online);
        let mut own = 0usize;
        assert_eq!(create(a, 1, 1, &mut own), 0);
        let mut other = 0usize;
        assert_eq!(create(a, 1, 1, &mut other), 0);
        assert_eq!(terminate(a, other, true, core::ptr::null_mut(), 0), 0);
        close_q(a, other);
        privilege_and_package_updates(q, user, own, "thread-pool queue after another queue terminated");
        close_q(a, own);
        close_q(a, queue);
    }
}
