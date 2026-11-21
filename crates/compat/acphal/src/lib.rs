use compat_common::{record, E_NOTIMPL, S_OK};
use core::ffi::c_void;
use std::alloc::{alloc_zeroed, Layout};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

pub mod shape;
pub mod audiorec;
pub use audiorec::{AcpHalAllocateShapeContexts, AcpHalCreate, AcpHalReleaseShapeContexts, ApuAlloc, ApuCreateHeap, ApuFree};

const M: &str = "acphal";

static ENGINE: Mutex<Option<shape::Engine>> = Mutex::new(None);

fn engine() -> std::sync::MutexGuard<'static, Option<shape::Engine>> {
    ENGINE.lock().unwrap_or_else(|e| e.into_inner())
}

fn map_apu_locked(a: u32) -> usize {
    lock().map_apu(a)
}

pub const HAL_E_QUEUE_FULL: i32 = 0x8AC8_0003u32 as i32;

pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
pub const E_PENDING_HAL: i32 = 0x8000_000Eu32 as i32;
pub const APU_E_HEAP_ALREADY_CREATED: i32 = 0x8ACA_0001u32 as i32;
pub const APU_E_HEAP_NOT_CREATED: i32 = 0x8ACA_0002u32 as i32;
pub const SHAPE_E_ALREADY_ALLOCATED: i32 = 0x8AC8_0001u32 as i32;
pub const SHAPE_E_NOT_ALLOCATED: i32 = 0x8AC8_0002u32 as i32;
pub const SHAPE_E_BUSY: i32 = 0x8AC8_0010u32 as i32;
pub const HAL_E_ALREADY_CONNECTED: i32 = 0x8AC8_0009u32 as i32;
pub const HAL_E_NOT_CONNECTED: i32 = 0x8AC8_000Au32 as i32;
pub const HAL_E_UNSUPPORTED: i32 = 0x8AC8_0F0Eu32 as i32;

const DEFAULT_SIZE_A: u32 = 0x0400_0000;
const DEFAULT_SIZE_B: u32 = 0x0011_4000;
const APU_BASE_A: u32 = 0x0100_0000;

fn log_limited(counter: &AtomicU32, func: &str, detail: &str) {
    if counter.fetch_add(1, Ordering::Relaxed) < 8 {
        record(M, func, detail);
    }
}

#[derive(Clone, Copy, Debug)]
struct Block {
    off: u32,
    size: u32,
    used: bool,
    pad: u32,
    req: u32,
}

struct Heap {
    va: usize,
    apu: u32,
    size: u32,
    blocks: Vec<Block>,
    free: u32,
    used: u32,
    overhead: u32,
    count: u32,
}

impl Heap {
    fn new(size: u32, apu: u32) -> Option<Heap> {
        let size = (size + 0xfff) & !0xfff;
        let layout = Layout::from_size_align(size as usize, 0x10000).ok()?;
        let va = unsafe { alloc_zeroed(layout) } as usize;
        if va == 0 {
            return None;
        }
        Some(Heap {
            va,
            apu,
            size,
            blocks: vec![Block { off: 0, size, used: false, pad: 0, req: 0 }],
            free: size,
            used: 0,
            overhead: 0,
            count: 0,
        })
    }

    fn largest_free(&self) -> u32 {
        self.blocks.iter().filter(|b| !b.used).map(|b| b.size).max().unwrap_or(0)
    }

    fn alloc(&mut self, size: u32, align: u32) -> Option<(usize, u32)> {
        let mut best: Option<(usize, u32)> = None;
        for (i, b) in self.blocks.iter().enumerate() {
            if b.used {
                continue;
            }
            let apu = self.apu as u64 + b.off as u64;
            let pad = (((apu + align as u64 - 1) & !(align as u64 - 1)) - apu) as u64;
            if pad + size as u64 <= b.size as u64 && best.map_or(true, |(_, s)| b.size < s) {
                best = Some((i, b.size));
            }
        }
        let (i, _) = best?;
        let b = self.blocks[i];
        let apu = self.apu as u64 + b.off as u64;
        let pad = (((apu + align as u64 - 1) & !(align as u64 - 1)) - apu) as u32;
        let total = pad + size;
        if b.size > total {
            self.blocks.insert(i + 1, Block { off: b.off + total, size: b.size - total, used: false, pad: 0, req: 0 });
        }
        self.blocks[i] = Block { off: b.off, size: total, used: true, pad, req: size };
        self.free -= total;
        self.used += size;
        self.overhead += pad;
        self.count += 1;
        let user = b.off + pad;
        unsafe { core::ptr::write_bytes((self.va + user as usize) as *mut u8, 0, size as usize) };
        Some((self.va + user as usize, self.apu + user))
    }

    fn find_used_by_user(&self, p: usize) -> Option<usize> {
        if p < self.va || p >= self.va + self.size as usize {
            return None;
        }
        let off = (p - self.va) as u32;
        let i = self.blocks.partition_point(|b| b.off <= off).checked_sub(1)?;
        let b = self.blocks[i];
        (b.used && b.off + b.pad == off).then_some(i)
    }

    fn free(&mut self, p: usize) -> bool {
        let Some(mut i) = self.find_used_by_user(p) else { return false };
        let b = self.blocks[i];
        self.free += b.size;
        self.used -= b.req;
        self.overhead -= b.pad;
        self.count -= 1;
        self.blocks[i] = Block { off: b.off, size: b.size, used: false, pad: 0, req: 0 };
        if i + 1 < self.blocks.len() && !self.blocks[i + 1].used {
            self.blocks[i].size += self.blocks[i + 1].size;
            self.blocks.remove(i + 1);
        }
        if i > 0 && !self.blocks[i - 1].used {
            self.blocks[i - 1].size += self.blocks[i].size;
            self.blocks.remove(i);
            i -= 1;
        }
        let _ = i;
        true
    }

    fn used_block_containing_off(&self, off: u32) -> Option<Block> {
        let i = self.blocks.partition_point(|b| b.off <= off).checked_sub(1)?;
        let b = self.blocks[i];
        (b.used && off >= b.off + b.pad && off < b.off + b.size).then_some(b)
    }

    fn map_va(&self, p: usize) -> u32 {
        if p < self.va || p >= self.va + self.size as usize {
            return 0;
        }
        let off = (p - self.va) as u32;
        match self.used_block_containing_off(off) {
            Some(_) => self.apu + off,
            None => 0,
        }
    }

    fn map_apu(&self, a: u32) -> usize {
        if a < self.apu || a - self.apu >= self.size {
            return 0;
        }
        let off = a - self.apu;
        match self.used_block_containing_off(off) {
            Some(_) => self.va + off as usize,
            None => 0,
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        if let Ok(l) = Layout::from_size_align(self.size as usize, 0x10000) {
            unsafe { std::alloc::dealloc(self.va as *mut u8, l) };
        }
    }
}

struct ShapeAlloc {
    ptrs: [usize; 6],
}

struct State {
    cfg: [u32; 2],
    heaps: [Option<Heap>; 2],
    internal: Option<Heap>,
    shape: Option<ShapeAlloc>,
}

static STATE: Mutex<State> = Mutex::new(State {
    cfg: [DEFAULT_SIZE_A, DEFAULT_SIZE_B],
    heaps: [None, None],
    internal: None,
    shape: None,
});

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

impl State {
    fn created(&self) -> bool {
        self.heaps[0].is_some()
    }

