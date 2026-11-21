use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use compat_common::record;

use crate::device_wrap::Guid;
use crate::gpu_wait;

pub const SLOTS: u32 = 4096;
const STATE_COMMON: u32 = 0;
const STATE_UAV: u32 = 0x8;
const STATE_COPY_DEST: u32 = 0x400;
const STATE_COPY_SOURCE: u32 = 0x800;
const STATE_GENERIC_READ: u32 = 0xac3;
const LIST_TYPE_BUNDLE: u32 = 1;
const LIST_TYPE_COPY: u32 = 3;
const QUEUE_ADDREF: usize = 1;
const HEAP_READBACK: u32 = 3;
const DEV_CREATE_ALLOCATOR: usize = 9;
const DEV_CREATE_LIST: usize = 12;
const DEV_CREATE_COMMITTED: usize = 27;
const DEV_CREATE_FENCE: usize = 36;
const RES_MAP: usize = 8;
const LIST_CLOSE: usize = 9;
const LIST_RESET: usize = 10;
const LIST_COPY_BUFFER_REGION: usize = 15;
const ALLOC_RESET: usize = 8;
const FENCE_COMPLETED: usize = 8;
const FENCE_SIGNAL: usize = 10;
const IID_ALLOCATOR: Guid = Guid { d1: 0x6102_dee4, d2: 0xaf59, d3: 0x4b09, d4: [0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24] };
const IID_GRAPHICS_LIST: Guid = Guid { d1: 0x5b16_0d0f, d2: 0xac1b, d3: 0x4185, d4: [0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55] };
const IID_RESOURCE: Guid = Guid { d1: 0x6964_42be, d2: 0xa72e, d3: 0x4059, d4: [0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad] };
const IID_FENCE: Guid = Guid { d1: 0x0a75_3dcf, d2: 0xc4d8, d3: 0x4b91, d4: [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76] };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    pub id: u64,
    pub res: usize,
    pub off: u64,
    pub wide: bool,
    pub func: u32,
    pub value: u64,
    pub mask: u64,
}

struct Checker {
    list: usize,
    alloc: usize,
    ty: u32,
    fence: usize,
    value: u64,
    busy: bool,
    slots: Vec<(u64, u32, bool)>,
}

static PROBES: Mutex<Vec<Probe>> = Mutex::new(Vec::new());
static CHECKERS: Mutex<Vec<Checker>> = Mutex::new(Vec::new());
static READBACK: Mutex<Option<(usize, usize)>> = Mutex::new(None);
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static EXEC_FN: AtomicUsize = AtomicUsize::new(0);
static KNOWN: Mutex<Vec<(usize, u32)>> = Mutex::new(Vec::new());
static WATCHER: std::sync::Once = std::sync::Once::new();
static LOG: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicUsize = AtomicUsize::new(0);

pub fn armed_total() -> usize {
    ARMED.load(Ordering::Relaxed)
}

fn once(n: usize) -> bool {
    LOG.fetch_add(1, Ordering::Relaxed) < n
}

unsafe fn vslot(obj: *mut c_void, i: usize) -> usize {
    unsafe { *(*(obj as *const *const usize)).add(i) }
}

pub fn set_exec(exec: usize) {
    if exec != 0 {
        EXEC_FN.store(exec, Ordering::Relaxed);
    }
}

pub fn probe_target(addr: u64, wide: bool) -> Option<(usize, u64)> {
    let (e, off) = crate::gpu_va::lookup_gpu(addr)?;
    (e.buffer && off + if wide { 8 } else { 4 } <= e.len).then_some((e.res, off))
}

pub fn samplable(list_type: u32) -> bool {
    list_type <= LIST_TYPE_COPY && list_type != LIST_TYPE_BUNDLE
}

