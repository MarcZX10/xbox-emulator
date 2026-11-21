use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use dxcompiler_xs::com::*;
use dxcompiler_xs::guids::*;
use dxcompiler_xs::host::{self, Host};
use dxcompiler_xs::native;
use dxcompiler_xs::proxy;
use dxcompiler_xs::{create_with, Route};
use std::path::PathBuf;

extern "system" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(h: *mut c_void, f: u32, n: usize) -> *mut c_void;
    fn HeapReAlloc(h: *mut c_void, f: u32, p: *mut c_void, n: usize) -> *mut c_void;
    fn HeapFree(h: *mut c_void, f: u32, p: *mut c_void) -> i32;
    fn HeapSize(h: *mut c_void, f: u32, p: *const c_void) -> usize;
}

fn console_dlls() -> Vec<PathBuf> {
    ["Fortnite", "XDefiant"]
        .iter()
        .map(|t| PathBuf::from(format!(r"D:\xbox-pc-port\games\{t}\extracted\EmbeddedXvd\Windows\System32\dxcompiler_xs.dll")))
        .filter(|p| p.is_file())
        .collect()
}

fn sdk_host() -> Option<Host> {
    host::sdk_dirs().iter().find_map(|d| host::load_from_dir(d).filter(|h| unsafe { h.supports_utils() }))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

unsafe fn slot<T: Copy>(obj: *mut c_void, i: usize) -> T {
    let vt = *(obj as *const *const usize);
    core::mem::transmute_copy(&*vt.add(i))
}

type CreateBlobFn = unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut *mut c_void) -> Hr;
type PtrFn = unsafe extern "system" fn(*mut c_void) -> *mut c_void;
type SizeFn = unsafe extern "system" fn(*mut c_void) -> usize;
type EncFn = unsafe extern "system" fn(*mut c_void, *mut i32, *mut u32) -> Hr;
type PartFn = unsafe extern "system" fn(*mut c_void, *const DxcBuffer, u32, *mut *mut c_void, *mut u32) -> Hr;
type ConvFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr;
type MoveFn = unsafe extern "system" fn(*mut c_void, *const c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr;

#[test]
fn guid_text_roundtrip() {
    assert_eq!(IID_IDXC_UTILS.to_string(), "{4605C4CB-2019-492A-ADA4-65F20BB7D67F}");
    assert_eq!(guid("6245d6af-66e0-48fd-80b4-4d271796748c"), CLSID_DXC_LIBRARY);
    assert_eq!(&CLSID_DXC_COMPILER.bytes_le()[..4], &[0x93, 0x2D, 0xE2, 0x73]);
}

#[test]
fn every_guid_appears_in_the_console_dll() {
    for dll in console_dlls() {
        let bytes = std::fs::read(&dll).unwrap();
        for (g, name, _) in CLASSES.iter().chain(INTERFACES.iter()) {
            if *g == CLSID_DXC_PDB_UTILS || *g == IID_IDXC_PDB_UTILS {
                continue;
            }
            assert!(contains(&bytes, &g.bytes_le()), "{name} {g} not in {}", dll.display());
        }
    }
}

#[test]
fn pc_guids_appear_in_the_host_dll() {
    let Some(dir) = host::sdk_dirs().into_iter().next() else { return };
    let bytes = std::fs::read(dir.join("dxcompiler.dll")).unwrap();
    for (g, name, kind) in CLASSES.iter().chain(INTERFACES.iter()) {
        let hit = contains(&bytes, &g.bytes_le());
        if *kind == Kind::Pc && *g != IID_INOMARSHAL && *g != IID_IMALLOC && *g != IID_IUNKNOWN {
            assert!(hit, "{name} {g} missing from host dxcompiler.dll");
        }
        if *kind == Kind::XboxOnly {
            assert!(!hit, "{name} {g} unexpectedly in host dxcompiler.dll");
        }
    }
}

#[test]
fn null_out_pointer_is_e_pointer() {
    unsafe {
        let (hr, r) = create_with(None, core::ptr::null_mut(), &CLSID_DXC_UTILS, &IID_IDXC_UTILS, core::ptr::null_mut());
        assert_eq!((hr, r), (E_POINTER, Route::Refused));
        let mut p = 8usize as *mut c_void;
        let (hr, _) = create_with(None, core::ptr::null_mut(), core::ptr::null(), &IID_IDXC_UTILS, &mut p);
        assert_eq!(hr, E_POINTER);
        assert!(p.is_null());
    }
}

#[test]
fn xbox_only_classes_and_interfaces_are_refused() {
    let host = sdk_host();
    for h in [None, host.as_ref()] {
        for (c, _, kind) in CLASSES {
            if *kind != Kind::XboxOnly {
                continue;
            }
            let mut p = 8usize as *mut c_void;
            let (hr, r) = unsafe { create_with(h, core::ptr::null_mut(), c, &IID_IUNKNOWN, &mut p) };
            assert_eq!((hr, r), (E_NOINTERFACE, Route::Refused), "{c}");
            assert!(p.is_null());
        }
        for i in [IID_IXBOX_DXC_CONTAINER, IID_IXBOX_DXC_SHADER_STATS, IID_IXBOX_DXC_BACKEND_DEBUG_INFO, IID_IXBOX_DXC_REG_PRESSURE, IID_IXDXR_COMPILER] {
            let mut p = core::ptr::null_mut();
            let (hr, _) = unsafe { create_with(h, core::ptr::null_mut(), &CLSID_DXC_COMPILER, &i, &mut p) };
            assert_eq!(hr, E_NOINTERFACE);
            assert!(p.is_null());
        }
    }
}

#[test]
fn compiler_without_host_is_class_not_registered() {
    let mut p = 8usize as *mut c_void;
    let (hr, r) = unsafe { create_with(None, core::ptr::null_mut(), &CLSID_DXC_COMPILER, &IID_IDXC_COMPILER3, &mut p) };
    assert_eq!((hr, r), (REGDB_E_CLASSNOTREG, Route::Refused));
    assert!(p.is_null());
}

unsafe fn fortnite_pso_blob(utils: *mut c_void) {
    let src = vec![0xCDu8; 64];
    let mut blob = core::ptr::null_mut();
    let hr = slot::<CreateBlobFn>(utils, 6)(utils, src.as_ptr() as *const c_void, 64, 0, &mut blob);
    drop(src);
    assert_eq!(hr, S_OK);
    assert!(!blob.is_null());
    assert_eq!(slot::<SizeFn>(blob, 4)(blob), 64);
    let p = slot::<PtrFn>(blob, 3)(blob) as *mut u8;
    for i in 0..64 {
        *p.add(i) = i as u8;
    }
    let again = core::slice::from_raw_parts(slot::<PtrFn>(blob, 3)(blob) as *const u8, 64);
    assert_eq!(again[63], 63);
    let enc = query(blob, &IID_IDXC_BLOB_ENCODING).expect("IDxcBlobEncoding");
    let (mut known, mut cp) = (7, 7);
    assert_eq!(slot::<EncFn>(enc, 5)(enc, &mut known, &mut cp), S_OK);
    assert_eq!((known, cp), (1, 0));
    release(enc);
    assert!(query(blob, &IID_IDXC_BLOB_UTF8).is_none());
    assert_eq!(release(blob), 0);
}

#[test]
fn native_utils_serves_fortnite_pso_cache_blobs() {
    unsafe {
        let mut u = core::ptr::null_mut();
        let (hr, r) = create_with(None, core::ptr::null_mut(), &CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut u);
        assert_eq!((hr, r), (S_OK, Route::Native));
        fortnite_pso_blob(u);
        assert_eq!(release(u), 0);
    }
}

#[test]
fn host_utils_serves_fortnite_pso_cache_blobs() {
    let Some(h) = sdk_host() else { return };
    unsafe {
        let mut u = core::ptr::null_mut();
        let (hr, r) = create_with(Some(&h), core::ptr::null_mut(), &CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut u);
        assert_eq!((hr, r), (S_OK, Route::Host));
        fortnite_pso_blob(u);
        release(u);
    }
}

#[test]
fn exported_factory_never_leaves_fortnite_with_a_null_utils() {
    unsafe {
        let mut u = core::ptr::null_mut();
        let hr = dxcompiler_xs::DxcCreateInstance(
            &CLSID_DXC_UTILS as *const _ as *const c_void,
            &IID_IDXC_UTILS as *const _ as *const c_void,
            &mut u,
        );
        assert_eq!(hr, S_OK);
        assert!(!u.is_null());
        fortnite_pso_blob(u);
        release(u);
    }
}

#[test]
fn native_library_and_text_conversion() {
    unsafe {
        let mut lib = core::ptr::null_mut();
        assert_eq!(native::create_utils(&IID_IDXC_LIBRARY, &mut lib), S_OK);
        let utils = query(lib, &IID_IDXC_UTILS).unwrap();
        let back = query(utils, &IID_IDXC_LIBRARY).unwrap();
        assert_eq!(back, lib);
        release(back);
        let text = "float4 main() : SV_Target { return 1; }";
        let mut b = core::ptr::null_mut();
        assert_eq!(slot::<CreateBlobFn>(lib, 7)(lib, text.as_ptr() as *const c_void, text.len() as u32, DXC_CP_UTF8, &mut b), S_OK);
        assert!(query(b, &IID_IDXC_BLOB_UTF8).map(|p| release(p)).is_some());
        let mut w = core::ptr::null_mut();
        assert_eq!(slot::<ConvFn>(utils, 11)(utils, b, &mut w), S_OK);
        let wb = query(w, &IID_IDXC_BLOB_UTF16).unwrap();
        let n = slot::<SizeFn>(wb, 7)(wb);
        let s = String::from_utf16(core::slice::from_raw_parts(slot::<PtrFn>(wb, 6)(wb) as *const u16, n)).unwrap();
        assert_eq!(s, text);
        release(wb);
        let mut u8b = core::ptr::null_mut();
        assert_eq!(slot::<ConvFn>(lib, 11)(lib, w, &mut u8b), S_OK);
        assert_eq!(&blob_bytes(u8b)[..text.len()], text.as_bytes());
        assert_eq!(slot::<SizeFn>(u8b, 7)(u8b), text.len());
        let mut sub = core::ptr::null_mut();
        type SubFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr;
        assert_eq!(slot::<SubFn>(utils, 3)(utils, b, 7, 6, &mut sub), S_OK);
        assert_eq!(blob_bytes(sub), b"main()");
        assert_eq!(slot::<SubFn>(utils, 3)(utils, b, 7, 1000, &mut sub), E_INVALIDARG);
        assert!(sub.is_null());
        for o in [w, u8b, b] {
            release(o);
        }
        release(utils);
        assert_eq!(release(lib), 0);
    }
}

fn container(parts: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let head = 32 + parts.len() * 4;
    let mut offs = Vec::new();
    let mut body = Vec::new();
    for (cc, data) in parts {
        offs.push((head + body.len()) as u32);
        body.extend_from_slice(*cc);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
    }
    let total = (head + body.len()) as u32;
    let mut v = Vec::new();
    v.extend_from_slice(b"DXBC");
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&total.to_le_bytes());
    v.extend_from_slice(&(parts.len() as u32).to_le_bytes());
    for o in offs {
        v.extend_from_slice(&o.to_le_bytes());
    }
    v.extend_from_slice(&body);
    v
}

fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

unsafe fn part_via(utils: *mut c_void, bytes: &[u8], cc: u32) -> (Hr, usize, u32) {
    let buf = DxcBuffer { ptr: bytes.as_ptr() as *const c_void, size: bytes.len(), encoding: 0 };
    let (mut d, mut n) = (core::ptr::null_mut(), 0u32);
    let hr = slot::<PartFn>(utils, 12)(utils, &buf, cc, &mut d, &mut n);
    let off = if hr >= 0 { d as usize - bytes.as_ptr() as usize } else { 0 };
    (hr, off, if hr >= 0 { n } else { 0 })
}

#[test]
fn container_parts_match_the_host() {
    let c = container(&[(b"RTS0", b"rootsig!"), (b"DXIL", b"0123456789ab")]);
    let bad = b"NOPE and some more bytes to look like a header.....".to_vec();
    let mut nat = core::ptr::null_mut();
    unsafe {
        assert_eq!(native::create_utils(&IID_IDXC_UTILS, &mut nat), S_OK);
        assert_eq!(part_via(nat, &c, fourcc(b"DXIL")), (S_OK, 32 + 8 + 8 + 8 + 8, 12));
        assert_eq!(part_via(nat, &c, fourcc(b"RTS0")), (S_OK, 32 + 8 + 8, 8));
        assert_eq!(part_via(nat, &c, fourcc(b"PSV0")).0, DXC_E_MISSING_PART);
        assert_eq!(part_via(nat, &bad, fourcc(b"DXIL")).0, DXC_E_CONTAINER_INVALID);
        if let Some(h) = sdk_host() {
            let mut hu = core::ptr::null_mut();
            assert!(h.create(&CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut hu) >= 0);
            for cc in [b"DXIL", b"RTS0", b"PSV0"] {
                assert_eq!(part_via(nat, &c, fourcc(cc)), part_via(hu, &c, fourcc(cc)), "{}", String::from_utf8_lossy(cc));
            }
            assert_eq!(part_via(nat, &bad, fourcc(b"DXIL")).0, part_via(hu, &bad, fourcc(b"DXIL")).0);
            release(hu);
        }
        release(nat);
    }
}

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);

