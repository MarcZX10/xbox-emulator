use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::Mutex;
use compat_common::record;

const N: usize = 128;
static PREV: [AtomicUsize; N] = [const { AtomicUsize::new(0) }; N];
static ARMED: AtomicBool = AtomicBool::new(false);
static FINISH: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct ListRec {
    from_reset: bool,
    lines: Vec<String>,
}

#[derive(Default)]
struct Rec {
    lists: HashMap<usize, ListRec>,
    frame: Vec<String>,
    psos: BTreeSet<usize>,
    roots: BTreeSet<usize>,
    resources: BTreeSet<usize>,
    vas: BTreeSet<u64>,
    sigs: BTreeSet<usize>,
    executes: usize,
}

static REC: Mutex<Option<Rec>> = Mutex::new(None);
static STATES: Mutex<Option<HashMap<usize, u32>>> = Mutex::new(None);
static ROOTS: Mutex<Option<HashMap<usize, Vec<u8>>>> = Mutex::new(None);
static VIEWS: Mutex<Option<HashMap<usize, (char, usize, Option<Vec<u8>>)>>> = Mutex::new(None);
static SIGS: Mutex<Option<HashMap<usize, (u32, Vec<[u32; 4]>, usize)>>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Rec) -> R) -> R {
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Rec::default))
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

unsafe fn bytes<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(p, n)
    }
}

unsafe fn vt(obj: *mut c_void) -> *const usize {
    *(obj as *const *const usize)
}

fn push(list: *mut c_void, line: String) {
    let key = crate::list_split::origin_of(list);
    with(|r| r.lists.entry(key).or_default().lines.push(line));
}

pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

pub(crate) fn push_line(list: *mut c_void, line: String) {
    push(list, line);
}

pub(crate) fn note_resource(res: usize) {
    with(|r| note_res(r, res));
}

pub(crate) fn root_blob(obj: usize) -> Option<Vec<u8>> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&obj).cloned())
}

pub(crate) fn view_res(handle: usize) -> Option<usize> {
    VIEWS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&handle).map(|v| v.1))
}

pub(crate) fn tracked_state(res: usize) -> Option<u32> {
    state_of(res).or_else(|| crate::rtcap::state_of(res))
}

fn note_res(r: &mut Rec, res: usize) {
    if res != 0 {
        r.resources.insert(res);
    }
}

pub fn note_root_signature(obj: usize, blob: &[u8]) {
    if obj == 0 {
        return;
    }
    ROOTS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(obj, blob.to_vec());
}

pub fn note_view(handle: usize, kind: char, res: usize, desc: Option<&[u8]>) {
    if !crate::rtcap::enabled() {
        return;
    }
    VIEWS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(handle, (kind, res, desc.map(|d| d.to_vec())));
}

pub fn note_signature(sig: usize, stride: u32, args: &[[u32; 4]], root: usize) {
    if sig == 0 || !crate::rtcap::enabled() {
        return;
    }
    SIGS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(sig, (stride, args.to_vec(), root));
}

pub unsafe fn note_barriers(n: u32, p: *const u8) {
    if p.is_null() {
        return;
    }
    let mut g = STATES.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    for i in 0..n as usize {
        let b = p.add(i * 32);
        if core::ptr::read_unaligned(b as *const u32) != 0 {
            continue;
        }
        let res = core::ptr::read_unaligned(b.add(8) as *const usize);
        let sub = core::ptr::read_unaligned(b.add(16) as *const u32);
        let after = core::ptr::read_unaligned(b.add(24) as *const u32);
        if sub == u32::MAX || sub == 0 {
            m.insert(res, after);
        }
        let mut mx = MIXED.lock().unwrap_or_else(|e| e.into_inner());
        let s = mx.get_or_insert_with(BTreeSet::new);
        if sub == u32::MAX {
            s.remove(&res);
        } else {
            s.insert(res);
        }
    }
}

static MIXED: Mutex<Option<BTreeSet<usize>>> = Mutex::new(None);

pub(crate) fn mixed_state(res: usize) -> bool {
    MIXED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.contains(&res)).unwrap_or(false)
}

fn state_of(res: usize) -> Option<u32> {
    STATES.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&res).copied())
}

unsafe fn heap_start(h: *mut c_void, slot: usize) -> u64 {
    let f: unsafe extern "system" fn(*mut c_void, *mut u64) -> u64 = core::mem::transmute(*vt(h).add(slot));
    let mut out = 0u64;
    let p = &mut out as *mut u64;
    let rax = f(h, p);
    if rax == p as u64 { out } else { rax }
}

macro_rules! prev {
    ($slot:expr, fn($($t:ty),*) $(-> $r:ty)?) => {{
        let f: unsafe extern "system" fn(*mut c_void $(, $t)*) $(-> $r)? = core::mem::transmute(PREV[$slot].load(Ordering::Relaxed));
        f
    }};
}