    fn create(&mut self) -> i32 {
        if self.created() {
            return S_OK;
        }
        let size_a = (self.cfg[0] + 0xfff) & !0xfff;
        let Some(mut a) = Heap::new(size_a, APU_BASE_A) else { return E_OUTOFMEMORY };
        let apu_b = APU_BASE_A + ((size_a + 0xffff) & !0xffff);
        let Some(b) = Heap::new(self.cfg[1] + 0x2000, apu_b) else { return E_OUTOFMEMORY };
        let apu_i = apu_b + ((b.size + 0xffff) & !0xffff);
        let mut internal = Heap::new(0x1000, apu_i);
        a.alloc(0x1000, 0x100);
        a.alloc(0x14, 4);
        a.alloc(8, 4);
        if let Some(h) = internal.as_mut() {
            h.alloc(0x4c, 4);
        }
        self.heaps = [Some(a), Some(b)];
        self.internal = internal;
        S_OK
    }

    fn alloc(&mut self, heap: u32, size: u32, align: u32) -> Result<(usize, u32), i32> {
        let idx = match heap {
            1 => 0,
            2 => 1,
            _ => return Err(E_INVALIDARG),
        };
        if size == 0 || align == 0 || align % 4 != 0 || !align.is_power_of_two() {
            return Err(E_INVALIDARG);
        }
        let hr = self.create();
        if hr < 0 {
            return Err(hr);
        }
        let align = if align & 0xff != 0 { 0x100 } else { align };
        let Some(size) = size.checked_add(align - 1).map(|s| s & !(align - 1)) else { return Err(E_OUTOFMEMORY) };
        self.heaps[idx].as_mut().unwrap().alloc(size, align).ok_or(E_OUTOFMEMORY)
    }

    fn free(&mut self, p: usize) -> bool {
        self.heaps.iter_mut().flatten().any(|h| h.free(p))
    }

    fn map_va(&self, p: usize) -> u32 {
        self.heaps.iter().flatten().map(|h| h.map_va(p)).find(|&a| a != 0).unwrap_or(0)
    }

    fn user_size(&self, p: usize) -> u32 {
        self.heaps.iter().flatten().find_map(|h| h.find_used_by_user(p).map(|i| h.blocks[i].size - h.blocks[i].pad)).unwrap_or(0)
    }

