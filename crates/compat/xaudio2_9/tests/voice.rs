mod common;

use common::*;
use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use XAudio2_9::ffi::*;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}


type HeapFn = unsafe extern "system" fn(u32, u32) -> i32;
type AllocFn = unsafe extern "system" fn(*mut *mut c_void, *mut u32, u32, u32, u32) -> i32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn target_file(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    for dir in exe.ancestors() {
        let p = dir.join(name);
        if p.is_file() {
            return p;
        }
        let p = dir.join("debug").join(name);
        if p.is_file() {
            return p;
        }
    }
    panic!("{name} not found above {}; build it with cargo build -p acphal -p xaudio2_9 into the same target dir", exe.display());
}

struct Apu {
    alloc: AllocFn,
}

fn apu() -> &'static Apu {
    static A: OnceLock<usize> = OnceLock::new();
    let f = *A.get_or_init(|| unsafe {
        let p = target_file("acphal.dll");
        let m = LoadLibraryW(wide(p.to_str().unwrap()).as_ptr());
        assert!(!m.is_null(), "LoadLibrary {}", p.display());
        let heap: HeapFn = core::mem::transmute(GetProcAddress(m, b"ApuCreateHeap\0".as_ptr()));
        let alloc = GetProcAddress(m, b"ApuAlloc\0".as_ptr());
        assert!(!alloc.is_null());
        assert!(heap(0x40_0000, 0x1_0000) >= 0);
        alloc as usize
    });
    static S: OnceLock<Apu> = OnceLock::new();
    S.get_or_init(|| Apu { alloc: unsafe { core::mem::transmute(f) } })
}

fn apu_copy(data: &[u8]) -> *mut u8 {
    let a = apu();
    let mut p: *mut c_void = core::ptr::null_mut();
    let mut addr = 0u32;
    let hr = unsafe { (a.alloc)(&mut p, &mut addr, data.len() as u32, XMA_BYTES_PER_PACKET, 1) };
    assert!(hr >= 0 && !p.is_null(), "ApuAlloc {hr:#x}");
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), p as *mut u8, data.len()) };
    p as *mut u8
}

unsafe fn call<F: Copy>(obj: *mut c_void, i: usize) -> F {
    let f = slot(obj, i);
    core::mem::transmute_copy(&f)
}

#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Start(usize),
    End(usize),
    Loop(usize),
    StreamEnd,
    Error(usize, i32),
}

#[repr(C)]
struct Cb {
    vtbl: *const Fp,
    ev: Mutex<Vec<Ev>>,
}

unsafe fn ev(this: *mut c_void, e: Ev) {
    (*(this as *mut Cb)).ev.lock().unwrap().push(e);
}
unsafe extern "system" fn pass_start(_: *mut c_void, _: u32) {}
unsafe extern "system" fn pass_end(_: *mut c_void) {}
unsafe extern "system" fn stream_end(t: *mut c_void) {
    ev(t, Ev::StreamEnd)
}
unsafe extern "system" fn buf_start(t: *mut c_void, c: *mut c_void) {
    ev(t, Ev::Start(c as usize))
}
unsafe extern "system" fn buf_end(t: *mut c_void, c: *mut c_void) {
    ev(t, Ev::End(c as usize))
}
unsafe extern "system" fn loop_end(t: *mut c_void, c: *mut c_void) {
    ev(t, Ev::Loop(c as usize))
}
unsafe extern "system" fn voice_error(t: *mut c_void, c: *mut c_void, hr: i32) {
    ev(t, Ev::Error(c as usize, hr))
}

struct Table([Fp; 7]);
unsafe impl Sync for Table {}
static CB_VTBL: Table = Table([pass_start as Fp, pass_end as Fp, stream_end as Fp, buf_start as Fp, buf_end as Fp, loop_end as Fp, voice_error as Fp]);

