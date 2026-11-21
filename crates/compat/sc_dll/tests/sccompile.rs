#![allow(non_snake_case)]

mod kit;

use core::ffi::c_void;
use sc_dll::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, prot: u32) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, prot: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

const CREATE_INPUT: usize = 0x98;
const COMPILE_INPUT: usize = 0x160;
const SC_VERSION_DURANGO: u32 = 0xc;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

static T_CREATE: AtomicUsize = AtomicUsize::new(0);
static T_COMPILE: AtomicUsize = AtomicUsize::new(0);
static T_DESTROY: AtomicUsize = AtomicUsize::new(0);

#[derive(Default, Clone)]
struct Log {
    create: Vec<(usize, u32)>,
    compile: Vec<usize>,
    destroy: Vec<usize>,
    mismatches: Vec<String>,
}

static LOG: Mutex<Log> = Mutex::new(Log { create: Vec::new(), compile: Vec::new(), destroy: Vec::new(), mismatches: Vec::new() });

unsafe fn rd<T: Copy>(p: usize, o: usize) -> T {
    ((p + o) as *const T).read_unaligned()
}

unsafe fn wr<T: Copy>(b: &mut [u8], o: usize, v: T) {
    (b.as_mut_ptr().add(o) as *mut T).write_unaligned(v)
}

unsafe fn create_input(captured: usize) -> Box<[u8; CREATE_INPUT]> {
    let mut b = Box::new([0u8; CREATE_INPUT]);
    wr(&mut b[..], 0x00, u32::MAX);
    wr(&mut b[..], 0x04, u32::MAX);
    wr(&mut b[..], 0x08, SC_VERSION_DURANGO);
    wr(&mut b[..], 0x28, 1u32);
    wr(&mut b[..], 0x30, rd::<usize>(captured, 0x30));
    wr(&mut b[..], 0x38, rd::<usize>(captured, 0x38));
    wr(&mut b[..], 0x40, rd::<usize>(captured, 0x40));
    wr(&mut b[..], 0x90, rd::<usize>(captured, 0x90));
    b
}

unsafe fn compile_input(captured: usize) -> Box<[u8; COMPILE_INPUT]> {
    let mut b = Box::new([0u8; COMPILE_INPUT]);
    wr(&mut b[..], 0xc0, rd::<usize>(captured, 0xc0));
    for o in (0xc8..0xe0).step_by(8) {
        wr(&mut b[..], o, rd::<u64>(captured, o));
    }
    wr(&mut b[..], 0xe0, rd::<usize>(captured, 0xe0));
    wr(&mut b[..], 0xe8, rd::<u32>(captured, 0xe8));
    wr(&mut b[..], 0xf0, rd::<usize>(captured, 0xf0));
    b
}

fn check(what: &str, ours: &[u8], captured: usize) {
    let theirs = unsafe { core::slice::from_raw_parts(captured as *const u8, ours.len()) };
    if let Some(i) = ours.iter().zip(theirs).position(|(a, b)| a != b) {
        LOG.lock().unwrap().mismatches.push(format!("{what} differs at {i:#x}: ours {:#x} console {:#x}", ours[i], theirs[i]));
    }
}

fn nested() -> bool {
    DEPTH.with(|d| d.get() > 0)
}

fn enter<R>(f: impl FnOnce() -> R) -> R {
    DEPTH.with(|d| d.set(d.get() + 1));
    let r = f();
    DEPTH.with(|d| d.set(d.get() - 1));
    r
}

unsafe fn original(t: &AtomicUsize) -> Fwd {
    core::mem::transmute(t.load(Ordering::SeqCst))
}