#[repr(C)]
struct TestMallocVtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hr,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    alloc: unsafe extern "system" fn(*mut c_void, usize) -> *mut c_void,
    realloc: unsafe extern "system" fn(*mut c_void, *mut c_void, usize) -> *mut c_void,
    free: unsafe extern "system" fn(*mut c_void, *mut c_void),
    get_size: unsafe extern "system" fn(*mut c_void, *mut c_void) -> usize,
    did_alloc: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    heap_minimize: unsafe extern "system" fn(*mut c_void),
}

unsafe extern "system" fn m_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> Hr {
    if *iid == IID_IUNKNOWN || *iid == IID_IMALLOC {
        *out = this;
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}
unsafe extern "system" fn m_ref(_: *mut c_void) -> u32 {
    2
}
unsafe extern "system" fn m_alloc(_: *mut c_void, n: usize) -> *mut c_void {
    ALLOCS.fetch_add(1, Ordering::SeqCst);
    HeapAlloc(GetProcessHeap(), 0, n.max(1))
}
unsafe extern "system" fn m_realloc(_: *mut c_void, p: *mut c_void, n: usize) -> *mut c_void {
    ALLOCS.fetch_add(1, Ordering::SeqCst);
    if p.is_null() {
        HeapAlloc(GetProcessHeap(), 0, n.max(1))
    } else {
        HeapReAlloc(GetProcessHeap(), 0, p, n.max(1))
    }
}
unsafe extern "system" fn m_free(_: *mut c_void, p: *mut c_void) {
    if !p.is_null() {
        FREES.fetch_add(1, Ordering::SeqCst);
        HeapFree(GetProcessHeap(), 0, p);
    }
}
unsafe extern "system" fn m_size(_: *mut c_void, p: *mut c_void) -> usize {
    HeapSize(GetProcessHeap(), 0, p)
}
unsafe extern "system" fn m_did(_: *mut c_void, _: *mut c_void) -> i32 {
    -1
}
unsafe extern "system" fn m_min(_: *mut c_void) {}

static TEST_MALLOC_VTBL: TestMallocVtbl = TestMallocVtbl {
    qi: m_qi,
    add_ref: m_ref,
    release: m_ref,
    alloc: m_alloc,
    realloc: m_realloc,
    free: m_free,
    get_size: m_size,
    did_alloc: m_did,
    heap_minimize: m_min,
};

struct TestMalloc {
    _vtbl: &'static TestMallocVtbl,
}

static TEST_MALLOC: TestMalloc = TestMalloc { _vtbl: &TEST_MALLOC_VTBL };

fn test_malloc() -> *mut c_void {
    &TEST_MALLOC as *const TestMalloc as *mut c_void
}

#[test]
fn move_to_blob_frees_through_the_callers_imalloc() {
    unsafe {
        let mut u = core::ptr::null_mut();
        assert_eq!(native::create_utils(&IID_IDXC_UTILS, &mut u), S_OK);
        let p = m_alloc(core::ptr::null_mut(), 16);
        let before = FREES.load(Ordering::SeqCst);
        let mut b = core::ptr::null_mut();
        assert_eq!(slot::<MoveFn>(u, 5)(u, p, test_malloc(), 16, DXC_CP_ACP, &mut b), S_OK);
        assert_eq!(slot::<PtrFn>(b, 3)(b), p);
        assert_eq!(release(b), 0);
        assert!(FREES.load(Ordering::SeqCst) > before);
        release(u);
    }
}

#[test]
fn create_instance2_passes_the_imalloc_to_the_host() {
    let Some(h) = sdk_host() else { return };
    unsafe {
        let before = ALLOCS.load(Ordering::SeqCst);
        let mut u = core::ptr::null_mut();
        let (hr, r) = create_with(Some(&h), test_malloc(), &CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut u);
        assert_eq!((hr, r), (S_OK, Route::Host));
        if h.create2 != 0 {
            assert!(ALLOCS.load(Ordering::SeqCst) > before, "host did not allocate through the passed IMalloc");
        }
        release(u);
        let mut v = core::ptr::null_mut();
        let (hr, _) = create_with(Some(&h), core::ptr::null_mut(), &CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut v);
        assert_eq!(hr, S_OK);
        release(v);
    }
}

type Compile3Fn = unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const *const u16, u32, *mut c_void, *const Guid, *mut *mut c_void) -> Hr;
type StatusFn = unsafe extern "system" fn(*mut c_void, *mut Hr) -> Hr;

