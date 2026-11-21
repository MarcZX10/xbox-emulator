use super::{
    alloc_impl, alloc_shape_impl, create_heap_impl, free_impl, hal_connect, hal_counter, hal_create_impl, hal_destroy, hal_disconnect, hal_num_messages, hal_pop_message, hal_release, hal_submit,
    hal_unsupported, lock, map_apu_locked, msg_capacity, release_shape_impl, service_engine, shape, AcpHal, S_OK,
};
use core::ffi::c_void;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub const MAGIC: &[u8; 8] = b"TBAREC1A";

pub const T_CREATE_HEAP: u8 = 1;
pub const T_ALLOC: u8 = 2;
pub const T_FREE: u8 = 3;
pub const T_ALLOC_SHAPE: u8 = 4;
pub const T_RELEASE_SHAPE: u8 = 5;
pub const T_HAL_CREATE: u8 = 6;
pub const T_CONNECT: u8 = 7;
pub const T_DISCONNECT: u8 = 8;
pub const T_SUBMIT: u8 = 9;
pub const T_POP: u8 = 10;
pub const T_NUM: u8 = 11;
pub const T_DESTROY: u8 = 12;
pub const T_TICK: u8 = 13;
pub const T_MEM: u8 = 14;
pub const T_PATCH: u8 = 15;

pub const NULL_PAYLOAD: u32 = u32::MAX;
const PACKET: usize = 2048;
const TICK_BUDGET_NS: u64 = 2_666_667;

static FORCE: AtomicBool = AtomicBool::new(false);
static REPLAYING: AtomicBool = AtomicBool::new(false);
static OPEN_TRIED: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();
    !REPLAYING.load(Ordering::Relaxed) && (FORCE.load(Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var("TOMBOX_AUDIO_RECORD").is_ok_and(|v| v.trim() == "1")))
}

pub fn replaying() -> bool {
    REPLAYING.load(Ordering::Relaxed)
}

pub fn payload_len(kind: u32) -> usize {
    match kind {
        shape::CMD_FLOWGRAPH => 8,
        shape::CMD_CONFIG | shape::CMD_XMA_ENABLE_ONE | shape::CMD_XMA_DISABLE_ONE => 4,
        shape::CMD_XMA_ENABLE_MASK | shape::CMD_XMA_DISABLE_MASK => 64,
        shape::CMD_XMA_UPDATE => 12,
        shape::CMD_DMA_CONSUMED | shape::CMD_XMA_OUT_CONSUMED => 8,
        shape::CMD_XMA_SET_INPUT => 24,
        _ => 0,
    }
}

unsafe fn rd32(va: usize, off: usize) -> u32 {
    core::ptr::read_unaligned((va + off) as *const u32)
}

