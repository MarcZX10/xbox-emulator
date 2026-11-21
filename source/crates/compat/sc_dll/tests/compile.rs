#![allow(non_snake_case)]

mod kit;

use core::ffi::c_void;
use sc_dll::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static LOG: Mutex<Vec<(&'static str, usize)>> = Mutex::new(Vec::new());

fn ours(name: &str) -> usize {
    if let Some(e) = FORWARDED.iter().find(|e| e.0 == name) {
        return e.1 as usize;
    }
    match name {
        "SC_Assemble" => SC_Assemble as *const () as usize,
        "SC_AssembleHandle_GetShader" => SC_AssembleHandle_GetShader as *const () as usize,
        "SC_AssembleHandle_Destroy" => SC_AssembleHandle_Destroy as *const () as usize,
        "SC_MetadataHandle_Destroy" => SC_MetadataHandle_Destroy as *const () as usize,
        "SC_DebugMapHandle_GetMap" => SC_DebugMapHandle_GetMap as *const () as usize,
        "SC_RegisterLifetimeHandle_GetMap" => SC_RegisterLifetimeHandle_GetMap as *const () as usize,
        _ => panic!("no emulator export {name}"),
    }
}

macro_rules! wrappers {
    ($($n:ident),*) => {
        $(
            mod $n {
                pub unsafe extern "system" fn w(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
                    let f: sc_dll::Fwd = core::mem::transmute(super::ours(stringify!($n)));
                    let r = f(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
                    super::LOG.lock().unwrap().push((stringify!($n), r));
                    r
                }
            }
        )*
        fn wrapper(name: &str) -> Option<usize> {
            match name {
                $(stringify!($n) => Some($n::w as usize),)*
                _ => None,
            }
        }
    };
}

wrappers!(
    SC_CompressExtent,
    SC_ConvertRootSignatureToSCInputs,
    SCSimpleCompileShader,
    SC_SCDX12TableDesc_Destroy,
    SC_AssembleHandle_Destroy,
    SC_CompressExtent_Destroy,
    SC_Assemble,
    SC_OutputHandle_ReleaseSerializedObjectExtent,
    SC_OutputHandle_GetSerializedObjectExtent,
    SC_MetadataHandle_Destroy,
    SC_RegisterLifetimeHandle_GetMap,
    SC_AssembleHandle_GetShader,
    SCSimpleDumpShader,
    SC_VIH_GetOutputStats,
    SC_OutputHandle_Destroy,
    SC_OutputHandle_CreateFromExtent,
    SC_DebugMapHandle_GetMap,
    SC_OutputHandle_CreateFromShaderObject,
    SC_OutputHandle_GetShaderObject
);

static UNWRAPPED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static REAL_GPA: OnceLock<usize> = OnceLock::new();

unsafe extern "system" fn gpa_hook(m: *mut c_void, name: *const u8) -> *mut c_void {
    let real: unsafe extern "system" fn(*mut c_void, *const u8) -> *mut c_void = core::mem::transmute(*REAL_GPA.get().unwrap());
    if m as usize == console::module() && name as usize > 0xffff {
        let n = std::ffi::CStr::from_ptr(name as *const core::ffi::c_char).to_string_lossy().into_owned();
        match wrapper(&n) {
            Some(w) => return w as *mut c_void,
            None => UNWRAPPED.lock().unwrap().push(n),
        }
    }
    real(m, name)
}

const VS: &str = "float4 main(float4 p:POSITION):SV_Position{return p*2;}";
const PS: &str = "Texture2D t:register(t0); SamplerState s:register(s0); cbuffer C:register(b0){float4 k;};\n[RootSignature(\"RootFlags(0), DescriptorTable(SRV(t0)), DescriptorTable(Sampler(s0)), CBV(b0)\")]\nfloat4 main(float2 uv:TEXCOORD):SV_Target{return t.Sample(s,uv)*k;}";
const CS: &str = "RWStructuredBuffer<uint> o:register(u0);\n[RootSignature(\"RootFlags(0), UAV(u0)\")]\n[numthreads(8,8,1)] void main(uint3 i:SV_DispatchThreadID){o[i.y*64+i.x]=i.x*i.y+7;}";
const VS_RS: &str = "[RootSignature(\"RootFlags(ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT)\")]\nfloat4 main(float4 p:POSITION):SV_Position{return p*2;}";

struct Fixture {
    dxbc: Vec<(&'static str, Vec<u8>)>,
    dxil: Vec<(&'static str, Vec<u8>)>,
    log: Vec<(&'static str, usize)>,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        assert!(console::module() != 0, "console sc_dll.dll not found");
        let dir = console::dir().unwrap().to_path_buf();
        assert!(kit::module("scdxil.dll") != 0 && kit::module("newbe.dll") != 0, "console scdxil/newbe siblings not preloaded");
        let d3dc = kit::load(&dir.join("d3dcompiler_47.dll"));
        assert!(d3dc != 0);
        for (name, slot) in unsafe { kit::imports(d3dc, "SC_DLL.dll") } {
            let w = wrapper(&name).unwrap_or_else(|| panic!("console d3dcompiler_47 imports unwrapped {name}"));
            unsafe { kit::patch(slot, w) };
        }
        let dxc = kit::load(&dir.join("dxcompiler_x.dll"));
        assert!(dxc != 0);
        let mut hooked = false;
        for (name, slot) in unsafe { kit::imports(dxc, "api-ms-win-core-libraryloader-l1-2-0.dll") } {
            if name == "GetProcAddress" {
                let prev = unsafe { kit::patch(slot, gpa_hook as usize) };
                let _ = REAL_GPA.set(prev);
                hooked = true;
            }
        }
        assert!(hooked);
        let mut dxbc = Vec::new();
        for (n, src, t) in [("vs", VS, "vs_5_0"), ("ps", PS, "ps_5_0"), ("cs", CS, "cs_5_0")] {
            dxbc.push((n, kit::d3dcompile(d3dc, src, t).unwrap_or_else(|e| panic!("D3DCompile {n}: {e}"))));
        }
        let mut dxil = Vec::new();
        for (n, src, t) in [("vs", VS_RS, "vs_6_0"), ("ps", PS, "ps_6_0"), ("cs", CS, "cs_6_0")] {
            dxil.push((n, kit::dxc_compile(dxc, src, t).unwrap_or_else(|e| panic!("dxcompiler_x {n}: {e}"))));
        }
        assert!(UNWRAPPED.lock().unwrap().is_empty(), "{:?}", UNWRAPPED.lock().unwrap());
        Fixture { dxbc, dxil, log: LOG.lock().unwrap().clone() }
    })
}

fn calls(name: &str) -> Vec<usize> {
    fixture().log.iter().filter(|e| e.0 == name).map(|e| e.1).collect()
}

fn stats(dxbc: &[u8], variant: i32) -> Option<[u32; 0x44]> {
    let mut s = [0u32; 0x44];
    let f: unsafe extern "system" fn(*const u8, i32, usize, *mut u32, u32) -> u8 = unsafe { core::mem::transmute(SC_GetShaderStatsFromDXBC as Fwd) };
    let ok = unsafe { f(dxbc.as_ptr(), variant, 0, s.as_mut_ptr(), 0x110) };
    (ok & 1 == 1).then_some(s)
}

#[test]
fn SCSimpleCompileShader_compiles_dxbc_to_durango_gcn() {
    let f = fixture();
    let r = calls("SCSimpleCompileShader");
    assert!(r.len() >= 6, "{r:?}");
    assert!(r.iter().all(|v| *v as u32 == 0), "{r:?}");
    for (n, b) in &f.dxbc {
        let shex = kit::chunk(b, b"SHEX").unwrap();
        let ext: Vec<_> = kit::extents(shex).into_iter().filter(|(h, _)| h & 0x3ffff == 0x10017).collect();
        assert!(!ext.is_empty(), "{n}: no precompiled extents");
        let st = stats(b, -1).unwrap();
        let code = st[8] as usize;
        assert!(code >= 8 && code % 4 == 0, "{n}: code size {code}");
        let mut found = Vec::new();
        for (_, e) in &ext {
            if let Some(isa) = kit::isa_before_endpgm(e, code) {
                found.push(isa);
            }
        }
        assert!(!found.is_empty(), "{n}: no s_endpgm-terminated GCN program of {code} bytes");
        let has = |e: kit::Enc| found.iter().any(|isa| isa.iter().any(|c| c.0 == e));
        match *n {
            "cs" => assert!(has(kit::Enc::Mubuf) || has(kit::Enc::Mtbuf), "{n}: no buffer store"),
            _ => assert!(has(kit::Enc::Exp), "{n}: no export"),
        }
    }
}

#[test]
fn SCSimpleCompileShader_compiles_dxil_to_durango_gcn() {
    let f = fixture();
    for (n, b) in &f.dxil {
        let progs = kit::dxil_programs(b).unwrap_or_else(|e| panic!("{n}: {e}"));
        for p in &progs {
            assert!(p.code.len() >= 3, "{n}: {:?}", p.code);
            assert_eq!(u32::from_le_bytes(p.object[..4].try_into().unwrap()) as usize, p.object.len(), "{n}: object size field");
        }
        let has = |e: kit::Enc| progs.iter().any(|p| p.code.iter().any(|c| c.0 == e));
        match *n {
            "cs" => assert!(has(kit::Enc::Mubuf) || has(kit::Enc::Mtbuf), "{n}: no buffer store"),
            _ => assert!(has(kit::Enc::Exp), "{n}: no export"),
        }
    }
}

#[test]
fn SC_ConvertRootSignatureToSCInputs_converts_real_root_signatures() {
    let r = calls("SC_ConvertRootSignatureToSCInputs");
    assert!(r.len() >= 4, "{r:?}");
    assert!(r.iter().all(|v| *v as u32 == 0), "{r:?}");
    assert!(!calls("SC_SCDX12TableDesc_Destroy").is_empty());
}

#[test]
fn SC_CompressExtent_packs_compiled_objects() {
    let r = calls("SC_CompressExtent");
    assert!(r.len() >= 3, "{r:?}");
    assert!(r.iter().all(|v| *v as u32 == 0), "{r:?}");
    assert!(calls("SC_OutputHandle_GetSerializedObjectExtent").iter().all(|p| *p != 0));
}

#[test]
fn SC_GetShaderStatsFromDXBC_reports_real_stats() {
    let f = fixture();
    for (n, b) in &f.dxbc {
        let s = stats(b, -1).unwrap_or_else(|| panic!("{n}"));
        assert_eq!((s[4], s[5]), (256, 512), "{n}: register limits");
        assert!(s[6] >= 1 && s[6] <= 256, "{n}: vgprs {}", s[6]);
        assert!(s[7] >= 1 && s[7] <= 512, "{n}: sgprs {}", s[7]);
        assert!(s[3] >= 1 && s[3] <= 40, "{n}: waves {}", s[3]);
        assert!(s[8] > 0 && s[8] % 4 == 0, "{n}: code size {}", s[8]);
        assert!(s[2] != 0, "{n}: unique hash");
        let mut garbage = [0u8; 64];
        garbage[..4].copy_from_slice(b"DXBC");
        assert!(stats(&garbage, -1).is_none());
    }
}

unsafe fn vih(shex: &[u8], typed: bool) -> usize {
    if typed {
        c4(SC_InstantiateTypedVIHFromDXBC, shex.as_ptr() as usize, (-1i64) as usize, (-1i64) as usize, 0)
    } else {
        c4(SC_InstantiateVIHFromDXBC, shex.as_ptr() as usize, (-1i64) as usize, 0, 0)
    }
}

unsafe fn c4(f: Fwd, a: usize, b: usize, c: usize, d: usize) -> usize {
    f(a, b, c, d, 0, 0, 0, 0, 0, 0, 0, 0)
}

fn check_vih(h: usize, dxbc: &[u8], n: &str) {
    unsafe {
        assert!(h != 0, "{n}");
        let st = stats(dxbc, -1).unwrap();
        assert_eq!(c4(SC_VIH_GetUniqueHash, h, 0, 0, 0) as u32, st[2], "{n}: unique hash");
        let mut flags = [0u8; 16];
        assert_eq!(c4(SC_VIH_GetNamedFlags, h, flags.as_mut_ptr() as usize, 0, 0) as u8, 1, "{n}");
        let mut cs = [0u8; 32];
        assert_eq!(c4(SC_VIH_GetCompileSettings, h, cs.as_mut_ptr() as usize, 0, 0) as u8, 1, "{n}");
        assert_eq!(c4(SC_VIH_GetPDBNameLengthInBytes, h, 0, 0, 0) as u32, 0, "{n}: no PDB section in a non-debug build");
        c4(SC_VIH_CloseVIH, h, 0, 0, 0);
    }
}

#[test]
fn SC_InstantiateVIHFromDXBC_reads_compiled_metadata() {
    for (n, b) in &fixture().dxbc {
        let shex = kit::chunk(b, b"SHEX").unwrap();
        check_vih(unsafe { vih(shex, false) }, b, n);
    }
}

#[test]
fn SC_InstantiateTypedVIHFromDXBC_reads_compiled_metadata() {
    for (n, b) in &fixture().dxbc {
        let shex = kit::chunk(b, b"SHEX").unwrap();
        check_vih(unsafe { vih(shex, true) }, b, n);
    }
}

#[test]
fn compile_paths_only_use_emulator_exports() {
    let f = fixture();
    let mut by: HashMap<&str, usize> = HashMap::new();
    for (n, _) in &f.log {
        *by.entry(n).or_default() += 1;
    }
    for n in ["SC_OutputHandle_CreateFromShaderObject", "SC_OutputHandle_Destroy", "SC_OutputHandle_GetShaderObject"] {
        assert!(by.get(n).copied().unwrap_or(0) > 0, "{n} not reached: {by:?}");
    }
}
