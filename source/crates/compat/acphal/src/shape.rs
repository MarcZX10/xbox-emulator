use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

pub const MSG_AUDIO_FRAME_START: u32 = 1;
pub const MSG_FLOWGRAPH_COMPLETED: u32 = 2;
pub const MSG_SRC_BLOCKED: u32 = 4;
pub const MSG_DMA_BLOCKED: u32 = 8;
pub const MSG_COMMAND_COMPLETED: u32 = 0x10;
pub const MSG_FLOWGRAPH_TERMINATED: u32 = 0x20;
pub const MSG_ERROR: u32 = 0x40;
pub const MSG_DISCONNECTED: u32 = 0x80;

pub const CMD_FLOWGRAPH: u32 = 1;
pub const CMD_XMA_ENABLE_ONE: u32 = 5;
pub const CMD_XMA_ENABLE_MASK: u32 = 6;
pub const CMD_XMA_DISABLE_ONE: u32 = 7;
pub const CMD_XMA_DISABLE_MASK: u32 = 8;
pub const CMD_CONFIG: u32 = 2;
pub const CMD_XMA_UPDATE: u32 = 0xe;
pub const CMD_DMA_CONSUMED: u32 = 0x10;
pub const CMD_XMA_OUT_CONSUMED: u32 = 0x12;
pub const CMD_XMA_SET_INPUT: u32 = 0x13;
pub const CMD_MAX: u32 = 0x1b;

pub fn command_supported(kind: u32) -> bool {
    matches!(kind, CMD_FLOWGRAPH | CMD_CONFIG | CMD_XMA_ENABLE_ONE | CMD_XMA_ENABLE_MASK | CMD_XMA_DISABLE_ONE | CMD_XMA_DISABLE_MASK | CMD_XMA_UPDATE | CMD_DMA_CONSUMED | CMD_XMA_OUT_CONSUMED | CMD_XMA_SET_INPUT)
}

pub const OP_CLEAR: u64 = 1;
pub const OP_SRC: u64 = 2;
pub const OP_DMA: u64 = 0x10;

pub const FRAME: usize = 128;
pub const XMA_PACKET: usize = 2048;
pub const XMA_FRAME_SAMPLES: usize = 512;
pub const XMA_CTX: usize = 0x40;
pub const SRC_CTX: usize = 0x20;
pub const DMA_CTX: usize = 0x10;
pub const SRC_RATIO_ONE: f64 = 262144.0;

pub trait XmaDecoder: Send {
    fn decode_packet(&mut self, packet: &[u8], out: &mut Vec<i16>);
}

pub fn packet_frame_count(packet: &[u8]) -> usize {
    xma::packet_frame_count(packet) as usize
}

pub fn sample_rate_code(code: u32) -> u32 {
    [24000, 32000, 44100, 48000][(code & 3) as usize]
}

struct RealXma(Option<xma::Xma2Stream>);

impl XmaDecoder for RealXma {
    fn decode_packet(&mut self, packet: &[u8], out: &mut Vec<i16>) {
        if let Some(s) = self.0.as_mut() {
            s.decode_packet(packet, out);
        }
    }
}

pub type DecoderFactory = fn(channels: usize, rate: u32) -> Box<dyn XmaDecoder>;

pub fn xma_factory(channels: usize, rate: u32) -> Box<dyn XmaDecoder> {
    Box::new(RealXma(xma::Xma2Stream::new_hardware(rate, channels).ok()))
}

static FACTORY: Mutex<DecoderFactory> = Mutex::new(xma_factory);

pub fn set_decoder_factory(f: DecoderFactory) {
    *FACTORY.lock().unwrap_or_else(|e| e.into_inner()) = f;
}
#[derive(Clone, Copy, Default)]
pub struct Arrays {
    pub src: usize,
    pub dma: usize,
    pub xma: usize,
    pub n_src: u32,
    pub n_dma: u32,
    pub n_xma: u32,
}

