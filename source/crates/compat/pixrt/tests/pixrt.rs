use core::ffi::c_void;
use std::path::PathBuf;

use pixrt::{variant, CLASSIC, DLL_FILE, EXPORTS_TXT};

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
}

const MAGIC: u32 = 0x4a61_0f6c;
const VERSION: u32 = 0x31064;
const REFS: [&str; 4] = ["trackmania", "xdefiant", "terraria", "extracted"];

type InitFn = unsafe extern "system" fn(u32, u32, u32, *mut c_void, u32) -> i32;
type AllocFn = unsafe extern "system" fn(usize) -> *mut c_void;
type FreeFn = unsafe extern "system" fn(*mut c_void);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn newest(file: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let mut c: Vec<PathBuf> = exe.ancestors().map(|d| d.join(file)).collect();
    if let Some(b) = exe.ancestors().find(|d| d.file_name().map_or(false, |n| n == "build")) {
        for k in std::fs::read_dir(b).into_iter().flatten().flatten() {
            for h in std::fs::read_dir(k.path()).into_iter().flatten().flatten() {
                c.push(h.path().join("out").join(file));
            }
        }
    }
    c.into_iter()
        .filter_map(|p| std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(|t| (t, p)))
        .max()
        .map(|(_, p)| p)
}

fn load(file: &str) -> *mut c_void {
    let p = newest(file).unwrap_or_else(|| panic!("{file} not built (cargo build -p {})", file.trim_end_matches(".dll")));
    let h = unsafe { LoadLibraryW(wide(&p.display().to_string()).as_ptr()) };
    assert!(!h.is_null(), "LoadLibraryW({}) failed: {}", p.display(), unsafe { GetLastError() });
    h
}

fn module() -> *mut c_void {
    load("xmem.dll");
    load(DLL_FILE)
}

fn proc(h: *mut c_void, name: &str) -> *mut c_void {
    unsafe { GetProcAddress(h, format!("{name}\0").as_ptr()) }
}

fn console_dll(t: &str) -> PathBuf {
    PathBuf::from(format!(r"D:\TomBox\input\{t}\EmbeddedXvd\Windows\System32\{DLL_FILE}"))
}

fn implemented() -> Vec<(u16, String)> {
    let mut v: Vec<(u16, String)> = xone_exports::parse(EXPORTS_TXT).into_iter().filter(|e| e.status == "implemented").map(|e| (e.ordinal, e.name)).collect();
    v.sort();
    v
}

#[test]
fn table_matches_console_dll_and_built_dll() {
    let ex = xone_exports::parse(EXPORTS_TXT);
    assert_eq!(ex.len(), 3);
    for e in &ex {
        assert!(["implemented", "missing"].contains(&e.status.as_str()), "{}", e.name);
        assert!(!e.note.is_empty());
    }
    let want = xone_exports::table(&ex);
    let mut seen = 0;
    for t in REFS {
        let p = console_dll(t);
        if let Ok(b) = std::fs::read(&p) {
            assert_eq!(xone_exports::pe_exports(&b).unwrap(), want, "{}", p.display());
            seen += 1;
        }
    }
    assert!(seen > 0);
    let built = std::fs::read(newest(DLL_FILE).unwrap()).unwrap();
    let mut got = xone_exports::pe_exports(&built).unwrap();
    got.sort();
    assert_eq!(got, implemented());
}

#[test]
fn variant_follows_console_build() {
    let a = variant(true, 22621);
    assert!(!a.assume_overflow_without_device && !a.hypervisor_check && !a.dbgbreak && a.name_heap);
    assert_eq!((a.stats_flags, a.stats_bytes), (0x2000_8002, 0x78));
    let b = variant(true, 19041);
    assert!(!b.name_heap);
    assert_eq!((b.stats_flags, b.stats_bytes), (0x1000_8002, 0x50));
    let c = variant(false, 22000);
    assert!(c.assume_overflow_without_device && c.hypervisor_check && c.dbgbreak && c.name_heap);
    let d = variant(false, 19041);
    assert!(d.assume_overflow_without_device && !d.hypervisor_check && !d.dbgbreak && !d.name_heap);
}

