use compat_common::{record, E_NOTIMPL, S_OK};
use opusdec::OpusDecoder;
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub mod rec;
pub use rec::{XApuConnect, XApuDequeueResult, XApuDisconnect, XApuEnqueueCommand, XApuGetResultCount};

const M: &str = "xapu";

pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const XAPU_E_QUEUE_FULL: i32 = 0x8AC8_1000u32 as i32;
pub const XAPU_E_NO_RESULT: i32 = 0x8AC8_1001u32 as i32;
pub const XAPU_E_BAD_STREAM: i32 = 0x8AC8_1003u32 as i32;
pub const XAPU_E_NO_STREAMS: i32 = 0x8AC8_1004u32 as i32;
pub const XAPU_E_STREAM_ACTIVE: i32 = 0x8AC8_1005u32 as i32;
pub const XAPU_E_START_BUFFER: i32 = 0x8AC8_1006u32 as i32;

pub const CODEC_OPUS: u32 = 2;

pub const CMD_START: u32 = 1;
pub const CMD_STOP: u32 = 2;
pub const CMD_DATA: u32 = 3;
pub const CMD_RESET: u32 = 5;

pub const DATA_FLAG_END: u32 = 2;
pub const OPT_RESULT_EVENT: u32 = 8;

const TOTAL_STREAMS: u32 = 500;
const MAX_BASE_DATA: u32 = 0x2000_0000;
const RESULT_SIZE: usize = 0x50;
const CMD_SIZE: usize = 0x40;
const OPUS_RATE: i32 = 48000;
const OPUS_MAX_FRAME: usize = 5760;

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventW(attr: *mut core::ffi::c_void, manual: i32, initial: i32, name: *const u16) -> usize;
    fn SetEvent(h: usize) -> i32;
    fn CloseHandle(h: usize) -> i32;
}

static STREAMS_IN_USE: Mutex<u32> = Mutex::new(0);

fn streams_in_use() -> std::sync::MutexGuard<'static, u32> {
    STREAMS_IN_USE.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub static SYNC: AtomicBool = AtomicBool::new(false);

fn sync_mode() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();
    SYNC.load(Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var("TOMBOX_XAPU_SYNC").is_ok_and(|v| v.trim() == "1"))
}

struct Stream {
    channels: usize,
    start_gain: f32,
    gain: f32,
    decoder: Option<OpusDecoder>,
    fifo: Vec<VecDeque<f32>>,
    pcm: Vec<f32>,
    gains: Vec<f32>,
}

impl Stream {
    fn new() -> Stream {
        Stream { channels: 0, start_gain: 1.0, gain: 1.0, decoder: None, fifo: Vec::new(), pcm: Vec::new(), gains: Vec::new() }
    }
}

#[derive(Default, Clone, Copy)]
struct Ctl {
    active: bool,
    stop_pending: u32,
    queued: u32,
}

struct Job {
    seq: u32,
    kind: u32,
    cmd: [u8; CMD_SIZE],
    start: Option<(usize, f32, OpusDecoder)>,
}

#[derive(Default)]
struct Lane {
    jobs: VecDeque<Job>,
    running: bool,
}

struct Done {
    committed: VecDeque<[u8; RESULT_SIZE]>,
    ready: BTreeMap<u32, [u8; RESULT_SIZE]>,
    next: u32,
}

struct Shared {
    frame: usize,
    epoch: Instant,
    event: usize,
    work: Vec<Mutex<Stream>>,
    lanes: Vec<Mutex<Lane>>,
    done: Mutex<Done>,
    cv: Condvar,
}

impl Shared {
    fn complete(&self, seq: u32, r: [u8; RESULT_SIZE]) {
        let mut d = lock(&self.done);
        d.ready.insert(seq, r);
        let mut any = false;
        loop {
            let n = d.next;
            let Some(r) = d.ready.remove(&n) else { break };
            d.committed.push_back(r);
            d.next = n.wrapping_add(1);
            any = true;
        }
        if any {
            if self.event != 0 {
                unsafe { SetEvent(self.event) };
            }
            self.cv.notify_all();
        }
    }

    fn wait_through(&self, seq: u32) {
        let mut d = lock(&self.done);
        while (d.next.wrapping_sub(1).wrapping_sub(seq) as i32) < 0 {
            d = self.cv.wait(d).unwrap_or_else(|e| e.into_inner());
        }
    }
}

type Task = Box<dyn FnOnce() + Send>;

