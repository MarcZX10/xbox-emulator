use super::{connect_impl, dequeue_impl, disconnect_impl, enqueue_impl, obj, result_count_impl, CMD_DATA, CMD_SIZE, DATA_FLAG_END, OPUS_RATE, RESULT_SIZE, S_OK};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub const MAGIC: &[u8; 8] = b"TBAREC1X";

pub const T_CONNECT: u8 = 1;
pub const T_DISCONNECT: u8 = 2;
pub const T_ENQUEUE: u8 = 3;
pub const T_DEQUEUE: u8 = 4;
pub const T_COUNT: u8 = 5;

static FORCE: AtomicBool = AtomicBool::new(false);
static REPLAYING: AtomicBool = AtomicBool::new(false);
static OPEN_TRIED: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();
    !REPLAYING.load(Ordering::Relaxed) && (FORCE.load(Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var("TOMBOX_AUDIO_RECORD").is_ok_and(|v| v.trim() == "1")))
}

struct Rec {
    out: BufWriter<File>,
    epoch: Instant,
    last_flush: Instant,
}

static REC: Mutex<Option<Rec>> = Mutex::new(None);

impl Rec {
    fn new(path: &Path) -> std::io::Result<Rec> {
        let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
        out.write_all(MAGIC)?;
        Ok(Rec { out, epoch: Instant::now(), last_flush: Instant::now() })
    }

    fn write(&mut self, tag: u8, payload: &[u8]) {
        let t = self.epoch.elapsed().as_nanos() as u64;
        let _ = write_record(&mut self.out, tag, t, payload);
        if self.last_flush.elapsed().as_millis() >= 500 || tag == T_DISCONNECT {
            let _ = self.out.flush();
            self.last_flush = Instant::now();
        }
    }
}

fn le(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn word(p: &[u8], k: usize) -> u32 {
    p.get(k * 4..k * 4 + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn at32(p: &[u8], o: usize) -> u32 {
    p.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn at64(p: &[u8], o: usize) -> u64 {
    p.get(o..o + 8).map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()))
}

fn default_path() -> PathBuf {
    let dir = std::env::var_os("TOMBOX_AUDIO_RECORD_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("TomBox").join("audio-rec")
    });
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    dir.join(format!("xapu-{}-{secs}.tbrec", std::process::id()))
}

fn guard() -> MutexGuard<'static, Option<Rec>> {
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() && !OPEN_TRIED.swap(true, Ordering::SeqCst) {
        let path = default_path();
        match Rec::new(&path) {
            Ok(r) => {
                compat_common::record("xapu", "audiorec", &format!("recording to {}", path.display()));
                *g = Some(r);
            }
            Err(e) => compat_common::record("xapu", "audiorec", &format!("cannot create {}: {e}", path.display())),
        }
    }
    g
}

