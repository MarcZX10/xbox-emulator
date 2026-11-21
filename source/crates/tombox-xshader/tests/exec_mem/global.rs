use super::*;
use tombox_xshader::global_abi::{fault_of, window_cb, Window, TB_GLOBAL_SPACE};

fn g(op: u32, seg: u32, off: i32, addr: u32, data: u32, saddr: u32, vdst: u32) -> [u32; 2] {
    [0xdc00_0000 | (op << 18) | (seg << 14) | (off as u32 & 0xfff), addr | (data << 8) | (saddr << 16) | (vdst << 24)]
}

const W0: Window = Window { base: 0x10_0000_0000, size: 4096 };
const W1: Window = Window { base: 0x7f10_0000_0000, size: 1024 };
const MISS: u64 = 0x99_0000_0000;

fn m1(i: usize) -> u32 {
    (i as u32).wrapping_mul(0x0101_0101) ^ 0x5a00_8000
}

fn prog() -> Vec<u32> {
    let mut p = Prog::new();
    p.sv(20, W0.base as u32).sv(21, (W0.base >> 32) as u32);
    p.sv(22, W1.base as u32).sv(23, (W1.base >> 32) as u32);
    p.sv(24, MISS as u32).sv(25, (MISS >> 32) as u32);
    p.sv(26, W0.base as u32 + 2048).sv(27, (W0.base >> 32) as u32);
    p.raw(&[vop2(0x1a, 1, ic(2), 0)]);
    p.raw(&[vop2(0x1d, 2, LIT, 0), 0xa5a5_0000]);
    p.raw(&g(0x1c, 2, 0, 1, 2, 20, 0));
    p.raw(&g(0x0c, 2, 0, 1, 0, 22, 3));
    p.raw(&[vop2(0x25, 3, ic(1), 3)]);
    p.raw(&g(0x1c, 2, 256, 1, 3, 20, 0));
    p.raw(&[vop1(1, 4, v(1)), vop1(1, 5, 23)]);
    p.raw(&g(0x0c, 0, 4, 4, 0, 0x7d, 6));
    p.raw(&g(0x1c, 2, 512, 1, 6, 20, 0));
    p.raw(&g(0x18, 2, 1024, 0, 0, 20, 0));
    p.raw(&[vop2(0x1a, 8, ic(1), 0)]);
    p.raw(&g(0x0b, 2, 0, 8, 0, 22, 7));
    p.raw(&g(0x1c, 2, 1280, 1, 7, 20, 0));
    p.raw(&g(0x0c, 2, 2, 1, 0, 22, 9));
    p.raw(&g(0x1c, 2, 1536, 1, 9, 20, 0));
    p.raw(&g(0x1a, 2, 1792, 8, 2, 20, 0));
    p.raw(&[vop2(0x1a, 10, ic(3), 0)]);
    p.raw(&g(0x1d, 2, 0, 10, 2, 26, 0));
    p.raw(&g(0x1c, 2, 0, 1, 2, 24, 0));
    let mut w = p.w;
    w.push(S_ENDPGM);
    w
}