#[test]
fn host_compiler_is_wrapped_and_compiles_dxil() {
    let Some(h) = sdk_host() else { return };
    unsafe {
        let mut c = core::ptr::null_mut();
        let (hr, r) = create_with(Some(&h), core::ptr::null_mut(), &CLSID_DXC_COMPILER, &IID_IDXC_COMPILER3, &mut c);
        assert_eq!((hr, r), (S_OK, Route::Host));
        assert!(proxy::is_proxy(c));
        let src = "float4 main() : SV_Target { return float4(1, 0, 0, 1); }";
        let buf = DxcBuffer { ptr: src.as_ptr() as *const c_void, size: src.len(), encoding: DXC_CP_UTF8 };
        let args: Vec<Vec<u16>> = ["-T", "ps_6_0", "-E", "main"].iter().map(|a| wide(a)).collect();
        let ptrs: Vec<*const u16> = args.iter().map(|a| a.as_ptr()).collect();
        let mut res = core::ptr::null_mut();
        let hr = slot::<Compile3Fn>(c, 3)(c, &buf, ptrs.as_ptr(), ptrs.len() as u32, core::ptr::null_mut(), &IID_IDXC_RESULT, &mut res);
        assert_eq!(hr, S_OK);
        let mut st = -1;
        assert_eq!(slot::<StatusFn>(res, 3)(res, &mut st), S_OK);
        assert_eq!(st, S_OK);
        release(res);
        let legacy = query(c, &IID_IDXC_COMPILER).unwrap();
        assert!(proxy::is_proxy(legacy));
        let unk = query(c, &IID_IUNKNOWN).unwrap();
        assert_eq!(unk, c);
        release(unk);
        release(legacy);
        assert_eq!(release(c), 0);
    }
}

