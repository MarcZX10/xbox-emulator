use core::ffi::c_void;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use compat_common::record;

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid(u32, u16, u16, [u8; 8]);
const IID_DEVICE: Guid = Guid(0x189819f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);
const IID_PSO: Guid = Guid(0x765a30f3, 0xf624, 0x4c6f, [0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45]);

#[derive(Default)]
struct ListState {
    pso: usize,
    root_sig: usize,
    rtv: Vec<usize>,
    rt_count: u32,
    single: bool,
    dsv: Option<usize>,
    viewports: Vec<[f32; 6]>,
    scissors: Vec<[i32; 4]>,
    topo: Option<u32>,
    dirty: Vec<(usize, u32, u32)>,
}

static LISTS: Mutex<Option<HashMap<usize, ListState>>> = Mutex::new(None);
static PSOS: Mutex<Option<HashMap<(usize, u32, u32, bool), usize>>> = Mutex::new(None);
static RTV_INC: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static PASSES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

static LIVE: crate::live_sw::Armed = crate::live_sw::Armed::new("TOMBOX_BLEND_CLAMP", "blend_clamp", true);

pub fn enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    E.get_or_init(|| {
        let on = LIVE.armed();
        record("d3d12_xs", "blendclamp", if on {
            "small-float blend results clamp to the format's largest finite value (console round-toward-zero output conversion); live.cfg blend_clamp=0/1 toggles it"
        } else {
            "disabled by TOMBOX_BLEND_CLAMP=0"
        });
        on
    });
    static WAS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
    let on = LIVE.on();
    if WAS.swap(on, core::sync::atomic::Ordering::Relaxed) != on && on {
        *LISTS.lock().unwrap() = None;
    }
    on
}

pub fn passes() -> u64 {
    PASSES.load(core::sync::atomic::Ordering::Relaxed)
}

fn small_float(fmt: u32) -> Option<bool> {
    match fmt {
        26 => Some(false),
        2 | 10 | 34 | 54 => Some(true),
        _ => None,
    }
}

fn with<R>(list: usize, f: impl FnOnce(&mut ListState) -> R) -> R {
    let mut g = LISTS.lock().unwrap();
    f(g.get_or_insert_with(HashMap::new).entry(list).or_default())
}

pub fn on_reset(list: usize) {
    if !enabled() {
        return;
    }
    let mut g = LISTS.lock().unwrap();
    if let Some(m) = g.as_mut() {
        m.remove(&list);
    }
}

pub fn on_set_pso(list: usize, pso: usize) {
    if enabled() {
        with(list, |s| s.pso = pso);
    }
}

pub fn on_set_root_sig(list: usize, rs: usize) {
    if enabled() {
        with(list, |s| s.root_sig = rs);
    }
}

pub fn on_topology(list: usize, t: u32) {
    if enabled() {
        with(list, |s| s.topo = Some(t));
    }
}

pub unsafe fn on_viewports(list: usize, n: u32, p: *const [f32; 6]) {
    if !enabled() || p.is_null() {
        return;
    }
    let v = unsafe { core::slice::from_raw_parts(p, (n as usize).min(16)) }.to_vec();
    with(list, |s| s.viewports = v);
}

pub unsafe fn on_scissors(list: usize, n: u32, p: *const [i32; 4]) {
    if !enabled() || p.is_null() {
        return;
    }
    let v = unsafe { core::slice::from_raw_parts(p, (n as usize).min(16)) }.to_vec();
    with(list, |s| s.scissors = v);
}

pub unsafe fn on_set_targets(list: usize, n: u32, handles: *const usize, single: bool, dsv: *const usize) {
    if !enabled() {
        return;
    }
    let n = n.min(8);
    let rtv = if handles.is_null() || n == 0 {
        Vec::new()
    } else if single {
        vec![unsafe { *handles }]
    } else {
        unsafe { core::slice::from_raw_parts(handles, n as usize) }.to_vec()
    };
    let d = if dsv.is_null() { None } else { Some(unsafe { *dsv }) };
    with(list, |s| {
        s.rtv = rtv;
        s.rt_count = n;
        s.single = single;
        s.dsv = d;
    });
}