#[test]
fn tooling_state_matches_console_without_tools_memory() {
    load("xmem.dll");
    let v = variant(CLASSIC, 22621);
    let s = pixrt::build_state(&v);
    assert_eq!(s.heap, 0, "the emulator's xmem refuses the tools partition like a console without tooling memory");
    let mut want = Vec::new();
    if !CLASSIC {
        want.extend([pixrt::MSG_QUERY_FAILED, pixrt::MSG_ASSUME_OVERFLOW]);
    }
    if s.tools_bytes == 0 {
        want.push(pixrt::MSG_UNAVAILABLE);
    } else if s.tools_bytes < pixrt::LOW_TOOLS_MEMORY {
        want.push(pixrt::MSG_LOW);
    }
    if !s.overflow {
        want.push(pixrt::MSG_NO_OVERFLOW);
        if s.tools_bytes < pixrt::LOW_TOOLS_MEMORY {
            want.push(pixrt::MSG_SOME_FEATURES);
        }
    }
    want.push(pixrt::MSG_NO_HEAP);
    assert_eq!(s.messages, want);
    assert_eq!(s.overflow, !CLASSIC);
    assert_eq!(pixrt::guard_fails(&s), CLASSIC);
}

#[test]
fn tool_memory_matches_console() {
    let h = module();
    let f: InitFn = unsafe { core::mem::transmute(proc(h, "PixDllInitializeToolMemory")) };
    let mut t = [0x5a5a_5a5a_5a5a_5a5au64; 3];
    let hr = unsafe { f(MAGIC, VERSION, 0, t.as_mut_ptr() as *mut c_void, 16) };
    if CLASSIC {
        assert_eq!(hr, pixrt::E_OUTOFMEMORY);
        assert_eq!(t, [0x5a5a_5a5a_5a5a_5a5a; 3]);
        return;
    }
    assert_eq!(hr, 0);
    assert_eq!(t[2], 0x5a5a_5a5a_5a5a_5a5a);
    let base = h as u64;
    assert!(t[0] > base && t[1] > base && t[0] != t[1]);
    let alloc: AllocFn = unsafe { core::mem::transmute(t[0] as usize) };
    let free: FreeFn = unsafe { core::mem::transmute(t[1] as usize) };
    let p = unsafe { alloc(24) } as *mut u64;
    assert!(!p.is_null());
    assert_eq!(unsafe { *p.sub(1) }, 3, "no tools heap: block comes from the CRT fallback, tag 3");
    unsafe {
        core::ptr::write_bytes(p as *mut u8, 0xab, 24);
        free(p as *mut c_void);
        free(core::ptr::null_mut());
    }
    assert!(unsafe { alloc(0) }.is_null());
    let mut z = [7u64; 2];
    assert_eq!(unsafe { f(0, 0, 1, z.as_mut_ptr() as *mut c_void, 16) }, 0);
    assert_eq!(z, [0, 0]);
    let mut one = [7u64; 2];
    assert_eq!(unsafe { f(0, 0, 0, one.as_mut_ptr() as *mut c_void, 8) }, 0);
    assert_eq!((one[0], one[1]), (t[0], 7));
}

#[test]
fn from_title_matches_console() {
    let h = module();
    let p = proc(h, "PixDllInitializeFromTitle");
    if !CLASSIC {
        assert!(p.is_null());
        return;
    }
    let f: InitFn = unsafe { core::mem::transmute(p) };
    let mut t = [0x11u64; 15];
    assert_eq!(unsafe { f(MAGIC, VERSION, 0, t.as_mut_ptr() as *mut c_void, 0x78) }, pixrt::E_OUTOFMEMORY);
    assert_eq!(t, [0x11; 15]);
}

#[test]
fn initialize_matches_console() {
    let h = module();
    let p = proc(h, "PixDllInitialize");
    if !CLASSIC {
        assert!(p.is_null());
        return;
    }
    type Full = unsafe extern "system" fn(u32, u32, u32, u32, u32, u32, *mut c_void, u32) -> i32;
    let f: Full = unsafe { core::mem::transmute(p) };
    let mut t = [0x22u64; 4];
    assert_eq!(unsafe { f(MAGIC, VERSION, 0, 0, 0, 0, t.as_mut_ptr() as *mut c_void, 32) }, pixrt::E_OUTOFMEMORY);
    assert_eq!(t, [0x22; 4]);
}