pub fn record_to(path: &Path) -> std::io::Result<()> {
    let r = Rec::new(path)?;
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    OPEN_TRIED.store(true, Ordering::SeqCst);
    *g = Some(r);
    FORCE.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn stop_recording() {
    let mut g = REC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = g.as_mut() {
        let _ = r.out.flush();
    }
    *g = None;
    FORCE.store(false, Ordering::SeqCst);
}

fn op<R>(f: impl FnOnce() -> R, post: impl FnOnce(&mut Rec, &R)) -> R {
    if !enabled() {
        return f();
    }
    let mut g = guard();
    if g.is_none() {
        drop(g);
        return f();
    }
    let res = f();
    post(g.as_mut().unwrap(), &res);
    res
}

fn hl(h: usize) -> [u32; 2] {
    [h as u64 as u32, ((h as u64) >> 32) as u32]
}

#[no_mangle]
pub extern "system" fn XApuConnect(params: *const u32, region: *mut u8, handle: *mut usize) -> i32 {
    op(
        || connect_impl(params, region, handle),
        |r, hr| {
            let p: [u32; 6] = if params.is_null() { [0; 6] } else { core::array::from_fn(|i| unsafe { core::ptr::read_unaligned(params.add(i)) }) };
            let h = if handle.is_null() { 0 } else { unsafe { *handle } };
            let mut v = p.to_vec();
            v.extend_from_slice(&[*hr as u32, hl(h)[0], hl(h)[1], params.is_null() as u32]);
            r.write(T_CONNECT, &le(&v));
        },
    )
}

#[no_mangle]
pub extern "system" fn XApuDisconnect(h: usize) -> i32 {
    op(|| disconnect_impl(h), |r, hr| r.write(T_DISCONNECT, &le(&[hl(h)[0], hl(h)[1], *hr as u32])))
}

#[no_mangle]
pub extern "system" fn XApuEnqueueCommand(h: usize, cmd: *const u8, out_seq: *mut u32) -> i32 {
    let mut c = [0u8; CMD_SIZE];
    let mut input = Vec::new();
    if enabled() && !cmd.is_null() {
        let kind = unsafe { core::ptr::read_unaligned(cmd as *const u32) };
        let n = if kind == CMD_DATA { CMD_SIZE } else { 0x38 };
        unsafe { core::ptr::copy_nonoverlapping(cmd, c.as_mut_ptr(), n) };
        let len = at32(&c, 0x28) as usize;
        let ptr = at64(&c, 0x2c) as usize;
        if kind == CMD_DATA && ptr != 0 && len != 0 {
            input = unsafe { core::slice::from_raw_parts(ptr as *const u8, len.min(0x10000)) }.to_vec();
        }
    }
    op(
        || enqueue_impl(h, cmd, out_seq),
        |r, hr| {
            let seq = if out_seq.is_null() || *hr != S_OK { 0 } else { unsafe { *out_seq } };
            let mut v = le(&[hl(h)[0], hl(h)[1], *hr as u32, seq, cmd.is_null() as u32]);
            v.extend_from_slice(&c);
            v.extend_from_slice(&input);
            r.write(T_ENQUEUE, &v);
        },
    )
}

#[no_mangle]
pub extern "system" fn XApuDequeueResult(h: usize, out: *mut u8) -> i32 {
    op(
        || dequeue_impl(h, out),
        |r, hr| {
            let mut v = le(&[hl(h)[0], hl(h)[1], *hr as u32]);
            let mut res = [0u8; RESULT_SIZE];
            if *hr == S_OK && !out.is_null() {
                unsafe { core::ptr::copy_nonoverlapping(out, res.as_mut_ptr(), RESULT_SIZE) };
            }
            v.extend_from_slice(&res);
            r.write(T_DEQUEUE, &v);
        },
    )
}

#[no_mangle]
pub extern "system" fn XApuGetResultCount(h: usize, dequeued_out: *mut u32) -> u32 {
    op(
        || result_count_impl(h, dequeued_out),
        |r, ret| {
            let d = if dequeued_out.is_null() { 0 } else { unsafe { *dequeued_out } };
            r.write(T_COUNT, &le(&[hl(h)[0], hl(h)[1], *ret, d]));
        },
    )
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

pub struct Output {
    pub name: String,
    pub channels: u16,
    pub rate: u32,
    pub data: Vec<f32>,
}

pub struct Replay {
    pub report: String,
    pub csv: String,
    pub outputs: Vec<Output>,
}

#[derive(Default)]
struct RState {
    handles: HashMap<u64, usize>,
    outputs: HashMap<(u64, u32), usize>,
    outs: Vec<Output>,
    mism: HashMap<&'static str, u64>,
    examples: Vec<String>,
    decode_ns: Vec<(u64, u32)>,
    starved: u64,
    starved_samples: u64,
    decode_errors: u64,
    queue_full_rec: u64,
    queue_full_replay: u64,
    max_pending: usize,
    data_cmds: u64,
    last_t: u64,
    not_yet_done: u64,
    count_behind: u64,
}

impl RState {
    fn mismatch(&mut self, what: &'static str, t: u64, rec: u32, got: u32) {
        if rec == got {
            return;
        }
        *self.mism.entry(what).or_default() += 1;
        if self.examples.len() < 40 {
            self.examples.push(format!("t={:.3}ms {what}: recorded {rec:#x}, replay {got:#x}", t as f64 / 1e6));
        }
    }
}

pub fn replay(path: &Path) -> Result<Replay, String> {
    REPLAYING.store(true, Ordering::SeqCst);
    let was_sync = super::SYNC.swap(true, Ordering::SeqCst);
    let r = replay_inner(path);
    super::SYNC.store(was_sync, Ordering::SeqCst);
    REPLAYING.store(false, Ordering::SeqCst);
    r
}

fn replay_inner(path: &Path) -> Result<Replay, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rd = BufReader::with_capacity(1 << 20, f);
    let mut magic = [0u8; 8];
    rd.read_exact(&mut magic).map_err(|e| format!("header: {e}"))?;
    if &magic != MAGIC {
        return Err(format!("{}: not an xapu recording", path.display()));
    }
    let mut st = RState::default();
    let mut csv = String::from("t_ms,op,stream,op_us,pending_results,samples_out\n");
    while let Some((tag, t, p)) = read_record(&mut rd).map_err(|e| e.to_string())? {
        st.last_t = t;
        let w = |k: usize| word(&p, k);
        let rh = |k: usize| w(k) as u64 | (w(k + 1) as u64) << 32;
        match tag {
            T_CONNECT => {
                let params: [u32; 6] = core::array::from_fn(w);
                let mut region = [0u8; 16];
                let mut h = 0usize;
                let pp = if w(9) != 0 { core::ptr::null() } else { params.as_ptr() };
                let hr = connect_impl(pp, region.as_mut_ptr(), &mut h);
                st.mismatch("Connect hr", t, w(6), hr as u32);
                if hr == S_OK {
                    st.handles.insert(rh(7), h);
                }
            }
            T_DISCONNECT => {
                let h = st.handles.remove(&rh(0)).unwrap_or(0);
                let hr = disconnect_impl(h);
                st.mismatch("Disconnect hr", t, w(2), hr as u32);
            }
            T_ENQUEUE => {
                let key = rh(0);
                let h = st.handles.get(&key).copied().unwrap_or(0);
                let rec_hr = w(2);
                if rec_hr == super::XAPU_E_QUEUE_FULL as u32 {
                    st.queue_full_rec += 1;
                }
                let mut c = [0u8; CMD_SIZE];
                c.copy_from_slice(&p[20..20 + CMD_SIZE]);
                let input = p[20 + CMD_SIZE..].to_vec();
                let kind = at32(&c, 0);
                let stream = at32(&c, 4);
                let out_cap = at32(&c, 0x34) as usize;
                let mut outbuf = vec![0f32; out_cap.div_ceil(4)];
                if kind == CMD_DATA {
                    if at64(&c, 0x2c) != 0 {
                        c[0x2c..0x34].copy_from_slice(&(input.as_ptr() as u64).to_le_bytes());
                        c[0x28..0x2c].copy_from_slice(&(input.len() as u32).to_le_bytes());
                    }
                    if at64(&c, 0x38) != 0 {
                        c[0x38..0x40].copy_from_slice(&(outbuf.as_mut_ptr() as u64).to_le_bytes());
                    }
                }
                let mut seq = 0u32;
                let cp = if w(4) != 0 { core::ptr::null() } else { c.as_ptr() };
                let t0 = Instant::now();
                let hr = enqueue_impl(h, cp, &mut seq);
                let ns = t0.elapsed().as_nanos() as u64;
                if hr == super::XAPU_E_QUEUE_FULL {
                    st.queue_full_replay += 1;
                }
                st.mismatch("Enqueue hr", t, rec_hr, hr as u32);
                let mut samples = 0u32;
                if hr == S_OK && kind == CMD_DATA {
                    if let Some(a) = obj(h) {
                        let r = a.last_result().unwrap_or([0u8; RESULT_SIZE]);
                        samples = at32(&r, 0x30);
                        let covered = u16::from_le_bytes([r[0x1e], r[0x1f]]) as u32;
                        if at32(&r, 0x2c) != S_OK as u32 {
                            st.decode_errors += 1;
                        }
                        st.data_cmds += 1;
                        st.decode_ns.push((ns, covered));
                        let frame = a.frame();
                        let ch = a.channels(stream as usize);
                        let flags = at32(&c, 0xc);
                        if (samples as usize) < frame && flags & DATA_FLAG_END == 0 && at64(&c, 0x38) != 0 {
                            st.starved += 1;
                            st.starved_samples += (frame - samples as usize) as u64;
                        }
                        let slot = *st.outputs.entry((key, stream)).or_insert_with(|| {
                            st.outs.push(Output { name: format!("xapu_h{}_s{stream:03}_{ch}ch_48000", st.handles.len()), channels: ch as u16, rate: OPUS_RATE as u32, data: Vec::new() });
                            st.outs.len() - 1
                        });
                        for i in 0..samples as usize {
                            for k in 0..ch {
                                st.outs[slot].data.push(outbuf.get(k * frame + i).copied().unwrap_or(0.0));
                            }
                        }
                    }
                }
                let pending = obj(h).map_or(0, |a| a.pending());
                st.max_pending = st.max_pending.max(pending);
                csv.push_str(&format!("{:.3},enqueue{kind},{stream},{:.1},{pending},{samples}\n", t as f64 / 1e6, ns as f64 / 1e3));
            }
            T_DEQUEUE => {
                let h = st.handles.get(&rh(0)).copied().unwrap_or(0);
                if w(2) == super::XAPU_E_NO_RESULT as u32 && obj(h).is_some_and(|a| a.pending() > 0) {
                    st.not_yet_done += 1;
                    continue;
                }
                let mut r = [0u8; RESULT_SIZE];
                let hr = dequeue_impl(h, r.as_mut_ptr());
                st.mismatch("Dequeue hr", t, w(2), hr as u32);
                if hr == S_OK && w(2) == S_OK as u32 {
                    let rec = &p[12..12 + RESULT_SIZE];
                    st.mismatch("result status", t, at32(rec, 0x2c), at32(&r, 0x2c));
                    st.mismatch("result samples", t, at32(rec, 0x30), at32(&r, 0x30));
                }
                let pending = obj(h).map_or(0, |a| a.pending());
                csv.push_str(&format!("{:.3},dequeue,-,0,{pending},0\n", t as f64 / 1e6));
            }
            T_COUNT => {
                let h = st.handles.get(&rh(0)).copied().unwrap_or(0);
                let mut d = 0u32;
                let ret = result_count_impl(h, &mut d);
                if w(2) < ret {
                    st.count_behind += 1;
                } else {
                    st.mismatch("GetResultCount", t, w(2), ret);
                }
            }
            _ => {}
        }
    }
    let report = build_report(path, &st);
    Ok(Replay { report, csv, outputs: std::mem::take(&mut st.outs) })
}

fn build_report(path: &Path, st: &RState) -> String {
    let mut r = format!("xapu replay of {}\nrecorded span {:.3} s\n\n", path.display(), st.last_t as f64 / 1e9);
    r.push_str("Opus data commands (decode + output, replay CPU)\n");
    if st.decode_ns.is_empty() {
        r.push_str("  none\n");
    } else {
        let mut ns: Vec<u64> = st.decode_ns.iter().map(|x| x.0).collect();
        ns.sort_unstable();
        let total: u64 = ns.iter().sum();
        let audio: f64 = st.decode_ns.iter().map(|x| x.1 as f64 / OPUS_RATE as f64).sum();
        let late = st.decode_ns.iter().filter(|x| x.1 != 0 && x.0 as f64 > x.1 as f64 / OPUS_RATE as f64 * 1e9).count();
        let pct = |q: f64| ns[((ns.len() - 1) as f64 * q) as usize] as f64 / 1e3;
        r.push_str(&format!("  commands {}  audio decoded {:.3} s  CPU {:.3} ms  real-time factor {:.1}x\n", st.data_cmds, audio, total as f64 / 1e6, audio / (total as f64 / 1e9).max(1e-12)));
        r.push_str(&format!("  per command: mean {:.1} us  p50 {:.1}  p99 {:.1}  max {:.1} us  slower than own audio: {late}\n", total as f64 / ns.len() as f64 / 1e3, pct(0.5), pct(0.99), pct(1.0)));
        if st.last_t > 0 {
            r.push_str(&format!("  decoder load over the recorded span: {:.2}% of one core\n", total as f64 / st.last_t as f64 * 100.0));
        }
    }
    r.push_str(&format!("  decode errors {}\n\nqueues and stalls\n", st.decode_errors));
    r.push_str(&format!("  max pending results {}\n  QUEUE_FULL: recorded {}  replay {}\n", st.max_pending, st.queue_full_rec, st.queue_full_replay));
    r.push_str(&format!("  underruns (data command returned fewer than frame samples, not end of stream) {}  missing samples {}\n", st.starved, st.starved_samples));
    r.push_str(&format!("  live run still decoding (replay decodes synchronously): Dequeue returned NO_RESULT {} times, GetResultCount was lower {} times\n", st.not_yet_done, st.count_behind));
    r.push_str("\nreplay vs recording\n");
    if st.mism.is_empty() {
        r.push_str("  every return value and result matched the recording\n");
    } else {
        let mut m: Vec<_> = st.mism.iter().collect();
        m.sort();
        for (k, v) in m {
            r.push_str(&format!("  {k}: {v} differ\n"));
        }
        for e in &st.examples {
            r.push_str(&format!("    {e}\n"));
        }
    }
    r.push_str("\noutputs\n");
    for o in &st.outs {
        let frames = o.data.len() / o.channels.max(1) as usize;
        r.push_str(&format!("  {}.wav  {} frames ({:.3} s)\n", o.name, frames, frames as f64 / o.rate as f64));
    }
    r
}