unsafe fn run(words: &[u32]) -> Result<(Vec<u32>, Vec<u32>, [u32; 4], String), String> {
    let ex = cs_extract(words, 64);
    let t = translate(&ex)?;
    if !t.needs_global {
        return Err(format!("needs_global not set\n{}", t.hlsl));
    }
    let code = dxc::compile(&t.hlsl, &dxc::profile_for(t.profile)).map_err(|e| format!("dxc: {e}\n{}", t.hlsl))?;
    let m = CTX.get_or_init(|| Mutex::new(ctx_new()));
    let c = m.lock().unwrap_or_else(|e| e.into_inner());
    let g = &c.gpu;
    let blob = ser_ranges(&[vec![[2, 1, 0, TB_GLOBAL_SPACE, 0xffff_ffff], [1, 3, 0, TB_GLOBAL_SPACE, 0xffff_ffff]]])?;
    let mut rs: *mut c_void = core::ptr::null_mut();
    let hr = vcall!(g.device, 16, fn(u32, *const u8, usize, *const Guid, *mut *mut c_void) -> i32, 0, blob.as_ptr(), blob.len(), &d3d::IID_ROOTSIG, &mut rs);
    if hr < 0 {
        return Err(format!("CreateRootSignature {hr:#x}"));
    }
    let mut d = [0u8; 56];
    d[0..8].copy_from_slice(&(rs as u64).to_le_bytes());
    d[8..16].copy_from_slice(&(code.as_ptr() as u64).to_le_bytes());
    d[16..24].copy_from_slice(&(code.len() as u64).to_le_bytes());
    let mut pso: *mut c_void = core::ptr::null_mut();
    let hr = vcall!(g.device, 11, fn(*const u8, *const Guid, *mut *mut c_void) -> i32, d.as_ptr(), &d3d::IID_PSO, &mut pso);
    if hr < 0 {
        d3d::release(rs);
        return Err(format!("CreateComputePipelineState hr={:#010x}{}\n{}", hr as u32, g.messages(), t.hlsl));
    }
    let (vh, vcpu, vgpu, vinc) = g.heap(0, 4, true)?;
    let sizes = [W0.size as u64, W1.size as u64, 256u64];
    let init: [Vec<u32>; 3] = [vec![0; W0.size as usize / 4], (0..W1.size as usize / 4).map(m1).collect(), vec![0; 64]];
    let mut kill = vec![pso, rs, vh];
    let mut bufs = Vec::new();
    for k in 0..3 {
        let b = g.buffer(sizes[k], 1, d3d::COPY_DEST, true)?;
        let up = g.buffer(sizes[k], 2, d3d::GENERIC_READ, false)?;
        let rb = g.buffer(sizes[k], 3, d3d::COPY_DEST, false)?;
        let mut bytes: Vec<u8> = init[k].iter().flat_map(|w| w.to_le_bytes()).collect();
        bytes.resize(sizes[k] as usize, 0);
        Gpu::map_write(up, &bytes);
        let mut ud = [0u8; 40];
        ud[0..4].copy_from_slice(&39u32.to_le_bytes());
        ud[4..8].copy_from_slice(&1u32.to_le_bytes());
        ud[16..20].copy_from_slice(&((sizes[k] / 4) as u32).to_le_bytes());
        ud[32..36].copy_from_slice(&1u32.to_le_bytes());
        vcall!(g.device, 19, fn(*mut c_void, *mut c_void, *const u8, usize) -> (), b, core::ptr::null_mut(), ud.as_ptr(), vcpu + (1 + k) * vinc);
        kill.extend([b, up, rb]);
        bufs.push((b, up, rb));
    }
    let cb = g.buffer(256, 2, d3d::GENERIC_READ, false)?;
    kill.push(cb);
    let cbw = window_cb(&[W0, W1], [0, 0]);
    let mut cbd: Vec<u8> = cbw.iter().flat_map(|w| w.to_le_bytes()).collect();
    cbd.resize(256, 0);
    Gpu::map_write(cb, &cbd);
    let cv = [Gpu::gpu_va(cb), 256u64];
    vcall!(g.device, 17, fn(*const u64, usize) -> (), cv.as_ptr(), vcpu);
    drop(c);
    let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
    c.gpu.begin();
    let l = c.gpu.list;
    for (k, &(b, up, _)) in bufs.iter().enumerate() {
        vcall!(l, 15, fn(*mut c_void, u64, *mut c_void, u64, u64) -> (), b, 0, up, 0, sizes[k]);
        c.gpu.barrier(b, d3d::COPY_DEST, 8);
    }
    let heaps = [vh];
    vcall!(l, 28, fn(u32, *const *mut c_void) -> (), 1, heaps.as_ptr());
    vcall!(l, 29, fn(*mut c_void) -> (), rs);
    vcall!(l, 31, fn(u32, u64) -> (), 0, vgpu);
    vcall!(l, 25, fn(*mut c_void) -> (), pso);
    vcall!(l, 14, fn(u32, u32, u32) -> (), 1, 1, 1);
    for (k, &(b, _, rb)) in bufs.iter().enumerate() {
        c.gpu.barrier(b, 8, d3d::COPY_SOURCE);
        vcall!(l, 15, fn(*mut c_void, u64, *mut c_void, u64, u64) -> (), rb, 0, b, 0, sizes[k]);
    }
    let r = c.gpu.submit();
    let read = |k: usize| -> Vec<u32> { Gpu::map_read(bufs[k].2, sizes[k] as usize).unwrap_or_default().chunks(4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).collect() };
    let out = r.map(|_| {
        let f = read(2);
        (read(0), read(1), [f[0], f[1], f[2], f[3]], t.hlsl.clone())
    });
    for k in kill {
        d3d::release(k);
    }
    out
}