unsafe extern "system" fn h_close(this: *mut c_void) -> i32 {
    if ARMED.load(Ordering::Relaxed) {
        push(this, "close".into());
    }
    prev!(9, fn() -> i32)(this)
}
unsafe extern "system" fn h_reset(this: *mut c_void, alloc: *mut c_void, pso: *mut c_void) -> i32 {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            r.lists.insert(crate::list_split::origin_of(this), ListRec { from_reset: true, lines: vec![format!("reset pso={:#x}", pso as usize)] });
            if !pso.is_null() {
                r.psos.insert(pso as usize);
            }
        });
    }
    prev!(10, fn(*mut c_void, *mut c_void) -> i32)(this, alloc, pso)
}
unsafe extern "system" fn h_draw(this: *mut c_void, a: u32, b: u32, c: u32, d: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("draw {a} {b} {c} {d}"));
    }
    prev!(12, fn(u32, u32, u32, u32))(this, a, b, c, d)
}
unsafe extern "system" fn h_draw_indexed(this: *mut c_void, a: u32, b: u32, c: u32, d: i32, e: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("drawidx {a} {b} {c} {d} {e}"));
    }
    prev!(13, fn(u32, u32, u32, i32, u32))(this, a, b, c, d, e)
}
unsafe extern "system" fn h_dispatch(this: *mut c_void, a: u32, b: u32, c: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("dispatch {a} {b} {c}"));
    }
    prev!(14, fn(u32, u32, u32))(this, a, b, c)
}
unsafe extern "system" fn h_copy_buffer(this: *mut c_void, dst: *mut c_void, doff: u64, src: *mut c_void, soff: u64, n: u64) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            note_res(r, dst as usize);
            note_res(r, src as usize);
        });
        push(this, format!("copybuf dst={:#x} {doff:#x} src={:#x} {soff:#x} n={n:#x}", dst as usize, src as usize));
    }
    prev!(15, fn(*mut c_void, u64, *mut c_void, u64, u64))(this, dst, doff, src, soff, n)
}
unsafe extern "system" fn h_copy_texture(this: *mut c_void, dst: *const u8, x: u32, y: u32, z: u32, src: *const u8, bx: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            if !dst.is_null() {
                note_res(r, core::ptr::read_unaligned(dst as *const usize));
            }
            if !src.is_null() {
                note_res(r, core::ptr::read_unaligned(src as *const usize));
            }
        });
        push(this, format!("copytex dst={} {x} {y} {z} src={} box={}", hex(bytes(dst, 48)), hex(bytes(src, 48)), hex(bytes(bx, 24))));
    }
    prev!(16, fn(*const u8, u32, u32, u32, *const u8, *const u8))(this, dst, x, y, z, src, bx)
}
unsafe extern "system" fn h_copy_resource(this: *mut c_void, dst: *mut c_void, src: *mut c_void) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            note_res(r, dst as usize);
            note_res(r, src as usize);
        });
        push(this, format!("copyres dst={:#x} src={:#x}", dst as usize, src as usize));
    }
    prev!(17, fn(*mut c_void, *mut c_void))(this, dst, src)
}
unsafe extern "system" fn h_topology(this: *mut c_void, t: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("topology {t}"));
    }
    prev!(20, fn(u32))(this, t)
}
unsafe extern "system" fn h_viewports(this: *mut c_void, n: u32, p: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("viewports {n} {}", hex(bytes(p, n as usize * 24))));
    }
    prev!(21, fn(u32, *const u8))(this, n, p)
}
unsafe extern "system" fn h_scissors(this: *mut c_void, n: u32, p: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("scissors {n} {}", hex(bytes(p, n as usize * 16))));
    }
    prev!(22, fn(u32, *const u8))(this, n, p)
}
unsafe extern "system" fn h_blend_factor(this: *mut c_void, p: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("blendfactor {}", hex(bytes(p, 16))));
    }
    prev!(23, fn(*const u8))(this, p)
}
unsafe extern "system" fn h_stencil_ref(this: *mut c_void, v: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("stencilref {v}"));
    }
    prev!(24, fn(u32))(this, v)
}
unsafe extern "system" fn h_set_pso(this: *mut c_void, pso: *mut c_void) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            r.psos.insert(pso as usize);
        });
        push(this, format!("pso {:#x}", pso as usize));
    }
    prev!(25, fn(*mut c_void))(this, pso)
}
unsafe extern "system" fn h_barrier(this: *mut c_void, n: u32, p: *const u8) {
    note_barriers(n, p);
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            for i in 0..n as usize {
                let b = p.add(i * 32);
                note_res(r, core::ptr::read_unaligned(b.add(8) as *const usize));
            }
        });
        push(this, format!("barrier {n} {}", hex(bytes(p, n as usize * 32))));
    }
    prev!(26, fn(u32, *const u8))(this, n, p)
}
unsafe extern "system" fn h_heaps(this: *mut c_void, n: u32, p: *const usize) {
    if ARMED.load(Ordering::Relaxed) {
        let mut s = format!("heaps {n}");
        for i in 0..n as usize {
            if p.is_null() {
                break;
            }
            let h = *p.add(i) as *mut c_void;
            if h.is_null() {
                continue;
            }
            let cpu = heap_start(h, 9) as usize;
            let gpu = heap_start(h, 10);
            let _ = write!(s, " {:#x}:cpu={cpu:#x}:gpu={gpu:#x}", h as usize);
        }
        push(this, s);
    }
    prev!(32, fn(u32, *const usize))(this, n, p)
}
unsafe extern "system" fn h_compute_root(this: *mut c_void, rs: *mut c_void) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            r.roots.insert(rs as usize);
        });
        push(this, format!("croot {:#x}", rs as usize));
    }
    prev!(33, fn(*mut c_void))(this, rs)
}
unsafe extern "system" fn h_gfx_root(this: *mut c_void, rs: *mut c_void) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            r.roots.insert(rs as usize);
        });
        push(this, format!("groot {:#x}", rs as usize));
    }
    prev!(34, fn(*mut c_void))(this, rs)
}
unsafe extern "system" fn h_compute_table(this: *mut c_void, i: u32, h: u64) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("ctable {i} {h:#x}"));
    }
    prev!(35, fn(u32, u64))(this, i, h)
}
unsafe extern "system" fn h_gfx_table(this: *mut c_void, i: u32, h: u64) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("gtable {i} {h:#x}"));
    }
    prev!(36, fn(u32, u64))(this, i, h)
}
unsafe extern "system" fn h_compute_const(this: *mut c_void, i: u32, v: u32, o: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("cconst {i} {o} {v:08x}"));
    }
    prev!(37, fn(u32, u32, u32))(this, i, v, o)
}
unsafe extern "system" fn h_gfx_const(this: *mut c_void, i: u32, v: u32, o: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("gconst {i} {o} {v:08x}"));
    }
    prev!(38, fn(u32, u32, u32))(this, i, v, o)
}
unsafe extern "system" fn h_compute_consts(this: *mut c_void, i: u32, n: u32, p: *const u8, o: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("cconsts {i} {o} {}", hex(bytes(p, n as usize * 4))));
    }
    prev!(39, fn(u32, u32, *const u8, u32))(this, i, n, p, o)
}
unsafe extern "system" fn h_gfx_consts(this: *mut c_void, i: u32, n: u32, p: *const u8, o: u32) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("gconsts {i} {o} {}", hex(bytes(p, n as usize * 4))));
    }
    prev!(40, fn(u32, u32, *const u8, u32))(this, i, n, p, o)
}
macro_rules! root_va {
    ($name:ident, $slot:literal, $tag:literal) => {
        unsafe extern "system" fn $name(this: *mut c_void, i: u32, va: u64) {
            if ARMED.load(Ordering::Relaxed) {
                with(|r| {
                    r.vas.insert(va);
                });
                push(this, format!(concat!($tag, " {} {:#x}"), i, va));
            }
            prev!($slot, fn(u32, u64))(this, i, va)
        }
    };
}
root_va!(h_ccbv, 41, "ccbv");
root_va!(h_gcbv, 42, "gcbv");
root_va!(h_csrv, 43, "csrv");
root_va!(h_gsrv, 44, "gsrv");
root_va!(h_cuav, 45, "cuav");
root_va!(h_guav, 46, "guav");
unsafe extern "system" fn h_ib(this: *mut c_void, p: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        if p.is_null() {
            push(this, "ib null".into());
        } else {
            let va = core::ptr::read_unaligned(p as *const u64);
            with(|r| {
                r.vas.insert(va);
            });
            push(this, format!("ib {va:#x} {} {}", core::ptr::read_unaligned(p.add(8) as *const u32), core::ptr::read_unaligned(p.add(12) as *const u32)));
        }
    }
    prev!(47, fn(*const u8))(this, p)
}
unsafe extern "system" fn h_vbs(this: *mut c_void, start: u32, n: u32, p: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        let mut s = format!("vbs {start} {n}");
        for i in 0..n as usize {
            if p.is_null() {
                break;
            }
            let v = p.add(i * 16);
            let va = core::ptr::read_unaligned(v as *const u64);
            with(|r| {
                r.vas.insert(va);
            });
            let _ = write!(s, " {va:#x}:{}:{}", core::ptr::read_unaligned(v.add(8) as *const u32), core::ptr::read_unaligned(v.add(12) as *const u32));
        }
        push(this, s);
    }
    prev!(48, fn(u32, u32, *const u8))(this, start, n, p)
}
unsafe extern "system" fn h_targets(this: *mut c_void, n: u32, handles: *const usize, single: i32, dsv: *const usize) {
    if ARMED.load(Ordering::Relaxed) {
        let mut s = format!("targets {n} single={single}");
        for i in 0..(n as usize).min(8) {
            if handles.is_null() {
                break;
            }
            let _ = write!(s, " {:#x}", if single != 0 { *handles } else { *handles.add(i) });
        }
        let _ = write!(s, " dsv={:#x}", if dsv.is_null() { 0 } else { *dsv });
        push(this, s);
    }
    prev!(50, fn(u32, *const usize, i32, *const usize))(this, n, handles, single, dsv)
}
unsafe extern "system" fn h_clear_dsv(this: *mut c_void, h: usize, flags: u32, depth: f32, stencil: u8, n: u32, rects: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("cleardsv {h:#x} {flags} {:08x} {stencil} {n} {}", depth.to_bits(), hex(bytes(rects, n as usize * 16))));
    }
    prev!(51, fn(usize, u32, f32, u8, u32, *const u8))(this, h, flags, depth, stencil, n, rects)
}
unsafe extern "system" fn h_clear_rtv(this: *mut c_void, h: usize, color: *const u8, n: u32, rects: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        push(this, format!("clearrtv {h:#x} {} {n} {}", hex(bytes(color, 16)), hex(bytes(rects, n as usize * 16))));
    }
    prev!(52, fn(usize, *const u8, u32, *const u8))(this, h, color, n, rects)
}
unsafe extern "system" fn h_clear_uav_uint(this: *mut c_void, gpu: u64, cpu: usize, res: *mut c_void, v: *const u8, n: u32, rects: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| note_res(r, res as usize));
        push(this, format!("clearuavu {gpu:#x} {cpu:#x} {:#x} {} {n} {}", res as usize, hex(bytes(v, 16)), hex(bytes(rects, n as usize * 16))));
    }
    prev!(53, fn(u64, usize, *mut c_void, *const u8, u32, *const u8))(this, gpu, cpu, res, v, n, rects)
}
unsafe extern "system" fn h_clear_uav_float(this: *mut c_void, gpu: u64, cpu: usize, res: *mut c_void, v: *const u8, n: u32, rects: *const u8) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| note_res(r, res as usize));
        push(this, format!("clearuavf {gpu:#x} {cpu:#x} {:#x} {} {n} {}", res as usize, hex(bytes(v, 16)), hex(bytes(rects, n as usize * 16))));
    }
    prev!(54, fn(u64, usize, *mut c_void, *const u8, u32, *const u8))(this, gpu, cpu, res, v, n, rects)
}
unsafe extern "system" fn h_indirect(this: *mut c_void, sig: *mut c_void, n: u32, buf: *mut c_void, off: u64, cbuf: *mut c_void, coff: u64) {
    if ARMED.load(Ordering::Relaxed) {
        with(|r| {
            r.sigs.insert(sig as usize);
            note_res(r, buf as usize);
            note_res(r, cbuf as usize);
        });
        push(this, format!("indirect sig={:#x} {n} buf={:#x} {off:#x} count={:#x} {coff:#x}", sig as usize, buf as usize, cbuf as usize));
    }
    prev!(59, fn(*mut c_void, u32, *mut c_void, u64, *mut c_void, u64))(this, sig, n, buf, off, cbuf, coff)
}