fn spawn_task(t: Task) -> Result<(), Task> {
    static POOL: OnceLock<Option<Mutex<mpsc::Sender<Task>>>> = OnceLock::new();
    let pool = POOL.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Task>();
        let rx = Arc::new(Mutex::new(rx));
        let n = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 8);
        let mut started = 0;
        for i in 0..n {
            let rx = rx.clone();
            let ok = std::thread::Builder::new()
                .name(format!("xapu-worker-{i}"))
                .spawn(move || loop {
                    let t = lock(&rx).recv();
                    match t {
                        Ok(t) => t(),
                        Err(_) => break,
                    }
                })
                .is_ok();
            started += ok as usize;
        }
        record(M, "pool", &format!("{started} host decode workers"));
        (started > 0).then(|| Mutex::new(tx))
    });
    match pool {
        Some(tx) => lock(tx).send(t).map_err(|e| e.0),
        None => Err(t),
    }
}

fn drain(sh: Arc<Shared>, s: usize) {
    loop {
        let job = {
            let mut l = lock(&sh.lanes[s]);
            match l.jobs.pop_front() {
                Some(j) => j,
                None => {
                    l.running = false;
                    return;
                }
            }
        };
        let Job { seq, kind, cmd, start } = job;
        let r = {
            let mut st = lock(&sh.work[s]);
            let st = &mut *st;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&sh, st, kind, &cmd, start))).unwrap_or_else(|_| {
                record(M, "worker", &format!("decode of seq {seq} stream {s} panicked -> E_FAIL"));
                base_result(sh.epoch, &cmd, E_FAIL)
            })
        };
        sh.complete(seq, r);
    }
}

fn run_job(sh: &Shared, st: &mut Stream, kind: u32, cmd: &[u8; CMD_SIZE], start: Option<(usize, f32, OpusDecoder)>) -> [u8; RESULT_SIZE] {
    match kind {
        CMD_START => {
            if let Some((ch, gain, dec)) = start {
                st.channels = ch;
                st.start_gain = gain;
                st.gain = gain;
                st.decoder = Some(dec);
                st.fifo = vec![VecDeque::new(); ch];
            }
            base_result(sh.epoch, cmd, S_OK)
        }
        CMD_RESET => {
            reset_stream(st);
            base_result(sh.epoch, cmd, S_OK)
        }
        CMD_STOP => base_result(sh.epoch, cmd, S_OK),
        _ => data_command(sh.frame, sh.epoch, st, cmd),
    }
}

pub struct Apu {
    codec: u32,
    depth: u32,
    frame: usize,
    mem: usize,
    mem_layout: Option<Layout>,
    event: usize,
    ctl: Vec<Ctl>,
    enqueued: u32,
    dequeued: u32,
    lock: Mutex<()>,
    sh: Arc<Shared>,
}

impl Apu {
    pub(crate) fn last_result(&self) -> Option<[u8; RESULT_SIZE]> {
        lock(&self.sh.done).committed.back().copied()
    }

    pub(crate) fn pending(&self) -> usize {
        lock(&self.sh.done).committed.len()
    }

    pub(crate) fn channels(&self, s: usize) -> usize {
        self.sh.work.get(s).map_or(1, |w| lock(w).channels.max(1))
    }

    pub(crate) fn frame(&self) -> usize {
        self.frame
    }

    pub(crate) fn wait_idle(&self) {
        self.sh.wait_through(self.enqueued);
    }
}

fn obj(h: usize) -> Option<&'static mut Apu> {
    if h == 0 || h == usize::MAX {
        return None;
    }
    Some(unsafe { &mut *((!h) as *mut Apu) })
}

unsafe fn rd32(p: *const u8, o: usize) -> u32 {
    core::ptr::read_unaligned(p.add(o) as *const u32)
}

