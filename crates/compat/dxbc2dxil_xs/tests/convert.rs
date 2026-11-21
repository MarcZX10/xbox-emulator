use core::ffi::c_void;
use dxbc2dxil_xs::{Allocator, SCDxil_ConvertDxbcToDxil, CONSOLE};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(h: *mut c_void, flags: u32, n: usize) -> *mut c_void;
    fn HeapFree(h: *mut c_void, flags: u32, p: *mut c_void) -> i32;
}

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
const CTX: usize = 0x5eed;

unsafe extern "system" fn heap_alloc(ctx: *mut c_void, n: u32) -> *mut c_void {
    assert_eq!(ctx as usize, CTX);
    ALLOCS.fetch_add(1, Ordering::SeqCst);
    HeapAlloc(GetProcessHeap(), 0, n as usize)
}

unsafe extern "system" fn heap_free(_ctx: *mut c_void, p: *mut c_void) {
    HeapFree(GetProcessHeap(), 0, p);
}

fn allocator() -> Allocator {
    Allocator { alloc: Some(heap_alloc), free: Some(heap_free), ctx: CTX as *mut c_void }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn load(p: &Path) -> usize {
    unsafe { LoadLibraryExW(wide(&p.to_string_lossy()).as_ptr(), core::ptr::null_mut(), 8) as usize }
}

unsafe fn proc(h: usize, n: &str) -> usize {
    GetProcAddress(h as *mut c_void, format!("{n}\0").as_ptr()) as usize
}

unsafe fn vt(obj: usize, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn blob(b: usize) -> Vec<u8> {
    let ptr: unsafe extern "system" fn(usize) -> *const u8 = core::mem::transmute(vt(b, 3));
    let len: unsafe extern "system" fn(usize) -> usize = core::mem::transmute(vt(b, 4));
    let (p, n) = (ptr(b), len(b));
    let v = if p.is_null() || n == 0 { Vec::new() } else { core::slice::from_raw_parts(p, n).to_vec() };
    let rel: unsafe extern "system" fn(usize) -> u32 = core::mem::transmute(vt(b, 2));
    rel(b);
    v
}

fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let mut g = [0u8; 16];
    g[..4].copy_from_slice(&a.to_le_bytes());
    g[4..6].copy_from_slice(&b.to_le_bytes());
    g[6..8].copy_from_slice(&c.to_le_bytes());
    g[8..].copy_from_slice(&d);
    g
}

type D3DCompile = unsafe extern "system" fn(*const u8, usize, *const u8, usize, usize, *const u8, *const u8, u32, u32, *mut usize, *mut usize) -> i32;

fn fxc(src: &str, target: &str) -> Vec<u8> {
    let h = load(Path::new(r"C:\Windows\System32\D3DCompiler_47.dll"));
    assert!(h != 0, "D3DCompiler_47.dll");
    unsafe {
        let f: D3DCompile = core::mem::transmute(proc(h, "D3DCompile"));
        let t = format!("{target}\0");
        let (mut code, mut err) = (0usize, 0usize);
        let hr = f(src.as_ptr(), src.len(), b"t.hlsl\0".as_ptr(), 0, 0, b"main\0".as_ptr(), t.as_ptr(), 0, 0, &mut code, &mut err);
        let msg = if err != 0 { String::from_utf8_lossy(&blob(err)).into_owned() } else { String::new() };
        assert!(hr >= 0 && code != 0, "D3DCompile {hr:#x} {msg}");
        blob(code)
    }
}

fn host_dxc() -> usize {
    let kits = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&kits).unwrap().flatten().map(|e| e.path().join("x64")).filter(|p| p.join("dxcompiler.dll").is_file()).collect();
    dirs.sort();
    let d = dirs.pop().expect("Windows SDK dxcompiler.dll");
    load(&d.join("dxil.dll"));
    let h = load(&d.join("dxcompiler.dll"));
    assert!(h != 0);
    h
}

unsafe fn create(h: usize, clsid: [u8; 16], iid: [u8; 16]) -> usize {
    let f: unsafe extern "system" fn(*const u8, *const u8, *mut usize) -> i32 = core::mem::transmute(proc(h, "DxcCreateInstance"));
    let mut o = 0;
    assert!(f(clsid.as_ptr(), iid.as_ptr(), &mut o) >= 0 && o != 0);
    o
}

fn disassemble(dxil: &[u8]) -> String {
    let h = host_dxc();
    unsafe {
        let utils = create(h, guid(0x6245_d6af, 0x66e0, 0x48fd, [0x80, 0xb4, 0x4d, 0x27, 0x17, 0x96, 0x74, 0x8c]), guid(0x4605_c4cb, 0x2019, 0x492a, [0xad, 0xa4, 0x65, 0xf2, 0x0b, 0xb7, 0xd6, 0x7f]));
        let comp = create(h, guid(0x73e2_2d93, 0xe6ce, 0x47f3, [0xb5, 0xbf, 0xf0, 0x66, 0x4f, 0x39, 0xc1, 0xb0]), guid(0x8c21_0bf3, 0x011f, 0x4422, [0x8d, 0x70, 0x6f, 0x9a, 0xcb, 0x8d, 0xb6, 0x17]));
        let mk: unsafe extern "system" fn(usize, *const u8, u32, u32, *mut usize) -> i32 = core::mem::transmute(vt(utils, 6));
        let mut b = 0;
        assert!(mk(utils, dxil.as_ptr(), dxil.len() as u32, 0, &mut b) >= 0);
        let dis: unsafe extern "system" fn(usize, usize, *mut usize) -> i32 = core::mem::transmute(vt(comp, 5));
        let mut text = 0;
        let hr = dis(comp, b, &mut text);
        assert!(hr >= 0 && text != 0, "host DXC Disassemble {hr:#x}");
        String::from_utf8_lossy(&blob(text)).into_owned()
    }
}

