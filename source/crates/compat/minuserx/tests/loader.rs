use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

use compat_common::os_host::{self, patch, routes};
use minuserx::{DLL_FILE, EXPORTS_TXT};

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
    fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    fn GetLastError() -> u32;
    fn SetLastError(e: u32);
}

const BUILD: u32 = 22621;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn built() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let mut c: Vec<PathBuf> = exe.ancestors().map(|d| d.join(DLL_FILE)).collect();
    if let Some(b) = exe.ancestors().find(|d| d.file_name().map_or(false, |n| n == "build")) {
        for k in std::fs::read_dir(b).into_iter().flatten().flatten() {
            for h in std::fs::read_dir(k.path()).into_iter().flatten().flatten() {
                c.push(h.path().join("out").join(DLL_FILE));
            }
        }
    }
    c.into_iter()
        .filter_map(|p| std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(|t| (t, p)))
        .max()
        .map(|(_, p)| p)
        .unwrap_or_else(|| panic!("{DLL_FILE} not built near {}", exe.display()))
}

fn proc(h: usize, name: &str) -> usize {
    unsafe { GetProcAddress(h as *mut c_void, format!("{name}\0").as_ptr()) as usize }
}

fn ord(h: usize, n: u16) -> usize {
    unsafe { GetProcAddress(h as *mut c_void, n as usize as *const u8) as usize }
}

fn dll() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(|| {
        std::env::set_var("TOMBOX_PLM_WATCH", "0");
        let path = built().display().to_string();
        std::env::set_var("TOMBOX_MINUSERX", &path);
        let h = unsafe { LoadLibraryW(wide(&path).as_ptr()) } as usize;
        assert_ne!(h, 0, "LoadLibraryW({path})");
        os_host::ensure();
        h
    })
}

struct Row {
    ord: u16,
    name: String,
    status: String,
    target: Option<String>,
}

fn rows() -> Vec<Row> {
    EXPORTS_TXT
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (head, note) = l.split_once('|').unwrap();
            let f: Vec<&str> = head.split_whitespace().collect();
            let target = note.trim().strip_prefix("PE forwarder to user32.").map(|t| t.split(';').next().unwrap().trim().to_string());
            Row { ord: f[0].parse().unwrap(), name: f[1].to_string(), status: f[2].to_string(), target }
        })
        .collect()
}

fn user32() -> usize {
    let mut h = unsafe { GetModuleHandleW(wide("user32.dll").as_ptr()) } as usize;
    if h == 0 {
        h = unsafe { LoadLibraryW(wide("user32.dll").as_ptr()) } as usize;
    }
    assert_ne!(h, 0);
    h
}

#[test]
fn export_table_matches_the_console_ordinals() {
    let h = dll();
    let u = user32();
    for r in rows() {
        let by_ord = ord(h, r.ord);
        assert_ne!(by_ord, 0, "ordinal {} missing", r.ord);
        if !r.name.starts_with('#') {
            assert_eq!(proc(h, &r.name), by_ord, "{} not at ordinal {}", r.name, r.ord);
        }
        match r.status.as_str() {
            "forwarded" => {
                let t = r.target.as_deref().unwrap_or_else(|| panic!("{} has no forward target", r.name));
                assert_eq!(by_ord, proc(u, t), "{} does not resolve to user32!{t}", r.name);
            }
            "implemented" => {
                let own = proc(h, &format!("mux_{}", r.name.replace('#', "ord")));
                assert!(own == 0 || own == by_ord, "{}", r.name);
                assert!(by_ord >= h && by_ord < h + 0x100_0000, "{} not inside minuserx", r.name);
            }
            s => panic!("unexpected status {s}"),
        }
    }
    let f: unsafe extern "system" fn() -> usize = unsafe { core::mem::transmute(ord(h, 2534)) };
    unsafe { SetLastError(0) };
    assert_eq!(unsafe { f() }, 0);
    assert_eq!(unsafe { GetLastError() }, 0x8000_4001);
}

