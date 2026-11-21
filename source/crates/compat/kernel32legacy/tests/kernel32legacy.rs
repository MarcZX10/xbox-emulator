use core::ffi::c_void;
use std::path::PathBuf;

use compat_common::os_host::{affinity, dlldir};
use kernel32legacy::{forwards, numa_mask_for_group, GroupAffinity, ProcessorNumber, CONSOLE_REFS, DLL_FILE, EXPORTS_TXT};

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn SetLastError(e: u32);
    fn GetCurrentThread() -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetDllDirectoryW(n: u32, buf: *mut u16) -> u32;
    fn GetNumaNodeProcessorMaskEx(node: u16, a: *mut GroupAffinity) -> i32;
    fn GetCurrentProcessId() -> u32;
    fn CloseHandle(h: *mut c_void) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetCurrentProcessorNumberEx(n: *mut ProcessorNumber);
    fn RtlCreateQueryDebugBuffer(max_commit: u32, use_event_pair: u8) -> *mut c_void;
    fn RtlQueryProcessDebugInformation(pid: *mut c_void, flags: u32, buffer: *mut c_void) -> i32;
    fn RtlDestroyQueryDebugBuffer(buffer: *mut c_void) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

const STATUSES: [&str; 3] = ["forwarded", "implemented", "hooked"];