pub type ApuToVa = fn(u32) -> usize;

unsafe fn rd(p: usize, off: usize) -> u32 {
    core::ptr::read_unaligned((p + off) as *const u32)
}

pub static WRITE_LOG_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static WRITE_LOG: Mutex<Vec<(usize, u32)>> = Mutex::new(Vec::new());

fn log_write(addr: usize, mask: u32) {
    if WRITE_LOG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        WRITE_LOG.lock().unwrap_or_else(|e| e.into_inner()).push((addr, mask));
    }
}

unsafe fn wr(p: usize, off: usize, v: u32) {
    core::ptr::write_unaligned((p + off) as *mut u32, v);
    log_write(p + off, u32::MAX);
}

unsafe fn set_bits(p: usize, off: usize, shift: u32, width: u32, v: u32) {
    let m = ((1u64 << width) - 1) as u32;
    let old = rd(p, off);
    core::ptr::write_unaligned((p + off) as *mut u32, (old & !(m << shift)) | ((v & m) << shift));
    log_write(p + off, m << shift);
}

unsafe fn bits(p: usize, off: usize, shift: u32, width: u32) -> u32 {
    (rd(p, off) >> shift) & (((1u64 << width) - 1) as u32)
}

struct XmaState {
    on: bool,
    pending: VecDeque<i16>,
    decoder: Option<(usize, u32, Box<dyn XmaDecoder>)>,
    budget: usize,
    resume: Option<(u32, u32, u32)>,
    deferred: Option<u32>,
}

fn xma_ring_count(p: usize) -> u32 {
    match unsafe { bits(p, 0, 22, 5) } {
        0 => 32,
        c => c,
    }
}

impl XmaState {
    fn settle(&mut self, p: usize, force: bool) {
        let Some(b) = self.deferred else { return };
        let ch = unsafe { bits(p, 4, 29, 1) } as usize + 1;
        if force || self.pending.len() <= XMA_FRAME_SAMPLES * ch {
            self.deferred = None;
            unsafe {
                set_bits(p, 0, 20 + b, 1, 0);
                set_bits(p, 0x10, 31, 1, b ^ 1);
                set_bits(p, 8, 0, 26, 32);
            }
        }
    }

    fn decode_next(&mut self, p: usize, map: ApuToVa) -> bool {
        self.settle(p, true);
        unsafe {
            for _ in 0..2 {
                let cur = bits(p, 0x10, 31, 1);
                let valid = bits(p, 0, 20 + cur, 1) != 0;
                if !valid {
                    return false;
                }
                let count = if cur == 0 { bits(p, 0, 0, 12) } else { bits(p, 4, 0, 12) } as usize;
                let base = map(rd(p, if cur == 0 { 0x14 } else { 0x18 }));
                let off = bits(p, 8, 0, 26) as usize;
                let idx = off / (XMA_PACKET * 8);
                if idx >= count || base == 0 {
                    set_bits(p, 0, 20 + cur, 1, 0);
                    set_bits(p, 0x10, 31, 1, cur ^ 1);
                    set_bits(p, 8, 0, 26, 32);
                    continue;
                }
                let pkt = core::slice::from_raw_parts((base + idx * XMA_PACKET) as *const u8, XMA_PACKET);
                let ch = bits(p, 4, 29, 1) as usize + 1;
                let rate = sample_rate_code(bits(p, 4, 27, 2));
                if !matches!(&self.decoder, Some((c, r, _)) if *c == ch && *r == rate) {
                    let f = *FACTORY.lock().unwrap_or_else(|e| e.into_inner());
                    self.decoder = Some((ch, rate, f(ch, rate)));
                }
                let mut out = Vec::new();
                self.decoder.as_mut().unwrap().2.decode_packet(pkt, &mut out);
                self.pending.extend(out);
                let skip = xma::packet_skip_count(pkt) as usize;
                let mut next = if skip == 0xff { count } else { idx + 1 + skip };
                let loop_end = bits(p, 0x10, 0, 26) as usize;
                let loops = bits(p, 0, 12, 8);
                if loop_end != 0 && loops != 0 && loop_end / (XMA_PACKET * 8) == idx {
                    let loop_start = bits(p, 0xc, 0, 26) as usize;
                    if loops != 0xff {
                        set_bits(p, 0, 12, 8, loops - 1);
                    }
                    self.decoder = None;
                    set_bits(p, 8, 0, 26, loop_start as u32);
                    return true;
                }
                if next > count {
                    next = count;
                }
                if next >= count {
                    self.deferred = Some(cur);
                    self.settle(p, false);
                } else {
                    set_bits(p, 8, 0, 26, (next * XMA_PACKET * 8 + 32) as u32);
                }
                return true;
            }
            false
        }
    }