pub fn on_draw(list: usize) {
    if !enabled() {
        return;
    }
    let mut g = LISTS.lock().unwrap();
    let s = g.get_or_insert_with(HashMap::new).entry(list).or_default();
    if s.pso == 0 || s.rt_count == 0 {
        return;
    }
    let Some((d, _keep)) = crate::device_wrap::gfx_pso_desc(s.pso) else { return };
    let u = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let independent = u(0x7c) != 0;
    let samples = u(0x268).max(1);
    let n = u(0x240).min(s.rt_count).min(8);
    let inc = RTV_INC.load(core::sync::atomic::Ordering::Relaxed);
    for i in 0..n as usize {
        let fmt = u(0x244 + 4 * i);
        if small_float(fmt).is_none() {
            continue;
        }
        let b = if independent { 0x80 + 0x28 * i } else { 0x80 };
        let (en, op, opa, mask) = (u(b), u(b + 0x10), u(b + 0x1c), d[b + 0x24]);
        if en == 0 || mask == 0 || (matches!(op, 4 | 5) && matches!(opa, 4 | 5)) {
            continue;
        }
        let h = if s.single {
            if inc == 0 {
                continue;
            }
            s.rtv[0] + i * inc
        } else {
            match s.rtv.get(i) {
                Some(&h) => h,
                None => continue,
            }
        };
        if h != 0 && !s.dirty.iter().any(|e| e.0 == h) {
            s.dirty.push((h, fmt, samples));
        }
    }
}

static NEXT: [core::sync::atomic::AtomicUsize; 64] = [const { core::sync::atomic::AtomicUsize::new(0) }; 64];

fn next(slot: usize) -> usize {
    NEXT[slot].load(core::sync::atomic::Ordering::Relaxed)
}

pub unsafe fn install() {
    if !enabled() {
        return;
    }
    let hooks: [(usize, usize); 12] = [
        (9, h_close as usize), (10, h_reset as usize), (12, h_draw as usize), (13, h_draw_indexed as usize),
        (20, h_topo as usize), (21, h_viewports as usize), (22, h_scissors as usize), (25, h_set_pso as usize),
        (26, h_barrier as usize), (34, h_root_sig as usize), (50, h_targets as usize), (59, h_indirect as usize),
    ];
    for (slot, _) in hooks {
        let prev = crate::cmdlist_wrap::vt_get(slot);
        if prev == 0 {
            record("d3d12_xs", "blendclamp", &format!("command-list slot {slot} empty - clamp hooks not installed"));
            return;
        }
        NEXT[slot].store(prev, core::sync::atomic::Ordering::Relaxed);
    }
    for (slot, f) in hooks {
        crate::cmdlist_wrap::vt_set(slot, f);
    }
}

unsafe extern "system" fn h_close(this: *mut c_void) -> i32 {
    unsafe { flush(this) };
    let f: unsafe extern "system" fn(*mut c_void) -> i32 = unsafe { core::mem::transmute(next(9)) };
    unsafe { f(this) }
}

unsafe extern "system" fn h_reset(this: *mut c_void, alloc: *mut c_void, pso: *mut c_void) -> i32 {
    on_reset(this as usize);
    if !pso.is_null() {
        on_set_pso(this as usize, pso as usize);
    }
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 = unsafe { core::mem::transmute(next(10)) };
    unsafe { f(this, alloc, pso) }
}

unsafe extern "system" fn h_draw(this: *mut c_void, a: u32, b: u32, c: u32, d: u32) {
    let f: unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32) = unsafe { core::mem::transmute(next(12)) };
    unsafe { f(this, a, b, c, d) };
    on_draw(this as usize);
}

unsafe extern "system" fn h_draw_indexed(this: *mut c_void, a: u32, b: u32, c: u32, d: i32, e: u32) {
    let f: unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32) = unsafe { core::mem::transmute(next(13)) };
    unsafe { f(this, a, b, c, d, e) };
    on_draw(this as usize);
}

unsafe extern "system" fn h_topo(this: *mut c_void, t: u32) {
    on_topology(this as usize, t);
    let f: unsafe extern "system" fn(*mut c_void, u32) = unsafe { core::mem::transmute(next(20)) };
    unsafe { f(this, t) }
}

unsafe extern "system" fn h_viewports(this: *mut c_void, n: u32, p: *const [f32; 6]) {
    unsafe { on_viewports(this as usize, n, p) };
    let f: unsafe extern "system" fn(*mut c_void, u32, *const [f32; 6]) = unsafe { core::mem::transmute(next(21)) };
    unsafe { f(this, n, p) }
}

unsafe extern "system" fn h_scissors(this: *mut c_void, n: u32, p: *const [i32; 4]) {
    unsafe { on_scissors(this as usize, n, p) };
    let f: unsafe extern "system" fn(*mut c_void, u32, *const [i32; 4]) = unsafe { core::mem::transmute(next(22)) };
    unsafe { f(this, n, p) }
}

unsafe extern "system" fn h_set_pso(this: *mut c_void, pso: *mut c_void) {
    on_set_pso(this as usize, pso as usize);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = unsafe { core::mem::transmute(next(25)) };
    unsafe { f(this, pso) }
}

unsafe extern "system" fn h_barrier(this: *mut c_void, n: u32, b: *const u8) {
    unsafe { flush(this) };
    let f: unsafe extern "system" fn(*mut c_void, u32, *const u8) = unsafe { core::mem::transmute(next(26)) };
    unsafe { f(this, n, b) }
}