type Probe<'a> = &'a [(&'a str, &'a [&'a str])];

const WIN: &str = "ext-ms-win-rtcore-ntuser-window-l1-1-0.dll";
const WIN_A: &str = "ext-ms-win-rtcore-ntuser-window-ansi-l1-1-0.dll";
const MSG: &str = "ext-ms-win-rtcore-ntuser-message-l1-1-0.dll";
const CUR: &str = "ext-ms-win-rtcore-ntuser-cursor-l1-1-0.dll";

const PROBE: Probe = &[(WIN, &["DefWindowProcW", "ShowWindow", "DestroyWindow"]), (MSG, &["PeekMessageW", "TranslateMessage", "DispatchMessageW"]), (CUR, &["ShowCursor"])];
const FORTNITE: Probe = &[(WIN, &["CreateWindowExW", "DefWindowProcW", "DestroyWindow", "RegisterClassW", "ShowWindow"]), (MSG, &["DispatchMessageW", "PeekMessageW", "PostMessageW", "TranslateMessage"])];
const XDEFIANT: Probe = &[(MSG, &["DispatchMessageW", "PeekMessageW", "TranslateMessage"]), (WIN, &["DefWindowProcW", "DestroyWindow", "ShowWindow"]), (WIN_A, &["CreateWindowExA", "GetWindowLongPtrA", "RegisterClassExA", "SetWindowLongPtrA"])];
const TERRARIA: Probe = &[(MSG, &["DispatchMessageW", "PeekMessageW", "PostMessageW", "PostQuitMessage", "SendMessageW", "TranslateMessage"]), (CUR, &["LoadCursorW"]), (WIN, &["CreateWindowExW", "DefWindowProcW", "RegisterClassExW", "SetWindowLongPtrW", "ShowWindow"])];
const TRACKMANIA: Probe = &[(MSG, &["DispatchMessageW", "PeekMessageW", "PostQuitMessage", "SendMessageW", "SetTimer", "TranslateMessage"]), (WIN, &["DefWindowProcW", "DestroyWindow", "ShowWindow", "UnregisterClassW"]), (WIN_A, &["CreateWindowExA", "RegisterClassExA"]), (CUR, &["ShowCursor"])];

fn put(b: &mut [u8], at: usize, v: &[u8]) {
    b[at..at + v.len()].copy_from_slice(v);
}

fn title_image(probe: Probe) -> Vec<u8> {
    let mut b = vec![0u8; 0x800];
    put(&mut b, 0, b"MZ");
    put(&mut b, 0x3c, &0x40u32.to_le_bytes());
    put(&mut b, 0x40, b"PE\0\0");
    put(&mut b, 0x44, &0x8664u16.to_le_bytes());
    put(&mut b, 0x46, &1u16.to_le_bytes());
    put(&mut b, 0x54, &0xf0u16.to_le_bytes());
    put(&mut b, 0x56, &0x2022u16.to_le_bytes());
    let o = 0x58;
    put(&mut b, o, &0x20bu16.to_le_bytes());
    put(&mut b, o + 20, &0x1000u32.to_le_bytes());
    put(&mut b, o + 24, &0x1_8000_0000u64.to_le_bytes());
    put(&mut b, o + 32, &0x1000u32.to_le_bytes());
    put(&mut b, o + 36, &0x200u32.to_le_bytes());
    put(&mut b, o + 40, &6u16.to_le_bytes());
    put(&mut b, o + 48, &6u16.to_le_bytes());
    put(&mut b, o + 56, &0x2000u32.to_le_bytes());
    put(&mut b, o + 60, &0x200u32.to_le_bytes());
    put(&mut b, o + 68, &3u16.to_le_bytes());
    put(&mut b, o + 70, &0x0100u16.to_le_bytes());
    put(&mut b, o + 72, &0x10_0000u64.to_le_bytes());
    put(&mut b, o + 80, &0x1000u64.to_le_bytes());
    put(&mut b, o + 88, &0x10_0000u64.to_le_bytes());
    put(&mut b, o + 96, &0x1000u64.to_le_bytes());
    put(&mut b, o + 108, &16u32.to_le_bytes());
    let sh = o + 0xf0;
    put(&mut b, sh, b".idata\0\0");
    put(&mut b, sh + 8, &0x1000u32.to_le_bytes());
    put(&mut b, sh + 12, &0x1000u32.to_le_bytes());
    put(&mut b, sh + 16, &0x600u32.to_le_bytes());
    put(&mut b, sh + 20, &0x200u32.to_le_bytes());
    put(&mut b, sh + 36, &0xc000_0040u32.to_le_bytes());
    let rva = 0x1000usize;
    let raw = |r: usize| r - rva + 0x200;
    let desc_len = (probe.len() + 1) * 20;
    let mut cur = (rva + desc_len + 7) & !7;
    let mut names = Vec::new();
    for (dll, _) in probe.iter() {
        names.push(cur);
        put(&mut b, raw(cur), dll.as_bytes());
        cur += (dll.len() + 8) & !7;
    }
    let mut ints = Vec::new();
    for (_, fs) in probe.iter() {
        ints.push(cur);
        cur += (fs.len() + 1) * 8;
    }
    let iat_start = cur;
    let mut iats = Vec::new();
    for (_, fs) in probe.iter() {
        iats.push(cur);
        cur += (fs.len() + 1) * 8;
    }
    let iat_end = cur;
    for (i, (_, fs)) in probe.iter().enumerate() {
        put(&mut b, raw(rva) + i * 20, &(ints[i] as u32).to_le_bytes());
        put(&mut b, raw(rva) + i * 20 + 12, &(names[i] as u32).to_le_bytes());
        put(&mut b, raw(rva) + i * 20 + 16, &(iats[i] as u32).to_le_bytes());
        for (j, n) in fs.iter().enumerate() {
            put(&mut b, raw(ints[i]) + j * 8, &(cur as u64).to_le_bytes());
            put(&mut b, raw(iats[i]) + j * 8, &(cur as u64).to_le_bytes());
            put(&mut b, raw(cur) + 2, n.as_bytes());
            cur += (2 + n.len() + 2) & !1;
        }
    }
    assert!(raw(cur) <= b.len());
    put(&mut b, o + 112 + 8, &(rva as u32).to_le_bytes());
    put(&mut b, o + 112 + 12, &(desc_len as u32).to_le_bytes());
    put(&mut b, o + 112 + 12 * 8, &(iat_start as u32).to_le_bytes());
    put(&mut b, o + 112 + 12 * 8 + 4, &((iat_end - iat_start) as u32).to_le_bytes());
    b
}

fn status_of(name: &str) -> String {
    rows().into_iter().find(|r| r.name == name).map(|r| r.status).unwrap_or_else(|| panic!("{name} not in exports.txt"))
}

fn check_title(tag: &str, probe: Probe, build: u32) {
    let h = dll();
    let u = user32();
    let table = os_host::hook_table();
    for (dll, fs) in probe.iter() {
        for f in fs.iter() {
            let r = routes::lookup(dll, f, build).unwrap_or_else(|| panic!("{tag}: no route for {dll}!{f} at {build}"));
            assert_eq!((r.console, r.lib), ("minuserx.dll", "minuserx"), "{tag} {f}");
            assert_eq!(r.status, status_of(f), "{tag} {f}");
            let t = unsafe { routes::target(r) };
            assert_eq!(t, proc(h, f), "{tag} {f} route target is not the minuserx export");
            if r.status == "forwarded" {
                let fwd = rows().into_iter().find(|x| x.name == *f).and_then(|x| x.target).unwrap();
                assert_eq!(t, proc(u, &fwd), "{tag} {f} forwarder does not land in user32");
            } else {
                assert!(t >= h && t < h + 0x100_0000, "{tag} {f} not inside minuserx");
            }
        }
    }
    assert_eq!(routes::module("minuserx"), h);
    let dir = std::env::temp_dir().join(format!("mux_loader_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{tag}_probe.dll"));
    std::fs::write(&path, title_image(probe)).unwrap();
    let t = unsafe { LoadLibraryW(wide(&path.display().to_string()).as_ptr()) } as usize;
    assert_ne!(t, 0, "{tag}: loader rejected the probe title image: {}", unsafe { GetLastError() });
    let imports = unsafe { routes::imports(t as *mut u8) };
    assert_eq!(imports.len(), probe.iter().map(|(_, f)| f.len()).sum::<usize>());
    let mut hits = Vec::new();
    unsafe { routes::patch_image_for(t as *mut u8, build, table, &mut hits) };
    for imp in &imports {
        let slot = unsafe { core::ptr::read_volatile(imp.slot) };
        match table.iter().find(|x| x.name == imp.func && patch::dll_matches(&imp.dll, x.dlls)) {
            Some(x) => assert_eq!(slot, x.hook, "{tag} {} lost its os_host hook", imp.func),
            None => assert_eq!(slot, proc(h, &imp.func), "{tag} {} not bound to minuserx after routing: {hits:?}", imp.func),
        }
        assert_ne!(slot, 0);
    }
    let mut again = Vec::new();
    assert_eq!(unsafe { patch::patch_image(t as *mut u8, table, &mut again) }, 0, "{tag}: hook pass left {again:?} unpatched");
    assert_eq!(unsafe { routes::patch_image_for(t as *mut u8, build, table, &mut again) }, 0, "{tag}: routing not idempotent: {again:?}");
}

#[test]
fn title_bound_to_minuserx_keeps_os_host_hooks_and_routes_every_row() {
    check_title("probe", PROBE, BUILD);
}

#[test]
fn shipped_title_import_patterns_resolve() {
    check_title("fortnite", FORTNITE, 19041);
    check_title("xdefiant", XDEFIANT, 22000);
    check_title("terraria", TERRARIA, 22000);
    check_title("trackmania", TRACKMANIA, 22621);
}
