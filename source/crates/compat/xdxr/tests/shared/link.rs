use dxcompiler_xs::com::{release, Hr, S_OK};
use dxcompiler_xs::guids::{Guid, CLSID_DXC_COMPILER, CLSID_XDXR_COMPILER, IID_IDXC_COMPILER3, IID_IDXC_RESULT, IID_IXDXR_COMPILER};

#[repr(C)]
struct Lib {
    ptr: *const u8,
    size: u32,
    pad: u32,
}

#[repr(C)]
struct DxcBuffer {
    ptr: *const u8,
    size: usize,
    encoding: u32,
}

type LinkFn = unsafe extern "system" fn(*mut c_void, *const Lib, u32, *const *const u16, *mut u64, u32, u32, *mut *mut c_void) -> Hr;
type SetU32Fn = unsafe extern "system" fn(*mut c_void, u32) -> Hr;
type SetProviderFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32);
type GetFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hr;
type StatusFn = unsafe extern "system" fn(*mut c_void, *mut Hr) -> Hr;
type PtrFn = unsafe extern "system" fn(*mut c_void) -> *const u8;
type SizeFn = unsafe extern "system" fn(*mut c_void) -> usize;
type CreateFn = unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> Hr;
type Create2Fn = unsafe extern "system" fn(*mut c_void, *const Guid, *const Guid, *mut *mut c_void) -> Hr;
type Compile3Fn = unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const *const u16, u32, *mut c_void, *const Guid, *mut *mut c_void) -> Hr;

const RAYTRACING_LIBRARY: &str = r#"
RaytracingAccelerationStructure scene : register(t0);
RWTexture2D<float4> output : register(u0);
struct Payload { float4 color; };
[shader("raygeneration")]
void RayGen()
{
    RayDesc ray;
    ray.Origin = float3(DispatchRaysIndex().xy, -1.0);
    ray.Direction = float3(0.0, 0.0, 1.0);
    ray.TMin = 0.0;
    ray.TMax = 100.0;
    Payload p;
    p.color = float4(0.0, 0.0, 0.0, 0.0);
    TraceRay(scene, RAY_FLAG_NONE, 0xFF, 0, 1, 0, ray, p);
    output[DispatchRaysIndex().xy] = p.color;
}
[shader("miss")]
void Miss(inout Payload p)
{
    p.color = float4(0.0, 0.0, 1.0, 1.0);
}
[shader("closesthit")]
void Hit(inout Payload p, in BuiltInTriangleIntersectionAttributes a)
{
    p.color = float4(a.barycentrics, 0.0, 1.0);
}
"#;

const EXPORTS: [&str; 3] = ["RayGen", "Miss", "Hit"];

unsafe fn slot<T: Copy>(obj: *mut c_void, i: usize) -> T {
    let f = *(*(obj as *const *const usize)).add(i);
    core::mem::transmute_copy(&f)
}

unsafe fn blob_bytes(blob: *mut c_void) -> Vec<u8> {
    let p = slot::<PtrFn>(blob, 3)(blob);
    let n = slot::<SizeFn>(blob, 4)(blob);
    core::slice::from_raw_parts(p, n).to_vec()
}

fn console_dxcompiler() -> &'static console_dll::Console {
    let c = &dxcompiler_xs::xdxr::CONSOLE;
    assert!(c.module() != 0, "console dxcompiler_xs.dll did not load: {:?}", c.candidates());
    c
}

unsafe fn compile_library(src: &str) -> Vec<u8> {
    let create: CreateFn = core::mem::transmute(console_dxcompiler().proc("DxcCreateInstance").unwrap());
    let mut c = core::ptr::null_mut();
    assert_eq!(create(&CLSID_DXC_COMPILER, &IID_IDXC_COMPILER3, &mut c), S_OK);
    let args: Vec<Vec<u16>> = ["-T", "lib_6_3", "-HV", "2021"].iter().map(|a| wide(a)).collect();
    let argv: Vec<*const u16> = args.iter().map(|a| a.as_ptr()).collect();
    let buf = DxcBuffer { ptr: src.as_ptr(), size: src.len(), encoding: 65001 };
    let mut r = core::ptr::null_mut();
    assert_eq!(slot::<Compile3Fn>(c, 3)(c, &buf, argv.as_ptr(), argv.len() as u32, core::ptr::null_mut(), &IID_IDXC_RESULT, &mut r), S_OK);
    let mut status = -1;
    assert_eq!(slot::<StatusFn>(r, 3)(r, &mut status), S_OK);
    if status != 0 {
        let mut e = core::ptr::null_mut();
        slot::<GetFn>(r, 5)(r, &mut e);
        panic!("console dxcompiler_xs lib_6_3 compile {status:#x}: {}", String::from_utf8_lossy(&blob_bytes(e)));
    }
    let mut b = core::ptr::null_mut();
    assert_eq!(slot::<GetFn>(r, 4)(r, &mut b), S_OK);
    let out = blob_bytes(b);
    release(b);
    release(r);
    release(c);
    assert_eq!(&out[..4], b"DXBC");
    out
}

fn container_parts(blob: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let n = u32::from_le_bytes(blob[28..32].try_into().unwrap()) as usize;
    (0..n)
        .map(|i| {
            let o = u32::from_le_bytes(blob[32 + 4 * i..36 + 4 * i].try_into().unwrap()) as usize;
            let size = u32::from_le_bytes(blob[o + 4..o + 8].try_into().unwrap()) as usize;
            (blob[o..o + 4].try_into().unwrap(), o + 8, size)
        })
        .collect()
}

struct Linked {
    hr: Hr,
    outs: Vec<u64>,
    code: Vec<u8>,
}

unsafe fn xdxr_link_with(provider: usize, mode: u32, libs: &[&[u8]], names: &[&str], flags: u32) -> Linked {
    let create: Create2Fn = core::mem::transmute(console_dxcompiler().proc("CreateXdxrCompiler2").unwrap());
    let mut x = core::ptr::null_mut();
    assert_eq!(create(core::ptr::null_mut(), &CLSID_XDXR_COMPILER, &IID_IXDXR_COMPILER, &mut x), S_OK);
    slot::<SetProviderFn>(x, 10)(x, provider as *mut c_void, mode);
    assert_eq!(slot::<SetU32Fn>(x, 5)(x, 0), S_OK);
    let entries: Vec<Lib> = libs.iter().map(|l| Lib { ptr: l.as_ptr(), size: l.len() as u32, pad: 0 }).collect();
    let wnames: Vec<Vec<u16>> = names.iter().map(|n| wide(n)).collect();
    let pnames: Vec<*const u16> = wnames.iter().map(|n| n.as_ptr()).collect();
    let mut outs = vec![0u64; names.len()];
    let mut result = core::ptr::null_mut();
    let hr = slot::<LinkFn>(x, 8)(x, entries.as_ptr(), entries.len() as u32, pnames.as_ptr(), outs.as_mut_ptr(), names.len() as u32, flags, &mut result);
    let mut code = Vec::new();
    if hr >= 0 && !result.is_null() {
        let mut b = core::ptr::null_mut();
        let got = slot::<GetFn>(result, 4)(result, &mut b);
        assert_eq!(got, S_OK, "link result GetResult {got:#x}");
        code = blob_bytes(b);
        release(b);
    }
    if !result.is_null() {
        release(result);
    }
    release(x);
    Linked { hr, outs, code }
}