fn le(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn word(p: &[u8], k: usize) -> u32 {
    p.get(k * 4..k * 4 + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn contiguous(apu: u32, len: usize) -> Option<usize> {
    if apu == 0 || len == 0 {
        return None;
    }
    let last = apu.checked_add(len as u32 - 1)?;
    let va = map_apu_locked(apu);
    (va != 0 && map_apu_locked(last) == va + len - 1).then_some(va)
}

struct Region {
    apu: u32,
    va: usize,
    shadow: Vec<u32>,
}

pub struct Rec {
    out: BufWriter<File>,
    epoch: Instant,
    last_flush: Instant,
    regions: Vec<Region>,
    xma: (usize, u32),
    packets: HashMap<u32, Box<[u8]>>,
}

static REC: Mutex<Option<Rec>> = Mutex::new(None);

impl Rec {
    fn new(path: &Path) -> std::io::Result<Rec> {
        let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
        out.write_all(MAGIC)?;
        Ok(Rec { out, epoch: Instant::now(), last_flush: Instant::now(), regions: Vec::new(), xma: (0, 0), packets: HashMap::new() })
    }

    pub fn write(&mut self, tag: u8, payload: &[u8]) {
        let t = self.epoch.elapsed().as_nanos() as u64;
        let mut h = [0u8; 13];
        h[0] = tag;
        h[1..9].copy_from_slice(&t.to_le_bytes());
        h[9..13].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        let _ = self.out.write_all(&h);
        let _ = self.out.write_all(payload);
    }

    fn mem_va(&mut self, apu: u32, va: usize, len: usize) {
        let mut p = Vec::with_capacity(4 + len);
        p.extend_from_slice(&apu.to_le_bytes());
        p.extend_from_slice(unsafe { core::slice::from_raw_parts(va as *const u8, len) });
        self.write(T_MEM, &p);
    }

    fn mem_apu(&mut self, apu: u32, len: usize) {
        if let Some(va) = contiguous(apu, len) {
            self.mem_va(apu, va, len);
        }
    }

    fn scan_buf(&mut self, apu: u32, count: u32) {
        let count = count.min(0xfff) as usize;
        if count == 0 {
            return;
        }
        let Some(va) = contiguous(apu, count * PACKET) else { return };
        for k in 0..count {
            let a = apu + (k * PACKET) as u32;
            let s = unsafe { core::slice::from_raw_parts((va + k * PACKET) as *const u8, PACKET) };
            if self.packets.get(&a).is_some_and(|old| **old == *s) {
                continue;
            }
            self.packets.insert(a, s.into());
            self.mem_va(a, va + k * PACKET, PACKET);
        }
    }

    fn scan_ctx(&mut self, p: usize, only_valid: bool) {
        let (d0, d1, b0, b1) = unsafe { (rd32(p, 0), rd32(p, 4), rd32(p, 0x14), rd32(p, 0x18)) };
        if !only_valid || d0 & (1 << 20) != 0 {
            self.scan_buf(b0, d0 & 0xfff);
        }
        if !only_valid || d0 & (1 << 21) != 0 {
            self.scan_buf(b1, d1 & 0xfff);
        }
    }

    fn scan(&mut self) {
        let (va, n) = self.xma;
        if va == 0 {
            return;
        }
        for i in 0..n as usize {
            self.scan_ctx(va + i * shape::XMA_CTX, true);
        }
    }

    fn forget(&mut self, apu: u32, size: u32) {
        let end = apu as u64 + size as u64;
        self.packets.retain(|&a, _| (a as u64 + PACKET as u64) <= apu as u64 || a as u64 >= end);
    }

    fn diff(&mut self) {
        let mut patches: Vec<(u32, Vec<(u32, u32)>)> = Vec::new();
        for r in self.regions.iter_mut() {
            let n = r.shadow.len();
            let mut i = 0;
            while i < n {
                let live = unsafe { rd32(r.va, i * 4) };
                if live == r.shadow[i] {
                    i += 1;
                    continue;
                }
                let start = i;
                let mut items = Vec::new();
                let mut gap = 0;
                while i < n && gap <= 4 {
                    let live = unsafe { rd32(r.va, i * 4) };
                    let old = r.shadow[i];
                    gap = if live != old { 0 } else { gap + 1 };
                    items.push((old ^ live, live));
                    r.shadow[i] = live;
                    i += 1;
                }
                while items.last().is_some_and(|x| x.0 == 0) {
                    items.pop();
                }
                patches.push((r.apu + start as u32 * 4, items));
            }
        }
        for (apu, items) in patches {
            let mut p = Vec::with_capacity(8 + items.len() * 8);
            p.extend_from_slice(&apu.to_le_bytes());
            p.extend_from_slice(&(items.len() as u32).to_le_bytes());
            for (m, v) in items {
                p.extend_from_slice(&m.to_le_bytes());
                p.extend_from_slice(&v.to_le_bytes());
            }
            self.write(T_PATCH, &p);
        }
    }

    fn refresh(&mut self) {
        let log = std::mem::take(&mut *shape::WRITE_LOG.lock().unwrap_or_else(|e| e.into_inner()));
        for (addr, mask) in log {
            for r in self.regions.iter_mut() {
                if addr < r.va || addr >= r.va + r.shadow.len() * 4 {
                    continue;
                }
                let off = addr - r.va;
                let i = off / 4;
                if off % 4 != 0 {
                    let live = unsafe { rd32(r.va, i * 4) };
                    r.shadow[i] = live;
                    if i + 1 < r.shadow.len() {
                        r.shadow[i + 1] = unsafe { rd32(r.va, (i + 1) * 4) };
                    }
                } else {
                    let live = unsafe { rd32(r.va, i * 4) };
                    r.shadow[i] = (r.shadow[i] & !mask) | (live & mask);
                }
                break;
            }
        }
    }

    fn install(&mut self, n: [u32; 6], ptrs: [usize; 6], apus: [u32; 6]) {
        self.regions.clear();
        let xma_base = if n[4] != 0 { 4 } else { 5 };
        let list = [(apus[0], ptrs[0], n[0] as usize * shape::SRC_CTX), (apus[3], ptrs[3], n[3] as usize * shape::DMA_CTX), (apus[xma_base], ptrs[xma_base], n[4] as usize * shape::XMA_CTX + n[5] as usize * 0x20)];
        for (apu, va, len) in list {
            if va == 0 || len == 0 {
                continue;
            }
            self.mem_va(apu, va, len);
            let shadow = (0..len / 4).map(|i| unsafe { rd32(va, i * 4) }).collect();
            self.regions.push(Region { apu, va, shadow });
        }
        self.xma = (ptrs[4], n[4]);
    }

    fn flush_maybe(&mut self, force: bool) {
        if force || self.last_flush.elapsed().as_millis() >= 500 {
            let _ = self.out.flush();
            self.last_flush = Instant::now();
        }
    }
}

fn default_path() -> PathBuf {
    let dir = std::env::var_os("TOMBOX_AUDIO_RECORD_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("TomBox").join("audio-rec")
    });
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    dir.join(format!("acphal-{}-{secs}.tbrec", std::process::id()))
}

fn guard() -> MutexGuard<'static, Option<Rec>> {
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() && !OPEN_TRIED.swap(true, Ordering::SeqCst) {
        let path = default_path();
        match Rec::new(&path) {
            Ok(r) => {
                compat_common::record("acphal", "audiorec", &format!("recording to {}", path.display()));
                *g = Some(r);
                shape::WRITE_LOG_ON.store(true, Ordering::SeqCst);
            }
            Err(e) => compat_common::record("acphal", "audiorec", &format!("cannot create {}: {e}", path.display())),
        }
    }
    g
}

pub fn record_to(path: &Path) -> std::io::Result<()> {
    let r = Rec::new(path)?;
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    OPEN_TRIED.store(true, Ordering::SeqCst);
    *g = Some(r);
    shape::WRITE_LOG_ON.store(true, Ordering::SeqCst);
    FORCE.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn stop_recording() {
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = g.as_mut() {
        r.flush_maybe(true);
    }
    *g = None;
    shape::WRITE_LOG_ON.store(false, Ordering::SeqCst);
    shape::WRITE_LOG.lock().unwrap_or_else(|e| e.into_inner()).clear();
    FORCE.store(false, Ordering::SeqCst);
}

fn op<R>(pre: impl FnOnce(&mut Rec), f: impl FnOnce() -> R, post: impl FnOnce(&mut Rec, &R)) -> R {
    if !enabled() {
        return f();
    }
    let mut g = guard();
    let Some(rec) = g.as_mut() else {
        drop(g);
        return f();
    };
    rec.diff();
    pre(rec);
    rec.scan();
    shape::WRITE_LOG.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let res = f();
    post(rec, &res);
    rec.refresh();
    rec.flush_maybe(false);
    res
}

pub(crate) fn tick(f: fn()) {
    op(|_| {}, f, |r, _| r.write(T_TICK, &[]));
}

#[no_mangle]
pub extern "system" fn ApuCreateHeap(size_a: u32, size_b: u32) -> i32 {
    op(|_| {}, || create_heap_impl(size_a, size_b), |r, hr| r.write(T_CREATE_HEAP, &le(&[size_a, size_b, *hr as u32])))
}

#[no_mangle]
pub extern "system" fn ApuAlloc(pp: *mut *mut c_void, p_apu: *mut u32, size: u32, align: u32, heap: u32) -> i32 {
    op(
        |_| {},
        || alloc_impl(pp, p_apu, size, align, heap),
        |r, hr| {
            let apu = if *hr == S_OK && !pp.is_null() { lock().map_va(unsafe { *pp } as usize) } else { 0 };
            r.write(T_ALLOC, &le(&[size, align, heap, *hr as u32, apu]));
        },
    )
}

#[no_mangle]
pub extern "system" fn ApuFree(p: *mut c_void) -> i32 {
    op(
        |_| {},
        || {
            let (apu, size) = {
                let s = lock();
                (s.map_va(p as usize), s.user_size(p as usize))
            };
            (free_impl(p), apu, size)
        },
        |r, (hr, apu, size)| {
            r.write(T_FREE, &le(&[*apu, *hr as u32]));
            if *hr == S_OK {
                r.forget(*apu, *size);
            }
        },
    )
    .0
}

#[no_mangle]
pub extern "system" fn AcpHalAllocateShapeContexts(ctx: *mut u8) -> i32 {
    op(
        |_| {},
        || alloc_shape_impl(ctx),
        |r, hr| {
            if ctx.is_null() {
                r.write(T_ALLOC_SHAPE, &le(&[0, 0, 0, 0, 0, 0, *hr as u32, 0, 0, 0, 0, 0, 0]));
                return;
            }
            let n: [u32; 6] = core::array::from_fn(|k| unsafe { rd32(ctx as usize, k * 4) });
            let ok = *hr == S_OK;
            let ptrs: [usize; 6] = core::array::from_fn(|k| if ok { unsafe { core::ptr::read_unaligned(ctx.add(0x18 + k * 8) as *const usize) } } else { 0 });
            let apus: [u32; 6] = core::array::from_fn(|k| if ok { unsafe { rd32(ctx as usize, 0x48 + k * 4) } } else { 0 });
            let mut v = n.to_vec();
            v.push(*hr as u32);
            v.extend_from_slice(&apus);
            r.write(T_ALLOC_SHAPE, &le(&v));
            if ok {
                r.install(n, ptrs, apus);
            }
        },
    )
}

#[no_mangle]
pub extern "system" fn AcpHalReleaseShapeContexts() -> i32 {
    op(
        |_| {},
        || release_shape_impl(),
        |r, hr| {
            r.write(T_RELEASE_SHAPE, &le(&[*hr as u32]));
            if *hr == S_OK {
                r.regions.clear();
                r.xma = (0, 0);
                r.packets.clear();
            }
            r.flush_maybe(true);
        },
    )
}

#[no_mangle]
pub extern "system" fn AcpHalCreate(out: *mut *mut c_void) -> i32 {
    op(
        |_| {},
        || hal_create_impl(out),
        |r, hr| {
            if *hr == S_OK {
                let id = unsafe { (*(*out as *mut AcpHal)).id };
                r.write(T_HAL_CREATE, &le(&[id]));
            }
        },
    )
}

unsafe extern "system" fn rec_connect(this: *mut AcpHal, nc: u32, nm: u32) -> i32 {
    let id = (*this).id;
    op(|_| {}, || hal_connect(this, nc, nm), |r, hr| r.write(T_CONNECT, &le(&[id, nc, nm, *hr as u32])))
}

unsafe extern "system" fn rec_disconnect(this: *mut AcpHal) -> i32 {
    let id = (*this).id;
    op(|_| {}, || hal_disconnect(this), |r, hr| r.write(T_DISCONNECT, &le(&[id, *hr as u32])))
}

unsafe extern "system" fn rec_submit(this: *mut AcpHal, kind: u32, id: u64, frame: u32, payload: *const u8, fence: u32) -> i32 {
    let hid = (*this).id;
    let plen = payload_len(kind);
    let bytes: Option<Vec<u8>> = (!payload.is_null()).then(|| core::slice::from_raw_parts(payload, plen).to_vec());
    op(
        |r| {
            let Some(p) = bytes.as_ref() else { return };
            match kind {
                shape::CMD_FLOWGRAPH => r.mem_apu(word(p, 1), word(p, 0).min(0x10000) as usize * 8),
                shape::CMD_XMA_UPDATE => {
                    r.mem_apu(word(p, 2), shape::XMA_CTX);
                    if let Some(va) = contiguous(word(p, 2), shape::XMA_CTX) {
                        r.scan_ctx(va, false);
                    }
                }
                shape::CMD_XMA_SET_INPUT => r.scan_buf(word(p, 5), word(p, 2)),
                _ => {}
            }
        },
        || hal_submit(this, kind, id, frame, payload, fence),
        |r, hr| {
            let mut v = le(&[hid, kind, id as u32, (id >> 32) as u32, frame, fence, *hr as u32, bytes.as_ref().map_or(NULL_PAYLOAD, |b| b.len() as u32)]);
            if let Some(b) = bytes.as_ref() {
                v.extend_from_slice(b);
            }
            r.write(T_SUBMIT, &v);
        },
    )
}

unsafe extern "system" fn rec_pop(this: *mut AcpHal, out: *mut u8) -> i32 {
    let hid = (*this).id;
    op(
        |_| {},
        || hal_pop_message(this, out),
        |r, ret| {
            let mut v = le(&[hid, *ret as u32]);
            let mut m = [0u8; 28];
            if *ret == 1 && !out.is_null() {
                core::ptr::copy_nonoverlapping(out, m.as_mut_ptr(), 28);
            }
            v.extend_from_slice(&m);
            r.write(T_POP, &v);
        },
    )
}

unsafe extern "system" fn rec_num(this: *mut AcpHal) -> u32 {
    let hid = (*this).id;
    op(|_| {}, || hal_num_messages(this), |r, ret| r.write(T_NUM, &le(&[hid, *ret])))
}

unsafe extern "system" fn rec_release(this: *mut AcpHal) -> u32 {
    let hid = (*this).id;
    op(
        |_| {},
        || hal_release(this),
        |r, _| {
            r.write(T_DESTROY, &le(&[hid, 1]));
            r.flush_maybe(true);
        },
    )
}

unsafe extern "system" fn rec_destroy(this: *mut AcpHal, flags: u32) -> *mut AcpHal {
    let hid = (*this).id;
    op(
        |_| {},
        || hal_destroy(this, flags),
        |r, _| {
            r.write(T_DESTROY, &le(&[hid, flags]));
            r.flush_maybe(true);
        },
    )
}

pub(crate) fn recording_vtable() -> [usize; 12] {
    [
        rec_connect as *const () as usize,
        rec_disconnect as *const () as usize,
        rec_submit as *const () as usize,
        rec_pop as *const () as usize,
        rec_num as *const () as usize,
        rec_release as *const () as usize,
        hal_unsupported as *const () as usize,
        hal_unsupported as *const () as usize,
        hal_unsupported as *const () as usize,
        hal_unsupported as *const () as usize,
        hal_counter as *const () as usize,
        rec_destroy as *const () as usize,
    ]
}

pub struct Output {
    pub name: String,
    pub channels: u16,
    pub rate: u32,
    pub float: bool,
    pub data: Vec<u8>,
}

pub struct Replay {
    pub report: String,
    pub csv: String,
    pub outputs: Vec<Output>,
}

pub fn read_record(rd: &mut impl Read) -> std::io::Result<Option<(u8, u64, Vec<u8>)>> {
    let mut h = [0u8; 13];
    match rd.read_exact(&mut h) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let t = u64::from_le_bytes(h[1..9].try_into().unwrap());
    let len = u32::from_le_bytes(h[9..13].try_into().unwrap()) as usize;
    let mut p = vec![0u8; len];
    if rd.read_exact(&mut p).is_err() {
        return Ok(None);
    }
    Ok(Some((h[0], t, p)))
}

pub fn write_record(w: &mut impl Write, tag: u8, t_ns: u64, payload: &[u8]) -> std::io::Result<()> {
    w.write_all(&[tag])?;
    w.write_all(&t_ns.to_le_bytes())?;
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)
}

struct DecStat {
    ns: u64,
    frames: u64,
    rate: u32,
}

static DEC_STATS: Mutex<Vec<DecStat>> = Mutex::new(Vec::new());

struct TimedXma {
    inner: Box<dyn shape::XmaDecoder>,
    ch: usize,
    rate: u32,
}

impl shape::XmaDecoder for TimedXma {
    fn decode_packet(&mut self, packet: &[u8], out: &mut Vec<i16>) {
        let n0 = out.len();
        let t = Instant::now();
        self.inner.decode_packet(packet, out);
        let ns = t.elapsed().as_nanos() as u64;
        let frames = ((out.len() - n0) / self.ch.max(1)) as u64;
        DEC_STATS.lock().unwrap_or_else(|e| e.into_inner()).push(DecStat { ns, frames, rate: self.rate });
    }
}

fn timed_factory(ch: usize, rate: u32) -> Box<dyn shape::XmaDecoder> {
    Box::new(TimedXma { inner: shape::xma_factory(ch, rate), ch, rate })
}

#[derive(Default, Clone, Copy)]
struct OpStat {
    n: u64,
    total: u64,
    max: u64,
    max_t: u64,
    over: u64,
}

impl OpStat {
    fn add(&mut self, ns: u64, t: u64, budget: u64) {
        self.n += 1;
        self.total += ns;
        if ns > self.max {
            self.max = ns;
            self.max_t = t;
        }
        if budget != 0 && ns > budget {
            self.over += 1;
        }
    }
}

fn tag_name(t: u8) -> &'static str {
    match t {
        T_CREATE_HEAP => "CreateHeap",
        T_ALLOC => "Alloc",
        T_FREE => "Free",
        T_ALLOC_SHAPE => "AllocShape",
        T_RELEASE_SHAPE => "ReleaseShape",
        T_HAL_CREATE => "HalCreate",
        T_CONNECT => "Connect",
        T_DISCONNECT => "Disconnect",
        T_SUBMIT => "Submit",
        T_POP => "PopMessage",
        T_NUM => "GetNumMessages",
        T_DESTROY => "Destroy",
        T_TICK => "Tick",
        T_MEM => "Mem",
        T_PATCH => "Patch",
        _ => "?",
    }
}