fn part<'a>(dxbc: &'a [u8], cc: &[u8; 4]) -> Option<&'a [u8]> {
    let n = u32::from_le_bytes(dxbc[28..32].try_into().ok()?) as usize;
    (0..n).find_map(|i| {
        let o = u32::from_le_bytes(dxbc[32 + 4 * i..36 + 4 * i].try_into().ok()?) as usize;
        let sz = u32::from_le_bytes(dxbc[o + 4..o + 8].try_into().ok()?) as usize;
        (&dxbc[o..o + 4] == cc).then(|| &dxbc[o + 8..o + 8 + sz])
    })
}

fn convert(src: &[u8]) -> (u32, Vec<u8>) {
    let a = allocator();
    let (mut out, mut n) = (core::ptr::null_mut(), 0usize);
    let rc = unsafe { SCDxil_ConvertDxbcToDxil(src.as_ptr() as *const c_void, src.len(), &mut out, &mut n, &a) };
    let v = if out.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(out as *const u8, n).to_vec() } };
    if !out.is_null() {
        unsafe { heap_free(CTX as *mut c_void, out) };
    }
    (rc, v)
}

const PS: &str = "Texture2D t : register(t0); SamplerState s : register(s0); cbuffer C : register(b0) { float4 tint; }; float4 main(float4 p : SV_Position, float2 uv : TEXCOORD0) : SV_Target { return t.Sample(s, uv) * tint + p.x; }";
const CS: &str = "RWStructuredBuffer<uint> o : register(u0); [numthreads(64,1,1)] void main(uint3 id : SV_DispatchThreadID) { InterlockedAdd(o[id.x & 15], id.x * 3u); }";

#[test]
fn console_dll_is_the_title_dump_copy() {
    assert!(CONSOLE.module() != 0, "console dxbc2dxil_xs.dll did not load: {:?}", CONSOLE.candidates());
    let p = CONSOLE.path().unwrap();
    assert!(p.to_string_lossy().to_lowercase().ends_with("dxbc2dxil_xs.dll") || p.to_string_lossy().to_lowercase().ends_with("dxbc2dxil_xs_console.dll"), "{}", p.display());
}

#[test]
fn pixel_shader_dxbc_becomes_valid_dxil() {
    let dxbc = fxc(PS, "ps_5_0");
    assert!(part(&dxbc, b"DXIL").is_none() && part(&dxbc, b"SHEX").is_some());
    let before = ALLOCS.load(Ordering::SeqCst);
    let (rc, out) = convert(&dxbc);
    assert_eq!(rc, 0);
    assert!(ALLOCS.load(Ordering::SeqCst) > before, "output not allocated through the caller allocator");
    assert_eq!(&out[..4], b"DXBC");
    let dxil = part(&out, b"DXIL").expect("no DXIL part in the converted container");
    assert_eq!(u32::from_le_bytes(dxil[0..4].try_into().unwrap()) >> 16, 0, "program kind is not pixel");
    assert_eq!(&dxil[8..12], b"DXIL");
    let bc = u32::from_le_bytes(dxil[16..20].try_into().unwrap()) as usize;
    assert_eq!(&dxil[8 + bc..12 + bc], b"BC\xC0\xDE");
    let text = disassemble(&out);
    for needle in ["dx.op.sample.f32", "dx.op.cbufferLoadLegacy.f32", "dx.op.storeOutput.f32", "dx.op.loadInput.f32", "SV_Target", "!{!\"ps\", i32 6, i32 0}"] {
        assert!(text.contains(needle), "{needle} missing in\n{text}");
    }
}

#[test]
fn compute_shader_atomics_survive_conversion() {
    let dxbc = fxc(CS, "cs_5_0");
    let (rc, out) = convert(&dxbc);
    assert_eq!(rc, 0);
    let dxil = part(&out, b"DXIL").expect("DXIL part");
    assert_eq!(u32::from_le_bytes(dxil[0..4].try_into().unwrap()) >> 16, 5, "program kind is not compute");
    let text = disassemble(&out);
    for needle in ["dx.op.atomicBinOp.i32", "dx.op.threadId.i32", "!{i32 64, i32 1, i32 1}", "!{!\"cs\", i32 6, i32 0}", "!convertedFromDxbc"] {
        assert!(text.contains(needle), "{needle} missing in\n{text}");
    }
}

#[test]
fn container_already_holding_dxil_is_copied_unchanged() {
    let dxbc = fxc(PS, "ps_5_0");
    let (rc, once) = convert(&dxbc);
    assert_eq!(rc, 0);
    let (rc2, twice) = convert(&once);
    assert_eq!(rc2, 0);
    assert_eq!(once, twice);
}

#[test]
fn null_outputs_are_the_console_failure() {
    let dxbc = fxc(PS, "ps_5_0");
    let a = allocator();
    let mut n = 0usize;
    let mut out = core::ptr::null_mut();
    unsafe {
        assert_eq!(SCDxil_ConvertDxbcToDxil(dxbc.as_ptr() as *const c_void, dxbc.len(), core::ptr::null_mut(), &mut n, &a), 1);
        assert_eq!(SCDxil_ConvertDxbcToDxil(dxbc.as_ptr() as *const c_void, dxbc.len(), &mut out, core::ptr::null_mut(), &a), 1);
    }
}
