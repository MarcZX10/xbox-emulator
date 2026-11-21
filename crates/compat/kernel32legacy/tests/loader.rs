use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use compat_common::os_host::{self, affinity, dlldir, k32legacy, patch};
use kernel32legacy::DLL_FILE;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
    fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    fn GetCurrentThread() -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetLastError() -> u32;
}

type NumaFn = unsafe extern "system" fn(u8, *mut u64) -> i32;
type NameFn = unsafe extern "system" fn(*mut u16, *mut u32) -> i32;
type SetTam = unsafe extern "system" fn(*mut c_void, usize) -> usize;
type GetPam = unsafe extern "system" fn(*mut c_void, *mut usize, *mut usize) -> i32;
type SetPam = unsafe extern "system" fn(*mut c_void, usize) -> i32;

static SERIAL: Mutex<()> = Mutex::new(());

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

fn dll() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(|| {
        std::env::set_var("TOMBOX_PLM_WATCH", "0");
        std::env::set_var(k32legacy::BUILD_VAR, "22621");
        let path = built().display().to_string();
        std::env::set_var(k32legacy::DLL_VAR, &path);
        let h = unsafe { LoadLibraryW(wide(&path).as_ptr()) } as usize;
        assert_ne!(h, 0, "LoadLibraryW({path})");
        os_host::ensure();
        assert!(os_host::is_owner(), "test exe copy of compat-common must own the in-title hooks");
        h
    })
}

#[test]
fn os_build_picks_the_ordinal_table() {
    assert_eq!(k32legacy::dll_name(19041), "kernel32legacy_19041.dll");
    assert_eq!(k32legacy::dll_name(21999), "kernel32legacy_19041.dll");
    assert_eq!(k32legacy::dll_name(22000), "kernel32legacy.dll");
    assert_eq!(k32legacy::dll_name(22621), "kernel32legacy.dll");
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    dll();
    assert_eq!(k32legacy::console_os_build(), 22621);
    let dir = PathBuf::from(r"C:\title");
    let c = k32legacy::candidates(19041, Some(dir.clone()));
    assert_eq!(c.last().unwrap(), &dir.join("kernel32legacy_19041.dll").display().to_string());
    assert_eq!(c.first().unwrap(), &built().display().to_string());
}

#[test]
fn shared_table_round_trips() {
    let s = k32legacy::local();
    assert_eq!(k32legacy::decode(&k32legacy::encode(s)), Some(s));
    assert_eq!(k32legacy::decode("1,2,3"), None);
    assert_eq!(k32legacy::decode("1,2,0,4"), None);
    assert_eq!(k32legacy::decode("1,zz,3,4"), None);
}

const PROBE_DLL: &str = "api-ms-win-core-kernel32-legacy-l1-1-0.dll";
const PROBE_IMPORTS: [&str; 4] = ["GetNumaNodeProcessorMask", "GetComputerNameW", "SetDllDirectoryW", "SetThreadAffinityMask"];

fn put(b: &mut [u8], at: usize, v: &[u8]) {
    b[at..at + v.len()].copy_from_slice(v);
}

fn title_image() -> Vec<u8> {
    let mut b = vec![0u8; 0x400];
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
    put(&mut b, sh + 16, &0x200u32.to_le_bytes());
    put(&mut b, sh + 20, &0x200u32.to_le_bytes());
    put(&mut b, sh + 36, &0xc000_0040u32.to_le_bytes());
    let rva = 0x1000usize;
    let raw = |r: usize| r - rva + 0x200;
    let name_rva = rva + 40;
    let int_rva = name_rva + 48;
    let iat_rva = int_rva + (PROBE_IMPORTS.len() + 1) * 8;
    let mut hn = iat_rva + (PROBE_IMPORTS.len() + 1) * 8;
    put(&mut b, raw(rva), &(int_rva as u32).to_le_bytes());
    put(&mut b, raw(rva) + 12, &(name_rva as u32).to_le_bytes());
    put(&mut b, raw(rva) + 16, &(iat_rva as u32).to_le_bytes());
    put(&mut b, raw(name_rva), PROBE_DLL.as_bytes());
    for (i, n) in PROBE_IMPORTS.iter().enumerate() {
        put(&mut b, raw(int_rva) + i * 8, &(hn as u64).to_le_bytes());
        put(&mut b, raw(iat_rva) + i * 8, &(hn as u64).to_le_bytes());
        put(&mut b, raw(hn) + 2, n.as_bytes());
        hn += (2 + n.len() + 2) & !1;
    }
    assert!(raw(hn) <= b.len());
    put(&mut b, o + 112 + 8, &(rva as u32).to_le_bytes());
    put(&mut b, o + 112 + 12, &40u32.to_le_bytes());
    put(&mut b, o + 112 + 12 * 8, &(iat_rva as u32).to_le_bytes());
    put(&mut b, o + 112 + 12 * 8 + 4, &(((PROBE_IMPORTS.len() + 1) * 8) as u32).to_le_bytes());
    b
}