#[derive(Default)]
struct RState {
    hals: HashMap<u32, *mut AcpHal>,
    shape: Option<([u32; 6], [usize; 6])>,
    last_xma: Vec<u32>,
    last_dma: Vec<u32>,
    xma_out: HashMap<u32, (u16, u32, usize)>,
    dma_out: HashMap<u32, usize>,
    outputs: Vec<Output>,
    produced: HashMap<u32, u32>,
    mismatches: HashMap<&'static str, u64>,
    examples: Vec<String>,
    stats: HashMap<u8, OpStat>,
    msg_kinds: HashMap<u32, u64>,
    saturated: u64,
    enables: u64,
    enables_empty: u64,
    empty_examples: Vec<String>,
    enable_blocks: u64,
    queue_full_rec: u64,
    queue_full_replay: u64,
    unmapped: u64,
    max_depth: usize,
    max_ready: u32,
    last_t: u64,
    records: u64,
}

impl RState {
    fn mismatch(&mut self, what: &'static str, detail: String) {
        *self.mismatches.entry(what).or_default() += 1;
        if self.examples.len() < 40 {
            self.examples.push(detail);
        }
    }

    fn hal(&self, id: u32) -> Option<*mut AcpHal> {
        self.hals.get(&id).copied()
    }

    fn xma_ctx(&self, i: usize) -> Option<usize> {
        let (n, p) = self.shape?;
        (i < n[4] as usize && p[4] != 0).then(|| p[4] + i * shape::XMA_CTX)
    }