    fn service(&mut self, p: usize, map: ApuToVa) {
        if !self.on {
            return;
        }
        unsafe {
            if bits(p, 4, 31, 1) == 0 {
                return;
            }
            let out = map(rd(p, 0x1c));
            if out == 0 {
                return;
            }
            let c = xma_ring_count(p);
            while self.budget > 0 {
                let w = bits(p, 0, 27, 5) % c;
                let r = bits(p, 0x24, 0, 5) % c;
                if (r + c - w - 1) % c == 0 {
                    break;
                }
                if self.pending.len() < FRAME && !self.decode_next(p, map) {
                    break;
                }
                if self.pending.len() < FRAME {
                    continue;
                }
                let blk = (out + w as usize * FRAME * 2) as *mut i16;
                for k in 0..FRAME {
                    *blk.add(k) = self.pending.pop_front().unwrap();
                }
                set_bits(p, 0, 27, 5, (w + 1) % c);
                self.budget -= 1;
                self.settle(p, false);
            }
            self.resume = Some(xma_position(p));
        }
    }
}

pub struct Voice {
    i: u32,
    p: usize,
    map: ApuToVa,
    st: XmaState,
}

unsafe impl Send for Voice {}

impl Voice {
    pub fn service(&mut self) {
        self.st.service(self.p, self.map);
    }
}

fn worker_count() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 16))
}

pub fn service_parallel(voices: &mut [Voice]) {
    let workers = worker_count().min(voices.len());
    if workers <= 1 {
        for v in voices.iter_mut() {
            v.service();
        }
        return;
    }
    let (first, rest) = voices.split_first_mut().unwrap();
    let done = std::sync::Arc::new((Mutex::new(rest.len()), std::sync::Condvar::new()));
    {
        let tx = pool().lock().unwrap_or_else(|e| e.into_inner());
        for v in rest.iter_mut() {
            let _ = tx.send(Job { voice: v as *mut Voice as usize, done: done.clone() });
        }
    }
    first.service();
    let (m, cv) = &*done;
    let mut left = m.lock().unwrap_or_else(|e| e.into_inner());
    while *left > 0 {
        left = cv.wait(left).unwrap_or_else(|e| e.into_inner());
    }
}

struct Job {
    voice: usize,
    done: std::sync::Arc<(Mutex<usize>, std::sync::Condvar)>,
}

fn pool() -> &'static Mutex<std::sync::mpsc::Sender<Job>> {
    static P: std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<Job>>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let rx = std::sync::Arc::new(Mutex::new(rx));
        for k in 0..worker_count().saturating_sub(1).max(1) {
            let rx = rx.clone();
            let _ = std::thread::Builder::new().name(format!("tombox-shape-xma{k}")).spawn(move || loop {
                let job = match rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                    Ok(j) => j,
                    Err(_) => return,
                };
                let v = job.voice as *mut Voice;
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*v).service() }));
                let (m, cv) = &*job.done;
                *m.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
                cv.notify_all();
            });
        }
        Mutex::new(tx)
    })
}