    fn map_apu(&self, a: u32) -> usize {
        self.heaps.iter().flatten().map(|h| h.map_apu(a)).find(|&p| p != 0).unwrap_or(0)
    }
}

pub(crate) extern "system" fn create_heap_impl(size_a: u32, size_b: u32) -> i32 {
    let mut s = lock();
    let hr = if size_a == 0 {
        E_INVALIDARG
    } else if s.created() {
        APU_E_HEAP_ALREADY_CREATED
    } else {
        s.cfg[0] = size_a;
        if size_b != 0 {
            s.cfg[1] = size_b;
        }
        s.create()
    };
    record(M, "ApuCreateHeap", &format!("sizeA={size_a:#x} sizeB={size_b:#x} -> {hr:#x}"));
    hr
}

pub(crate) extern "system" fn alloc_impl(pp: *mut *mut c_void, p_apu: *mut u32, size: u32, align: u32, heap: u32) -> i32 {
    static LOG: AtomicU32 = AtomicU32::new(0);
    if !pp.is_null() {
        unsafe { *pp = core::ptr::null_mut() };
    }
    if !p_apu.is_null() {
        unsafe { core::ptr::write_unaligned(p_apu, 0) };
    }
    if pp.is_null() {
        return E_INVALIDARG;
    }
    let r = lock().alloc(heap, size, align);
    match r {
        Ok((va, apu)) => {
            unsafe { *pp = va as *mut c_void };
            if !p_apu.is_null() {
                unsafe { core::ptr::write_unaligned(p_apu, apu) };
            }
            log_limited(&LOG, "ApuAlloc", &format!("size={size:#x} align={align:#x} heap={heap} -> va={va:#x} apu={apu:#x}"));
            S_OK
        }
        Err(hr) => {
            record(M, "ApuAlloc", &format!("size={size:#x} align={align:#x} heap={heap} -> {hr:#x}"));
            hr
        }
    }
}

pub(crate) extern "system" fn free_impl(p: *mut c_void) -> i32 {
    if p.is_null() {
        return E_INVALIDARG;
    }
    if lock().free(p as usize) {
        S_OK
    } else {
        record(M, "ApuFree", &format!("{p:p} not an APU allocation"));
        E_INVALIDARG
    }
}

#[no_mangle]
pub extern "system" fn ApuMapVirtualAddress(p: *const c_void) -> u32 {
    lock().map_va(p as usize)
}

#[no_mangle]
pub extern "system" fn ApuMapApuAddress(a: u32) -> *mut c_void {
    lock().map_apu(a) as *mut c_void
}

#[no_mangle]
pub extern "system" fn ApuIsVirtualAddressValid(p: *const c_void, alignment: u32) -> i32 {
    let a = lock().map_va(p as usize);
    let al = if alignment == 0 { 1 } else { alignment };
    (a != 0 && a & al.wrapping_sub(1) == 0) as i32
}

#[no_mangle]
pub extern "system" fn ApuHeapGetState(out: *mut u32, heap: u32) -> i32 {
    let idx = match heap {
        1 => 0,
        2 => 1,
        _ => return E_INVALIDARG,
    };
    if out.is_null() {
        return E_INVALIDARG;
    }
    let s = lock();
    let Some(h) = s.heaps[idx].as_ref() else { return APU_E_HEAP_NOT_CREATED };
    let v = [h.free, h.used, h.overhead, h.largest_free(), h.count];
    for (i, x) in v.iter().enumerate() {
        unsafe { core::ptr::write_unaligned(out.add(i), *x) };
    }
    S_OK
}

const SHAPE_SIZES: [(u32, u32, u32); 6] = [
    (0x1000, 0x20, 0x20),
    (0x1000, 0xA0, 0x10),
    (0x1000, 0x20, 0x20),
    (0x1000, 0x10, 0x10),
    (0x200, 0x40, 0x40),
    (0xE00, 0x20, 0x40),
];

pub(crate) extern "system" fn alloc_shape_impl(ctx: *mut u8) -> i32 {
    if ctx.is_null() {
        return E_INVALIDARG;
    }
    let rd = |o: usize| unsafe { core::ptr::read_unaligned(ctx.add(o) as *const u32) };
    let n: [u32; 6] = [rd(0), rd(4), rd(8), rd(0xc), rd(0x10), rd(0x14)];
    if n.iter().zip(SHAPE_SIZES.iter()).any(|(c, l)| *c > l.0) {
        return E_INVALIDARG;
    }
    if (n[0] != 0 || n[5] != 0) && n[0] != n[4] + n[5] {
        return E_INVALIDARG;
    }
    let mut s = lock();
    if s.shape.is_some() {
        return SHAPE_E_ALREADY_ALLOCATED;
    }
    let mut ptrs = [0usize; 6];
    let mut apus = [0u32; 6];
    let mut fail = S_OK;
    for k in 0..4 {
        if n[k] == 0 {
            continue;
        }
        match s.alloc(2, n[k] * SHAPE_SIZES[k].1, SHAPE_SIZES[k].2) {
            Ok((va, apu)) => {
                ptrs[k] = va;
                apus[k] = apu;
            }
            Err(hr) => {
                fail = hr;
                break;
            }
        }
    }
    if fail == S_OK && n[4] + n[5] != 0 {
        match s.alloc(2, n[4] * 0x40 + n[5] * 0x20, 0x40) {
            Ok((va, apu)) => {
                if n[4] != 0 {
                    ptrs[4] = va;
                    apus[4] = apu;
                }
                if n[5] != 0 {
                    ptrs[5] = va + n[4] as usize * 0x40;
                    apus[5] = apu + n[4] * 0x40;
                }
            }
            Err(hr) => fail = hr,
        }
    }
    if fail != S_OK {
        for k in 0..5 {
            let p = if k == 4 && ptrs[4] == 0 { ptrs[5] } else { ptrs[k] };
            if p != 0 {
                s.free(p);
            }
        }
        record(M, "AcpHalAllocateShapeContexts", &format!("counts={n:x?} -> {fail:#x}"));
        return fail;
    }
    for k in 0..6 {
        unsafe {
            core::ptr::write_unaligned(ctx.add(0x18 + k * 8) as *mut usize, ptrs[k]);
            core::ptr::write_unaligned(ctx.add(0x48 + k * 4) as *mut u32, apus[k]);
        }
    }
    let combined = if n[4] != 0 { ptrs[4] } else { ptrs[5] };
    s.shape = Some(ShapeAlloc { ptrs: [ptrs[0], ptrs[1], ptrs[2], ptrs[3], combined, 0] });
    drop(s);
    let arrays = shape::Arrays { src: ptrs[0], dma: ptrs[3], xma: ptrs[4], n_src: n[0], n_dma: n[3], n_xma: n[4] };
    *engine() = Some(shape::Engine::new(arrays, map_apu_locked));
    start_service_thread();
    record(M, "AcpHalAllocateShapeContexts", &format!("src={} eq={} fv={} dma={} xma={} pcm={} -> S_OK (CPU SHAPE engine)", n[0], n[1], n[2], n[3], n[4], n[5]));
    S_OK
}

pub(crate) extern "system" fn release_shape_impl() -> i32 {
    let mut s = lock();
    let Some(sh) = s.shape.take() else { return SHAPE_E_NOT_ALLOCATED };
    *engine() = None;
    for p in sh.ptrs {
        if p != 0 {
            s.free(p);
        }
    }
    let _ = SHAPE_E_BUSY;
    record(M, "AcpHalReleaseShapeContexts", "-> S_OK");
    S_OK
}

#[repr(C)]
struct AcpHal {
    vtbl: *const usize,
    id: u32,
    connected: bool,
    cmd_ring: usize,
    msg_ring: usize,
    num_commands: u32,
    num_messages: u32,
    messages: std::collections::VecDeque<[u8; 28]>,
    submitted: u64,
}

static HAL_ID: AtomicU32 = AtomicU32::new(0);
static HAL_CONNECTED: AtomicUsize = AtomicUsize::new(0);
const MAX_CONNECTED: usize = 4;

unsafe extern "system" fn hal_connect(this: *mut AcpHal, num_commands: u32, num_messages: u32) -> i32 {
    let h = &mut *this;
    if h.connected {
        return HAL_E_ALREADY_CONNECTED;
    }
    if num_commands == 0 {
        return E_INVALIDARG;
    }
    free_rings(h);
    let _l = msg_lock();
    h.messages.clear();
    if HAL_CONNECTED.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTED {
        HAL_CONNECTED.fetch_sub(1, Ordering::SeqCst);
        return E_UNEXPECTED;
    }
    let mut s = lock();
    let cmd = match num_commands.checked_mul(0x10c).ok_or(E_OUTOFMEMORY).and_then(|sz| s.alloc(1, sz, 0x100)) {
        Ok(v) => v.0,
        Err(hr) => {
            HAL_CONNECTED.fetch_sub(1, Ordering::SeqCst);
            return hr;
        }
    };
    let msg = if num_messages != 0 {
        match num_messages.checked_mul(0x100).ok_or(E_OUTOFMEMORY).and_then(|sz| s.alloc(1, sz, 0x100)) {
            Ok(v) => v.0,
            Err(hr) => {
                s.free(cmd);
                HAL_CONNECTED.fetch_sub(1, Ordering::SeqCst);
                return hr;
            }
        }
    } else {
        0
    };
    h.cmd_ring = cmd;
    h.msg_ring = msg;
    h.num_commands = num_commands;
    h.num_messages = num_messages;
    h.connected = true;
    record(M, "IAcpHal::Connect", &format!("id={} commands={num_commands} messages={num_messages} -> S_OK", h.id));
    S_OK
}

unsafe fn free_rings(h: &mut AcpHal) {
    let mut s = lock();
    if h.cmd_ring != 0 {
        s.free(h.cmd_ring);
    }
    if h.msg_ring != 0 {
        s.free(h.msg_ring);
    }
    h.cmd_ring = 0;
    h.msg_ring = 0;
}

unsafe fn disconnect(h: &mut AcpHal) -> i32 {
    if !h.connected {
        return HAL_E_NOT_CONNECTED;
    }
    h.connected = false;
    HAL_CONNECTED.fetch_sub(1, Ordering::SeqCst);
    let frame = engine().as_ref().map_or(0, |e| e.frame);
    push_messages(h, vec![shape::Message { kind: shape::MSG_DISCONNECTED, a: h.id, b: 0, c: frame }], None);
    S_OK
}

unsafe extern "system" fn hal_disconnect(this: *mut AcpHal) -> i32 {
    disconnect(&mut *this)
}

fn msg_capacity(h: &AcpHal) -> usize {
    if h.num_messages == 0 {
        0x100
    } else {
        h.num_messages as usize
    }
}

static MSG_LOCK: Mutex<()> = Mutex::new(());

fn msg_lock() -> std::sync::MutexGuard<'static, ()> {
    MSG_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

static MSG_DROPPED: AtomicU32 = AtomicU32::new(0);

fn push_messages(h: &mut AcpHal, msgs: Vec<shape::Message>, last: Option<shape::Message>) {
    let cap = msg_capacity(h);
    let stamp = audio_clock_samples();
    let _l = msg_lock();
    for m in msgs.into_iter().chain(last) {
        if h.messages.len() >= cap {
            if MSG_DROPPED.fetch_add(1, Ordering::Relaxed) == 0 {
                record(M, "IAcpHal::MessageRing", &format!("ring full ({cap} unread slots): new messages are not written until the title pops, as the console ring's valid-slot check (PopMessage 0x180001a50 clears slot+0, GetNumMessages 0x180001b00 counts consecutive valid slots)"));
            }
            continue;
        }
        h.messages.push_back(m.encode(stamp));
    }
}

unsafe extern "system" fn hal_submit(this: *mut AcpHal, kind: u32, id: u64, frame: u32, payload: *const u8, fence: u32) -> i32 {
    static LOG: AtomicU32 = AtomicU32::new(0);
    let h = &mut *this;
    if !h.connected {
        return E_PENDING_HAL;
    }
    if kind == 0 || kind > shape::CMD_MAX {
        return E_INVALIDARG;
    }
    if payload.is_null() && kind != 4 {
        return E_INVALIDARG;
    }
    if !shape::command_supported(kind) {
        log_limited(&LOG, "IAcpHal::SubmitCommands", &format!("type={kind:#x} id={id:#x} -> E_NOTIMPL (no console title or system DLL on disk issues this type; semantics unknown)"));
        return E_NOTIMPL;
    }
    let mut g = engine();
    let e = g.get_or_insert_with(|| shape::Engine::new(shape::Arrays::default(), map_apu_locked));
    let mut msgs = Vec::new();
    let last = if kind == shape::CMD_FLOWGRAPH {
        let count = core::ptr::read_unaligned(payload as *const u32) as usize;
        let apu = core::ptr::read_unaligned(payload.add(4) as *const u32);
        let va = map_apu_locked(apu);
        let ok = if va != 0 && count != 0 {
            let cmds = core::slice::from_raw_parts(va as *const u64, count.min(0x10000)).to_vec();
            e.run_flowgraph(&cmds, &mut msgs)
        } else {
            msgs.push(shape::Message { kind: shape::MSG_FLOWGRAPH_TERMINATED, a: 0, b: 0, c: e.frame });
            false
        };
        ok.then(|| shape::Message { kind: shape::MSG_FLOWGRAPH_COMPLETED, a: apu, b: id, c: e.frame })
    } else if kind == shape::CMD_XMA_ENABLE_ONE {
        let idx = core::ptr::read_unaligned(payload as *const u32);
        e.control(kind, payload);
        let frame = e.frame;
        let voice = e.take_voice(idx);
        drop(g);
        if let Some(mut v) = voice {
            v.service();
            g = engine();
            if let Some(e) = g.as_mut() {
                e.put_voice(v);
            }
        } else {
            g = engine();
        }
        (id != 0).then(|| shape::Message { kind: shape::MSG_COMMAND_COMPLETED, a: 0, b: id, c: frame })
    } else {
        e.control(kind, payload);
        e.service_all();
        (id != 0).then(|| shape::Message { kind: shape::MSG_COMMAND_COMPLETED, a: 0, b: id, c: e.frame })
    };
    drop(g);
    if fence != 0 {
        let fva = map_apu_locked(fence);
        if fva != 0 {
            core::ptr::write_volatile(fva as *mut u32, 1);
        }
    }
    h.submitted += 1;
    log_limited(&LOG, "IAcpHal::SubmitCommands", &format!("type={kind} id={id:#x} frame={frame:#x} fence={fence:#x} -> S_OK (CPU SHAPE)"));
    push_messages(h, msgs, last);
    S_OK
}
fn start_service_thread() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if audiorec::replaying() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("tombox-shape".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_micros(2667));
        audiorec::tick(service_engine);
    });
}