#[test]
fn resolver_redirects_kernel32_legacy_imports_to_the_compat_dll() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let h = dll();
    assert_eq!(k32legacy::module(), h);
    assert!(os_host::load_watch_active());
    let table = os_host::hook_table();
    let ours: Vec<_> = table.iter().filter(|x| x.dlls == k32legacy::CONSOLE_DLLS).collect();
    let names = unsafe { k32legacy::export_names(h) };
    assert_eq!(names.len(), 303);
    assert_eq!(ours.len(), names.len() - 1);
    assert!(!ours.iter().any(|x| x.name == "SetDllDirectoryW"));
    for x in &ours {
        assert_eq!(x.hook, proc(h, x.name), "{}", x.name);
    }
    let dir = std::env::temp_dir().join(format!("k32l_loader_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("title_probe.dll");
    std::fs::write(&path, title_image()).unwrap();
    let t = unsafe { LoadLibraryW(wide(&path.display().to_string()).as_ptr()) } as usize;
    assert_ne!(t, 0, "loader rejected the probe title image: {}", unsafe { GetLastError() });
    let k32 = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) } as usize;
    let slot = |n: &str| unsafe { k32legacy::import_slot(t, k32legacy::API_SET_PREFIX, n) }.unwrap_or_else(|| panic!("probe has no {n} import"));
    let numa = proc(h, "GetNumaNodeProcessorMask");
    let sta = proc(h, "SetThreadAffinityMask");
    assert_ne!(numa, proc(k32, "GetNumaNodeProcessorMask"));
    assert_ne!(sta, proc(k32, "SetThreadAffinityMask"));
    assert_eq!(slot("GetNumaNodeProcessorMask"), numa);
    assert_eq!(slot("SetThreadAffinityMask"), sta);
    assert_eq!(slot("GetComputerNameW"), proc(h, "GetComputerNameW"));
    assert_eq!(slot("SetDllDirectoryW"), dlldir::set_dll_directory_w as *const () as usize);
    let f: NumaFn = unsafe { core::mem::transmute(slot("GetNumaNodeProcessorMask")) };
    let (mut m, mut want) = (0u64, 0u64);
    assert_eq!(unsafe { f(0, &mut m) }, 1);
    assert_eq!(unsafe { kernel32legacy::k32l_GetNumaNodeProcessorMask(0, &mut want) }, 1);
    assert_eq!(m, want);
    let g: NameFn = unsafe { core::mem::transmute(slot("GetComputerNameW")) };
    let mut buf = [0u16; 64];
    let mut len = 64u32;
    assert_ne!(unsafe { g(buf.as_mut_ptr(), &mut len) }, 0);
    assert!(len > 0);
    let mut hits = Vec::new();
    assert_eq!(unsafe { patch::patch_image(t as *mut u8, table, &mut hits) }, 0, "late-load watch left {hits:?} unpatched");
}

#[test]
fn hooked_rows_share_state_with_the_in_title_hooks() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let h = dll();
    assert_eq!(std::env::var(k32legacy::SHARED_VAR).ok().and_then(|v| k32legacy::decode(&v)), Some(k32legacy::local()));
    let set_t: SetTam = unsafe { core::mem::transmute(proc(h, "SetThreadAffinityMask")) };
    let get_p: GetPam = unsafe { core::mem::transmute(proc(h, "GetProcessAffinityMask")) };
    let set_p: SetPam = unsafe { core::mem::transmute(proc(h, "SetProcessAffinityMask")) };
    let t = affinity::topology();
    let full = t.full as usize;
    let cur = unsafe { GetCurrentThread() };
    assert_ne!(unsafe { set_t(cur, 1) }, 0);
    assert_eq!(unsafe { affinity::set_thread_affinity_mask(cur, full) }, 1);
    assert_ne!(unsafe { affinity::set_thread_affinity_mask(cur, 1) }, 0);
    assert_eq!(unsafe { set_t(cur, full) }, 1);
    let orig = affinity::process_console_mask();
    let proc_h = unsafe { GetCurrentProcess() };
    assert_eq!(unsafe { set_p(proc_h, 1) }, 1);
    assert_eq!(affinity::process_console_mask(), 1);
    let (mut p, mut s) = (0usize, 0usize);
    assert_eq!(unsafe { affinity::get_process_affinity_mask(proc_h, &mut p, &mut s) }, 1);
    assert_eq!((p, s as u64), (1, t.full));
    assert_eq!(unsafe { affinity::set_process_affinity_mask(proc_h, orig as usize) }, 1);
    let (mut p, mut s) = (0usize, 0usize);
    assert_eq!(unsafe { get_p(proc_h, &mut p, &mut s) }, 1);
    assert_eq!(p as u64, orig);
}