    fn dma_ctx(&self, i: usize) -> Option<usize> {
        let (n, p) = self.shape?;
        (i < n[3] as usize && p[3] != 0).then(|| p[3] + i * shape::DMA_CTX)
    }

    fn sync_indices(&mut self) {
        let (nx, nd) = self.shape.map_or((0, 0), |(n, _)| (n[4] as usize, n[3] as usize));
        self.last_xma = (0..nx).map(|i| self.xma_ctx(i).map_or(0, |p| unsafe { xma_w(p) })).collect();
        self.last_dma = (0..nd).map(|i| self.dma_ctx(i).map_or(0, |p| unsafe { dma_w(p) })).collect();
    }

    fn capture(&mut self) {
        self.produced.clear();
        let (nx, nd) = self.shape.map_or((0, 0), |(n, _)| (n[4] as usize, n[3] as usize));
        if self.last_xma.len() != nx || self.last_dma.len() != nd {
            self.sync_indices();
            return;
        }
        for i in 0..nx {
            let Some(p) = self.xma_ctx(i) else { continue };
            unsafe {
                let c = xma_ring(p);
                let w = xma_w(p);
                let lw = self.last_xma[i];
                self.last_xma[i] = w;
                if w == lw {
                    continue;
                }
                let k = (w + c - lw) % c;
                self.produced.insert(i as u32, k);
                let out = map_apu_locked(rd32(p, 0x1c));
                if out == 0 {
                    continue;
                }
                let d1 = rd32(p, 4);
                let ch = ((d1 >> 29) & 1) as u16 + 1;
                let rate = shape::sample_rate_code((d1 >> 27) & 3);
                let slot = match self.xma_out.get(&(i as u32)) {
                    Some(&(c0, r0, s)) if c0 == ch && r0 == rate => s,
                    _ => {
                        let seg = self.outputs.iter().filter(|o| o.name.starts_with(&format!("xma{i:03}_"))).count();
                        self.outputs.push(Output { name: format!("xma{i:03}_{seg}_{ch}ch_{rate}"), channels: ch, rate, float: false, data: Vec::new() });
                        self.xma_out.insert(i as u32, (ch, rate, self.outputs.len() - 1));
                        self.outputs.len() - 1
                    }
                };
                for j in 0..k {
                    let blk = ((lw + j) % c) as usize;
                    let s = core::slice::from_raw_parts((out + blk * shape::FRAME * 2) as *const u8, shape::FRAME * 2);
                    self.outputs[slot].data.extend_from_slice(s);
                }
            }
        }
        for d in 0..nd {
            let Some(p) = self.dma_ctx(d) else { continue };
            unsafe {
                let w = dma_w(p);
                let lw = self.last_dma[d];
                self.last_dma[d] = w;
                if w == lw {
                    continue;
                }
                let d3 = rd32(p, 0xc);
                let count = dma_size(p);
                let chans = ((d3 >> 16) & 7) + 1;
                let ch = (d3 >> 24) & 7;
                let base = map_apu_locked(rd32(p, 8) & !0x1ff);
                if base == 0 {
                    continue;
                }
                let slot = *self.dma_out.entry(d as u32).or_insert_with(|| {
                    self.outputs.push(Output { name: format!("dma{d:04}_ch{ch}_48000"), channels: 1, rate: 48000, float: true, data: Vec::new() });
                    self.outputs.len() - 1
                });
                let k = (w + count - lw) % count;
                for j in 0..k {
                    let blk = (lw + j) % count;
                    let off = ((blk * chans + ch) as usize) * shape::FRAME * 4;
                    let s = core::slice::from_raw_parts((base + off) as *const u8, shape::FRAME * 4);
                    self.outputs[slot].data.extend_from_slice(s);
                }
            }
        }
    }