#[test]
fn global_windows_load_store_and_fault() {
    let words = prog();
    let (w0, w1, fault, hlsl) = unsafe { run(&words) }.unwrap_or_else(|e| panic!("{e}"));
    let mut bad = Vec::new();
    let mut chk = |what: &str, k: usize, got: u32, want: u32| {
        if got != want {
            bad.push(format!("{what} [{k}] {got:08x} want {want:08x}"));
        }
    };
    for t in 0..64usize {
        let tv = t as u32 ^ 0xa5a5_0000;
        chk("store_dword", t, w0[t], tv);
        chk("load_dword+1", t, w0[64 + t], m1(t).wrapping_add(1));
        chk("flat_load_dword", t, w0[128 + t], m1(t + 1));
        let h = if t % 2 == 0 { m1(t / 2) & 0xffff } else { m1(t / 2) >> 16 };
        chk("load_sshort", t, w0[320 + t], h as u16 as i16 as i32 as u32);
        chk("load_dword unaligned", t, w0[384 + t], (m1(t) >> 16) | (m1(t + 1) << 16));
        chk("store_dwordx2 lo", t, w0[512 + 2 * t], tv);
        chk("store_dwordx2 hi", t, w0[513 + 2 * t], m1(t).wrapping_add(1));
    }
    for k in 0..16usize {
        let b = (4 * k as u32) | (4 * k as u32 + 1) << 8 | (4 * k as u32 + 2) << 16 | (4 * k as u32 + 3) << 24;
        chk("store_byte", k, w0[256 + k], b);
    }
    for k in 0..32usize {
        chk("store_short", k, w0[448 + k], (2 * k as u32) | (2 * k as u32 + 1) << 16);
    }
    for (k, &x) in w1.iter().enumerate() {
        chk("window1 untouched", k, x, m1(k));
    }
    let f = fault_of(&fault);
    chk("fault count", 0, f.count, 64);
    if !(MISS..MISS + 256).contains(&f.first_va) || f.first_va % 4 != 0 {
        bad.push(format!("fault first_va {:#x}", f.first_va));
    }
    assert!(bad.is_empty(), "{}\n{hlsl}", bad.join("\n"));
}

#[test]
fn global_routing_and_refusals() {
    let isa = |w: &[u32]| -> Vec<u8> { w.iter().flat_map(|x| x.to_le_bytes()).collect() };
    for (op, m) in [(0x08u32, "flat_load_ubyte"), (0x0b, "flat_load_sshort"), (0x18, "flat_store_byte"), (0x1a, "flat_store_short"), (0x1c, "flat_store_dword"), (0x1d, "flat_store_dwordx2"), (0x1e, "flat_store_dwordx4"), (0x1f, "flat_store_dwordx3")] {
        let got = tombox_xshader::rdna2::disassemble(&isa(&g(op, 2, 0, 1, 2, 20, 3))).into_iter().next().map(|i| i.mnemonic).unwrap_or_default();
        assert_eq!(got, m);
    }
    let mut p = Prog::new();
    p.raw(&g(0x1c, 1, 0, 1, 2, 0x7d, 0));
    let mut w = p.w.clone();
    w.push(S_ENDPGM);
    let e = translate(&cs_extract(&w, 64)).err().unwrap_or_default();
    assert!(e.contains("scratch"), "{e}");
    let mut p = Prog::new();
    p.raw(&g(0x30, 2, 0, 1, 2, 20, 3));
    let mut w = p.w.clone();
    w.push(S_ENDPGM);
    let e = translate(&cs_extract(&w, 64)).err().unwrap_or_default();
    assert!(e.contains("unsupported"), "{e}");
}
