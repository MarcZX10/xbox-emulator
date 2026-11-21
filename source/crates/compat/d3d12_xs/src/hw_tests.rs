use core::ffi::c_void;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::device_wrap::Guid;

const IID_DEVICE: Guid = Guid { d1: 0x1898_19f1, d2: 0x1db6, d3: 0x4b57, d4: [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7] };
const IID_QUEUE: Guid = Guid { d1: 0x0ec8_70a6, d2: 0x5d7e, d3: 0x4c22, d4: [0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed] };
const IID_ALLOCATOR: Guid = Guid { d1: 0x6102_dee4, d2: 0xaf59, d3: 0x4b09, d4: [0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24] };
const IID_LIST: Guid = Guid { d1: 0x5b16_0d0f, d2: 0xac1b, d3: 0x4185, d4: [0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55] };
const IID_RESOURCE: Guid = Guid { d1: 0x6964_42be, d2: 0xa72e, d3: 0x4059, d4: [0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad] };
const IID_FENCE: Guid = Guid { d1: 0x0a75_3dcf, d2: 0xc4d8, d3: 0x4b91, d4: [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76] };
const IID_ROOT_SIGNATURE: Guid = Guid { d1: 0xc54a_6b66, d2: 0x72df, d3: 0x4ee8, d4: [0x8b, 0xe5, 0xa9, 0x46, 0xa1, 0x42, 0x92, 0x14] };
const IID_PSO: Guid = Guid { d1: 0x765a_30f3, d2: 0xf624, d3: 0x4c6f, d4: [0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45] };
const IID_FACTORY4: Guid = Guid { d1: 0x1bc6_ea02, d2: 0xef36, d3: 0x464f, d4: [0xbf, 0x0c, 0x21, 0xca, 0x39, 0xe5, 0x16, 0x8a] };
const IID_ADAPTER: Guid = Guid { d1: 0x2411_e7e1, d2: 0x12ac, d3: 0x4ccf, d4: [0xbd, 0x14, 0x97, 0x98, 0xe8, 0x53, 0x4d, 0xc0] };

const ST_COMMON: u32 = 0;
const ST_COPY_DEST: u32 = 0x400;
const ST_COPY_SOURCE: u32 = 0x800;
const ST_GENERIC_READ: u32 = 0xac3;
const HEAP_DEFAULT: u32 = 1;
const HEAP_UPLOAD: u32 = 2;
const HEAP_READBACK: u32 = 3;
const FLAG_UAV: u32 = 4;
const FUNC_EQUAL: u32 = 3;
const FUNC_GREATER_EQUAL: u32 = 7;

const X_CLOSE: usize = 9;
const X_DISPATCH: usize = 14;
const X_COPY_BUFFER_REGION: usize = 15;
const X_SET_PSO: usize = 25;
const X_BARRIER: usize = 26;
const X_SET_COMPUTE_ROOT_SIGNATURE: usize = 33;
const X_SET_COMPUTE_ROOT_CONSTANT: usize = 37;
const PC_SET_COMPUTE_ROOT_SIGNATURE: usize = 29;
const PC_SET_COMPUTE_ROOT_UAV: usize = 41;

extern "system" {
    fn QueryPerformanceCounter(v: *mut i64) -> i32;
    fn QueryPerformanceFrequency(v: *mut i64) -> i32;
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
}

unsafe fn vs(o: *mut c_void, i: usize) -> usize {
    unsafe { *(*(o as *const *const usize)).add(i) }
}

macro_rules! call {
    ($o:expr, $i:expr, ($($t:ty),*) -> $r:ty $(, $a:expr)*) => {{
        let o = $o as *mut c_void;
        let f: unsafe extern "system" fn(*mut c_void $(, $t)*) -> $r = core::mem::transmute(vs(o, $i));
        f(o $(, $a)*)
    }};
}

struct Gpu {
    dev: usize,
    a: usize,
    b: usize,
    warp: bool,
}

unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