unsafe fn xma_position(p: usize) -> (u32, u32, u32) {
    let cur = bits(p, 0x10, 31, 1);
    (bits(p, 8, 0, 26), cur, rd(p, if cur == 0 { 0x14 } else { 0x18 }))
}

unsafe fn dma_ring_size(p: usize) -> u32 {
    bits(p, 0xc, 0, 5) + 1
}

unsafe fn xma_pass_blocks(p: usize) -> usize {
    let n = bits(p, 4, 20, 4).max(1) as usize;
    n * (bits(p, 4, 29, 1) as usize + 1)
}

#[derive(Default)]
struct SrcState {
    hist: VecDeque<[f32; 2]>,
    pos: f64,
    blk_pos: usize,
}

pub struct Engine {
    pub arrays: Arrays,
    xma: HashMap<u32, XmaState>,
    src: HashMap<u32, SrcState>,
    bufs: HashMap<u32, Box<[f32; FRAME]>>,
    pub config: u32,
    pub frame: u32,
    map: ApuToVa,
}

pub struct Message {
    pub kind: u32,
    pub a: u32,
    pub b: u64,
    pub c: u32,
}

impl Message {
    pub fn encode(&self, stamp: u32) -> [u8; 28] {
        let mut m = [0u8; 28];
        m[0..4].copy_from_slice(&self.kind.to_le_bytes());
        m[8..12].copy_from_slice(&stamp.to_le_bytes());
        m[12..16].copy_from_slice(&self.a.to_le_bytes());
        m[16..24].copy_from_slice(&self.b.to_le_bytes());
        m[24..28].copy_from_slice(&self.c.to_le_bytes());
        m
    }
}

impl Engine {
    pub fn new(arrays: Arrays, map: ApuToVa) -> Engine {
        Engine { arrays, xma: HashMap::new(), src: HashMap::new(), bufs: HashMap::new(), config: 0, frame: 0, map }
    }

    fn xma_ptr(&self, i: u32) -> Option<usize> {
        (i < self.arrays.n_xma && self.arrays.xma != 0).then(|| self.arrays.xma + i as usize * XMA_CTX)
    }

    fn src_ptr(&self, i: u32) -> Option<usize> {
        (i < self.arrays.n_src && self.arrays.src != 0).then(|| self.arrays.src + i as usize * SRC_CTX)
    }

    fn dma_ptr(&self, i: u32) -> Option<usize> {
        (i < self.arrays.n_dma && self.arrays.dma != 0).then(|| self.arrays.dma + i as usize * DMA_CTX)
    }

    fn set_xma(&mut self, i: u32, on: bool) {
        let Some(p) = self.xma_ptr(i) else { return };
        let st = self.xma.entry(i).or_insert_with(|| XmaState {
            on: false,
            pending: VecDeque::new(),
            decoder: None,
            budget: 0,
            resume: None,
            deferred: None,
        });
        if on && !st.on {
            let here = unsafe { xma_position(p) };
            if st.resume != Some(here) {
                st.pending.clear();
                st.decoder = None;
                st.resume = None;
                st.deferred = None;
                self.src.remove(&i);
            }
            st.budget = unsafe { xma_pass_blocks(p) };
        }
        st.on = on;
    }

    fn grant_pass(&mut self, i: u32) {
        let Some(p) = self.xma_ptr(i) else { return };
        if let Some(st) = self.xma.get_mut(&i) {
            st.budget = st.budget.max(unsafe { xma_pass_blocks(p) });
        }
    }

    fn reset_xma_stream(&mut self, i: u32) {
        if let Some(st) = self.xma.get_mut(&i) {
            st.pending.clear();
            st.decoder = None;
            st.resume = None;
            st.deferred = None;
        }
        self.src.remove(&i);
    }