pub unsafe fn install(xbox_vt: &[AtomicUsize]) {
    if !crate::rtcap::enabled() {
        return;
    }
    let hooks: [(usize, usize); 42] = [
        (9, h_close as usize), (10, h_reset as usize), (12, h_draw as usize), (13, h_draw_indexed as usize),
        (14, h_dispatch as usize), (15, h_copy_buffer as usize), (16, h_copy_texture as usize), (17, h_copy_resource as usize),
        (20, h_topology as usize), (21, h_viewports as usize), (22, h_scissors as usize), (23, h_blend_factor as usize),
        (24, h_stencil_ref as usize), (25, h_set_pso as usize), (26, h_barrier as usize), (32, h_heaps as usize),
        (33, h_compute_root as usize), (34, h_gfx_root as usize), (35, h_compute_table as usize), (36, h_gfx_table as usize),
        (37, h_compute_const as usize), (38, h_gfx_const as usize), (39, h_compute_consts as usize), (40, h_gfx_consts as usize),
        (41, h_ccbv as usize), (42, h_gcbv as usize), (43, h_csrv as usize), (44, h_gsrv as usize),
        (45, h_cuav as usize), (46, h_guav as usize), (47, h_ib as usize), (48, h_vbs as usize),
        (50, h_targets as usize), (51, h_clear_dsv as usize), (52, h_clear_rtv as usize), (53, h_clear_uav_uint as usize),
        (54, h_clear_uav_float as usize), (59, h_indirect as usize), (0, 0), (0, 0), (0, 0), (0, 0),
    ];
    let mut n = 0;
    for (slot, f) in hooks {
        if f == 0 {
            continue;
        }
        PREV[slot].store(xbox_vt[slot].load(Ordering::Relaxed), Ordering::Relaxed);
        xbox_vt[slot].store(f, Ordering::Relaxed);
        n += 1;
    }
    record("d3d12_xs", "replay", &format!("frame recorder installed on {n} command-list methods; trigger file %TEMP%\\tombox_replay_now"));
}