fn adapter_is_warp() -> bool {
    std::env::var("TOMBOX_HW_ADAPTER").map(|v| v.eq_ignore_ascii_case("warp")).unwrap_or(false)
}

unsafe fn warp_adapter() -> *mut c_void {
    let name: Vec<u16> = "dxgi.dll\0".encode_utf16().collect();
    let h = unsafe { LoadLibraryW(name.as_ptr()) };
    assert!(!h.is_null());
    let p = unsafe { GetProcAddress(h, b"CreateDXGIFactory2\0".as_ptr()) };
    assert!(!p.is_null());
    let create: unsafe extern "system" fn(u32, *const Guid, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(p) };
    let mut f: *mut c_void = core::ptr::null_mut();
    assert!(unsafe { create(0, &IID_FACTORY4, &mut f) } >= 0);
    let mut a: *mut c_void = core::ptr::null_mut();
    assert!(call!(f, 27, (*const Guid, *mut *mut c_void) -> i32, &IID_ADAPTER, &mut a) >= 0);
    a
}

unsafe fn queue(dev: *mut c_void) -> usize {
    let desc = [0u32; 4];
    let mut q: *mut c_void = core::ptr::null_mut();
    assert!(call!(dev, 8, (*const u32, *const Guid, *mut *mut c_void) -> i32, desc.as_ptr(), &IID_QUEUE, &mut q) >= 0);
    let f16 = unsafe { vs(q, 16) };
    let f17 = unsafe { vs(q, 17) };
    crate::sys_ts::note_queue(q, f16, f17);
    crate::present::apply_queue_vtable(q);
    q as usize
}

fn gpu() -> &'static Gpu {
    static G: OnceLock<Gpu> = OnceLock::new();
    G.get_or_init(|| unsafe {
        crate::enable_experimental_shader_models();
        let warp = adapter_is_warp();
        let adapter = if warp { warp_adapter() } else { core::ptr::null_mut() };
        let p = crate::host_proc(b"D3D12CreateDevice\0");
        let create: extern "system" fn(*mut c_void, u32, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(p);
        let mut dev: *mut c_void = core::ptr::null_mut();
        let hr = create(adapter, 0xb000, &IID_DEVICE, &mut dev);
        assert!(hr >= 0 && !dev.is_null(), "D3D12CreateDevice ({}) hr={hr:#010x}", if warp { "WARP" } else { "hardware" });
        Gpu { dev: dev as usize, a: queue(dev), b: queue(dev), warp }
    })
}

struct Buf {
    res: usize,
    va: u64,
    cpu: *mut u8,
}