    fn xma_update(&mut self, i: u32, mask: u32, staging: usize) {
        let Some(p) = self.xma_ptr(i) else { return };
        if staging == 0 {
            return;
        }
        let fields: [(u32, usize, u32, u32); 19] = [
            (0, 0, 20, 1),
            (1, 0, 21, 1),
            (4, 0, 0, 12),
            (5, 4, 0, 12),
            (19, 0x14, 0, 32),
            (20, 0x18, 0, 32),
            (14, 8, 0, 26),
            (10, 4, 24, 3),
            (18, 0x10, 31, 1),
            (24, 0, 27, 5),
            (13, 0x24, 0, 5),
            (23, 4, 31, 1),
            (13, 4, 31, 1),
            (23, 0x24, 0, 5),
            (2, 0, 12, 8),
            (2, 4, 12, 2),
            (2, 4, 17, 3),
            (2, 0xc, 0, 26),
            (2, 0x10, 0, 26),
        ];
        let loop_group = mask & ((1 << 2) | (1 << 7) | (1 << 8) | (1 << 15) | (1 << 16)) != 0;
        for (bit, off, shift, width) in fields {
            let on = if bit == 2 { loop_group } else { mask & (1 << bit) != 0 };
            if on {
                unsafe {
                    let v = bits(staging, off, shift, width.min(31));
                    if width == 32 {
                        wr(p, off, rd(staging, off));
                    } else {
                        set_bits(p, off, shift, width, v);
                    }
                }
            }
        }
        if mask & (1 << 14) != 0 {
            self.reset_xma_stream(i);
        }
    }

    fn xma_set_input(&mut self, w: &[u32; 6]) {
        let [i, buf, count, valid, changed, addr] = *w;
        let Some(p) = self.xma_ptr(i) else { return };
        let b = buf & 1;
        unsafe {
            if b == 0 {
                set_bits(p, 0, 0, 12, count);
            } else {
                set_bits(p, 4, 0, 12, count);
            }
            if changed != 0 {
                wr(p, if b == 0 { 0x14 } else { 0x18 }, addr);
            }
            set_bits(p, 0, 20 + b, 1, (valid != 0) as u32);
        }
    }

    pub fn control(&mut self, kind: u32, payload: *const u8) {
        if payload.is_null() {
            return;
        }
        let w = |k: usize| unsafe { rd(payload as usize, k * 4) };
        match kind {
            CMD_CONFIG => self.config = w(0),
            CMD_XMA_ENABLE_ONE | CMD_XMA_DISABLE_ONE => self.set_xma(w(0), kind == CMD_XMA_ENABLE_ONE),
            CMD_XMA_ENABLE_MASK | CMD_XMA_DISABLE_MASK => {
                for word in 0..16u32 {
                    let m = w(word as usize);
                    for b in 0..32 {
                        if m & (1 << b) != 0 {
                            self.set_xma(word * 32 + b, kind == CMD_XMA_ENABLE_MASK);
                        }
                    }
                }
            }
            CMD_XMA_UPDATE => {
                let staging = (self.map)(w(2));
                self.xma_update(w(0), w(1), staging);
            }
            CMD_DMA_CONSUMED => {
                if let Some(p) = self.dma_ptr(w(0)) {
                    let n = w(1);
                    unsafe {
                        let size = dma_ring_size(p);
                        let r = bits(p, 4, 0, 5) % size;
                        set_bits(p, 4, 0, 5, (r + n % size) % size);
                        if n != 0 {
                            set_bits(p, 4, 16, 1, 0);
                        }
                    }
                }
            }
            CMD_XMA_OUT_CONSUMED => {
                if let Some(p) = self.xma_ptr(w(0)) {
                    let c = Self::ring_count(p);
                    unsafe {
                        let r = bits(p, 0x24, 0, 5);
                        set_bits(p, 0x24, 0, 5, (r + w(1) % c) % c);
                    }
                    self.grant_pass(w(0));
                }
            }
            CMD_XMA_SET_INPUT => {
                let v: [u32; 6] = core::array::from_fn(w);
                self.xma_set_input(&v);
            }
            _ => {}
        }
    }
    fn ring_count(p: usize) -> u32 {
        xma_ring_count(p)
    }