fn put32(r: &mut [u8], o: usize, v: u32) {
    r[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(r: &mut [u8], o: usize, v: u64) {
    r[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

pub(crate) extern "system" fn connect_impl(params: *const u32, region: *mut u8, handle: *mut usize) -> i32 {
    if region.is_null() || handle.is_null() {
        return E_POINTER;
    }
    unsafe {
        *handle = 0;
        core::ptr::write_bytes(region, 0, 16);
    }
    if params.is_null() {
        return E_INVALIDARG;
    }
    let p: [u32; 6] = core::array::from_fn(|i| unsafe { core::ptr::read_unaligned(params.add(i)) });
    let [codec, streams, depth, mem_size, frame, options] = p;
    let ok = (1..=3).contains(&codec)
        && (1..=TOTAL_STREAMS).contains(&streams)
        && (1..=16).contains(&depth)
        && (codec == 1 || ((0x80..=0x400).contains(&frame) && frame % 2 == 0))
        && options & !0xF == 0;
    if !ok {
        record(M, "XApuConnect", &format!("params={p:x?} -> E_INVALIDARG"));
        return E_INVALIDARG;
    }
    if mem_size > MAX_BASE_DATA {
        return E_OUTOFMEMORY;
    }
    if codec != CODEC_OPUS {
        record(M, "XApuConnect", &format!("codec={codec} -> E_NOTIMPL (only processType 2 = Opus is identified; no title uses 1 or 3)"));
        return E_NOTIMPL;
    }
    {
        let mut u = streams_in_use();
        if *u + streams > TOTAL_STREAMS {
            return XAPU_E_NO_STREAMS;
        }
        *u += streams;
    }
    let (mem, mem_layout) = if mem_size != 0 {
        let l = Layout::from_size_align(mem_size as usize, 0x10000).unwrap();
        let m = unsafe { alloc_zeroed(l) } as usize;
        if m == 0 {
            *streams_in_use() -= streams;
            return E_OUTOFMEMORY;
        }
        (m, Some(l))
    } else {
        (0, None)
    };
    let event = if options & OPT_RESULT_EVENT != 0 { unsafe { CreateEventW(core::ptr::null_mut(), 0, 0, core::ptr::null()) } } else { 0 };
    let sh = Arc::new(Shared {
        frame: frame as usize,
        epoch: Instant::now(),
        event,
        work: (0..streams).map(|_| Mutex::new(Stream::new())).collect(),
        lanes: (0..streams).map(|_| Mutex::new(Lane::default())).collect(),
        done: Mutex::new(Done { committed: VecDeque::new(), ready: BTreeMap::new(), next: 1 }),
        cv: Condvar::new(),
    });
    let apu = Box::into_raw(Box::new(Apu { codec, depth, frame: frame as usize, mem, mem_layout, event, ctl: vec![Ctl::default(); streams as usize], enqueued: 0, dequeued: 0, lock: Mutex::new(()), sh }));
    unsafe {
        core::ptr::write_unaligned(region as *mut u64, mem as u64);
        core::ptr::write_unaligned(region.add(8) as *mut u64, event as u64);
        *handle = !(apu as usize);
    }
    record(M, "XApuConnect", &format!("codec={codec} streams={streams} depth={depth} mem={mem_size:#x} frame={frame} options={options:#x} -> S_OK (Opus decoded asynchronously on host workers)"));
    S_OK
}

pub(crate) extern "system" fn disconnect_impl(h: usize) -> i32 {
    let Some(a) = obj(h) else { return E_INVALIDARG };
    a.wait_idle();
    let outstanding = a.dequeued.wrapping_add(a.pending() as u32) != a.enqueued;
    *streams_in_use() -= a.ctl.len() as u32;
    if let Some(l) = a.mem_layout {
        unsafe { dealloc(a.mem as *mut u8, l) };
    }
    if a.event != 0 {
        unsafe { CloseHandle(a.event) };
    }
    drop(unsafe { Box::from_raw(a as *mut Apu) });
    record(M, "XApuDisconnect", "-> torn down");
    if outstanding {
        XAPU_E_NO_RESULT
    } else {
        S_OK
    }
}

fn base_result(epoch: Instant, cmd: &[u8; CMD_SIZE], status: i32) -> [u8; RESULT_SIZE] {
    let mut r = [0u8; RESULT_SIZE];
    r[0..0x18].copy_from_slice(&cmd[0..0x18]);
    let t = (epoch.elapsed().as_nanos() / 100) as u64;
    put64(&mut r, 0x24, t);
    put32(&mut r, 0x2c, status as u32);
    r
}

fn prepare_start(frame: usize, cmd: &[u8; CMD_SIZE]) -> Result<(usize, f32, OpusDecoder), i32> {
    let ch = u32::from_le_bytes(cmd[0x18..0x1c].try_into().unwrap()) as usize;
    let gain = f32::from_le_bytes(cmd[0x28..0x2c].try_into().unwrap());
    let size = u32::from_le_bytes(cmd[0x2c..0x30].try_into().unwrap()) as usize;
    if !(1..=2).contains(&ch) {
        return Err(E_INVALIDARG);
    }
    let need_out = ch * frame * 4;
    if size < need_out || size - need_out < ch * 480 * 4 {
        return Err(XAPU_E_START_BUFFER);
    }
    let dec = OpusDecoder::new(OPUS_RATE, ch).map_err(|_| E_INVALIDARG)?;
    Ok((ch, if gain.is_finite() { gain } else { 1.0 }, dec))
}

fn reset_stream(st: &mut Stream) {
    st.gain = st.start_gain;
    if let Some(d) = st.decoder.as_mut() {
        d.reset();
    }
    for f in st.fifo.iter_mut() {
        f.clear();
    }
}

fn data_command(frame: usize, epoch: Instant, st: &mut Stream, cmd: &[u8; CMD_SIZE]) -> [u8; RESULT_SIZE] {
    let g = |o: usize| u32::from_le_bytes(cmd[o..o + 4].try_into().unwrap());
    let g64 = |o: usize| u64::from_le_bytes(cmd[o..o + 8].try_into().unwrap());
    let step = f32::from_bits(g(0x18));
    let target = f32::from_bits(g(0x1c));
    let skip = g(0x20) as usize;
    let keep = g(0x24);
    let in_len = g(0x28) as usize;
    let in_ptr = g64(0x2c) as usize;
    let out_cap = g(0x34) as usize;
    let out_ptr = g64(0x38) as usize;
    let mut status = S_OK;
    let ch = st.channels.max(1);
    let mut covered = 0usize;
    if in_ptr != 0 && in_len != 0 {
        let pkt = unsafe { core::slice::from_raw_parts(in_ptr as *const u8, in_len.min(0x10000)) };
        if st.pcm.len() != OPUS_MAX_FRAME * ch {
            st.pcm = vec![0f32; OPUS_MAX_FRAME * ch];
        }
        let n = match st.decoder.as_mut() {
            Some(d) => d.decode_float(pkt, &mut st.pcm, OPUS_MAX_FRAME as i32, false),
            None => opusdec::celt::OPUS_INTERNAL_ERROR,
        };
        if n < 0 {
            status = E_FAIL;
        } else {
            let n = n as usize;
            let start = skip.min(n);
            let end = if keep == u32::MAX { n } else { (start + keep as usize).min(n) };
            let pcm = &st.pcm;
            for (c, f) in st.fifo.iter_mut().enumerate().take(ch) {
                f.extend((start..end).map(|i| pcm[i * ch + c]));
            }
            covered = end - start;
        }
    }
    let avail = st.fifo.first().map_or(0, |f| f.len());
    let fit = if out_ptr == 0 { 0 } else { out_cap / (4 * ch) };
    let count = avail.min(frame).min(fit);
    let step = if step.is_finite() { step } else { 0.0 };
    let target = if target.is_finite() { target } else { st.gain };
    let mut gain = st.gain;
    st.gains.clear();
    for _ in 0..count {
        if gain != target {
            gain += step;
            if (step >= 0.0 && gain > target) || (step < 0.0 && gain < target) || step == 0.0 {
                gain = target;
            }
        }
        st.gains.push(gain);
    }
    for c in 0..ch {
        if count == 0 {
            break;
        }
        let dst = unsafe { (out_ptr as *mut f32).add(c * frame) };
        let mut put = |i: usize, v: f32| unsafe { core::ptr::write_unaligned(dst.add(i), v) };
        match st.fifo.get_mut(c) {
            Some(f) => {
                let have = count.min(f.len());
                for (i, (s, g)) in f.drain(..have).chain(core::iter::repeat(0.0)).zip(&st.gains).enumerate() {
                    put(i, s * g);
                }
            }
            None => {
                for (i, g) in st.gains.iter().enumerate() {
                    put(i, 0.0 * g);
                }
            }
        }
    }
    if count == 0 && gain != target && step == 0.0 {
        gain = target;
    }
    st.gain = gain;
    let left = st.fifo.first().map_or(0, |f| f.len());
    let mut r = base_result(epoch, cmd, status);
    r[0x1c..0x1e].copy_from_slice(&(left.min(0xffff) as u16).to_le_bytes());
    r[0x1e..0x20].copy_from_slice(&(covered.min(0xffff) as u16).to_le_bytes());
    put32(&mut r, 0x30, count as u32);
    put32(&mut r, 0x34, (count * ch * 4) as u32);
    put64(&mut r, 0x38, out_ptr as u64);
    put32(&mut r, 0x40, gain.to_bits());
    put64(&mut r, 0x48, in_ptr as u64);
    r
}

pub(crate) extern "system" fn enqueue_impl(h: usize, cmd: *const u8, out_seq: *mut u32) -> i32 {
    static LOG: AtomicU32 = AtomicU32::new(0);
    let Some(a) = obj(h) else { return E_INVALIDARG };
    if cmd.is_null() {
        return E_INVALIDARG;
    }
    let lk: &'static Mutex<()> = unsafe { &*(&a.lock as *const Mutex<()>) };
    let g = lock(lk);
    let kind = unsafe { rd32(cmd, 0) };
    let stream = unsafe { rd32(cmd, 4) } as usize;
    if !matches!(kind, CMD_START | CMD_STOP | CMD_DATA | CMD_RESET) {
        return E_INVALIDARG;
    }
    if stream >= a.ctl.len() {
        return XAPU_E_BAD_STREAM;
    }
    let mut c = [0u8; CMD_SIZE];
    let copy = if kind == CMD_DATA { CMD_SIZE } else { 0x38 };
    unsafe { core::ptr::copy_nonoverlapping(cmd, c.as_mut_ptr(), copy) };
    {
        let ctl = &a.ctl[stream];
        if kind == CMD_START && ctl.active {
            return XAPU_E_STREAM_ACTIVE;
        }
        if kind != CMD_START && !ctl.active {
            return XAPU_E_BAD_STREAM;
        }
        if ctl.queued >= a.depth {
            return XAPU_E_QUEUE_FULL;
        }
    }
    let start = if kind == CMD_START {
        match prepare_start(a.frame, &c) {
            Ok(x) => Some(x),
            Err(e) => return e,
        }
    } else {
        None
    };
    let ctl = &mut a.ctl[stream];
    match kind {
        CMD_START => ctl.active = true,
        CMD_STOP => ctl.stop_pending += 1,
        _ => {}
    }
    ctl.queued += 1;
    a.enqueued = a.enqueued.wrapping_add(1);
    let seq = a.enqueued;
    if !out_seq.is_null() {
        unsafe { *out_seq = seq };
    }
    let sh = a.sh.clone();
    let launch = {
        let mut l = lock(&sh.lanes[stream]);
        l.jobs.push_back(Job { seq, kind, cmd: c, start });
        !std::mem::replace(&mut l.running, true)
    };
    let codec = a.codec;
    drop(g);
    let sync = sync_mode();
    if launch {
        if sync {
            drain(sh.clone(), stream);
        } else {
            let s2 = sh.clone();
            if let Err(t) = spawn_task(Box::new(move || drain(s2, stream))) {
                t();
            }
        }
    }
    if sync {
        sh.wait_through(seq);
    }
    let n = LOG.fetch_add(1, Ordering::Relaxed);
    if n < 8 {
        record(M, "XApuEnqueueCommand", &format!("codec={codec} type={kind} stream={stream} seq={seq} -> S_OK (queued to host decode worker)"));
    }
    S_OK
}

pub(crate) extern "system" fn dequeue_impl(h: usize, out: *mut u8) -> i32 {
    let Some(a) = obj(h) else { return E_INVALIDARG };
    if out.is_null() {
        return E_POINTER;
    }
    let _g = lock(&a.lock);
    let r = lock(&a.sh.done).committed.pop_front();
    match r {
        Some(r) => {
            let kind = u32::from_le_bytes(r[0..4].try_into().unwrap());
            let s = u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize;
            if let Some(c) = a.ctl.get_mut(s) {
                c.queued = c.queued.saturating_sub(1);
                if kind == CMD_STOP && c.stop_pending > 0 {
                    c.stop_pending -= 1;
                    c.active = false;
                    let mut st = lock(&a.sh.work[s]);
                    st.decoder = None;
                    st.fifo.clear();
                }
            }
            a.dequeued = a.dequeued.wrapping_add(1);
            unsafe { core::ptr::copy_nonoverlapping(r.as_ptr(), out, RESULT_SIZE) };
            S_OK
        }
        None => XAPU_E_NO_RESULT,
    }
}

pub(crate) extern "system" fn result_count_impl(h: usize, dequeued_out: *mut u32) -> u32 {
    let Some(a) = obj(h) else { return 0 };
    let _g = lock(&a.lock);
    if !dequeued_out.is_null() {
        unsafe { *dequeued_out = a.dequeued };
    }
    a.pending() as u32
}

#[no_mangle]
pub extern "system" fn XApuGetUtilization(codec: u32, kind: u32, out: *mut u32) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { core::ptr::write_unaligned(out as *mut u64, 0) };
    if !(1..=3).contains(&codec) || !(1..=2).contains(&kind) {
        return E_INVALIDARG;
    }
    let used = *streams_in_use();
    unsafe {
        *out = TOTAL_STREAMS - used;
        *out.add(1) = TOTAL_STREAMS;
    }
    S_OK
}

#[cfg(test)]
mod tests;