#[test]
fn compile_argument_summary() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let s = proxy::summarize(None, None, &a(&["-T", "cs_6_6", "-E", "CSMain", "-O3"]));
    assert_eq!((s.profile.as_deref(), s.entry.as_deref()), (Some("cs_6_6"), Some("CSMain")));
    let s = proxy::summarize(None, None, &a(&["/Tps_6_0", "/Emain", "-enable-16bit-types"]));
    assert_eq!((s.profile.as_deref(), s.entry.as_deref()), (Some("ps_6_0"), Some("main")));
    let s = proxy::summarize(Some("VS".into()), Some("vs_6_5".into()), &a(&["-D__XBOX_SCARLETT=1", "-Zi"]));
    assert_eq!((s.profile.as_deref(), s.entry.as_deref()), (Some("vs_6_5"), Some("VS")));
    assert_eq!(s.xbox, vec!["-D__XBOX_SCARLETT=1".to_string()]);
}

#[test]
fn xdxr_factories_follow_the_console_contract() {
    unsafe {
        let mut p = 8usize as *mut c_void;
        assert_eq!(dxcompiler_xs::CreateXdxrCompiler(core::ptr::null(), core::ptr::null(), core::ptr::null_mut()), E_POINTER);
        let other = &CLSID_DXC_COMPILER as *const _ as *const c_void;
        let xdxr = &CLSID_XDXR_COMPILER as *const _ as *const c_void;
        assert_eq!(dxcompiler_xs::CreateXdxrCompiler(other, xdxr, &mut p), REGDB_E_CLASSNOTREG);
        assert!(p.is_null());
        assert_eq!(dxcompiler_xs::CreateXdxrCompiler2(test_malloc(), xdxr, xdxr, &mut p), S_OK);
        assert!(!p.is_null());
        assert_eq!(release(p), 0);
    }
}