fn audio_clock_samples() -> u32 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let t = EPOCH.get_or_init(std::time::Instant::now).elapsed();
    (t.as_nanos() * 48 / 1_000_000) as u32
}

fn service_engine() {
    if let Some(e) = engine().as_mut() {
        e.service_all();
    }
}

unsafe extern "system" fn hal_pop_message(this: *mut AcpHal, out: *mut u8) -> i32 {
    let h = &mut *this;
    if out.is_null() {
        return 0;
    }
    service_engine();
    let _l = msg_lock();
    match h.messages.pop_front() {
        Some(m) => {
            core::ptr::copy_nonoverlapping(m.as_ptr(), out, 28);
            1
        }
        None => 0,
    }
}

unsafe extern "system" fn hal_num_messages(this: *mut AcpHal) -> u32 {
    service_engine();
    let _l = msg_lock();
    (*this).messages.len() as u32
}

unsafe extern "system" fn hal_release(this: *mut AcpHal) -> u32 {
    hal_destroy(this, 1);
    0
}

unsafe extern "system" fn hal_unsupported(_this: *mut AcpHal) -> i32 {
    HAL_E_UNSUPPORTED
}

const HAL_E_REGION_NOT_SUPPORTED: i32 = 0x8007_0032u32 as i32;

unsafe extern "system" fn hal_counter(_this: *mut AcpHal, out: *mut u64) -> i32 {
    static LOG: AtomicU32 = AtomicU32::new(0);
    if out.is_null() {
        return E_INVALIDARG;
    }
    core::ptr::write_unaligned(out, 0);
    log_limited(&LOG, "IAcpHal::MapDriverRegion", "slot 10: console 0x180001b90 -> 0x180004c50 -> driver vtbl+0x38 0x180004a60(type 1) sends IOCTL 0x8000e023 and returns the user VA of a host-defined shared region (VirtualAudioBus.sys 0x12afc maps it); no such region exists here, so *out = 0 and the IOCTL failure HRESULT 0x80070032 is returned, as the console does when the driver refuses the request");
    HAL_E_REGION_NOT_SUPPORTED
}

unsafe extern "system" fn hal_destroy(this: *mut AcpHal, flags: u32) -> *mut AcpHal {
    let h = &mut *this;
    let _ = disconnect(h);
    free_rings(h);
    let _l = msg_lock();
    h.messages.clear();
    if flags & 1 != 0 {
        drop(Box::from_raw(this));
        return core::ptr::null_mut();
    }
    this
}

struct VTable([usize; 12]);
unsafe impl Sync for VTable {}

static VTBL: std::sync::OnceLock<VTable> = std::sync::OnceLock::new();

fn vtbl() -> *const usize {
    VTBL.get_or_init(|| {
        if audiorec::enabled() {
            return VTable(audiorec::recording_vtable());
        }
        VTable([
            hal_connect as *const () as usize,
            hal_disconnect as *const () as usize,
            hal_submit as *const () as usize,
            hal_pop_message as *const () as usize,
            hal_num_messages as *const () as usize,
            hal_release as *const () as usize,
            hal_unsupported as *const () as usize,
            hal_unsupported as *const () as usize,
            hal_unsupported as *const () as usize,
            hal_unsupported as *const () as usize,
            hal_counter as *const () as usize,
            hal_destroy as *const () as usize,
        ])
    })
    .0
    .as_ptr()
}