pub fn source_barrier(state: u32, list_type: u32) -> Result<Option<(u32, u32)>, &'static str> {
    match state {
        STATE_COMMON | STATE_COPY_SOURCE | STATE_GENERIC_READ => Ok(None),
        STATE_UAV if list_type == LIST_TYPE_COPY => Err("the watched buffer is in UNORDERED_ACCESS, which a copy queue cannot transition"),
        s if list_type == LIST_TYPE_COPY && s != STATE_COPY_DEST => Err("the watched buffer state cannot be transitioned on a copy queue"),
        s => Ok(Some((s, STATE_COPY_SOURCE))),
    }
}

pub fn satisfied(p: &Probe, mem: u64) -> bool {
    gpu_wait::compare(p.func, mem, p.mask, p.value)
}

fn live() -> bool {
    !PROBES.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

unsafe fn readback(dev: *mut c_void) -> Option<(usize, usize)> {
    let mut g = READBACK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = *g {
        return Some(r);
    }
    let heap: [u32; 5] = [HEAP_READBACK, 0, 0, 1, 1];
    let mut d = [0u8; 56];
    d[0..4].copy_from_slice(&1u32.to_le_bytes());
    d[0x10..0x18].copy_from_slice(&(SLOTS as u64 * 8).to_le_bytes());
    d[0x18..0x1c].copy_from_slice(&1u32.to_le_bytes());
    d[0x1c..0x1e].copy_from_slice(&1u16.to_le_bytes());
    d[0x1e..0x20].copy_from_slice(&1u16.to_le_bytes());
    d[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
    d[0x2c..0x30].copy_from_slice(&1u32.to_le_bytes());
    let cc: unsafe extern "system" fn(*mut c_void, *const u32, u32, *const u8, u32, *const c_void, *const Guid, *mut *mut c_void) -> i32 =
        unsafe { core::mem::transmute(vslot(dev, DEV_CREATE_COMMITTED)) };
    let mut res: *mut c_void = core::ptr::null_mut();
    let hr = unsafe { cc(dev, heap.as_ptr(), 0, d.as_ptr(), STATE_COPY_DEST, core::ptr::null(), &IID_RESOURCE, &mut res) };
    if hr < 0 || res.is_null() {
        record("d3d12_xs", "wait_probe", &format!("readback buffer create FAILED hr={hr:#010x}"));
        return None;
    }
    let map: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut u8) -> i32 = unsafe { core::mem::transmute(vslot(res, RES_MAP)) };
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { map(res, 0, core::ptr::null(), &mut p) } < 0 || p.is_null() {
        record("d3d12_xs", "wait_probe", "readback buffer Map FAILED");
        return None;
    }
    *g = Some((res as usize, p as usize));
    *g
}

unsafe fn fence_value(f: usize) -> u64 {
    let c: unsafe extern "system" fn(*mut c_void) -> u64 = unsafe { core::mem::transmute(vslot(f as *mut c_void, FENCE_COMPLETED)) };
    unsafe { c(f as *mut c_void) }
}

unsafe fn checker(dev: *mut c_void, ty: u32) -> Option<usize> {
    let mut g = CHECKERS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = g.iter().position(|c| !c.busy && c.ty == ty && unsafe { fence_value(c.fence) } >= c.value) {
        let c = &mut g[i];
        let ar: unsafe extern "system" fn(*mut c_void) -> i32 = unsafe { core::mem::transmute(vslot(c.alloc as *mut c_void, ALLOC_RESET)) };
        let lr: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 = unsafe { core::mem::transmute(vslot(c.list as *mut c_void, LIST_RESET)) };
        if unsafe { ar(c.alloc as *mut c_void) } < 0 || unsafe { lr(c.list as *mut c_void, c.alloc as *mut c_void, core::ptr::null_mut()) } < 0 {
            return None;
        }
        c.busy = true;
        return Some(i);
    }
    let ca: unsafe extern "system" fn(*mut c_void, u32, *const Guid, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(vslot(dev, DEV_CREATE_ALLOCATOR)) };
    let cl: unsafe extern "system" fn(*mut c_void, u32, u32, *mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(vslot(dev, DEV_CREATE_LIST)) };
    let cf: unsafe extern "system" fn(*mut c_void, u64, u32, *const Guid, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(vslot(dev, DEV_CREATE_FENCE)) };
    let (mut a, mut l, mut f): (*mut c_void, *mut c_void, *mut c_void) = (core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
    if unsafe { ca(dev, ty, &IID_ALLOCATOR, &mut a) } < 0 || a.is_null()
        || unsafe { cl(dev, 0, ty, a, core::ptr::null_mut(), &IID_GRAPHICS_LIST, &mut l) } < 0 || l.is_null()
        || unsafe { cf(dev, 0, 0, &IID_FENCE, &mut f) } < 0 || f.is_null()
    {
        record("d3d12_xs", "wait_probe", &format!("probe list (type {ty}) create FAILED"));
        return None;
    }
    g.push(Checker { list: l as usize, alloc: a as usize, ty, fence: f as usize, value: 0, busy: true, slots: Vec::new() });
    Some(g.len() - 1)
}

unsafe fn submit_check(queue: *mut c_void, ty: u32) {
    let exec = EXEC_FN.load(Ordering::Relaxed);
    let signal = gpu_wait::signal_fn();
    let probes: Vec<Probe> = PROBES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if probes.is_empty() || exec == 0 || signal == 0 || !samplable(ty) {
        return;
    }
    let dev = unsafe { gpu_wait::device_of(queue) };
    if dev.is_null() {
        return;
    }
    let Some((rb, _)) = (unsafe { readback(dev) }) else { return };
    let Some(i) = (unsafe { checker(dev, ty) }) else { return };
    let (list, fence) = {
        let g = CHECKERS.lock().unwrap_or_else(|e| e.into_inner());
        (g[i].list as *mut c_void, g[i].fence)
    };
    let mut slots = Vec::new();
    let cp: unsafe extern "system" fn(*mut c_void, usize, u64, usize, u64, u64) = unsafe { core::mem::transmute(vslot(list, LIST_COPY_BUFFER_REGION)) };
    for p in &probes {
        let st = crate::gpu_va::state(p.res);
        let b = match source_barrier(st, ty) {
            Ok(b) => b,
            Err(why) => {
                if once(8) {
                    record("d3d12_xs", "wait_probe", &format!("wait condition {} not sampled on a type-{ty} queue: {why}", p.id));
                }
                continue;
            }
        };
        let slot = (NEXT_SLOT.fetch_add(1, Ordering::Relaxed) % SLOTS as usize) as u32;
        unsafe {
            if let Some((a, c)) = b { crate::fill::barrier_pub(list, p.res, a, c); }
            cp(list, rb, slot as u64 * 8, p.res, p.off, if p.wide { 8 } else { 4 });
            if let Some((a, c)) = b { crate::fill::barrier_pub(list, p.res, c, a); }
        }
        slots.push((p.id, slot, p.wide));
    }
    let close: unsafe extern "system" fn(*mut c_void) -> i32 = unsafe { core::mem::transmute(vslot(list, LIST_CLOSE)) };
    if unsafe { close(list) } < 0 || slots.is_empty() {
        CHECKERS.lock().unwrap_or_else(|e| e.into_inner())[i].busy = false;
        return;
    }
    let exec: gpu_wait::ExecFn = unsafe { core::mem::transmute(exec) };
    let l = [list];
    unsafe { exec(queue, 1, l.as_ptr()) };
    let value = {
        let mut g = CHECKERS.lock().unwrap_or_else(|e| e.into_inner());
        g[i].value += 1;
        g[i].slots = slots;
        g[i].value
    };
    let s: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = unsafe { core::mem::transmute(signal) };
    unsafe { s(queue, fence as *mut c_void, value) };
    start_watcher();
}

fn start_watcher() {
    WATCHER.call_once(|| {
        std::thread::spawn(|| loop {
            unsafe { sweep() };
            let idle = !live();
            std::thread::sleep(std::time::Duration::from_micros(if idle { 2000 } else { 100 }));
        });
    });
}

unsafe fn sweep() {
    let Some((_, base)) = *READBACK.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    let mut done: Vec<(u64, u64)> = Vec::new();
    {
        let mut g = CHECKERS.lock().unwrap_or_else(|e| e.into_inner());
        for c in g.iter_mut().filter(|c| c.busy && !c.slots.is_empty()) {
            if unsafe { fence_value(c.fence) } < c.value {
                continue;
            }
            for &(id, slot, wide) in &c.slots {
                let p = (base + slot as usize * 8) as *const u8;
                let v = unsafe { if wide { core::ptr::read_volatile(p as *const u64) } else { core::ptr::read_volatile(p as *const u32) as u64 } };
                done.push((id, v));
            }
            c.slots.clear();
            c.busy = false;
        }
    }
    let ready: Vec<u64> = {
        let mut g = PROBES.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Vec::new();
        g.retain(|p| {
            if !gpu_wait::cond_live(p.id) {
                return false;
            }
            if done.iter().any(|&(id, v)| id == p.id && satisfied(p, v)) {
                r.push(p.id);
                return false;
            }
            true
        });
        r
    };
    for id in ready {
        if let Some((f, v)) = gpu_wait::cond_release(id) {
            let s: unsafe extern "system" fn(*mut c_void, u64) -> i32 = unsafe { core::mem::transmute(vslot(f as *mut c_void, FENCE_SIGNAL)) };
            unsafe { s(f as *mut c_void, v) };
        }
    }
}

unsafe fn know(queue: *mut c_void, ty: u32) -> Vec<(usize, u32)> {
    let mut g = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    if samplable(ty) && !g.iter().any(|&(q, _)| q == queue as usize) {
        let add: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { core::mem::transmute(vslot(queue, QUEUE_ADDREF)) };
        unsafe { add(queue) };
        g.push((queue as usize, ty));
    }
    g.clone()
}

pub unsafe fn arm(queue: *mut c_void, ty: u32, p: Probe) {
    ARMED.fetch_add(1, Ordering::Relaxed);
    PROBES.lock().unwrap_or_else(|e| e.into_inner()).push(p);
    unsafe { submit_check(queue, ty) };
    for (q, t) in unsafe { know(queue, ty) } {
        if q != queue as usize {
            unsafe { submit_check(q as *mut c_void, t) };
        }
    }
}

pub unsafe fn after_submit(queue: *mut c_void, ty: u32) {
    unsafe { know(queue, ty) };
    if live() {
        unsafe { submit_check(queue, ty) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_compare_and_choose_copy_source_barriers() {
        let p = Probe { id: 1, res: 0, off: 0, wide: false, func: gpu_wait::FUNC_GREATER_EQUAL, value: 7, mask: u64::MAX };
        assert!(!satisfied(&p, 6));
        assert!(satisfied(&p, 7));
        assert_eq!(source_barrier(STATE_COMMON, 0), Ok(None));
        assert_eq!(source_barrier(STATE_GENERIC_READ, 2), Ok(None));
        assert_eq!(source_barrier(STATE_UAV, 0), Ok(Some((STATE_UAV, STATE_COPY_SOURCE))));
        assert_eq!(source_barrier(STATE_COPY_DEST, LIST_TYPE_COPY), Ok(Some((STATE_COPY_DEST, STATE_COPY_SOURCE))));
        assert!(source_barrier(STATE_UAV, LIST_TYPE_COPY).is_err());
        assert!(source_barrier(0x80, LIST_TYPE_COPY).is_err());
        assert!(samplable(0) && samplable(2) && samplable(3));
        assert!(!samplable(LIST_TYPE_BUNDLE) && !samplable(4) && !samplable(6));
    }
}