    fn ring_stats(&self) -> (u32, u32) {
        let nx = self.shape.map_or(0, |(n, _)| n[4] as usize);
        let mut ready = 0;
        let mut active = 0;
        for i in 0..nx {
            let Some(p) = self.xma_ctx(i) else { continue };
            unsafe {
                let d0 = rd32(p, 0);
                let d1 = rd32(p, 4);
                if d1 & (1 << 31) == 0 {
                    continue;
                }
                let c = xma_ring(p);
                ready += (xma_w(p) + c - (rd32(p, 0x24) & 31) % c) % c;
                if d0 & (3 << 20) != 0 {
                    active += 1;
                }
            }
        }
        (ready, active)
    }

    fn depth(&self) -> usize {
        self.hals.values().map(|&h| unsafe { (*h).messages.len() }).sum()
    }
}

unsafe fn xma_ring(p: usize) -> u32 {
    match (rd32(p, 0) >> 22) & 31 {
        0 => 32,
        c => c,
    }
}

unsafe fn xma_w(p: usize) -> u32 {
    (rd32(p, 0) >> 27) % xma_ring(p)
}

unsafe fn dma_size(p: usize) -> u32 {
    (rd32(p, 0xc) & 31) + 1
}

unsafe fn dma_w(p: usize) -> u32 {
    ((rd32(p, 4) >> 8) & 31) % dma_size(p)
}