    pub fn service_xma(&mut self, i: u32) {
        let Some(p) = self.xma_ptr(i) else { return };
        let map = self.map;
        if let Some(st) = self.xma.get_mut(&i) {
            st.service(p, map);
        }
    }

    pub fn take_voice(&mut self, i: u32) -> Option<Voice> {
        let p = self.xma_ptr(i)?;
        let st = self.xma.remove(&i)?;
        Some(Voice { i, p, map: self.map, st })
    }

    pub fn put_voice(&mut self, mut v: Voice) {
        if let Some(newer) = self.xma.remove(&v.i) {
            v.st.on = newer.on;
            v.st.budget = v.st.budget.max(newer.budget);
        }
        self.xma.insert(v.i, v.st);
    }

    pub fn service_all(&mut self) {
        let ids: Vec<u32> = self.xma.iter().filter(|(_, s)| s.on).map(|(k, _)| *k).collect();
        if ids.len() < 2 {
            for i in ids {
                self.service_xma(i);
            }
            return;
        }
        let mut voices: Vec<Voice> = ids.into_iter().filter_map(|i| self.take_voice(i)).collect();
        service_parallel(&mut voices);
        for v in voices {
            self.put_voice(v);
        }
    }

    fn pull_frame(&mut self, i: u32) -> Option<[f32; 2]> {
        let p = self.xma_ptr(i)?;
        let ch = unsafe { bits(p, 4, 29, 1) } as usize + 1;
        let per_block = FRAME / ch;
        let c = Self::ring_count(p);
        for _ in 0..2 {
            let w = unsafe { bits(p, 0, 27, 5) } % c;
            let r = unsafe { bits(p, 0x24, 0, 5) } % c;
            if r == w {
                self.grant_pass(i);
                self.service_xma(i);
                continue;
            }
            let out = (self.map)(unsafe { rd(p, 0x1c) });
            if out == 0 {
                return None;
            }
            let st = self.src.entry(i).or_default();
            let blk = (out + r as usize * FRAME * 2) as *const i16;
            let s0 = (unsafe { *blk.add(st.blk_pos * ch) }) as f32 / 32768.0;
            let s1 = if ch == 2 { (unsafe { *blk.add(st.blk_pos * ch + 1) }) as f32 / 32768.0 } else { s0 };
            st.blk_pos += 1;
            if st.blk_pos >= per_block {
                st.blk_pos = 0;
                unsafe { set_bits(p, 0x24, 0, 5, (r + 1) % c) };
            }
            return Some([s0, s1]);
        }
        None
    }

    fn buf(&mut self, b: u32) -> &mut [f32; FRAME] {
        self.bufs.entry(b).or_insert_with(|| Box::new([0.0; FRAME]))
    }