pub(crate) extern "system" fn hal_create_impl(out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    let obj = Box::new(AcpHal {
        vtbl: vtbl(),
        id: HAL_ID.fetch_add(1, Ordering::Relaxed),
        connected: false,
        cmd_ring: 0,
        msg_ring: 0,
        num_commands: 0,
        num_messages: 0,
        messages: Default::default(),
        submitted: 0,
    });
    let p = Box::into_raw(obj);
    unsafe { *out = p as *mut c_void };
    record(M, "AcpHalCreate", &format!("-> IAcpHal @ {p:p}"));
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn reset() {
        let mut s = lock();
        s.heaps = [None, None];
        s.internal = None;
        s.shape = None;
        s.cfg = [DEFAULT_SIZE_A, DEFAULT_SIZE_B];
    }

    fn state(heap: u32) -> [u32; 5] {
        let mut o = [0u32; 5];
        assert_eq!(ApuHeapGetState(o.as_mut_ptr(), heap), S_OK);
        o
    }

    #[test]
    fn create_heap_rules() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut o = [0u32; 5];
        assert_eq!(ApuHeapGetState(o.as_mut_ptr(), 1), APU_E_HEAP_NOT_CREATED);
        assert_eq!(ApuCreateHeap(0, 0), E_INVALIDARG);
        assert_eq!(ApuCreateHeap(0x100000, 0x20000), S_OK);
        assert_eq!(ApuCreateHeap(0x100000, 0), APU_E_HEAP_ALREADY_CREATED);
        let a = state(1);
        assert_eq!(a[4], 3);
        assert_eq!(a[0] + a[1] + a[2], 0x100000);
        let b = state(2);
        assert_eq!(b[0], 0x22000);
        assert_eq!(ApuHeapGetState(o.as_mut_ptr(), 3), E_INVALIDARG);
    }

    #[test]
    fn alloc_validation_and_mapping() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut p: *mut c_void = core::ptr::null_mut();
        let mut a = 0u32;
        assert_eq!(ApuAlloc(&mut p, &mut a, 0x100, 3, 1), E_INVALIDARG);
        assert_eq!(ApuAlloc(&mut p, &mut a, 0x100, 12, 1), E_INVALIDARG);
        assert_eq!(ApuAlloc(&mut p, &mut a, 0, 0x100, 1), E_INVALIDARG);
        assert_eq!(ApuAlloc(&mut p, &mut a, 0x100, 0x100, 7), E_INVALIDARG);
        assert_eq!(ApuAlloc(core::ptr::null_mut(), &mut a, 0x100, 0x100, 1), E_INVALIDARG);
        assert_eq!(ApuAlloc(&mut p, &mut a, 0x123, 0x1000, 2), S_OK);
        assert!(!p.is_null());
        assert_eq!(a & 0xfff, 0);
        assert_ne!(a, 0);
        assert_eq!(ApuMapVirtualAddress(p), a);
        assert_eq!(ApuMapVirtualAddress((p as usize + 0x10) as *const c_void), a + 0x10);
        assert_eq!(ApuMapApuAddress(a + 0x20) as usize, p as usize + 0x20);
        assert_eq!(ApuIsVirtualAddressValid(p, 0x1000), 1);
        assert_eq!(ApuIsVirtualAddressValid((p as usize + 4) as *const c_void, 0x10), 0);
        assert_eq!(ApuIsVirtualAddressValid((p as usize + 4) as *const c_void, 0), 1);
        let x = 5u32;
        assert_eq!(ApuMapVirtualAddress(&x as *const u32 as *const c_void), 0);
        assert_eq!(ApuMapApuAddress(0).is_null(), true);
        assert_eq!(ApuFree(p), S_OK);
        assert_eq!(ApuMapVirtualAddress(p), 0);
        assert_eq!(ApuFree(p), E_INVALIDARG);
    }

    #[test]
    fn small_align_becomes_256_and_free_coalesces() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut ps = Vec::new();
        for _ in 0..3 {
            let mut p: *mut c_void = core::ptr::null_mut();
            let mut a = 0u32;
            assert_eq!(ApuAlloc(&mut p, &mut a, 0x10, 4, 2), S_OK);
            assert_eq!(a & 0xff, 0);
            ps.push(p);
        }
        let before = state(2);
        assert_eq!(before[1], 0x300);
        assert_eq!(before[4], 3);
        ApuFree(ps[1]);
        ApuFree(ps[0]);
        ApuFree(ps[2]);
        let after = state(2);
        assert_eq!(after[1], 0);
        assert_eq!(after[4], 0);
        assert_eq!(after[0], after[3]);
        let s = lock();
        assert_eq!(s.heaps[1].as_ref().unwrap().blocks.len(), 1);
    }

    #[test]
    fn best_fit_reuses_smallest_hole() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut v = Vec::new();
        for sz in [0x400u32, 0x100, 0x800, 0x100] {
            let mut p: *mut c_void = core::ptr::null_mut();
            let mut a = 0;
            assert_eq!(ApuAlloc(&mut p, &mut a, sz, 0x100, 2), S_OK);
            v.push(p);
        }
        ApuFree(v[0]);
        ApuFree(v[2]);
        let mut p: *mut c_void = core::ptr::null_mut();
        let mut a = 0;
        assert_eq!(ApuAlloc(&mut p, &mut a, 0x300, 0x100, 2), S_OK);
        assert_eq!(p, v[0]);
    }

    #[test]
    fn out_of_memory() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut p: *mut c_void = core::ptr::null_mut();
        let mut a = 0;
        assert_eq!(ApuAlloc(&mut p, &mut a, 0x200000, 0x100, 2), E_OUTOFMEMORY);
        assert!(p.is_null());
        assert_eq!(a, 0);
    }

    #[test]
    fn shape_contexts() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(AcpHalReleaseShapeContexts(), SHAPE_E_NOT_ALLOCATED);
        let mut c = [0u8; 0x60];
        c[0x10..0x14].copy_from_slice(&0x201u32.to_le_bytes());
        assert_eq!(AcpHalAllocateShapeContexts(c.as_mut_ptr()), E_INVALIDARG);
        c[0x10..0x14].copy_from_slice(&0x200u32.to_le_bytes());
        c[0..4].copy_from_slice(&5u32.to_le_bytes());
        assert_eq!(AcpHalAllocateShapeContexts(c.as_mut_ptr()), E_INVALIDARG);
        c[0..4].copy_from_slice(&0x210u32.to_le_bytes());
        c[0x14..0x18].copy_from_slice(&0x10u32.to_le_bytes());
        c[0xC..0x10].copy_from_slice(&0x40u32.to_le_bytes());
        assert_eq!(AcpHalAllocateShapeContexts(c.as_mut_ptr()), S_OK);
        let rp = |o: usize| usize::from_le_bytes(c[o..o + 8].try_into().unwrap());
        let ra = |o: usize| u32::from_le_bytes(c[o..o + 4].try_into().unwrap());
        assert_ne!(rp(0x18), 0);
        assert_eq!(rp(0x20), 0);
        assert_ne!(rp(0x30), 0);
        assert_eq!(rp(0x40), rp(0x38) + 0x200 * 0x40);
        assert_eq!(ra(0x5C), ra(0x58) + 0x200 * 0x40);
        assert_eq!(ra(0x58) & 0x3f, 0);
        assert_eq!(ApuMapVirtualAddress(rp(0x38) as *const c_void), ra(0x58));
        assert_eq!(AcpHalAllocateShapeContexts(c.as_mut_ptr()), SHAPE_E_ALREADY_ALLOCATED);
        assert_eq!(AcpHalReleaseShapeContexts(), S_OK);
        assert_eq!(state(2)[4], 0);
    }

    #[test]
    fn hal_object_vtable() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(AcpHalCreate(core::ptr::null_mut()), E_INVALIDARG);
        let mut o: *mut c_void = core::ptr::null_mut();
        assert_eq!(AcpHalCreate(&mut o), S_OK);
        let vt = unsafe { *(o as *const *const usize) };
        let slot = |i: usize| unsafe { *vt.add(i) };
        unsafe {
            let connect: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 = core::mem::transmute(slot(0));
            let disc: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(1));
            let submit: unsafe extern "system" fn(*mut c_void, u32, u64, u32, *const u8, u32) -> i32 = core::mem::transmute(slot(2));
            let pop: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32 = core::mem::transmute(slot(3));
            let num: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(4));
            let release: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(5));
            let unsup: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(7));
            let counter: unsafe extern "system" fn(*mut c_void, *mut u64) -> i32 = core::mem::transmute(slot(10));
            assert_eq!(submit(o, 1, 0, 0, 8 as *const u8, 0), E_PENDING_HAL);
            assert_eq!(disc(o), HAL_E_NOT_CONNECTED);
            assert_eq!(connect(o, 0, 0), E_INVALIDARG);
            assert_eq!(ApuCreateHeap(DEFAULT_SIZE_A, 0), S_OK);
            let used0 = state(1)[1];
            assert_eq!(connect(o, 0x10, 0x10), S_OK);
            assert_eq!(state(1)[1], used0 + 0x1100 + 0x1000);
            assert_eq!(connect(o, 0x10, 0x10), HAL_E_ALREADY_CONNECTED);
            assert_eq!(submit(o, 1, 0, 0, core::ptr::null(), 0), E_INVALIDARG);
            assert_eq!(submit(o, 0, 0, 0, [0u8; 4].as_ptr(), 0), E_INVALIDARG);
            assert_eq!(submit(o, 0x1c, 0, 0, [0u8; 4].as_ptr(), 0), E_INVALIDARG);
            assert_eq!(submit(o, 4, 0, 0, core::ptr::null(), 0), E_NOTIMPL);
            let (fence_va, fence_apu) = alloc1(0x100, 0x100);
            let cfg = 0x72u32.to_le_bytes();
            assert_eq!(submit(o, 2, 0, 0, cfg.as_ptr(), 0), S_OK);
            assert_eq!(num(o), 0);
            assert_eq!(submit(o, 2, 0x77, 0, cfg.as_ptr(), fence_apu), S_OK);
            assert_eq!(core::ptr::read_volatile(fence_va as *const u32), 1);
            let mut m = [0u8; 28];
            assert_eq!(num(o), 1);
            assert_eq!(pop(o, m.as_mut_ptr()), 1);
            assert_eq!(u32::from_le_bytes(m[0..4].try_into().unwrap()), shape::MSG_COMMAND_COMPLETED);
            assert_eq!(pop(o, m.as_mut_ptr()), 0);
            assert_eq!(num(o), 0);
            assert_eq!(unsup(o), HAL_E_UNSUPPORTED);
            let mut n = 0u64;
            assert_eq!(counter(o, core::ptr::null_mut()), E_INVALIDARG);
            n = 7;
            assert_eq!(counter(o, &mut n), 0x8007_0032u32 as i32);
            assert_eq!(n, 0);
            assert_eq!(ApuFree(fence_va as *mut c_void), S_OK);
            assert_eq!(disc(o), S_OK);
            assert_eq!(disc(o), HAL_E_NOT_CONNECTED);
            assert_eq!(submit(o, 2, 0, 0, cfg.as_ptr(), 0), E_PENDING_HAL);
            assert_eq!(num(o), 1);
            assert_eq!(pop(o, m.as_mut_ptr()), 1);
            assert_eq!(u32::from_le_bytes(m[0..4].try_into().unwrap()), shape::MSG_DISCONNECTED);
            assert_eq!(pop(o, m.as_mut_ptr()), 0);
            assert_eq!(state(1)[1], used0 + 0x1100 + 0x1000);
            assert_eq!(connect(o, 0x10, 0x10), S_OK);
            assert_eq!(state(1)[1], used0 + 0x1100 + 0x1000);
            assert_eq!(disc(o), S_OK);
            release(o);
            assert_eq!(state(1)[1], used0);
        }
    }

    struct ConstDecoder(usize);

    impl shape::XmaDecoder for ConstDecoder {
        fn decode_packet(&mut self, packet: &[u8], out: &mut Vec<i16>) {
            for _ in 0..shape::packet_frame_count(packet) * shape::XMA_FRAME_SAMPLES {
                out.push(16384);
                if self.0 == 2 {
                    out.push(-8192);
                }
            }
        }
    }

    fn const_factory(channels: usize, _rate: u32) -> Box<dyn shape::XmaDecoder> {
        Box::new(ConstDecoder(channels))
    }

    struct CountDecoder(i16);

    impl shape::XmaDecoder for CountDecoder {
        fn decode_packet(&mut self, packet: &[u8], out: &mut Vec<i16>) {
            for _ in 0..shape::packet_frame_count(packet) * shape::XMA_FRAME_SAMPLES {
                out.push(self.0);
                self.0 = self.0.wrapping_add(1);
            }
        }
    }

    fn count_factory(_channels: usize, _rate: u32) -> Box<dyn shape::XmaDecoder> {
        Box::new(CountDecoder(0))
    }

    #[test]
    fn shape_fortnite_cpu_read_decodes_requested_subframes_and_resumes() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        shape::set_decoder_factory(count_factory);
        let r = rig(false, 4);
        let (fva, fapu) = alloc1(0x100, 0x100);
        let out = map_apu_locked(unsafe { r32(r.xma, 0x1c) });
        let idx = 0u32.to_le_bytes();
        let mut next = 0i16;
        for _ in 0..6 {
            unsafe {
                let d1 = r32(r.xma, 4);
                w32(r.xma, 4, (d1 & !(0xf << 20)) | (2 << 20));
                for kind in [5u32, 7] {
                    core::ptr::write_volatile(fva as *mut u32, 0);
                    assert_eq!((r.submit)(r.hal, kind, 0xff, u32::MAX, idx.as_ptr(), fapu), S_OK);
                    assert_eq!(core::ptr::read_volatile(fva as *const u32), 1);
                }
                let w = (r32(r.xma, 0) >> 27) & 31;
                let rd = r32(r.xma, 0x24) & 31;
                assert_eq!((w + 31 - rd) % 31, 2);
                for b in 0..2 {
                    let blk = out + ((rd as usize + b) % 31) * 256;
                    for k in 0..128 {
                        assert_eq!(core::ptr::read_unaligned((blk + k * 2) as *const i16), next);
                        next = next.wrapping_add(1);
                    }
                }
                w32(r.xma, 0x24, (r32(r.xma, 0x24) & !31) | w);
            }
        }
        assert_eq!(ApuFree(fva as *mut c_void), S_OK);
        finish(r);
    }

    fn alloc1(size: u32, align: u32) -> (usize, u32) {
        let mut p: *mut c_void = core::ptr::null_mut();
        let mut a = 0u32;
        assert_eq!(ApuAlloc(&mut p, &mut a, size, align, 1), S_OK);
        (p as usize, a)
    }

    unsafe fn w32(p: usize, off: usize, v: u32) {
        core::ptr::write_unaligned((p + off) as *mut u32, v);
    }

    unsafe fn r32(p: usize, off: usize) -> u32 {
        core::ptr::read_unaligned((p + off) as *const u32)
    }

    type SubmitFn = unsafe extern "system" fn(*mut c_void, u32, u64, u32, *const u8, u32) -> i32;
    type PopFn = unsafe extern "system" fn(*mut c_void, *mut u8) -> i32;

    struct Rig {
        hal: *mut c_void,
        submit: SubmitFn,
        pop: PopFn,
        xma: usize,
        dst: usize,
        cmd_va: usize,
        cmd_apu: u32,
    }

    impl Rig {
        fn messages(&self) -> Vec<[u8; 28]> {
            let mut v = Vec::new();
            let mut m = [0u8; 28];
            while unsafe { (self.pop)(self.hal, m.as_mut_ptr()) } != 0 {
                v.push(m);
            }
            v
        }

        fn flowgraph(&self, cmds: &[u64]) -> Vec<[u8; 28]> {
            for (i, c) in cmds.iter().enumerate() {
                unsafe { core::ptr::write_unaligned((self.cmd_va + i * 8) as *mut u64, *c) };
            }
            let mut pl = [0u8; 16];
            pl[0..4].copy_from_slice(&(cmds.len() as u32).to_le_bytes());
            pl[4..8].copy_from_slice(&self.cmd_apu.to_le_bytes());
            assert_eq!(unsafe { (self.submit)(self.hal, 1, 0x34, u32::MAX, pl.as_ptr(), 0) }, S_OK);
            let m = self.messages();
            for d in 0..2u32 {
                self.consume(d, 1);
            }
            m
        }

        fn consume(&self, dma: u32, n: u32) {
            let mut p = [0u8; 8];
            p[0..4].copy_from_slice(&dma.to_le_bytes());
            p[4..8].copy_from_slice(&n.to_le_bytes());
            assert_eq!(unsafe { (self.submit)(self.hal, 0x10, 0, u32::MAX, p.as_ptr(), 0) }, S_OK);
        }

        fn flowgraph_raw(&self, cmds: &[u64]) -> Vec<[u8; 28]> {
            for (i, c) in cmds.iter().enumerate() {
                unsafe { core::ptr::write_unaligned((self.cmd_va + i * 8) as *mut u64, *c) };
            }
            let mut pl = [0u8; 16];
            pl[0..4].copy_from_slice(&(cmds.len() as u32).to_le_bytes());
            pl[4..8].copy_from_slice(&self.cmd_apu.to_le_bytes());
            assert_eq!(unsafe { (self.submit)(self.hal, 1, 0x35, u32::MAX, pl.as_ptr(), 0) }, S_OK);
            self.messages()
        }
    }

    fn rig(stereo: bool, packets: u32) -> Rig {
        reset();
        assert_eq!(ApuCreateHeap(DEFAULT_SIZE_A, 0), S_OK);
        let mut c = [0u8; 0x60];
        c[0..4].copy_from_slice(&2u32.to_le_bytes());
        c[0xC..0x10].copy_from_slice(&4u32.to_le_bytes());
        c[0x10..0x14].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(AcpHalAllocateShapeContexts(c.as_mut_ptr()), S_OK);
        let rp = |o: usize| usize::from_le_bytes(c[o..o + 8].try_into().unwrap());
        let (src, dma, xma) = (rp(0x18), rp(0x30), rp(0x38));
        let (inp, inp_apu) = alloc1(packets * 2048, 0x800);
        for k in 0..packets as usize {
            unsafe { *((inp + k * 2048) as *mut u8) = 1 << 2 };
        }
        let (_out, out_apu) = alloc1(0x1f00, 0x100);
        let (dst, dst_apu) = alloc1(4 * 2 * 128 * 4, 0x200);
        let (cmd_va, cmd_apu) = alloc1(0x100, 0x100);
        unsafe {
            w32(xma, 0, packets | (1 << 20) | (31 << 22));
            w32(xma, 4, (1 << 31) | if stereo { 1 << 29 } else { 0 });
            w32(xma, 8, 32);
            w32(xma, 0x14, inp_apu);
            w32(xma, 0x1c, out_apu);
            w32(src, 4, 262144);
            for (d, ch) in [(0usize, 0u32), (1, 1)] {
                w32(dma + d * 0x10, 8, dst_apu);
                w32(dma + d * 0x10, 0xc, 4 | (1 << 16) | (ch << 24));
            }
        }
        let mut o: *mut c_void = core::ptr::null_mut();
        assert_eq!(AcpHalCreate(&mut o), S_OK);
        let vt = unsafe { *(o as *const *const usize) };
        unsafe {
            let connect: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 = core::mem::transmute(*vt);
            assert_eq!(connect(o, 0x100, 0x100), S_OK);
            Rig {
                hal: o,
                submit: core::mem::transmute::<usize, SubmitFn>(*vt.add(2)),
                pop: core::mem::transmute::<usize, PopFn>(*vt.add(3)),
                xma,
                dst,
                cmd_va,
                cmd_apu,
            }
        }
    }

    fn finish(r: Rig) {
        unsafe {
            let release: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*(*(r.hal as *const *const usize)).add(5));
            release(r.hal);
        }
        shape::set_decoder_factory(shape::xma_factory);
        assert_eq!(AcpHalReleaseShapeContexts(), S_OK);
    }

    fn msg_kind(m: &[u8; 28]) -> u32 {
        u32::from_le_bytes(m[0..4].try_into().unwrap())
    }

    fn msg_id(m: &[u8; 28]) -> u64 {
        u64::from_le_bytes(m[16..24].try_into().unwrap())
    }

    #[test]
    fn command_encodings_match_xdefiant() {
        assert_eq!(shape::encode_clear(5), (5u64 << 33) | 0x802001);
        assert_eq!(shape::encode_src(3, 7, 8), (((8u64 << 13 | 7) << 12 | 3) << 13) | 2);
        assert_eq!(shape::encode_dma(9, 7), ((9u64 << 13 | 7) << 14) | 0x10);
    }

    #[test]
    fn default_decoder_is_real_xma2() {
        assert_eq!(shape::sample_rate_code(0), 24000);
        assert_eq!(shape::sample_rate_code(3), 48000);
        let mut d = shape::xma_factory(2, 48000);
        let mut pkt = [0u8; 2048];
        pkt[0] = 3 << 2;
        let mut out = Vec::new();
        for _ in 0..4 {
            d.decode_packet(&pkt, &mut out);
        }
        assert_eq!(out.len() % 2, 0);
    }

    #[test]
    fn shape_real_decoder_runs_by_default() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        shape::set_decoder_factory(shape::xma_factory);
        let r = rig(true, 2);
        let zero = 0u32.to_le_bytes();
        assert_eq!(unsafe { (r.submit)(r.hal, 5, 0x31, u32::MAX, zero.as_ptr(), 0) }, S_OK);
        let m = r.messages();
        assert_eq!((msg_kind(&m[0]), msg_id(&m[0])), (shape::MSG_COMMAND_COMPLETED, 0x31));
        let cmds = [shape::encode_clear(0), shape::encode_src(0, 0, 1), shape::encode_dma(0, 0)];
        let m = r.flowgraph(&cmds);
        assert_eq!(msg_kind(m.last().unwrap()), shape::MSG_FLOWGRAPH_COMPLETED);
        finish(r);
    }

    #[test]
    fn shape_mono_pipeline() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        shape::set_decoder_factory(const_factory);
        let r = rig(false, 2);
        let mut mask = [0u8; 64];
        mask[0] = 1;
        assert_eq!(unsafe { (r.submit)(r.hal, 6, 0x33, u32::MAX, mask.as_ptr(), 0) }, S_OK);
        let m = r.messages();
        assert_eq!(m.len(), 1);
        assert_eq!((msg_kind(&m[0]), msg_id(&m[0])), (shape::MSG_COMMAND_COMPLETED, 0x33));
        let cmds = [shape::encode_clear(0), shape::encode_src(0, 0, 0), shape::encode_dma(0, 0)];
        for k in 0..7 {
            let m = r.flowgraph(&cmds);
            assert_eq!(msg_kind(m.last().unwrap()), shape::MSG_FLOWGRAPH_COMPLETED);
            assert_eq!(msg_id(m.last().unwrap()), 0x34);
            assert!(m.iter().all(|x| msg_kind(x) != shape::MSG_SRC_BLOCKED), "frame {k}");
            let blk = (k % 4) * 2 * 128;
            for n in 0..128 {
                let v = unsafe { *((r.dst as *const f32).add(blk + n)) };
                assert!((v - 0.5).abs() < 1e-6, "frame {k} sample {n} = {v}");
            }
        }
        assert_eq!(unsafe { r32(r.xma, 0) } & (1 << 20), 0);
        r.flowgraph(&cmds);
        let m = r.flowgraph(&cmds);
        assert!(m.iter().any(|x| msg_kind(x) == shape::MSG_SRC_BLOCKED));
        finish(r);
    }

    #[test]
    fn shape_stereo_split_and_disable() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        shape::set_decoder_factory(const_factory);
        let r = rig(true, 4);
        let zero = 0u32.to_le_bytes();
        assert_eq!(unsafe { (r.submit)(r.hal, 5, 0x33, u32::MAX, zero.as_ptr(), 0) }, S_OK);
        r.messages();
        let cmds = [shape::encode_clear(0), shape::encode_clear(1), shape::encode_src(0, 0, 1), shape::encode_dma(0, 0), shape::encode_dma(1, 1)];
        let m = r.flowgraph(&cmds);
        assert_eq!(msg_kind(m.last().unwrap()), shape::MSG_FLOWGRAPH_COMPLETED);
        for n in 0..128 {
            let l = unsafe { *((r.dst as *const f32).add(n)) };
            let rr = unsafe { *((r.dst as *const f32).add(128 + n)) };
            assert!((l - 0.5).abs() < 1e-6 && (rr + 0.25).abs() < 1e-6, "{n}: {l} {rr}");
        }
        let in_ring = unsafe { r32(r.xma, 0) } >> 27;
        assert!(in_ring > 0);
        assert_eq!(unsafe { (r.submit)(r.hal, 7, 0x32, u32::MAX, zero.as_ptr(), 0) }, S_OK);
        let m = r.messages();
        assert_eq!((msg_kind(&m[0]), msg_id(&m[0])), (shape::MSG_COMMAND_COMPLETED, 0x32));
        let before = unsafe { r32(r.xma, 0) };
        r.flowgraph(&cmds);
        r.flowgraph(&cmds);
        assert_eq!(unsafe { r32(r.xma, 0) } >> 27, before >> 27);
        finish(r);
    }

    #[test]
    fn shape_queue_full() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let r = rig(false, 1);
        let x = [0u8; 32];
        let (fva, fapu) = alloc1(0x100, 0x100);
        for id in 1..=0x300u64 {
            unsafe { core::ptr::write_volatile(fva as *mut u32, 0) };
            assert_eq!(unsafe { (r.submit)(r.hal, 2, id, u32::MAX, x.as_ptr(), fapu) }, S_OK);
            assert_eq!(unsafe { core::ptr::read_volatile(fva as *const u32) }, 1);
        }
        let m = r.messages();
        assert_eq!(m.len(), 0x100);
        let id = |k: &[u8; 28]| u64::from_le_bytes(k[16..24].try_into().unwrap());
        assert_eq!(id(&m[0]), 1);
        assert_eq!(id(&m[0xff]), 0x100);
        assert_eq!(unsafe { (r.submit)(r.hal, 2, 0x301, u32::MAX, x.as_ptr(), 0) }, S_OK);
        assert_eq!(id(&r.messages()[0]), 0x301);
        assert_eq!(ApuFree(fva as *mut c_void), S_OK);
        finish(r);
    }

    #[test]
    fn shape_dma_ring_blocks_until_consumed() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        shape::set_decoder_factory(const_factory);
        let r = rig(false, 4);
        let mut mask = [0u8; 64];
        mask[0] = 1;
        assert_eq!(unsafe { (r.submit)(r.hal, 6, 0x33, u32::MAX, mask.as_ptr(), 0) }, S_OK);
        r.messages();
        let cmds = [shape::encode_clear(0), shape::encode_src(0, 0, 0), shape::encode_dma(0, 0)];
        let blocked = |m: &[[u8; 28]]| m.iter().any(|x| msg_kind(x) == shape::MSG_DMA_BLOCKED && u32::from_le_bytes(x[12..16].try_into().unwrap()) == 0);
        let dma = engine().as_ref().unwrap().arrays.dma;
        for k in 0..5u32 {
            assert!(!blocked(&r.flowgraph_raw(&cmds)));
            let d1 = unsafe { r32(dma, 4) };
            assert_eq!(d1 & 31, 0);
            assert_eq!((d1 >> 8) & 31, (k + 1) % 5);
            assert_eq!((d1 >> 16) & 1, (k == 4) as u32);
        }
        let m = r.flowgraph_raw(&cmds);
        assert!(blocked(&m));
        assert_eq!(msg_kind(m.last().unwrap()), shape::MSG_FLOWGRAPH_COMPLETED);
        r.consume(0, 2);
        let d1 = unsafe { r32(dma, 4) };
        assert_eq!((d1 & 31, (d1 >> 8) & 31, (d1 >> 16) & 1), (2, 0, 0));
        for _ in 0..2 {
            assert!(!blocked(&r.flowgraph_raw(&cmds)));
        }
        assert!(blocked(&r.flowgraph_raw(&cmds)));
        finish(r);
    }

    #[test]
    fn shape_unknown_opcode_terminates_flowgraph() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let r = rig(false, 1);
        let m = r.flowgraph_raw(&[shape::encode_clear(0), 0x7]);
        assert_eq!(m.len(), 1);
        assert_eq!(msg_kind(&m[0]), shape::MSG_FLOWGRAPH_TERMINATED);
        assert_eq!(u32::from_le_bytes(m[0][16..20].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(m[0][20..24].try_into().unwrap()), 0);
        finish(r);
    }

    #[test]
    fn shape_xma_set_input_update_and_loop() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        shape::set_decoder_factory(const_factory);
        let r = rig(false, 2);
        let (_b1, b1_apu) = alloc1(2 * 2048, 0x800);
        let set = [0u32, 1, 2, 1, 1, b1_apu];
        let bytes: Vec<u8> = set.iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(unsafe { (r.submit)(r.hal, 0x13, 0, u32::MAX, bytes.as_ptr(), 0) }, S_OK);
        unsafe {
            assert_eq!(r32(r.xma, 4) & 0xfff, 2);
            assert_ne!(r32(r.xma, 0) & (1 << 21), 0);
            assert_eq!(r32(r.xma, 0x18), b1_apu);
        }
        let (stage, stage_apu) = alloc1(0x40, 0x100);
        unsafe {
            w32(stage, 0, 5 | (3 << 12));
            w32(stage, 0xc, 32);
            w32(stage, 0x10, 2048 * 8 + 100);
        }
        let upd: Vec<u8> = [0u32, (1 << 4) | (1 << 2), stage_apu].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(unsafe { (r.submit)(r.hal, 0xe, 0, u32::MAX, upd.as_ptr(), 0) }, S_OK);
        unsafe {
            assert_eq!(r32(r.xma, 0) & 0xfff, 5);
            assert_eq!((r32(r.xma, 0) >> 12) & 0xff, 3);
            assert_eq!(r32(r.xma, 0x10) & 0x3ff_ffff, 2048 * 8 + 100);
            w32(r.xma, 0, (r32(r.xma, 0) & !0xfff) | 2);
        }
        let zero = 0u32.to_le_bytes();
        assert_eq!(unsafe { (r.submit)(r.hal, 5, 0x31, u32::MAX, zero.as_ptr(), 0) }, S_OK);
        r.messages();
        let cmds = [shape::encode_clear(0), shape::encode_src(0, 0, 0), shape::encode_dma(0, 0)];
        for _ in 0..6 {
            r.flowgraph(&cmds);
        }
        unsafe {
            assert!((r32(r.xma, 0) >> 12) & 0xff < 3);
        }
        finish(r);
    }
}