fn pe_exports(bytes: &[u8]) -> Vec<(u16, String)> {
    let u16_at = |o: usize| u16::from_le_bytes(bytes[o..o + 2].try_into().unwrap());
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let pe = u32_at(0x3c) as usize;
    let nsec = u16_at(pe + 6) as usize;
    let opt = pe + 24;
    let secs = opt + u16_at(pe + 20) as usize;
    let map = |rva: u32| -> usize {
        (0..nsec)
            .find_map(|i| {
                let s = secs + i * 40;
                let (va, vs, raw) = (u32_at(s + 12), u32_at(s + 8).max(u32_at(s + 16)), u32_at(s + 20));
                (rva >= va && rva < va + vs).then(|| (rva - va + raw) as usize)
            })
            .unwrap()
    };
    let ed = map(u32_at(opt + 112));
    let base = u32_at(ed + 16);
    let (n, names, ords) = (u32_at(ed + 24) as usize, map(u32_at(ed + 32)), map(u32_at(ed + 36)));
    let mut out: Vec<(u16, String)> = (0..n)
        .map(|i| {
            let np = map(u32_at(names + i * 4));
            let end = bytes[np..].iter().position(|&c| c == 0).unwrap();
            ((u16_at(ords + i * 2) as u32 + base) as u16, String::from_utf8_lossy(&bytes[np..np + end]).into_owned())
        })
        .collect();
    out.sort();
    out
}

fn table() -> Vec<(u16, String, bool)> {
    dxcompiler_xs::EXPORTS
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f[0].parse().unwrap(), f[1].to_string(), f.len() > 2)
        })
        .collect()
}

#[test]
fn export_table_matches_the_console_dll() {
    let want: Vec<(u16, String)> = table().into_iter().map(|(o, n, _)| (o, n)).collect();
    for dll in console_dlls() {
        let mut got = pe_exports(&std::fs::read(&dll).unwrap());
        got.sort();
        let mut w = want.clone();
        w.sort();
        assert_eq!(got, w, "{}", dll.display());
    }
}