unsafe extern "system" fn h_root_sig(this: *mut c_void, rs: *mut c_void) {
    if with(this as usize, |s| s.root_sig) != rs as usize {
        unsafe { flush(this) };
    }
    on_set_root_sig(this as usize, rs as usize);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = unsafe { core::mem::transmute(next(34)) };
    unsafe { f(this, rs) }
}

unsafe extern "system" fn h_targets(this: *mut c_void, n: u32, handles: *const usize, single: i32, dsv: *const usize) {
    unsafe { flush(this) };
    if single != 0 && RTV_INC.load(core::sync::atomic::Ordering::Relaxed) == 0 {
        unsafe { device_of(this) };
    }
    unsafe { on_set_targets(this as usize, n, handles, single != 0, dsv) };
    let f: unsafe extern "system" fn(*mut c_void, u32, *const usize, i32, *const usize) = unsafe { core::mem::transmute(next(50)) };
    unsafe { f(this, n, handles, single, dsv) }
}

unsafe extern "system" fn h_indirect(this: *mut c_void, sig: *mut c_void, n: u32, buf: *mut c_void, off: u64, cbuf: *mut c_void, coff: u64) {
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut c_void, u64, *mut c_void, u64) = unsafe { core::mem::transmute(next(59)) };
    unsafe { f(this, sig, n, buf, off, cbuf, coff) };
    on_draw(this as usize);
}

unsafe fn real(slot: usize) -> usize {
    unsafe { crate::cmdlist_wrap::real_method(slot) }
}

unsafe fn device_of(list: *mut c_void) -> *mut c_void {
    let mut dev: *mut c_void = core::ptr::null_mut();
    let f: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(real(7)) };
    if unsafe { f(list, &IID_DEVICE, &mut dev) } < 0 || dev.is_null() {
        return core::ptr::null_mut();
    }
    let vt = unsafe { *(dev as *const *const usize) };
    let release: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { core::mem::transmute(*vt.add(2)) };
    if RTV_INC.load(core::sync::atomic::Ordering::Relaxed) == 0 {
        let inc: unsafe extern "system" fn(*mut c_void, u32) -> u32 = unsafe { core::mem::transmute(*vt.add(15)) };
        RTV_INC.store(unsafe { inc(dev, 2) } as usize, core::sync::atomic::Ordering::Relaxed);
    }
    unsafe { release(dev) };
    dev
}

fn shaders() -> Option<&'static (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
    static S: OnceLock<Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)>> = OnceLock::new();
    S.get_or_init(|| {
        let vs = "float4 main(uint id : SV_VertexID) : SV_Position { float2 p = float2((id << 1) & 2, id & 2); return float4(p * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0); }";
        let ps11 = "float4 main() : SV_Target { return float4(65024.0, 65024.0, 64512.0, 65504.0); }";
        let ps16 = "float4 main() : SV_Target { return float4(65504.0, 65504.0, 65504.0, 65504.0); }";
        let ps16n = "float4 main() : SV_Target { return float4(-65504.0, -65504.0, -65504.0, -65504.0); }";
        let c = |src: &str, p: &str| tombox_xshader::dxc::compile(src, p);
        match (c(vs, "vs_6_0"), c(ps11, "ps_6_0"), c(ps16, "ps_6_0"), c(ps16n, "ps_6_0")) {
            (Ok(a), Ok(b), Ok(d), Ok(e)) => Some((a, b, d, e)),
            (a, b, d, e) => {
                record("d3d12_xs", "blendclamp", &format!("shader compile failed: {:?} {:?} {:?} {:?}", a.err(), b.err(), d.err(), e.err()));
                None
            }
        }
    })
    .as_ref()
}