unsafe fn buffer(dev: usize, heap: u32, size: u64, flags: u32, state: u32) -> Buf {
    let h: [u32; 5] = [heap, 0, 0, 1, 1];
    let mut d = [0u8; 56];
    d[0..4].copy_from_slice(&1u32.to_le_bytes());
    d[0x10..0x18].copy_from_slice(&size.to_le_bytes());
    d[0x18..0x1c].copy_from_slice(&1u32.to_le_bytes());
    d[0x1c..0x1e].copy_from_slice(&1u16.to_le_bytes());
    d[0x1e..0x20].copy_from_slice(&1u16.to_le_bytes());
    d[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
    d[0x2c..0x30].copy_from_slice(&1u32.to_le_bytes());
    d[0x30..0x34].copy_from_slice(&flags.to_le_bytes());
    let mut r: *mut c_void = core::ptr::null_mut();
    let hr = call!(dev, 27, (*const u32, u32, *const u8, u32, *const c_void, *const Guid, *mut *mut c_void) -> i32,
        h.as_ptr(), 0, d.as_ptr(), state, core::ptr::null(), &IID_RESOURCE, &mut r);
    assert!(hr >= 0, "CreateCommittedResource hr={hr:#010x}");
    let va = call!(r, 11, () -> u64);
    crate::gpu_va::insert(crate::gpu_va::Entry { start: va, len: size, res: r as usize, buffer: true });
    crate::gpu_va::set_state(r as usize, state);
    let mut cpu: *mut u8 = core::ptr::null_mut();
    if heap != HEAP_DEFAULT {
        assert!(call!(r, 8, (u32, *const c_void, *mut *mut u8) -> i32, 0, core::ptr::null(), &mut cpu) >= 0);
    }
    Buf { res: r as usize, va, cpu }
}

unsafe fn raw_list(dev: usize) -> *mut c_void {
    let mut a: *mut c_void = core::ptr::null_mut();
    assert!(call!(dev, 9, (u32, *const Guid, *mut *mut c_void) -> i32, 0, &IID_ALLOCATOR, &mut a) >= 0);
    let mut l: *mut c_void = core::ptr::null_mut();
    assert!(call!(dev, 12, (u32, u32, *mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32, 0, 0, a, core::ptr::null_mut(), &IID_LIST, &mut l) >= 0);
    l
}

unsafe fn title_list(dev: usize) -> *mut c_void {
    crate::cmdlist_wrap::wrap(unsafe { raw_list(dev) })
}

unsafe fn fence(dev: usize) -> *mut c_void {
    let mut f: *mut c_void = core::ptr::null_mut();
    assert!(call!(dev, 36, (u64, u32, *const Guid, *mut *mut c_void) -> i32, 0, 0, &IID_FENCE, &mut f) >= 0);
    f
}

unsafe fn submit(q: usize, list: *mut c_void, f: *mut c_void, v: u64) {
    let l = [list];
    call!(q, 10, (u32, *const *mut c_void) -> (), 1, l.as_ptr());
    call!(q, 14, (*mut c_void, u64) -> i32, f, v);
}

fn reached(f: *mut c_void, v: u64, within: Duration) -> bool {
    let t = Instant::now();
    loop {
        if unsafe { call!(f, 8, () -> u64) } >= v {
            return true;
        }
        if t.elapsed() > within {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

unsafe fn rd(b: &Buf, off: usize) -> u32 {
    unsafe { core::ptr::read_volatile(b.cpu.add(off) as *const u32) }
}

unsafe fn compute_pso(dev: usize, hlsl: &str, rs: Option<*mut c_void>) -> (*mut c_void, *mut c_void) {
    let code = tombox_xshader::dxc::compile(hlsl, "cs_6_0").expect("dxc");
    let rs = rs.unwrap_or_else(|| {
        let mut r: *mut c_void = core::ptr::null_mut();
        assert!(call!(dev, 16, (u32, *const u8, usize, *const Guid, *mut *mut c_void) -> i32, 0, code.as_ptr(), code.len(), &IID_ROOT_SIGNATURE, &mut r) >= 0);
        r
    });
    let mut d = [0u8; 56];
    d[0..8].copy_from_slice(&(rs as u64).to_le_bytes());
    d[8..16].copy_from_slice(&(code.as_ptr() as u64).to_le_bytes());
    d[16..24].copy_from_slice(&(code.len() as u64).to_le_bytes());
    let mut pso: *mut c_void = core::ptr::null_mut();
    let hr = call!(dev, 11, (*const u8, *const Guid, *mut *mut c_void) -> i32, d.as_ptr(), &IID_PSO, &mut pso);
    assert!(hr >= 0, "CreateComputePipelineState hr={hr:#010x}");
    (rs, pso)
}

fn chain_len(shim: *mut c_void) -> usize {
    crate::list_split::expand(&[crate::cmdlist_wrap::unwrap(shim)]).len()
}

unsafe fn waiter(g: &Gpu, flag: &Buf, func: u32, value: u32, out: &Buf) -> *mut c_void {
    let w = unsafe { title_list(g.dev) };
    call!(w, crate::cl_ext::CL_WRITE32_BOTTOM_OF_PIPE, (u64, u32, u32, u32) -> (), out.va, 1, 0, 0);
    call!(w, crate::cl_ext::CL_WAIT32, (u64, u32, u32, u32, u32) -> (), flag.va, func, value, 0xffff_ffff, 0);
    call!(w, X_COPY_BUFFER_REGION, (usize, u64, usize, u64, u64) -> (), out.res, 8, flag.res, 0, 4);
    call!(w, crate::cl_ext::CL_WRITE32_BOTTOM_OF_PIPE, (u64, u32, u32, u32) -> (), out.va + 16, 2, 0, 0);
    assert!(call!(w, X_CLOSE, () -> i32) >= 0);
    w
}

#[test]
#[ignore = "needs a D3D12 device; run: cargo test -p d3d12_xs --lib hw_ -- --ignored --test-threads=1 (TOMBOX_HW_ADAPTER=warp for WARP)"]
fn hw_mid_list_wait_orders_after_a_value_written_by_another_list() {
    let g = gpu();
    unsafe {
        let flag = buffer(g.dev, HEAP_DEFAULT, 256, FLAG_UAV, ST_COMMON);
        let out = buffer(g.dev, HEAP_READBACK, 256, 0, ST_COPY_DEST);
        let w = waiter(g, &flag, FUNC_GREATER_EQUAL, 42, &out);
        assert_eq!(chain_len(w), 2, "the wait did not split the list");
        let fa = fence(g.dev);
        submit(g.a, w, fa, 1);
        assert!(!reached(fa, 1, Duration::from_millis(300)), "the waiter ran past the wait before the value was written");
        assert_eq!(rd(&out, 0), 1, "the part before the wait did not execute");
        assert_eq!(rd(&out, 16), 0);
        let p = title_list(g.dev);
        call!(p, crate::cl_ext::CL_WRITE32_BOTTOM_OF_PIPE, (u64, u32, u32, u32) -> (), flag.va, 42, 0, 0);
        assert!(call!(p, X_CLOSE, () -> i32) >= 0);
        let fb = fence(g.dev);
        submit(g.b, p, fb, 1);
        assert!(reached(fb, 1, Duration::from_secs(5)));
        assert!(reached(fa, 1, Duration::from_secs(5)), "the waiter never resumed after the producer wrote the value");
        assert_eq!((rd(&out, 0), rd(&out, 8), rd(&out, 16)), (1, 42, 2));
    }
}

#[test]
#[ignore = "needs a D3D12 device; run: cargo test -p d3d12_xs --lib hw_ -- --ignored --test-threads=1 (TOMBOX_HW_ADAPTER=warp for WARP)"]
fn hw_mid_list_wait_after_its_producer_was_submitted_uses_the_address_fence() {
    let g = gpu();
    unsafe {
        let flag = buffer(g.dev, HEAP_DEFAULT, 256, FLAG_UAV, ST_COMMON);
        let out = buffer(g.dev, HEAP_READBACK, 256, 0, ST_COPY_DEST);
        let p = title_list(g.dev);
        call!(p, crate::cl_ext::CL_WRITE32_BOTTOM_OF_PIPE, (u64, u32, u32, u32) -> (), flag.va, 42, 0, 0);
        assert!(call!(p, X_CLOSE, () -> i32) >= 0);
        let fb = fence(g.dev);
        submit(g.b, p, fb, 1);
        let w = waiter(g, &flag, FUNC_GREATER_EQUAL, 42, &out);
        assert_eq!(chain_len(w), 2, "the wait did not split the list");
        let before = crate::wait_probe::armed_total();
        let fa = fence(g.dev);
        submit(g.a, w, fa, 1);
        assert_eq!(crate::wait_probe::armed_total(), before, "a probe was armed although the producing write was already submitted");
        assert!(reached(fb, 1, Duration::from_secs(5)));
        assert!(reached(fa, 1, Duration::from_secs(5)), "the waiter never resumed");
        assert_eq!((rd(&out, 0), rd(&out, 8), rd(&out, 16)), (1, 42, 2));
    }
}

#[test]
#[ignore = "needs a D3D12 device; run: cargo test -p d3d12_xs --lib hw_ -- --ignored --test-threads=1 (TOMBOX_HW_ADAPTER=warp for WARP)"]
fn hw_mid_list_wait_on_a_shader_written_default_heap_value() {
    let g = gpu();
    unsafe {
        let flag = buffer(g.dev, HEAP_DEFAULT, 256, FLAG_UAV, ST_COMMON);
        let out = buffer(g.dev, HEAP_READBACK, 256, 0, ST_COPY_DEST);
        let w = waiter(g, &flag, FUNC_EQUAL, 77, &out);
        assert_eq!(chain_len(w), 2);
        let fa = fence(g.dev);
        submit(g.a, w, fa, 1);
        assert!(!reached(fa, 1, Duration::from_millis(300)), "the waiter ran past a wait on a value no shader had written yet");
        assert_eq!((rd(&out, 0), rd(&out, 16)), (1, 0));
        let (rs, pso) = compute_pso(g.dev, "#define RS \"UAV(u0)\"\nRWByteAddressBuffer o : register(u0);\n[RootSignature(RS)]\n[numthreads(1, 1, 1)]\nvoid main() { o.Store(0, 77u); }\n", None);
        let p = raw_list(g.dev);
        call!(p, PC_SET_COMPUTE_ROOT_SIGNATURE, (*mut c_void) -> (), rs);
        call!(p, X_SET_PSO, (*mut c_void) -> (), pso);
        call!(p, PC_SET_COMPUTE_ROOT_UAV, (u32, u64) -> (), 0, flag.va);
        call!(p, X_DISPATCH, (u32, u32, u32) -> (), 1, 1, 1);
        assert!(call!(p, X_CLOSE, () -> i32) >= 0);
        let fb = fence(g.dev);
        submit(g.b, p, fb, 1);
        assert!(reached(fb, 1, Duration::from_secs(5)));
        assert!(reached(fa, 1, Duration::from_secs(5)), "the waiter never resumed after the shader wrote the value");
        assert_eq!((rd(&out, 0), rd(&out, 8), rd(&out, 16)), (1, 77, 2));
    }
}

#[test]
#[ignore = "needs a D3D12 device; run: cargo test -p d3d12_xs --lib hw_ -- --ignored --test-threads=1 (TOMBOX_HW_ADAPTER=warp for WARP)"]
fn hw_gds_round_trip_through_the_shader_binding() {
    let g = gpu();
    unsafe {
        let mut param = [0u8; 32];
        param[0..4].copy_from_slice(&1u32.to_le_bytes());
        param[16..20].copy_from_slice(&1u32.to_le_bytes());
        let mut desc = [0u8; 40];
        desc[0..4].copy_from_slice(&1u32.to_le_bytes());
        desc[8..16].copy_from_slice(&(param.as_ptr() as u64).to_le_bytes());
        let (mut blob, mut err): (*mut c_void, *mut c_void) = (core::ptr::null_mut(), core::ptr::null_mut());
        assert!(crate::D3D12SerializeRootSignature(desc.as_ptr() as *const c_void, 1, &mut blob, &mut err) >= 0);
        let bp = call!(blob, 3, () -> *const u8);
        let bl = call!(blob, 4, () -> usize);
        let (patched, idx) = crate::gds::patch_root_signature(bp, bl).expect("GDS root parameter not appended");
        assert_eq!(idx, 1);
        let mut rs: *mut c_void = core::ptr::null_mut();
        assert!(call!(g.dev, 16, (u32, *const u8, usize, *const Guid, *mut *mut c_void) -> i32, 0, patched.as_ptr(), patched.len(), &IID_ROOT_SIGNATURE, &mut rs) >= 0);
        crate::gds::note_root_signature(rs, idx);
        let hlsl = format!("{}\ncbuffer C : register(b0) {{ uint add; }};\n[numthreads(64, 1, 1)]\nvoid main() {{ uint o; tb_gds.InterlockedAdd(16, add, o); }}\n", tombox_xshader::gds_abi::TB_GDS_HLSL_DECL);
        let (_, pso) = compute_pso(g.dev, &hlsl, Some(rs));
        let src = buffer(g.dev, HEAP_UPLOAD, 256, 0, ST_GENERIC_READ);
        core::ptr::write_volatile(src.cpu as *mut u32, 1000);
        core::ptr::write_volatile(src.cpu.add(4) as *mut u32, 5);
        let dst = buffer(g.dev, HEAP_DEFAULT, 256, 0, ST_COPY_DEST);
        let out = buffer(g.dev, HEAP_READBACK, 256, 0, ST_COPY_DEST);
        let l = title_list(g.dev);
        call!(l, crate::cl_ext::CL_WRITE_GDS, (u32, u32, u64, u32) -> (), 4, 2, src.va, 0);
        call!(l, X_SET_COMPUTE_ROOT_SIGNATURE, (*mut c_void) -> (), rs);
        call!(l, X_SET_PSO, (*mut c_void) -> (), pso);
        call!(l, X_SET_COMPUTE_ROOT_CONSTANT, (u32, u32, u32) -> (), 0, 3, 0);
        call!(l, X_DISPATCH, (u32, u32, u32) -> (), 4, 1, 1);
        call!(l, crate::cl_ext::CL_READ_GDS, (u32, u32, u64, u32) -> (), 4, 2, dst.va, 0);
        let mut b = [0u8; 32];
        b[8..16].copy_from_slice(&(dst.res as u64).to_le_bytes());
        b[16..20].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        b[20..24].copy_from_slice(&ST_COPY_DEST.to_le_bytes());
        b[24..28].copy_from_slice(&ST_COPY_SOURCE.to_le_bytes());
        call!(l, X_BARRIER, (u32, *const u8) -> (), 1, b.as_ptr());
        call!(l, X_COPY_BUFFER_REGION, (usize, u64, usize, u64, u64) -> (), out.res, 0, dst.res, 0, 8);
        assert!(call!(l, X_CLOSE, () -> i32) >= 0);
        let f = fence(g.dev);
        submit(g.a, l, f, 1);
        assert!(reached(f, 1, Duration::from_secs(5)));
        assert_eq!((rd(&out, 0), rd(&out, 4)), (1000 + 256 * 3, 5));
    }
}

#[test]
#[ignore = "needs a D3D12 device; run: cargo test -p d3d12_xs --lib hw_ -- --ignored --test-threads=1 (TOMBOX_HW_ADAPTER=warp for WARP)"]
fn hw_system_timestamp_is_monotonic_and_close_to_qpc() {
    let g = gpu();
    unsafe {
        let out = buffer(g.dev, HEAP_READBACK, 64, 0, ST_COPY_DEST);
        let l = title_list(g.dev);
        call!(l, crate::cl_ext::CL_WRITE64_BOTTOM_OF_PIPE, (u64, u64, u32, u32) -> (), out.va, 0, 0, crate::cl_ext::BOP_FLAG_SYSTEM_TIMESTAMP);
        call!(l, crate::cl_ext::CL_WRITE64_BOTTOM_OF_PIPE, (u64, u64, u32, u32) -> (), out.va + 8, 0, 0, crate::cl_ext::BOP_FLAG_SYSTEM_TIMESTAMP);
        assert!(call!(l, X_CLOSE, () -> i32) >= 0);
        let (mut t0, mut t1, mut qpf) = (0i64, 0i64, 0i64);
        QueryPerformanceFrequency(&mut qpf);
        let f = fence(g.dev);
        QueryPerformanceCounter(&mut t0);
        submit(g.a, l, f, 1);
        assert!(reached(f, 1, Duration::from_secs(5)));
        QueryPerformanceCounter(&mut t1);
        let v0 = core::ptr::read_volatile(out.cpu as *const u64) as i64;
        let v1 = core::ptr::read_volatile(out.cpu.add(8) as *const u64) as i64;
        let tol = qpf / 500;
        assert!(v0 != 0 && v1 != 0, "SYSTEM_TIMESTAMP not written");
        assert!(v0 <= v1, "not monotonic: {v0} > {v1}");
        assert!(v0 >= t0 - tol && v1 <= t1 + tol, "{} QPC window [{t0}, {t1}] (tol {tol}) but timestamps {v0}, {v1}", if g.warp { "WARP" } else { "hardware" });
    }
}