pub fn on_execute(queue: *mut c_void, lists: &[*mut c_void]) {
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    crate::replay_ext::note_queue(queue);
    let keys: Vec<usize> = lists.iter().map(|&l| crate::list_split::origin_of(l)).collect();
    with(|r| {
        r.executes += 1;
        let e = r.executes;
        let mut seen = std::collections::HashSet::new();
        for (&l, &key) in lists.iter().zip(&keys) {
            if !seen.insert(key) {
                continue;
            }
            let rec = r.lists.remove(&key).unwrap_or_default();
            r.frame.push(format!("execute #{e} queue={:#x} list={:#x} complete={}", queue as usize, l as usize, rec.from_reset));
            r.frame.extend(rec.lines.into_iter().map(|s| format!("  {s}")));
        }
    });
}

pub unsafe fn on_present(queue: *mut c_void) {
    if !crate::rtcap::enabled() {
        return;
    }
    if FINISH.swap(false, Ordering::Relaxed) {
        ARMED.store(false, Ordering::Relaxed);
        let rec = REC.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
        write_capture(queue, rec);
        return;
    }
    static LIVE: compat_common::live::Edge = compat_common::live::Edge::new();
    let trigger = std::env::temp_dir().join("tombox_replay_now");
    let live = LIVE.poll("record_frames").is_some();
    if live || trigger.exists() {
        let _ = std::fs::remove_file(&trigger);
        *REC.lock().unwrap_or_else(|e| e.into_inner()) = Some(Rec::default());
        ARMED.store(true, Ordering::Relaxed);
        FINISH.store(true, Ordering::Relaxed);
        record("d3d12_xs", "replay", "armed: recording the next frame");
    }
}

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);
const IID_DEVICE: Guid = Guid(0x189819f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);
const IID_ALLOCATOR: Guid = Guid(0x6102dee4, 0xaf59, 0x4b09, [0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24]);
const IID_GFX_LIST: Guid = Guid(0x5b160d0f, 0xac1b, 0x4185, [0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55]);
const IID_FENCE: Guid = Guid(0x0a753dcf, 0xc4d8, 0x4b91, [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76]);
const IID_RESOURCE: Guid = Guid(0x696442be, 0xa72e, 0x4059, [0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad]);
const COPY_SOURCE: u32 = 0x800;
const MAX_ONE: u64 = 256 << 20;
const MAX_TOTAL: u64 = 3 << 30;