unsafe fn clamp_pso(dev: *mut c_void, root_sig: usize, fmt: u32, samples: u32, neg: bool) -> usize {
    let key = (root_sig, fmt, samples, neg);
    if let Some(&p) = PSOS.lock().unwrap().get_or_insert_with(HashMap::new).get(&key) {
        return p;
    }
    let Some((vs, ps11, ps16, ps16n)) = shaders() else { return 0 };
    let ps = if neg { ps16n } else if fmt == 26 { ps11 } else { ps16 };
    let mut d = vec![0u8; 0x290];
    let put32 = |d: &mut Vec<u8>, o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let put64 = |d: &mut Vec<u8>, o: usize, v: u64| d[o..o + 8].copy_from_slice(&v.to_le_bytes());
    put64(&mut d, 0x00, root_sig as u64);
    put64(&mut d, 0x08, vs.as_ptr() as u64);
    put64(&mut d, 0x10, vs.len() as u64);
    put64(&mut d, 0x18, ps.as_ptr() as u64);
    put64(&mut d, 0x20, ps.len() as u64);
    put32(&mut d, 0x80, 1);
    put32(&mut d, 0x88, 2);
    put32(&mut d, 0x8c, 2);
    put32(&mut d, 0x90, if neg { 5 } else { 4 });
    put32(&mut d, 0x94, 2);
    put32(&mut d, 0x98, 2);
    put32(&mut d, 0x9c, if neg { 5 } else { 4 });
    put32(&mut d, 0xa0, 4);
    d[0xa4] = 0x0f;
    put32(&mut d, 0x1c0, 0xffff_ffff);
    put32(&mut d, 0x1c4, 3);
    put32(&mut d, 0x1c8, 1);
    put32(&mut d, 0x1dc, 1);
    put32(&mut d, 0x23c, 3);
    put32(&mut d, 0x240, 1);
    put32(&mut d, 0x244, fmt);
    put32(&mut d, 0x268, samples);
    let vt = unsafe { *(dev as *const *const usize) };
    let f: unsafe extern "system" fn(*mut c_void, *const u8, *const Guid, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(*vt.add(10)) };
    let mut out: *mut c_void = core::ptr::null_mut();
    let hr = unsafe { f(dev, d.as_ptr(), &IID_PSO, &mut out) };
    if hr < 0 || out.is_null() {
        record("d3d12_xs", "blendclamp", &format!("clamp PSO create failed hr={hr:#010x} fmt={fmt} samples={samples} neg={neg}"));
        PSOS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, 0);
        return 0;
    }
    PSOS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, out as usize);
    out as usize
}

pub unsafe fn flush(list: *mut c_void) {
    if !enabled() {
        return;
    }
    let (dirty, pso, root_sig, rtv, rt_count, single, dsv, vps, scs, topo) = {
        let mut g = LISTS.lock().unwrap();
        let Some(s) = g.as_mut().and_then(|m| m.get_mut(&(list as usize))) else { return };
        if s.dirty.is_empty() {
            return;
        }
        (core::mem::take(&mut s.dirty), s.pso, s.root_sig, s.rtv.clone(), s.rt_count, s.single, s.dsv, s.viewports.clone(), s.scissors.clone(), s.topo)
    };
    if root_sig == 0 {
        return;
    }
    let dev = unsafe { device_of(list) };
    if dev.is_null() {
        return;
    }
    let set_pso: unsafe extern "system" fn(*mut c_void, usize) = unsafe { core::mem::transmute(real(25)) };
    let om: unsafe extern "system" fn(*mut c_void, u32, *const usize, i32, *const usize) = unsafe { core::mem::transmute(real(46)) };
    let set_vp: unsafe extern "system" fn(*mut c_void, u32, *const [f32; 6]) = unsafe { core::mem::transmute(real(21)) };
    let set_sc: unsafe extern "system" fn(*mut c_void, u32, *const [i32; 4]) = unsafe { core::mem::transmute(real(22)) };
    let set_topo: unsafe extern "system" fn(*mut c_void, u32) = unsafe { core::mem::transmute(real(20)) };
    let draw: unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32) = unsafe { core::mem::transmute(real(12)) };
    let vp = [0.0f32, 0.0, 16384.0, 16384.0, 0.0, 1.0];
    let sc = [0i32, 0, 16384, 16384];
    let mut did = false;
    for (h, fmt, samples) in dirty {
        let neg_too = small_float(fmt) == Some(true);
        let passes: &[bool] = if neg_too { &[false, true] } else { &[false] };
        for &neg in passes {
            let p = unsafe { clamp_pso(dev, root_sig, fmt, samples, neg) };
            if p == 0 {
                continue;
            }
            unsafe {
                set_pso(list, p);
                om(list, 1, &h, 0, core::ptr::null());
                set_vp(list, 1, &vp);
                set_sc(list, 1, &sc);
                set_topo(list, 4);
                draw(list, 3, 1, 0, 0);
            }
            did = true;
            if PASSES.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 4 {
                record("d3d12_xs", "blendclamp", &format!("clamp pass on rtv={h:#x} fmt={fmt} samples={samples} neg={neg}"));
            }
        }
    }
    if !did {
        return;
    }
    unsafe {
        if pso != 0 {
            set_pso(list, pso);
        }
        let dsv_ptr = dsv.as_ref().map_or(core::ptr::null(), |d| d as *const usize);
        om(list, rt_count, if rtv.is_empty() { core::ptr::null() } else { rtv.as_ptr() }, single as i32, dsv_ptr);
        if !vps.is_empty() {
            set_vp(list, vps.len() as u32, vps.as_ptr());
        }
        if !scs.is_empty() {
            set_sc(list, scs.len() as u32, scs.as_ptr());
        }
        if let Some(t) = topo {
            set_topo(list, t);
        }
    }
}