unsafe extern "system" fn create_hook(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
    if nested() {
        return original(&T_CREATE)(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    }
    let ours = Box::leak(create_input(a1));
    check("SCCreate input", &ours[..], a1);
    let r = enter(|| SCCreate(ours.as_ptr() as usize, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12));
    let state = if r == 0 { u32::MAX } else { rd::<u32>(r, 0x1b0) };
    LOG.lock().unwrap().create.push((r, state));
    r
}

unsafe extern "system" fn compile_hook(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
    if nested() {
        return original(&T_COMPILE)(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    }
    let ours = Box::leak(compile_input(a2));
    check("SCCompileShader input", &ours[..], a2);
    let r = enter(|| SCCompileShader(a1, ours.as_ptr() as usize, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12));
    LOG.lock().unwrap().compile.push(r);
    r
}

unsafe extern "system" fn destroy_hook(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
    if nested() {
        return original(&T_DESTROY)(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    }
    let r = enter(|| SCDestroy(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12));
    LOG.lock().unwrap().destroy.push(r);
    r
}

fn abs_jmp(to: usize) -> [u8; 12] {
    let mut j = [0x48, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xe0];
    j[2..10].copy_from_slice(&to.to_le_bytes());
    j
}

unsafe fn hook(entry: usize, prologue: &[u8], to: usize, slot: &AtomicUsize) {
    let n = prologue.len();
    let code = core::slice::from_raw_parts(entry as *const u8, n);
    assert_eq!(code, prologue, "console prologue at {entry:#x} is not the expected relocatable one");
    let t = VirtualAlloc(core::ptr::null_mut(), 64, 0x3000, 0x40) as *mut u8;
    assert!(!t.is_null());
    core::ptr::copy_nonoverlapping(entry as *const u8, t, n);
    core::ptr::copy_nonoverlapping(abs_jmp(entry + n).as_ptr(), t.add(n), 12);
    slot.store(t as usize, Ordering::SeqCst);
    let mut old = 0u32;
    assert!(VirtualProtect(entry as *mut c_void, 12, 0x40, &mut old) != 0);
    core::ptr::copy_nonoverlapping(abs_jmp(to).as_ptr(), entry as *mut u8, 12);
    VirtualProtect(entry as *mut c_void, 12, old, &mut old);
    FlushInstructionCache(GetCurrentProcess(), entry as *const c_void, 12);
}

const PRO_CREATE: [u8; 15] = [0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x6c, 0x24, 0x18, 0x48, 0x89, 0x74, 0x24, 0x20];
const PRO_DESTROY: [u8; 12] = [0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0xd9, 0x48, 0x85, 0xc9];

fn pro_compile(entry: usize) -> [u8; 12] {
    let mut p = [0x40, 0x53, 0x48, 0x83, 0xec, 0x40, 0x4d, 0x8b, 0xd9, 0x48, 0x8b, 0xd9];
    p[8] = unsafe { *((entry + 8) as *const u8) };
    assert!(matches!(p[8], 0xd9 | 0xd1));
    p
}

const VS: &str = "float4 main(float4 p:POSITION):SV_Position{return p*2;}";
const PS: &str = "Texture2D t:register(t0); SamplerState s:register(s0); cbuffer C:register(b0){float4 k;};\nfloat4 main(float2 uv:TEXCOORD):SV_Target{return t.Sample(s,uv)*k;}";
const CS: &str = "RWStructuredBuffer<uint> o:register(u0);\n[numthreads(8,8,1)] void main(uint3 i:SV_DispatchThreadID){o[i.y*64+i.x]=i.x*i.y+7;}";
const SHADERS: [(&str, &str, &str); 3] = [("vs", VS, "vs_5_0"), ("ps", PS, "ps_5_0"), ("cs", CS, "cs_5_0")];

struct Run {
    direct: Vec<(&'static str, Vec<u8>)>,
    ours: Vec<(&'static str, Vec<u8>)>,
    log: Log,
}

fn proc(name: &str) -> usize {
    let h = console::module();
    let n = format!("{name}\0");
    unsafe { kit::GetProcAddress(h as *mut c_void, n.as_ptr()) as usize }
}

fn run() -> &'static Run {
    static R: OnceLock<Run> = OnceLock::new();
    R.get_or_init(|| {
        assert!(console::module() != 0, "console sc_dll.dll not found");
        let d3dc = kit::load(&console::dir().unwrap().join("d3dcompiler_47.dll"));
        assert!(d3dc != 0);
        let compile_all = || SHADERS.iter().map(|(n, s, t)| (*n, kit::d3dcompile(d3dc, s, t).unwrap_or_else(|e| panic!("D3DCompile {n}: {e}")))).collect::<Vec<_>>();
        let direct = compile_all();
        let (c, k, d) = (proc("SCCreate"), proc("SCCompileShader"), proc("SCDestroy"));
        assert!(c != 0 && k != 0 && d != 0);
        unsafe {
            hook(c, &PRO_CREATE, create_hook as usize, &T_CREATE);
            hook(k, &pro_compile(k), compile_hook as usize, &T_COMPILE);
            hook(d, &PRO_DESTROY, destroy_hook as usize, &T_DESTROY);
        }
        let ours = compile_all();
        Run { direct, ours, log: LOG.lock().unwrap().clone() }
    })
}

#[test]
fn SCCreate_builds_compilers_from_reconstructed_input() {
    let r = run();
    assert!(r.log.mismatches.is_empty(), "{:?}", r.log.mismatches);
    assert!(r.log.create.len() >= SHADERS.len(), "{:?}", r.log.create);
    for (h, state) in &r.log.create {
        assert!(*h != 0, "SCCreate returned NULL");
        assert_eq!(*state, 0, "compiler {h:#x} created in error state");
    }
    assert_eq!(r.log.destroy.len(), r.log.create.len());
    assert!(r.log.destroy.iter().all(|v| *v as u32 == 0), "{:?}", r.log.destroy);
}

#[test]
fn SCCompileShader_compiles_real_shaders_like_the_console() {
    let r = run();
    assert!(r.log.mismatches.is_empty(), "{:?}", r.log.mismatches);
    assert!(r.log.compile.len() >= SHADERS.len(), "{:?}", r.log.compile);
    assert!(r.log.compile.iter().all(|v| *v as u32 == 0), "{:?}", r.log.compile);
    for ((n, a), (_, b)) in r.direct.iter().zip(&r.ours) {
        let sa: Vec<_> = kit::extents(kit::chunk(a, b"SHEX").unwrap()).into_iter().filter(|(h, _)| h & 0x3ffff == 0x10017).collect();
        let sb: Vec<_> = kit::extents(kit::chunk(b, b"SHEX").unwrap()).into_iter().filter(|(h, _)| h & 0x3ffff == 0x10017).collect();
        assert!(!sb.is_empty(), "{n}: no precompiled programs");
        assert_eq!(sa.len(), sb.len(), "{n}");
        for ((_, x), (_, y)) in sa.iter().zip(&sb) {
            assert_eq!(x, y, "{n}: compiled object differs from the direct console compile");
            let code = kit::isa_after_padding(y).unwrap_or_else(|| panic!("{n}: no GCN program ending in s_endpgm"));
            assert!(code.len() >= 3, "{n}");
        }
        let has = |e: kit::Enc| sb.iter().any(|(_, y)| kit::isa_after_padding(y).unwrap().iter().any(|c| c.0 == e));
        match *n {
            "cs" => assert!(has(kit::Enc::Mubuf) || has(kit::Enc::Mtbuf), "{n}: no buffer store"),
            _ => assert!(has(kit::Enc::Exp), "{n}: no export"),
        }
        assert_eq!(a, b, "{n}: DXBC differs from the direct console compile");
    }
}