fn check(st: &mut RState, what: &'static str, t: u64, rec: u32, got: u32) {
    if rec != got {
        st.mismatch(what, format!("t={:.3}ms {what}: recorded {rec:#x}, replay {got:#x}", t as f64 / 1e6));
    }
}

pub fn replay(path: &Path) -> Result<Replay, String> {
    REPLAYING.store(true, Ordering::SeqCst);
    shape::set_decoder_factory(timed_factory);
    DEC_STATS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rd = BufReader::with_capacity(1 << 20, f);
    let mut magic = [0u8; 8];
    rd.read_exact(&mut magic).map_err(|e| format!("header: {e}"))?;
    if &magic != MAGIC {
        return Err(format!("{}: not an acphal recording (magic {:?})", path.display(), String::from_utf8_lossy(&magic)));
    }
    let mut st = RState::default();
    let mut csv = String::from("t_ms,op,kind,op_us,msg_depth,xma_ready_blocks,xma_active\n");
    let mut zero = vec![0u8; 256];
    let trace: Option<(f64, f64)> = std::env::var("TOMBOX_REPLAY_TRACE").ok().and_then(|s| {
        let (a, b) = s.split_once(':')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    });
    while let Some((tag, t, p)) = read_record(&mut rd).map_err(|e| e.to_string())? {
        let tracing = trace.is_some_and(|(a, b)| (a..=b).contains(&(t as f64 / 1e6)));
        if tracing {
            eprintln!("{:9.3} {} {}", t as f64 / 1e6, tag_name(tag), p.iter().take(48).map(|b| format!("{b:02x}")).collect::<String>());
        }
        st.records += 1;
        st.last_t = t;
        let w = |k: usize| word(&p, k);
        if tag == T_MEM || tag == T_PATCH {
            let apu = w(0);
            let len = if tag == T_MEM { p.len().saturating_sub(4) } else { w(1) as usize * 4 };
            match contiguous(apu, len) {
                Some(va) => unsafe {
                    if tag == T_MEM {
                        core::ptr::copy_nonoverlapping(p[4..].as_ptr(), va as *mut u8, len);
                    } else {
                        for k in 0..w(1) as usize {
                            let m = w(2 + k * 2);
                            let v = w(3 + k * 2);
                            let q = (va + k * 4) as *mut u32;
                            core::ptr::write_unaligned(q, (core::ptr::read_unaligned(q) & !m) | (v & m));
                        }
                    }
                },
                None => {
                    st.unmapped += 1;
                    if st.examples.len() < 40 {
                        st.examples.push(format!("t={:.3}ms {} to unmapped APU {apu:#x} len {len:#x}", t as f64 / 1e6, tag_name(tag)));
                    }
                }
            }
            st.sync_indices();
            continue;
        }
        let depth0: HashMap<u32, usize> = st.hals.iter().map(|(&k, &h)| (k, unsafe { (*h).messages.len() })).collect();
        let t0 = Instant::now();
        let mut kind = 0u32;
        match tag {
            T_CREATE_HEAP => {
                let hr = ApuCreateHeap(w(0), w(1));
                check(&mut st, "CreateHeap hr", t, w(2), hr as u32);
            }
            T_ALLOC => {
                let mut va: *mut c_void = core::ptr::null_mut();
                let mut apu = 0u32;
                let hr = ApuAlloc(&mut va, &mut apu, w(0), w(1), w(2));
                check(&mut st, "Alloc hr", t, w(3), hr as u32);
                check(&mut st, "Alloc address", t, w(4), apu);
            }
            T_FREE => {
                let va = map_apu_locked(w(0));
                let hr = if va == 0 { super::E_INVALIDARG } else { ApuFree(va as *mut c_void) };
                check(&mut st, "Free hr", t, w(1), hr as u32);
            }
            T_ALLOC_SHAPE => {
                let mut ctx = [0u8; 0x60];
                for k in 0..6 {
                    ctx[k * 4..k * 4 + 4].copy_from_slice(&w(k).to_le_bytes());
                }
                let hr = AcpHalAllocateShapeContexts(ctx.as_mut_ptr());
                check(&mut st, "AllocShape hr", t, w(6), hr as u32);
                if hr == S_OK {
                    let n: [u32; 6] = core::array::from_fn(w);
                    let ptrs: [usize; 6] = core::array::from_fn(|k| usize::from_le_bytes(ctx[0x18 + k * 8..0x20 + k * 8].try_into().unwrap()));
                    for k in 0..6 {
                        check(&mut st, "AllocShape address", t, w(7 + k), u32::from_le_bytes(ctx[0x48 + k * 4..0x4c + k * 4].try_into().unwrap()));
                    }
                    st.shape = Some((n, ptrs));
                    st.sync_indices();
                }
            }
            T_RELEASE_SHAPE => {
                let hr = AcpHalReleaseShapeContexts();
                check(&mut st, "ReleaseShape hr", t, w(0), hr as u32);
                if hr == S_OK {
                    st.shape = None;
                    st.sync_indices();
                }
            }
            T_HAL_CREATE => {
                let mut out: *mut c_void = core::ptr::null_mut();
                if AcpHalCreate(&mut out) == S_OK {
                    st.hals.insert(w(0), out as *mut AcpHal);
                }
            }
            T_CONNECT => match st.hal(w(0)) {
                Some(h) => {
                    let hr = unsafe { hal_connect(h, w(1), w(2)) };
                    check(&mut st, "Connect hr", t, w(3), hr as u32);
                }
                None => st.mismatch("unknown HAL", format!("Connect on HAL {}", w(0))),
            },
            T_DISCONNECT => match st.hal(w(0)) {
                Some(h) => {
                    let hr = unsafe { hal_disconnect(h) };
                    check(&mut st, "Disconnect hr", t, w(1), hr as u32);
                }
                None => st.mismatch("unknown HAL", format!("Disconnect on HAL {}", w(0))),
            },
            T_SUBMIT => {
                kind = w(1);
                let id = w(2) as u64 | (w(3) as u64) << 32;
                let plen = w(7);
                let rec_hr = w(6);
                let payload: *const u8 = if plen == NULL_PAYLOAD {
                    core::ptr::null()
                } else {
                    let b = &p[32..];
                    zero.iter_mut().for_each(|x| *x = 0);
                    zero[..b.len().min(256)].copy_from_slice(&b[..b.len().min(256)]);
                    zero.as_ptr()
                };
                if rec_hr == super::HAL_E_QUEUE_FULL as u32 {
                    st.queue_full_rec += 1;
                }
                match st.hal(w(0)) {
                    Some(h) => {
                        let hr = unsafe { hal_submit(h, kind, id, w(4), payload, w(5)) };
                        if hr == super::HAL_E_QUEUE_FULL {
                            st.queue_full_replay += 1;
                        }
                        check(&mut st, "Submit hr", t, rec_hr, hr as u32);
                    }
                    None => st.mismatch("unknown HAL", format!("Submit on HAL {}", w(0))),
                }
            }
            T_POP => match st.hal(w(0)) {
                Some(h) => {
                    let mut m = [0u8; 28];
                    let ret = unsafe { hal_pop_message(h, m.as_mut_ptr()) };
                    check(&mut st, "PopMessage ret", t, w(1), ret as u32);
                    if ret == 1 {
                        *st.msg_kinds.entry(u32::from_le_bytes(m[0..4].try_into().unwrap())).or_default() += 0;
                        if w(1) == 1 {
                            check(&mut st, "PopMessage kind", t, word(&p, 2), u32::from_le_bytes(m[0..4].try_into().unwrap()));
                        }
                    }
                }
                None => st.mismatch("unknown HAL", format!("Pop on HAL {}", w(0))),
            },
            T_NUM => match st.hal(w(0)) {
                Some(h) => {
                    let ret = unsafe { hal_num_messages(h) };
                    check(&mut st, "GetNumMessages", t, w(1), ret);
                }
                None => st.mismatch("unknown HAL", format!("GetNum on HAL {}", w(0))),
            },
            T_DESTROY => {
                if let Some(h) = st.hal(w(0)) {
                    unsafe { hal_destroy(h, w(1)) };
                    if w(1) & 1 != 0 {
                        st.hals.remove(&w(0));
                    }
                }
            }
            T_TICK => service_engine(),
            _ => st.mismatch("unknown record", format!("tag {tag} at t={t}")),
        }
        let ns = t0.elapsed().as_nanos() as u64;
        if tracing {
            let nx = st.shape.map_or(0, |(n, _)| n[4] as usize);
            for i in 0..nx {
                if let Some(c) = st.xma_ctx(i) {
                    let d: Vec<u32> = (0..10).map(|k| unsafe { rd32(c, k * 4) }).collect();
                    if d[1] & (1 << 31) != 0 {
                        eprintln!("          xma{i} {:08x?}", d);
                    }
                }
            }
            for h in st.hals.values() {
                let q = unsafe { &(**h).messages };
                eprintln!("          queue {:?}", q.iter().map(|m| m[0]).collect::<Vec<_>>());
            }
        }
        let budget = if tag == T_TICK { TICK_BUDGET_NS } else { 0 };
        st.stats.entry(tag).or_default().add(ns, t, budget);
        st.capture();
        if tag == T_SUBMIT {
            if let Some(h) = st.hal(w(0)) {
                let (len1, cap) = unsafe { ((*h).messages.len(), msg_capacity(&*h)) };
                let len0 = depth0.get(&w(0)).copied().unwrap_or(0);
                if len1 >= cap {
                    st.saturated += 1;
                } else if len1 > len0 {
                    let q = unsafe { &(*h).messages };
                    for m in q.iter().skip(len0) {
                        *st.msg_kinds.entry(u32::from_le_bytes(m[0..4].try_into().unwrap())).or_default() += 1;
                    }
                }
            }
            let pl = if w(7) == NULL_PAYLOAD { &[][..] } else { &p[32..] };
            let mut enabled_ids = Vec::new();
            if kind == shape::CMD_XMA_ENABLE_ONE {
                enabled_ids.push(word(pl, 0));
            } else if kind == shape::CMD_XMA_ENABLE_MASK {
                for wd in 0..16 {
                    let m = word(pl, wd);
                    for b in 0..32 {
                        if m & (1 << b) != 0 {
                            enabled_ids.push((wd * 32 + b) as u32);
                        }
                    }
                }
            }
            for i in enabled_ids {
                st.enables += 1;
                let k = st.produced.get(&i).copied().unwrap_or(0);
                st.enable_blocks += k as u64;
                let has_input = st.xma_ctx(i as usize).is_some_and(|p| unsafe { rd32(p, 0) } & (3 << 20) != 0);
                if k == 0 && has_input {
                    st.enables_empty += 1;
                    if st.empty_examples.len() < 200 {
                        if let Some(c) = st.xma_ctx(i as usize) {
                            let d: Vec<u32> = (0..10).map(|k| unsafe { rd32(c, k * 4) }).collect();
                            st.empty_examples.push(format!("t={:.3}ms xma{i} {:08x?}", t as f64 / 1e6, d));
                        }
                    }
                }
            }
        }
        let depth = st.depth();
        let (ready, active) = st.ring_stats();
        st.max_depth = st.max_depth.max(depth);
        st.max_ready = st.max_ready.max(ready);
        if tag == T_TICK || tag == T_SUBMIT || tag == T_POP {
            csv.push_str(&format!("{:.3},{},{kind:#x},{:.1},{depth},{ready},{active}\n", t as f64 / 1e6, tag_name(tag), ns as f64 / 1e3));
        }
    }
    let report = build_report(path, &st);
    REPLAYING.store(false, Ordering::SeqCst);
    Ok(Replay { report, csv, outputs: std::mem::take(&mut st.outputs) })
}