#[test]
fn built_dll_exports_console_ordinals() {
    let exe = std::env::current_exe().unwrap();
    let mut dirs: Vec<PathBuf> = exe.ancestors().skip(1).map(|d| d.to_path_buf()).collect();
    if let Ok(t) = std::env::var("CARGO_TARGET_DIR") {
        dirs.insert(0, PathBuf::from(t).join("release"));
    }
    let Some(dll) = dirs.iter().map(|d| d.join("dxcompiler_xs.dll")).find(|p| p.is_file()) else {
        panic!("built dxcompiler_xs.dll not found near {}", exe.display())
    };
    let got = pe_exports(&std::fs::read(&dll).unwrap());
    for (o, n, gated) in table() {
        let hit = got.iter().any(|(go, gn)| *go == o && *gn == n);
        let named = got.iter().any(|(_, gn)| *gn == n);
        if gated {
            assert!(hit || !named, "{n} exported at the wrong ordinal in {}: {got:?}", dll.display());
        } else {
            assert!(hit, "{n} not at ordinal {o} in {}: {got:?}", dll.display());
        }
    }
}

type SubBlobFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr;

unsafe fn encoding_of(b: *mut c_void) -> (i32, u32) {
    let enc = query(b, &IID_IDXC_BLOB_ENCODING).expect("IDxcBlobEncoding");
    let (mut k, mut c) = (-1, 0xFFFF_FFFF);
    slot::<EncFn>(enc, 5)(enc, &mut k, &mut c);
    release(enc);
    (k, c)
}

unsafe fn blob_traits(utils: *mut c_void, cp: u32) -> Vec<String> {
    let data: Vec<u8> = if cp == DXC_CP_UTF16 { wide("abc").iter().flat_map(|c| c.to_le_bytes()).collect() } else { b"abc ".to_vec() };
    let mut b = core::ptr::null_mut();
    assert_eq!(slot::<CreateBlobFn>(utils, 6)(utils, data.as_ptr() as *const c_void, data.len() as u32, cp, &mut b), S_OK);
    let mut v = vec![format!("enc={:?}", encoding_of(b))];
    for (n, i) in [("utf8", IID_IDXC_BLOB_UTF8), ("utf16", IID_IDXC_BLOB_UTF16)] {
        match query(b, &i) {
            Some(q) => {
                v.push(format!("{n}=yes len={}", slot::<SizeFn>(q, 7)(q)));
                release(q);
            }
            None => v.push(format!("{n}=no")),
        }
    }
    let mut sub = core::ptr::null_mut();
    if slot::<SubBlobFn>(utils, 3)(utils, b, 0, 2, &mut sub) >= 0 && !sub.is_null() {
        v.push(format!("sub-size={}", slot::<SizeFn>(sub, 4)(sub)));
        v.push(format!("sub-encoding-iface={}", query(sub, &IID_IDXC_BLOB_ENCODING).map(|q| release(q)).is_some()));
        release(sub);
    }
    let mut u8b = core::ptr::null_mut();
    if slot::<ConvFn>(utils, 10)(utils, b, &mut u8b) >= 0 {
        let q = query(u8b, &IID_IDXC_BLOB_UTF8).unwrap();
        let n = slot::<SizeFn>(q, 7)(q);
        let s = core::slice::from_raw_parts(slot::<PtrFn>(q, 6)(q) as *const u8, n).to_vec();
        v.push(format!("as-utf8={:?} enc={:?}", String::from_utf8_lossy(&s), encoding_of(u8b)));
        release(q);
        release(u8b);
    }
    release(b);
    v
}

#[test]
fn native_blob_semantics_match_the_host() {
    let Some(h) = sdk_host() else { return };
    unsafe {
        let (mut nat, mut hu) = (core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(native::create_utils(&IID_IDXC_UTILS, &mut nat), S_OK);
        assert!(h.create(&CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut hu) >= 0);
        for cp in [DXC_CP_ACP, DXC_CP_UTF8, DXC_CP_UTF16] {
            assert_eq!(blob_traits(nat, cp), blob_traits(hu, cp), "codepage {cp}");
        }
        release(nat);
        release(hu);
    }
}