fn new_cb() -> Box<Cb> {
    Box::new(Cb { vtbl: CB_VTBL.0.as_ptr(), ev: Mutex::new(Vec::new()) })
}

struct Rig {
    engine: *mut c_void,
    master: *mut c_void,
}

impl Rig {
    fn new(flags: u32) -> Rig {
        let mut engine = core::ptr::null_mut();
        let hr = unsafe { XAudio2_9::XAudio2Create(&mut engine, flags, 0) };
        assert!(hr >= 0, "XAudio2Create {hr:#x}");
        let mut master = core::ptr::null_mut();
        let hr = unsafe {
            let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, *const u16, *const c_void, i32) -> HRESULT = call(engine, xa::CREATE_MASTERING_VOICE);
            f(engine, &mut master, 0, 0, 0, core::ptr::null(), core::ptr::null(), 6)
        };
        assert!(hr >= 0, "no audio endpoint for a muted mastering voice: CreateMasteringVoice {hr:#x}");
        unsafe {
            let sv: unsafe extern "system" fn(*mut c_void, f32, u32) -> HRESULT = call(master, sv::SET_VOLUME);
            assert!(sv(master, 0.0, 0) >= 0);
            let gv: unsafe extern "system" fn(*mut c_void, *mut f32) = call(master, sv::GET_VOLUME);
            let mut v = 1.0f32;
            gv(master, &mut v);
            assert_eq!(v, 0.0);
            let st: unsafe extern "system" fn(*mut c_void) -> HRESULT = call(engine, xa::START_ENGINE);
            assert!(st(engine) >= 0);
        }
        Rig { engine, master }
    }

    fn source(&self, fmt: &XMA2WAVEFORMATEX, flags: u32, cb: *mut Cb) -> Result<*mut c_void, HRESULT> {
        let mut v = core::ptr::null_mut();
        let hr = unsafe {
            let f: XAudio2_9::engine::CreateSourceFn = call(self.engine, xa::CREATE_SOURCE_VOICE);
            f(self.engine, &mut v, fmt as *const _ as *const WAVEFORMATEX, flags, 2.0, cb as *mut c_void, core::ptr::null(), core::ptr::null())
        };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(v)
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        unsafe {
            let d: unsafe extern "system" fn(*mut c_void) = call(self.master, sv::DESTROY_VOICE);
            d(self.master);
            let r: unsafe extern "system" fn(*mut c_void) -> u32 = call(self.engine, xa::RELEASE);
            r(self.engine);
        }
    }
}

fn xma_fmt(ch: u16, rate: u32, samples: u32) -> XMA2WAVEFORMATEX {
    XMA2WAVEFORMATEX {
        wfx: WAVEFORMATEX { w_format_tag: WAVE_FORMAT_XMA2, n_channels: ch, n_samples_per_sec: rate, n_avg_bytes_per_sec: rate * ch as u32 * 2, n_block_align: ch * 2, w_bits_per_sample: 16, cb_size: 34 },
        num_streams: (ch + 1) / 2,
        channel_mask: 0,
        samples_encoded: samples,
        bytes_per_block: 0x8000,
        play_begin: 0,
        play_length: 0,
        loop_begin: 0,
        loop_length: 0,
        loop_count: 0,
        encoder_version: 4,
        block_count: 1,
    }
}

unsafe fn submit(v: *mut c_void, b: &XAUDIO2_BUFFER, wma: *const c_void) -> HRESULT {
    let f: unsafe extern "system" fn(*mut c_void, *const XAUDIO2_BUFFER, *const c_void) -> HRESULT = call(v, sv::SUBMIT_SOURCE_BUFFER);
    f(v, b, wma)
}

unsafe fn state(v: *mut c_void) -> XAUDIO2_VOICE_STATE {
    let f: unsafe extern "system" fn(*mut c_void, *mut XAUDIO2_VOICE_STATE, u32) = call(v, sv::GET_STATE);
    let mut s: XAUDIO2_VOICE_STATE = core::mem::zeroed();
    f(v, &mut s, 0);
    s
}