    fn run_src(&mut self, i: u32, ba: u32, bb: u32, msgs: &mut Vec<Message>) {
        let Some(sp) = self.src_ptr(i) else { return };
        let target = unsafe { bits(sp, 0x10, 0, 21) };
        let raw = if target != 0 { target } else { unsafe { bits(sp, 4, 0, 21) } };
        let ratio = if raw == 0 { 1.0 } else { raw as f64 / SRC_RATIO_ONE };
        let stereo = self.xma_ptr(i).map(|p| unsafe { bits(p, 4, 29, 1) } != 0).unwrap_or(false);
        let mut la = [0f32; FRAME];
        let mut lb = [0f32; FRAME];
        let mut starved = false;
        for n in 0..FRAME {
            while self.src.get(&i).map_or(0, |s| s.hist.len()) < 2 {
                match self.pull_frame(i) {
                    Some(f) => self.src.entry(i).or_default().hist.push_back(f),
                    None => {
                        starved = true;
                        break;
                    }
                }
            }
            if starved {
                break;
            }
            let st = self.src.get_mut(&i).unwrap();
            let t = st.pos as f32;
            let (x0, x1) = (st.hist[0], st.hist[1]);
            la[n] = x0[0] + (x1[0] - x0[0]) * t;
            lb[n] = x0[1] + (x1[1] - x0[1]) * t;
            st.pos += ratio;
            while st.pos >= 1.0 && !st.hist.is_empty() {
                st.hist.pop_front();
                st.pos -= 1.0;
            }
        }
        if starved && self.xma.get(&i).is_some_and(|s| s.on) {
            msgs.push(Message { kind: MSG_SRC_BLOCKED, a: i, b: 0, c: self.frame });
        }
        let a = self.buf(ba);
        for n in 0..FRAME {
            a[n] += la[n];
        }
        if bb != ba {
            let b = self.buf(bb);
            let src = if stereo { &lb } else { &la };
            for n in 0..FRAME {
                b[n] += src[n];
            }
        }
    }

    fn run_dma(&mut self, d: u32, b: u32, msgs: &mut Vec<Message>) {
        let Some(p) = self.dma_ptr(d) else { return };
        if unsafe { bits(p, 4, 16, 1) } != 0 {
            msgs.push(Message { kind: MSG_DMA_BLOCKED, a: d, b: 0, c: self.frame });
            return;
        }
        let data = *self.buf(b);
        unsafe {
            let base = (self.map)(rd(p, 8) & !0x1ff);
            if base == 0 {
                return;
            }
            let size = dma_ring_size(p);
            let chans = bits(p, 0xc, 16, 3) + 1;
            let ch = bits(p, 0xc, 24, 3);
            let r = bits(p, 4, 0, 5) % size;
            let w = bits(p, 4, 8, 5) % size;
            let dst = (base as *mut f32).add((w * chans + ch) as usize * FRAME);
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, FRAME);
            let nw = (w + 1) % size;
            set_bits(p, 4, 8, 5, nw);
            if nw == r {
                set_bits(p, 4, 16, 1, 1);
            }
        }
    }

    pub fn run_flowgraph(&mut self, cmds: &[u64], msgs: &mut Vec<Message>) -> bool {
        self.service_all();
        for (n, &c) in cmds.iter().enumerate() {
            match c & 0x1f {
                OP_CLEAR => {
                    let b = ((c >> 33) & 0x1fff) as u32;
                    *self.buf(b) = [0.0; FRAME];
                }
                OP_SRC => {
                    let s = ((c >> 13) & 0xfff) as u32;
                    let a = ((c >> 25) & 0x1fff) as u32;
                    let b = ((c >> 38) & 0x1fff) as u32;
                    self.run_src(s, a, b, msgs);
                }
                OP_DMA => {
                    let b = ((c >> 14) & 0x1fff) as u32;
                    let d = ((c >> 27) & 0xfff) as u32;
                    self.run_dma(d, b, msgs);
                }
                _ => {
                    msgs.push(Message { kind: MSG_FLOWGRAPH_TERMINATED, a: 0, b: n as u64, c: self.frame });
                    self.frame = self.frame.wrapping_add(1);
                    return false;
                }
            }
        }
        self.frame = self.frame.wrapping_add(1);
        self.service_all();
        true
    }
}

pub fn encode_clear(buf: u32) -> u64 {
    ((buf as u64 & 0x1fff) << 33) | 0x802001
}

pub fn encode_src(src: u32, a: u32, b: u32) -> u64 {
    ((((b as u64 & 0x1fff) << 13 | (a as u64 & 0x1fff)) << 12 | (src as u64 & 0xfff)) << 13) | OP_SRC
}

pub fn encode_dma(dma: u32, buf: u32) -> u64 {
    ((((dma as u64 & 0xfff) << 13) | (buf as u64 & 0x1fff)) << 14) | OP_DMA
}