fn console_refs() -> Vec<PathBuf> {
    CONSOLE_REFS
        .split(',')
        .map(|t| PathBuf::from(format!(r"D:\TomBox\input\{t}\EmbeddedXvd\Windows\System32\kernel32legacy.dll")))
        .filter(|p| p.is_file())
        .collect()
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

fn module() -> *mut c_void {
    let h = unsafe { LoadLibraryW(wide(&built().display().to_string()).as_ptr()) };
    assert!(!h.is_null(), "LoadLibraryW({DLL_FILE}) failed: {}", unsafe { GetLastError() });
    h
}

fn proc(h: *mut c_void, name: &str) -> *mut c_void {
    unsafe { GetProcAddress(h, format!("{name}\0").as_ptr()) }
}

#[test]
fn table_matches_console_dll() {
    let ex = xone_exports::parse(EXPORTS_TXT);
    assert_eq!(ex.len(), if DLL_FILE == "kernel32legacy.dll" { 303 } else { 299 });
    for e in &ex {
        assert!(STATUSES.contains(&e.status.as_str()), "{}: {}", e.name, e.status);
        assert!(!e.note.is_empty(), "{} has no note", e.name);
    }
    let want = xone_exports::table(&ex);
    let refs = console_refs();
    assert!(!refs.is_empty(), "no console kernel32legacy.dll found");
    for r in refs {
        let got = xone_exports::pe_exports(&std::fs::read(&r).unwrap()).unwrap();
        assert_eq!(got, want, "{}", r.display());
    }
}

#[test]
fn forwards_cover_every_forwarded_row() {
    let ex = xone_exports::parse(EXPORTS_TXT);
    let fw = forwards();
    let fwd_rows: Vec<&str> = ex.iter().filter(|e| e.status == "forwarded").map(|e| e.name.as_str()).collect();
    let mut a: Vec<&str> = fw.iter().map(|f| f.0.as_str()).collect();
    let mut b = fwd_rows.clone();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    for (_, dll, _) in &fw {
        assert!(["kernel32.dll", "kernelbase.dll", "ntdll.dll"].contains(&dll.as_str()), "{dll}");
    }
}

#[test]
fn built_dll_export_table() {
    let got = xone_exports::pe_exports(&std::fs::read(built()).unwrap()).unwrap();
    assert_eq!(got, xone_exports::table(&xone_exports::parse(EXPORTS_TXT)));
}

#[test]
fn every_forwarder_resolves_to_its_host_export() {
    let h = module();
    let fw = forwards();
    assert_eq!(fw.len(), xone_exports::parse(EXPORTS_TXT).iter().filter(|e| e.status == "forwarded").count());
    assert_eq!(fw.len(), if DLL_FILE == "kernel32legacy.dll" { 297 } else { 293 });
    for (name, dll, target) in fw {
        let host = unsafe { LoadLibraryW(wide(&dll).as_ptr()) };
        assert!(!host.is_null(), "{dll}");
        let want = proc(host, &target);
        assert!(!want.is_null(), "host {dll} has no {target}");
        assert_eq!(proc(h, &name), want, "{name} -> {dll}!{target}");
    }
}

#[test]
fn own_exports_live_in_the_compat_dll() {
    let h = module();
    let k32 = unsafe { LoadLibraryW(wide("kernel32.dll").as_ptr()) };
    for e in xone_exports::parse(EXPORTS_TXT).iter().filter(|e| e.status != "forwarded") {
        let p = proc(h, &e.name);
        assert!(!p.is_null(), "{}", e.name);
        assert_ne!(p, proc(k32, &e.name), "{} resolved to host kernel32", e.name);
        let base = h as usize;
        assert!((p as usize) > base && (p as usize) < base + 0x0100_0000, "{} outside the compat image", e.name);
        let mut code = p as *const u8;
        unsafe {
            if *code == 0xe9 {
                let rel = i32::from_le_bytes(core::slice::from_raw_parts(code.add(1), 4).try_into().unwrap());
                code = code.offset(5 + rel as isize);
            }
            assert!(!(*code == 0xff && *code.add(1) == 0x25), "{} is an import thunk, not the compat implementation", e.name);
        }
    }
}

#[test]
fn numa_mask_group_rule() {
    let node = GroupAffinity { mask: 0xff, group: 0, reserved: [0; 3] };
    assert_eq!(numa_mask_for_group(node, ProcessorNumber { group: 0, number: 3, reserved: 0 }), 0xff);
    assert_eq!(numa_mask_for_group(node, ProcessorNumber { group: 1, number: 0, reserved: 0 }), 0);
    let node1 = GroupAffinity { mask: 0x0f, group: 1, reserved: [0; 3] };
    assert_eq!(numa_mask_for_group(node1, ProcessorNumber { group: 1, number: 2, reserved: 0 }), 0x0f);
}

type NumaFn = unsafe extern "system" fn(u8, *mut u64) -> i32;

#[test]
fn numa_node_mask_matches_console_algorithm() {
    let f: NumaFn = unsafe { core::mem::transmute(proc(module(), "GetNumaNodeProcessorMask")) };
    for g in [kernel32legacy::k32l_GetNumaNodeProcessorMask as NumaFn, f] {
        let mut aff = GroupAffinity::default();
        assert_ne!(unsafe { GetNumaNodeProcessorMaskEx(0, &mut aff) }, 0);
        let mut cur = ProcessorNumber::default();
        unsafe { RtlGetCurrentProcessorNumberEx(&mut cur) };
        let mut m = 0u64;
        assert_eq!(unsafe { g(0, &mut m) }, 1);
        assert_eq!(m, affinity::topology().to_console(numa_mask_for_group(aff, cur)));
        assert_ne!(m, 0);
        let set: SetTam = unsafe { core::mem::transmute(proc(module(), "SetThreadAffinityMask")) };
        assert_ne!(unsafe { set(GetCurrentThread(), m as usize) }, 0, "node mask {m:#x} rejected by SetThreadAffinityMask");

        let mut probe = GroupAffinity::default();
        unsafe { SetLastError(0) };
        assert_eq!(unsafe { GetNumaNodeProcessorMaskEx(0xfe, &mut probe) }, 0);
        let want_err = unsafe { GetLastError() };
        let mut sentinel = 0x1234_5678_9abc_def0u64;
        unsafe { SetLastError(0) };
        assert_eq!(unsafe { g(0xfe, &mut sentinel) }, 0);
        assert_eq!(unsafe { GetLastError() }, want_err);
        assert_eq!(sentinel, 0x1234_5678_9abc_def0);
    }
}

type GetPam = unsafe extern "system" fn(*mut c_void, *mut usize, *mut usize) -> i32;
type SetTam = unsafe extern "system" fn(*mut c_void, usize) -> usize;

#[test]
fn affinity_rows_use_os_host_emulation() {
    let h = module();
    let get: GetPam = unsafe { core::mem::transmute(proc(h, "GetProcessAffinityMask")) };
    let set: SetTam = unsafe { core::mem::transmute(proc(h, "SetThreadAffinityMask")) };
    let (mut p, mut s) = (0usize, 0usize);
    assert_eq!(unsafe { get(GetCurrentProcess(), &mut p, &mut s) }, 1);
    let t = affinity::topology();
    assert_eq!(s as u64, t.full);
    assert_eq!(p as u64, affinity::process_console_mask());
    if t.full != u64::MAX {
        let outside = (t.full + 1) as usize;
        unsafe { SetLastError(0) };
        assert_eq!(unsafe { set(GetCurrentThread(), outside) }, 0);
        assert_eq!(unsafe { GetLastError() }, 87);
    }
    let prev = unsafe { set(GetCurrentThread(), 1) };
    assert_ne!(prev, 0);
    assert_eq!(unsafe { set(GetCurrentThread(), t.full as usize) }, 1);
}

type SetPam = unsafe extern "system" fn(*mut c_void, usize) -> i32;

#[test]
fn set_process_affinity_uses_os_host_emulation() {
    let h = module();
    let k32 = unsafe { LoadLibraryW(wide("kernel32.dll").as_ptr()) };
    let host_get: GetPam = unsafe { core::mem::transmute(proc(k32, "GetProcessAffinityMask")) };
    let host_set: SetPam = unsafe { core::mem::transmute(proc(k32, "SetProcessAffinityMask")) };
    let (mut orig, mut sys) = (0usize, 0usize);
    assert_ne!(unsafe { host_get(GetCurrentProcess(), &mut orig, &mut sys) }, 0);
    let set: SetPam = unsafe { core::mem::transmute(proc(h, "SetProcessAffinityMask")) };
    let get: GetPam = unsafe { core::mem::transmute(proc(h, "GetProcessAffinityMask")) };
    let t = affinity::topology();
    if t.full != u64::MAX {
        unsafe { SetLastError(0) };
        assert_eq!(unsafe { set(GetCurrentProcess(), (t.full + 1) as usize) }, 0);
        assert_eq!(unsafe { GetLastError() }, 87);
    }
    assert_eq!(unsafe { set(GetCurrentProcess(), t.full as usize) }, 1);
    let (mut p, mut s) = (0usize, 0usize);
    assert_eq!(unsafe { get(GetCurrentProcess(), &mut p, &mut s) }, 1);
    assert_eq!(p as u64, t.full);
    assert_eq!(s as u64, t.full);
    assert_ne!(unsafe { host_set(GetCurrentProcess(), orig) }, 0);
}

#[repr(C)]
struct ProcessEntry32W {
    size: u32,
    usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    pri: i32,
    flags: u32,
    exe: [u16; 260],
}

#[repr(C)]
struct ModuleEntry32W {
    size: u32,
    module_id: u32,
    pid: u32,
    glbl: u32,
    proc_usage: u32,
    base: *mut u8,
    base_size: u32,
    hmodule: *mut c_void,
    name: [u16; 256],
    path: [u16; 260],
}

type Snap = unsafe extern "system" fn(u32, u32) -> *mut c_void;
type PWalk = unsafe extern "system" fn(*mut c_void, *mut ProcessEntry32W) -> i32;
type MWalk = unsafe extern "system" fn(*mut c_void, *mut ModuleEntry32W) -> i32;

#[test]
fn snapshot_module_query_flags() {
    assert_eq!(kernel32legacy::module_query_flags(0x8), 0x8000_0001);
    assert_eq!(kernel32legacy::module_query_flags(0x10), 0x8000_0040);
    assert_eq!(kernel32legacy::module_query_flags(0x18), 0x8000_0041);
    assert_eq!(kernel32legacy::module_query_flags(0x2), 0x8000_0000);
    assert_eq!(core::mem::size_of::<ProcessEntry32W>(), 0x238);
    assert_eq!(core::mem::size_of::<ModuleEntry32W>(), 0x438);
}

#[test]
fn snapshot_of_own_process_walks_with_forwarded_walkers() {
    let h = module();
    let snap: Snap = unsafe { core::mem::transmute(proc(h, "CreateToolhelp32Snapshot")) };
    let pfirst: PWalk = unsafe { core::mem::transmute(proc(h, "Process32FirstW")) };
    let pnext: PWalk = unsafe { core::mem::transmute(proc(h, "Process32NextW")) };
    let mfirst: MWalk = unsafe { core::mem::transmute(proc(h, "Module32FirstW")) };
    let me = unsafe { GetCurrentProcessId() };
    let s = unsafe { snap(0x2 | 0x4 | 0x8, 0) };
    assert_ne!(s as isize, -1, "snapshot failed: {}", unsafe { GetLastError() });
    let mut pe: ProcessEntry32W = unsafe { core::mem::zeroed() };
    pe.size = 0x238;
    let mut found = false;
    let mut ok = unsafe { pfirst(s, &mut pe) };
    while ok != 0 {
        if pe.pid == me {
            found = true;
        }
        ok = unsafe { pnext(s, &mut pe) };
    }
    assert!(found);
    let mut m: ModuleEntry32W = unsafe { core::mem::zeroed() };
    m.size = 0x438;
    assert_eq!(unsafe { mfirst(s, &mut m) }, 1);
    assert_eq!(m.pid, me);
    let path = String::from_utf16_lossy(&m.path[..m.path.iter().position(|&c| c == 0).unwrap()]).to_lowercase();
    assert_eq!(path, std::env::current_exe().unwrap().display().to_string().to_lowercase());
    unsafe { CloseHandle(s) };
}

#[test]
fn snapshot_fails_like_console_when_the_debug_query_fails() {
    let snap: Snap = unsafe { core::mem::transmute(proc(module(), "CreateToolhelp32Snapshot")) };
    let bogus = 0xffff_fff0u32;
    let buf = unsafe { RtlCreateQueryDebugBuffer(0, 0) };
    assert!(!buf.is_null());
    let st = unsafe { RtlQueryProcessDebugInformation(bogus as i32 as isize as *mut c_void, kernel32legacy::module_query_flags(0x8), buf) };
    unsafe { RtlDestroyQueryDebugBuffer(buf) };
    assert!(st < 0, "query for a dead pid succeeded");
    unsafe { SetLastError(0) };
    assert_eq!(unsafe { snap(0x8, bogus) } as isize, -1);
    assert_eq!(unsafe { GetLastError() }, unsafe { RtlNtStatusToDosError(st) });
    assert_eq!(unsafe { kernel32legacy::console_snapshot_status(0x8, bogus) }, st);
    assert_eq!(unsafe { kernel32legacy::console_snapshot_status(0x2, bogus) }, 0);
}

type SetDir = unsafe extern "system" fn(*const u16) -> i32;

fn dll_dir() -> String {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetDllDirectoryW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

#[test]
fn set_dll_directory_uses_os_host_resolution() {
    let set: SetDir = unsafe { core::mem::transmute(proc(module(), "SetDllDirectoryW")) };
    let abs = std::env::temp_dir().join("k32l_dlldir");
    let abs_s = abs.display().to_string();
    assert_eq!(unsafe { set(wide(&abs_s).as_ptr()) }, 1);
    assert_eq!(dll_dir().to_lowercase(), abs_s.to_lowercase());
    let rel = "k32l_rel";
    assert_eq!(unsafe { set(wide(rel).as_ptr()) }, 1);
    let cwd = std::env::current_dir().unwrap();
    if let Some(full) = dlldir::resolve(rel, &cwd, dlldir::package_root()) {
        assert_eq!(dll_dir().to_lowercase(), full.to_lowercase());
    }
    assert_eq!(unsafe { set(core::ptr::null()) }, 1);
}