unsafe fn start(v: *mut c_void) {
    let f: unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT = call(v, sv::START);
    assert!(f(v, 0, 0) >= 0);
}

unsafe fn destroy(v: *mut c_void) {
    let f: unsafe extern "system" fn(*mut c_void) = call(v, sv::DESTROY_VOICE);
    f(v);
}

unsafe fn wait_idle(v: *mut c_void) -> XAUDIO2_VOICE_STATE {
    let t = Instant::now();
    loop {
        let s = state(v);
        if s.buffers_queued == 0 {
            return s;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "voice never drained: {} buffers queued", { s.buffers_queued });
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn eos(mut b: XAUDIO2_BUFFER) -> XAUDIO2_BUFFER {
    b.flags = XAUDIO2_END_OF_STREAM;
    b
}

unsafe fn wait_for(cb: *mut Cb, e: Ev) {
    let t = Instant::now();
    while !(*cb).ev.lock().unwrap().contains(&e) {
        assert!(t.elapsed() < Duration::from_secs(10), "callback {e:?} never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn buffer(p: *const u8, bytes: usize, ctx: usize) -> XAUDIO2_BUFFER {
    XAUDIO2_BUFFER { flags: 0, audio_bytes: bytes as u32, p_audio_data: p, p_context: ctx as *mut c_void, ..Default::default() }
}

#[test]
fn xma_voice_plays_decoded_frames_into_muted_master() {
    let rig = Rig::new(0);
    assert!(XAudio2_9::engine_is_wrapped(rig.engine));
    let cb = Box::into_raw(new_cb());
    let data = silent_stream(2, 2, 8);
    let p = apu_copy(&data);
    let want = expected_frames(2, 8);
    unsafe {
        let v = rig.source(&xma_fmt(2, 48000, want), 0, cb).unwrap();
        assert!(XAudio2_9::is_xma_voice(v));
        assert_eq!(submit(v, &buffer(p, data.len(), 0xC0FFEE), core::ptr::null()), S_OK);
        let s = state(v);
        assert_eq!({ s.buffers_queued }, 1);
        assert_eq!({ s.p_current_buffer_context } as usize, 0xC0FFEE);
        assert_eq!({ s.samples_played }, 0);
        start(v);
        let s = wait_idle(v);
        assert_eq!({ s.samples_played }, want as u64);
        assert_eq!(submit(v, &eos(buffer(p, data.len(), 0xBEEF)), core::ptr::null()), S_OK);
        wait_for(cb, Ev::StreamEnd);
        let s = wait_idle(v);
        assert_eq!({ s.samples_played }, 0);
        let ev = (*cb).ev.lock().unwrap().clone();
        assert_eq!(ev, vec![Ev::Start(0xC0FFEE), Ev::End(0xC0FFEE), Ev::Start(0xBEEF), Ev::End(0xBEEF), Ev::StreamEnd]);
        destroy(v);
        drop(Box::from_raw(cb));
    }
}

#[test]
fn xma_voice_play_region_and_loop_count_drive_sample_count() {
    let rig = Rig::new(0);
    let cb = Box::into_raw(new_cb());
    let data = silent_stream(1, 2, 8);
    let p = apu_copy(&data);
    let decoded = expected_frames(2, 8);
    unsafe {
        let v = rig.source(&xma_fmt(1, 32000, decoded), 0, cb).unwrap();
        let mut b = buffer(p, data.len(), 7);
        b.play_begin = 1024;
        b.play_length = 2048;
        b.loop_begin = 1536;
        b.loop_count = 2;
        assert_eq!(submit(v, &b, core::ptr::null()), S_OK);
        start(v);
        let s = wait_idle(v);
        assert_eq!({ s.samples_played }, 2048 + 2 * (1024 + 2048 - 1536) as u64);
        let ev = (*cb).ev.lock().unwrap().clone();
        assert_eq!(ev, vec![Ev::Start(7), Ev::Loop(7), Ev::Loop(7), Ev::End(7)]);
        destroy(v);
        drop(Box::from_raw(cb));
    }
}

#[test]
fn xma_voice_four_channel_multistream_and_back_to_back_buffers() {
    let rig = Rig::new(0);
    let cb = Box::into_raw(new_cb());
    let a = silent_stream(4, 2, 6);
    let b = silent_stream(4, 3, 4);
    let pa = apu_copy(&a);
    let pb = apu_copy(&b);
    unsafe {
        let mut fmt = xma_fmt(4, 48000, expected_frames(3, 4));
        fmt.channel_mask = 0x33;
        let v = rig.source(&fmt, 0, cb).unwrap();
        assert_eq!(submit(v, &buffer(pa, a.len(), 1), core::ptr::null()), S_OK);
        assert_eq!(submit(v, &buffer(pb, b.len(), 2), core::ptr::null()), S_OK);
        assert_eq!({ state(v).buffers_queued }, 2);
        start(v);
        let s = wait_idle(v);
        assert_eq!({ s.samples_played }, (expected_frames(2, 6) + 3 * 4 * FRAME) as u64);
        let ev = (*cb).ev.lock().unwrap().clone();
        assert_eq!(ev, vec![Ev::Start(1), Ev::End(1), Ev::Start(2), Ev::End(2)]);
        destroy(v);
        drop(Box::from_raw(cb));
    }
}

#[test]
fn xma_voice_rejects_what_the_console_rejects() {
    let rig = Rig::new(0);
    let data = silent_stream(2, 1, 4);
    let p = apu_copy(&data);
    unsafe {
        let fmt = xma_fmt(2, 48000, expected_frames(1, 4));
        assert_eq!(rig.source(&fmt, XAUDIO2_VOICE_NOSHAPE, core::ptr::null_mut()).unwrap_err(), XAUDIO2_E_INVALID_CALL);
        let mut bad = fmt;
        bad.wfx.cb_size = 22;
        assert_eq!(rig.source(&bad, 0, core::ptr::null_mut()).unwrap_err(), XAUDIO2_E_INVALID_CALL);
        let v = rig.source(&fmt, 0, core::ptr::null_mut()).unwrap();
        let wma = [0u32; 4];
        assert_eq!(submit(v, &buffer(p, data.len(), 0), wma.as_ptr() as *const c_void), XAUDIO2_E_INVALID_CALL);
        let heap = data.clone();
        assert_eq!(submit(v, &buffer(heap.as_ptr(), heap.len(), 0), core::ptr::null()), XAUDIO2_E_INVALID_CALL);
        let mut b = buffer(p, data.len(), 0);
        b.play_begin = 64;
        assert_eq!(submit(v, &b, core::ptr::null()), XAUDIO2_E_INVALID_CALL);
        let mut b = buffer(p, data.len() - 1, 0);
        b.play_begin = 0;
        assert_eq!(submit(v, &b, core::ptr::null()), XAUDIO2_E_INVALID_CALL);
        let mut meta = data.clone();
        meta[2] &= !7;
        let pm = apu_copy(&meta);
        assert_eq!(submit(v, &buffer(pm, meta.len(), 0), core::ptr::null()), XAUDIO2_E_INVALID_CALL);
        assert_eq!({ state(v).buffers_queued }, 0);
        destroy(v);
    }
}

#[test]
fn do_not_use_shape_engine_is_the_host_engine_with_console_processor() {
    let rig = Rig::new(XAUDIO2_DO_NOT_USE_SHAPE);
    assert!(!XAudio2_9::engine_is_wrapped(rig.engine));
    unsafe {
        let mut ext: *mut c_void = core::ptr::null_mut();
        let qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT = call(rig.engine, xa::QUERY_INTERFACE);
        assert_eq!(qi(rig.engine, &IID_IXAUDIO2_EXTENSION, &mut ext), S_OK);
        let gp: unsafe extern "system" fn(*mut c_void, *mut u32) = call(ext, ext::GET_PROCESSOR);
        let mut proc_ = 0u32;
        gp(ext, &mut proc_);
        assert_eq!(proc_, XAUDIO2_CONSOLE_DEFAULT_PROCESSOR);
        let r: unsafe extern "system" fn(*mut c_void) -> u32 = call(ext, xa::RELEASE);
        r(ext);
        let fmt = xma_fmt(2, 48000, 4096);
        assert!(rig.source(&fmt, 0, core::ptr::null_mut()).is_err());
    }
}

#[test]
fn shape_engine_exposes_extension_with_console_processor() {
    let rig = Rig::new(0);
    unsafe {
        let mut ext: *mut c_void = core::ptr::null_mut();
        let qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT = call(rig.engine, xa::QUERY_INTERFACE);
        assert_eq!(qi(rig.engine, &IID_IXAUDIO2_EXTENSION, &mut ext), S_OK);
        let gp: unsafe extern "system" fn(*mut c_void, *mut u32) = call(ext, ext::GET_PROCESSOR);
        let mut proc_ = 0u32;
        gp(ext, &mut proc_);
        assert_eq!(proc_, XAUDIO2_CONSOLE_DEFAULT_PROCESSOR);
        let mut back: *mut c_void = core::ptr::null_mut();
        let qi2: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT = call(ext, xa::QUERY_INTERFACE);
        assert_eq!(qi2(ext, &IID_IXAUDIO2, &mut back), S_OK);
        assert_eq!(back, rig.engine);
        let r: unsafe extern "system" fn(*mut c_void) -> u32 = call(back, xa::RELEASE);
        r(back);
        let r: unsafe extern "system" fn(*mut c_void) -> u32 = call(ext, xa::RELEASE);
        r(ext);
    }
}

unsafe fn set_ratio(v: *mut c_void, r: f32) {
    let f: unsafe extern "system" fn(*mut c_void, f32, u32) -> HRESULT = call(v, sv::SET_FREQUENCY_RATIO);
    assert!(f(v, r, 0) >= 0);
}

#[test]
fn xma_voice_plays_real_title_xma2_with_console_sample_numbering() {
    let rig = Rig::new(0);
    for (src, play_len, loop_) in [(MONO, None, false), (MONO, Some(8192u32), true), (QUAD, Some(44032), false), (STEREO, Some(16384), false)] {
        let r = load(src.0, src.1, src.2);
        let cb = Box::into_raw(new_cb());
        let p = apu_copy(&r.data);
        unsafe {
            let v = rig.source(&r.fmt, 0, cb).unwrap();
            set_ratio(v, 2.0);
            let mut b = buffer(p, r.data.len(), 0x51);
            b.play_begin = r.fmt.play_begin;
            b.play_length = play_len.unwrap_or(r.fmt.play_length);
            let mut want = b.play_length as u64;
            if loop_ {
                b.loop_begin = 1024;
                b.loop_length = 2048;
                b.loop_count = 1;
                want += 2048;
            }
            assert_eq!(submit(v, &b, core::ptr::null()), S_OK, "{}", src.0);
            start(v);
            let s = wait_idle(v);
            assert_eq!({ s.samples_played }, want, "{} {}ch", src.0, { r.fmt.wfx.n_channels });
            let ev = (*cb).ev.lock().unwrap().clone();
            let mut expect = vec![Ev::Start(0x51)];
            if loop_ {
                expect.push(Ev::Loop(0x51));
            }
            expect.push(Ev::End(0x51));
            assert_eq!(ev, expect, "{}", src.0);
            destroy(v);
            drop(Box::from_raw(cb));
        }
    }
}