extern "system" {
    fn Sleep(ms: u32);
}

macro_rules! vcall {
    ($obj:expr, $slot:expr, fn($($at:ty),*) -> $rt:ty $(, $arg:expr)*) => {{
        let f: unsafe extern "system" fn(*mut c_void $(, $at)*) -> $rt = core::mem::transmute(*vt($obj).add($slot));
        f($obj $(, $arg)*)
    }};
}

struct Readback {
    queue: *mut c_void,
    device: *mut c_void,
    alloc: *mut c_void,
    list: *mut c_void,
    fence: *mut c_void,
    value: u64,
    total: u64,
}

impl Readback {
    unsafe fn new(queue: *mut c_void) -> Option<Readback> {
        let mut device: *mut c_void = core::ptr::null_mut();
        if vcall!(queue, 7, fn(*const Guid, *mut *mut c_void) -> i32, &IID_DEVICE, &mut device) < 0 {
            return None;
        }
        let mut alloc: *mut c_void = core::ptr::null_mut();
        let mut list: *mut c_void = core::ptr::null_mut();
        let mut fence: *mut c_void = core::ptr::null_mut();
        if vcall!(device, 9, fn(u32, *const Guid, *mut *mut c_void) -> i32, 0, &IID_ALLOCATOR, &mut alloc) < 0
            || vcall!(device, 12, fn(u32, u32, *mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32, 0, 0, alloc, core::ptr::null_mut(), &IID_GFX_LIST, &mut list) < 0
            || vcall!(device, 36, fn(u64, u32, *const Guid, *mut *mut c_void) -> i32, 0, 0, &IID_FENCE, &mut fence) < 0
        {
            return None;
        }
        vcall!(list, 9, fn() -> i32);
        Some(Readback { queue, device, alloc, list, fence, value: 0, total: 0 })
    }

    unsafe fn buffer(&self, size: u64) -> *mut c_void {
        let heap: [u32; 5] = [3, 0, 0, 1, 1];
        let mut bd = [0u8; 0x38];
        bd[0..4].copy_from_slice(&1u32.to_le_bytes());
        bd[0x10..0x18].copy_from_slice(&size.to_le_bytes());
        bd[0x18..0x1c].copy_from_slice(&1u32.to_le_bytes());
        bd[0x1c..0x1e].copy_from_slice(&1u16.to_le_bytes());
        bd[0x1e..0x20].copy_from_slice(&1u16.to_le_bytes());
        bd[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
        bd[0x2c..0x30].copy_from_slice(&1u32.to_le_bytes());
        let mut buf: *mut c_void = core::ptr::null_mut();
        let hr = vcall!(self.device, 27, fn(*const u32, u32, *const u8, u32, *const c_void, *const Guid, *mut *mut c_void) -> i32,
            heap.as_ptr(), 0, bd.as_ptr(), COPY_DEST, core::ptr::null(), &IID_RESOURCE, &mut buf);
        if hr < 0 { core::ptr::null_mut() } else { buf }
    }

    unsafe fn submit(&mut self, res: usize, state: u32, record: impl FnOnce(*mut c_void)) -> bool {
        vcall!(self.alloc, 8, fn() -> i32);
        vcall!(self.list, 10, fn(*mut c_void, *mut c_void) -> i32, self.alloc, core::ptr::null_mut());
        let needs = state != 0 && state & COPY_SOURCE == 0;
        if needs {
            let b = [0u64, res as u64, 0xffff_ffffu64 | ((state as u64) << 32), COPY_SOURCE as u64];
            vcall!(self.list, 26, fn(u32, *const u64) -> (), 1, b.as_ptr());
        }
        record(self.list);
        if needs {
            let b = [0u64, res as u64, 0xffff_ffffu64 | ((COPY_SOURCE as u64) << 32), state as u64];
            vcall!(self.list, 26, fn(u32, *const u64) -> (), 1, b.as_ptr());
        }
        vcall!(self.list, 9, fn() -> i32);
        let lists = [self.list];
        vcall!(self.queue, 10, fn(u32, *const *mut c_void) -> (), 1, lists.as_ptr());
        self.value += 1;
        vcall!(self.queue, 14, fn(*mut c_void, u64) -> i32, self.fence, self.value);
        let mut spins = 0;
        while vcall!(self.fence, 8, fn() -> u64) < self.value && spins < 10_000 {
            Sleep(1);
            spins += 1;
        }
        spins < 10_000
    }

    unsafe fn read(buf: *mut c_void, size: usize) -> Option<Vec<u8>> {
        let mut data: *mut u8 = core::ptr::null_mut();
        let hr = vcall!(buf, 8, fn(u32, *const c_void, *mut *mut u8) -> i32, 0, core::ptr::null(), &mut data);
        if hr < 0 || data.is_null() {
            return None;
        }
        let v = core::slice::from_raw_parts(data, size).to_vec();
        vcall!(buf, 9, fn(u32, *const c_void) -> (), 0, core::ptr::null());
        Some(v)
    }

    unsafe fn read_buffer(&mut self, res: usize, size: u64, state: u32) -> Option<Vec<u8>> {
        let buf = self.buffer(size);
        if buf.is_null() {
            return None;
        }
        let ok = self.submit(res, state, |list| {
            vcall!(list, 15, fn(*mut c_void, u64, *mut c_void, u64, u64) -> (), buf, 0, res as *mut c_void, 0, size);
        });
        let out = if ok { Self::read(buf, size as usize) } else { None };
        vcall!(buf, 2, fn() -> u32);
        out
    }

    unsafe fn read_texture(&mut self, res: usize, desc: &[u8; 0x38], subs: u32, state: u32) -> Option<(Vec<[u8; 32]>, Vec<u8>)> {
        let mut fps = vec![[0u8; 32]; subs as usize];
        let mut total = 0u64;
        vcall!(self.device, 38, fn(*const u8, u32, u32, u64, *mut [u8; 32], *mut u32, *mut u64, *mut u64) -> (),
            desc.as_ptr(), 0, subs, 0, fps.as_mut_ptr(), core::ptr::null_mut(), core::ptr::null_mut(), &mut total);
        if total == 0 || total == u64::MAX || total > MAX_ONE || self.total + total > MAX_TOTAL {
            return None;
        }
        let buf = self.buffer(total);
        if buf.is_null() {
            return None;
        }
        let ok = self.submit(res, state, |list| {
            for (i, fp) in fps.iter().enumerate() {
                let mut dst = [0u8; 48];
                dst[0..8].copy_from_slice(&(buf as u64).to_le_bytes());
                dst[8..12].copy_from_slice(&1u32.to_le_bytes());
                dst[16..48].copy_from_slice(fp);
                let mut src = [0u8; 48];
                src[0..8].copy_from_slice(&(res as u64).to_le_bytes());
                src[16..20].copy_from_slice(&(i as u32).to_le_bytes());
                vcall!(list, 16, fn(*const u8, u32, u32, u32, *const u8, *const c_void) -> (), dst.as_ptr(), 0, 0, 0, src.as_ptr(), core::ptr::null());
            }
        });
        let out = if ok { Self::read(buf, total as usize) } else { None };
        vcall!(buf, 2, fn() -> u32);
        self.total += total;
        out.map(|d| (fps, d))
    }

    unsafe fn release(self) {
        vcall!(self.list, 2, fn() -> u32);
        vcall!(self.alloc, 2, fn() -> u32);
        vcall!(self.fence, 2, fn() -> u32);
        vcall!(self.device, 2, fn() -> u32);
    }
}
const COPY_DEST: u32 = 0x400;

unsafe fn res_desc(res: usize) -> [u8; 0x38] {
    let mut d = [0u8; 0x38];
    vcall!(res as *mut c_void, 10, fn(*mut u8) -> *mut u8, d.as_mut_ptr());
    d
}

fn u32at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

unsafe fn write_capture(queue: *mut c_void, mut rec: Rec) {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("tombox_replay").join(format!("{}", SEQ.fetch_add(1, Ordering::Relaxed)));
    for sub in ["pso", "rootsig", "data"] {
        let _ = std::fs::create_dir_all(dir.join(sub));
    }
    let _ = std::fs::write(dir.join("commands.txt"), rec.frame.join("\n") + "\n");
    let mut idx = String::new();
    for &p in &rec.psos {
        if p == 0 {
            continue;
        }
        match crate::device_x::pso_parts_for_replay(p) {
            Some((compute, root, parts)) => {
                let mut b = Vec::new();
                b.extend_from_slice(b"TBPL");
                b.extend_from_slice(&(parts.len() as u32).to_le_bytes());
                for part in &parts {
                    b.extend_from_slice(&(part.len() as u32).to_le_bytes());
                    b.extend_from_slice(part);
                }
                let _ = std::fs::write(dir.join("pso").join(format!("{p:x}.tbpl")), b);
                rec.roots.insert(root as usize);
                let mut stages = String::new();
                for (name, code) in crate::device_x::pso_stage_codes(p) {
                    match crate::shader_xlat::origin(code) {
                        Some((hash, blob)) => {
                            let _ = std::fs::write(dir.join("pso").join(format!("{p:x}_{name}.xbox")), &blob[..]);
                            let _ = write!(stages, " {name}={hash:016x}");
                        }
                        None => {
                            let _ = write!(stages, " {name}=unknown");
                        }
                    }
                }
                let _ = writeln!(idx, "pso {p:#x} {} root={root:#x} parts={}{stages}", if compute { "compute" } else { "graphics" }, parts.len());
            }
            None => {
                let _ = writeln!(idx, "pso {p:#x} UNREGISTERED");
            }
        }
    }
    let roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
    let sigs = SIGS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
    for &s in &rec.sigs {
        match sigs.get(&s) {
            Some((stride, args, root)) => {
                rec.roots.insert(*root);
                let a: Vec<String> = args.iter().map(|a| format!("{}:{}:{}:{}", a[0], a[1], a[2], a[3])).collect();
                let _ = writeln!(idx, "signature {s:#x} stride={stride} root={root:#x} args={}", a.join(","));
            }
            None => {
                let _ = writeln!(idx, "signature {s:#x} UNREGISTERED");
            }
        }
    }
    for &r in &rec.roots {
        if r == 0 {
            continue;
        }
        match roots.get(&r) {
            Some(b) => {
                let _ = std::fs::write(dir.join("rootsig").join(format!("{r:x}.bin")), b);
                let _ = writeln!(idx, "rootsig {r:#x} bytes={}", b.len());
            }
            None => {
                let _ = writeln!(idx, "rootsig {r:#x} UNREGISTERED");
            }
        }
    }
    let _ = std::fs::write(dir.join("objects.txt"), idx);

    let mut d = String::new();
    let views = VIEWS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
    let mut vk: Vec<_> = views.iter().collect();
    vk.sort_by_key(|e| *e.0);
    for (h, (kind, res, desc)) in vk {
        rec.resources.insert(*res);
        let _ = writeln!(d, "{kind}v {h:#x} res={res:#x} desc={}", desc.as_deref().map(hex).unwrap_or_else(|| "null".into()));
    }
    for (cpu, gpu, count, inc) in crate::bindtrace::heaps() {
        if gpu == 0 || inc == 0 {
            continue;
        }
        let _ = writeln!(d, "heap cpu={cpu:#x} gpu={gpu:#x} count={count} inc={inc}");
        for (h, desc) in crate::bindtrace::descs_in(cpu, count * inc) {
            let slot = (h - cpu) / inc;
            match desc {
                crate::bindtrace::Desc::Cbv { va, size } => {
                    rec.vas.insert(va);
                    let _ = writeln!(d, "  {slot} cbv va={va:#x} size={size}");
                }
                crate::bindtrace::Desc::Srv { res, desc } => {
                    rec.resources.insert(res);
                    let _ = writeln!(d, "  {slot} srv res={res:#x} desc={}", desc.map(|x| hex(&x)).unwrap_or_else(|| "null".into()));
                }
                crate::bindtrace::Desc::Uav { res, desc } => {
                    rec.resources.insert(res);
                    let _ = writeln!(d, "  {slot} uav res={res:#x} desc={}", desc.map(|x| hex(&x)).unwrap_or_else(|| "null".into()));
                }
                crate::bindtrace::Desc::Sampler { desc } => {
                    let _ = writeln!(d, "  {slot} sampler desc={}", hex(&desc));
                }
            }
        }
    }
    let _ = std::fs::write(dir.join("descriptors.txt"), d);

    let mut buffers: BTreeMap<u64, (usize, u64, u32)> = BTreeMap::new();
    let mut unresolved = Vec::new();
    for &va in &rec.vas {
        if va == 0 {
            continue;
        }
        match crate::bindtrace::buffer_at(va) {
            Some((base, res, size, heap)) => {
                buffers.insert(base, (res, size, heap));
                rec.resources.remove(&res);
            }
            None => unresolved.push(va),
        }
    }
    let mut rlist = String::new();
    let mut rb = Readback::new(queue);
    if rb.is_none() {
        let _ = writeln!(rlist, "readback objects could not be created; no resource data");
    }
    for (base, (res, size, heap)) in &buffers {
        let state = state_of(*res);
        let mut data = None;
        if *heap == 2 {
            data = crate::bindtrace::read_upload(*base);
        }
        if data.is_none() && *size <= MAX_ONE {
            if let (Some(r), Some(st)) = (rb.as_mut(), state) {
                if r.total + size <= MAX_TOTAL {
                    data = r.read_buffer(*res, *size, st);
                    r.total += size;
                }
            }
        }
        let file = format!("data/buf_{res:x}.bin");
        let saved = match &data {
            Some(v) => std::fs::write(dir.join(&file), v).is_ok(),
            None => false,
        };
        let _ = writeln!(rlist, "buffer res={res:#x} va={base:#x} size={size} heap={heap} state={} data={}",
            state.map(|s| format!("{s:#x}")).unwrap_or_else(|| "unknown".into()), if saved { file } else { "none".into() });
    }
    for va in unresolved {
        let _ = writeln!(rlist, "address {va:#x} not inside a known PC buffer");
    }
    for &res in &rec.resources {
        if res == 0 || buffers.values().any(|b| b.0 == res) {
            continue;
        }
        let desc = res_desc(res);
        let dim = u32at(&desc, 0);
        let state = state_of(res).or_else(|| crate::rtcap::state_of(res));
        let head = format!("resource res={res:#x} desc={} state={}", hex(&desc), state.map(|s| format!("{s:#x}")).unwrap_or_else(|| "unknown".into()));
        let mut saved = "none".to_string();
        if let (Some(r), Some(st)) = (rb.as_mut(), state) {
            if dim == 1 {
                let size = u64::from_le_bytes(desc[0x10..0x18].try_into().unwrap());
                if size <= MAX_ONE && r.total + size <= MAX_TOTAL {
                    if let Some(v) = r.read_buffer(res, size, st) {
                        let file = format!("data/res_{res:x}.bin");
                        if std::fs::write(dir.join(&file), v).is_ok() {
                            saved = file;
                        }
                    }
                    r.total += size;
                }
            } else if (2..=4).contains(&dim) && u32at(&desc, 0x24) == 1 {
                let mips = u16::from_le_bytes([desc[0x1e], desc[0x1f]]).max(1) as u32;
                let arr = if dim == 4 { 1 } else { u16::from_le_bytes([desc[0x1c], desc[0x1d]]).max(1) as u32 };
                if let Some((fps, v)) = r.read_texture(res, &desc, mips * arr, st) {
                    let file = format!("data/tex_{res:x}.bin");
                    let mut b = Vec::with_capacity(8 + fps.len() * 32 + v.len());
                    b.extend_from_slice(b"TBTX");
                    b.extend_from_slice(&(fps.len() as u32).to_le_bytes());
                    for fp in &fps {
                        b.extend_from_slice(fp);
                    }
                    b.extend_from_slice(&v);
                    if std::fs::write(dir.join(&file), b).is_ok() {
                        saved = file;
                    }
                }
            }
        }
        let _ = writeln!(rlist, "{head} data={saved}");
    }
    if let Some(r) = rb {
        let _ = writeln!(rlist, "read back {} bytes from the GPU", r.total);
        r.release();
    }
    crate::replay_ext::flush(queue, &dir, &mut rlist);
    let _ = std::fs::write(dir.join("resources.txt"), rlist);
    let _ = std::fs::write(std::env::temp_dir().join("tombox_capture_now"), "");
    record("d3d12_xs", "replay", &format!("frame recorded into {} ({} lines, {} pipelines, {} root signatures, {} buffers, {} other resources)",
        dir.display(), rec.frame.len(), rec.psos.len(), rec.roots.len(), buffers.len(), rec.resources.len()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_is_lowercase_pairs() {
        assert_eq!(super::hex(&[0x0a, 0xff, 0x10]), "0aff10");
    }
}