fn build_report(path: &Path, st: &RState) -> String {
    let mut r = String::new();
    let wall = st.last_t as f64 / 1e9;
    r.push_str(&format!("acphal replay of {}\n", path.display()));
    r.push_str(&format!("records {}  recorded span {:.3} s\n\n", st.records, wall));
    r.push_str("op timings (replay CPU)\n");
    let mut tags: Vec<_> = st.stats.iter().collect();
    tags.sort_by_key(|(k, _)| **k);
    for (k, s) in tags {
        r.push_str(&format!(
            "  {:<15} n={:<8} mean={:>9.1} us  max={:>9.1} us (at {:.3} ms){}\n",
            tag_name(*k),
            s.n,
            s.total as f64 / s.n.max(1) as f64 / 1e3,
            s.max as f64 / 1e3,
            s.max_t as f64 / 1e6,
            if *k == T_TICK { format!("  over 2667us budget: {}", s.over) } else { String::new() }
        ));
    }
    let d = DEC_STATS.lock().unwrap_or_else(|e| e.into_inner());
    r.push_str("\nXMA decode (per 2048-byte packet)\n");
    if d.is_empty() {
        r.push_str("  no packets decoded\n");
    } else {
        let mut ns: Vec<u64> = d.iter().map(|x| x.ns).collect();
        ns.sort_unstable();
        let total: u64 = ns.iter().sum();
        let audio: f64 = d.iter().map(|x| x.frames as f64 / x.rate.max(1) as f64).sum();
        let late = d.iter().filter(|x| x.frames != 0 && x.ns as f64 > x.frames as f64 / x.rate.max(1) as f64 * 1e9).count();
        let empty = d.iter().filter(|x| x.frames == 0).count();
        let pct = |q: f64| ns[((ns.len() - 1) as f64 * q) as usize] as f64 / 1e3;
        r.push_str(&format!("  packets {}  audio {:.3} s  decode CPU {:.3} ms\n", d.len(), audio, total as f64 / 1e6));
        r.push_str(&format!("  per packet: mean {:.1} us  p50 {:.1}  p99 {:.1}  max {:.1} us\n", total as f64 / d.len() as f64 / 1e3, pct(0.5), pct(0.99), pct(1.0)));
        r.push_str(&format!("  real-time factor {:.1}x  (decode slower than packet's own audio: {late})\n", audio / (total as f64 / 1e9).max(1e-12)));
        r.push_str(&format!("  packets that produced no samples: {empty}
"));
        if wall > 0.0 {
            r.push_str(&format!("  decoder load over the recorded span: {:.2}% of one core\n", total as f64 / 1e9 / wall * 100.0));
        }
    }
    r.push_str("\nqueues and stalls\n");
    r.push_str(&format!("  max message queue depth {}  submits with message queue saturated {}\n", st.max_depth, st.saturated));
    r.push_str(&format!("  QUEUE_FULL returns: recorded {}  replay {}\n", st.queue_full_rec, st.queue_full_replay));
    r.push_str(&format!("  max XMA ready blocks across contexts {}\n", st.max_ready));
    r.push_str(&format!("  XMA enables {}  blocks produced by enables {}  enables with input but no output {}\n", st.enables, st.enable_blocks, st.enables_empty));
    if std::env::var_os("TOMBOX_REPLAY_EMPTY").is_some() {
        for e in &st.empty_examples {
            r.push_str(&format!("    empty enable {e}
"));
        }
    }
    let mut kinds: Vec<_> = st.msg_kinds.iter().filter(|(_, v)| **v > 0).collect();
    kinds.sort();
    let name = |k: u32| match k {
        shape::MSG_AUDIO_FRAME_START => "FRAME_START",
        shape::MSG_FLOWGRAPH_COMPLETED => "FLOWGRAPH_COMPLETED",
        shape::MSG_SRC_BLOCKED => "SRC_BLOCKED(underrun)",
        shape::MSG_DMA_BLOCKED => "DMA_BLOCKED(overrun)",
        shape::MSG_COMMAND_COMPLETED => "COMMAND_COMPLETED",
        shape::MSG_FLOWGRAPH_TERMINATED => "FLOWGRAPH_TERMINATED",
        shape::MSG_ERROR => "ERROR",
        _ => "other",
    };
    for (k, v) in kinds {
        r.push_str(&format!("  messages {:#x} {:<24} {v}\n", k, name(*k)));
    }
    r.push_str(&format!("  Mem/Patch records to unmapped APU memory {}\n", st.unmapped));
    r.push_str("\nreplay vs recording\n");
    if st.mismatches.is_empty() {
        r.push_str("  every return value matched the recording\n");
    } else {
        let mut m: Vec<_> = st.mismatches.iter().collect();
        m.sort();
        for (k, v) in m {
            r.push_str(&format!("  {k}: {v} differ\n"));
        }
        for e in &st.examples {
            r.push_str(&format!("    {e}\n"));
        }
    }
    r.push_str("\noutputs\n");
    for o in &st.outputs {
        let frames = o.data.len() / (if o.float { 4 } else { 2 } * o.channels as usize).max(1);
        r.push_str(&format!("  {}.wav  {} frames ({:.3} s)\n", o.name, frames, frames as f64 / o.rate.max(1) as f64));
    }
    r
}
