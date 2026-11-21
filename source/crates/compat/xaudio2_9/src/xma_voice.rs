use crate::engine::{CreateSourceFn, Engine};
use crate::ffi::*;
use crate::xma_stream::{console_loop_fill, map_regions, validate_buffer, validate_format, XmaDecoder};
use core::ffi::c_void;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

const ENDED_KEEP: usize = 256;

struct Slot {
    ctx: usize,
    pcm: Option<Vec<i16>>,
}

#[derive(Default)]
struct Slots {
    next: usize,
    live: HashMap<usize, Slot>,
    ended: VecDeque<usize>,
}

struct Dec {
    dec: XmaDecoder,
    need_reset: bool,
}

#[repr(C)]
pub struct XmaVoice {
    vtbl: *const Fp,
    cb_vtbl: *const Fp,
    inner: *mut c_void,
    engine: *mut Engine,
    game_cb: *mut c_void,
    channels: u32,
    dec: Mutex<Dec>,
    slots: Mutex<Slots>,
}

unsafe impl Send for XmaVoice {}
unsafe impl Sync for XmaVoice {}

fn log(func: &str, detail: &str) {
    compat_common::record_raw("xaudio2_9", func, detail);
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn cb_offset() -> usize {
    core::mem::offset_of!(XmaVoice, cb_vtbl)
}

unsafe fn from_cb(p: *mut c_void) -> *mut XmaVoice {
    (p as *mut u8).sub(cb_offset()) as *mut XmaVoice
}

pub fn is_xma_voice(p: *mut c_void) -> bool {
    !p.is_null() && unsafe { (*(p as *mut XmaVoice)).vtbl } == SOURCE_VTBL.0.as_ptr()
}

pub fn pcm_format(f: &XMA2WAVEFORMATEX) -> WAVEFORMATEXTENSIBLE {
    let ch = f.wfx.n_channels;
    let rate = f.wfx.n_samples_per_sec;
    let mask = f.channel_mask;
    let base = WAVEFORMATEX {
        w_format_tag: if mask != 0 { WAVE_FORMAT_EXTENSIBLE } else { WAVE_FORMAT_PCM },
        n_channels: ch,
        n_samples_per_sec: rate,
        n_avg_bytes_per_sec: rate * ch as u32 * 2,
        n_block_align: ch * 2,
        w_bits_per_sample: 16,
        cb_size: if mask != 0 { 22 } else { 0 },
    };
    WAVEFORMATEXTENSIBLE { format: base, w_valid_bits_per_sample: 16, dw_channel_mask: mask, sub_format: KSDATAFORMAT_SUBTYPE_PCM }
}

pub unsafe fn create(
    e: *mut Engine,
    pp: *mut *mut c_void,
    fmt: *const WAVEFORMATEX,
    flags: u32,
    max_ratio: f32,
    cb: *mut c_void,
    sends: *const c_void,
    effects: *const c_void,
) -> HRESULT {
    let head = core::ptr::read_unaligned(fmt);
    if head.cb_size != 34 {
        log("CreateSourceVoice", &format!("XMA2 invalid source format: cbSize {} (must be 34)", { head.cb_size }));
        return XAUDIO2_E_INVALID_CALL;
    }
    let f = core::ptr::read_unaligned(fmt as *const XMA2WAVEFORMATEX);
    if let Err(why) = validate_format(&f) {
        log("CreateSourceVoice", &format!("XMA2 invalid source format: {why}"));
        return XAUDIO2_E_INVALID_CALL;
    }
    if flags & !XAUDIO2_SOURCE_VALID_FLAGS != 0 {
        log("CreateSourceVoice", &format!("Invalid flags {flags:#x}; only {XAUDIO2_SOURCE_VALID_FLAGS:#x} are valid"));
        return XAUDIO2_E_INVALID_CALL;
    }
    if flags & XAUDIO2_VOICE_NOSHAPE != 0 {
        log("CreateSourceVoice", "SHAPE (and XMA) has been disabled for this voice or engine instance");
        return XAUDIO2_E_INVALID_CALL;
    }
    let ch = f.wfx.n_channels as usize;
    let rate = f.wfx.n_samples_per_sec;
    let dec = match XmaDecoder::new(rate, ch) {
        Ok(d) => d,
        Err(err) => {
            log("CreateSourceVoice", &format!("XMA2 decoder rejected {ch} ch @ {rate} Hz: {err:?}"));
            return XAUDIO2_E_INVALID_CALL;
        }
    };
    let v = Box::into_raw(Box::new(XmaVoice {
        vtbl: SOURCE_VTBL.0.as_ptr(),
        cb_vtbl: CALLBACK_VTBL.0.as_ptr(),
        inner: core::ptr::null_mut(),
        engine: e,
        game_cb: cb,
        channels: ch as u32,
        dec: Mutex::new(Dec { dec, need_reset: false }),
        slots: Mutex::new(Slots::default()),
    }));
    let pcm = pcm_format(&f);
    let inner_engine = (*e).inner;
    let create: CreateSourceFn = core::mem::transmute(slot(inner_engine, xa::CREATE_SOURCE_VOICE));
    let cbp = core::ptr::addr_of_mut!((*v).cb_vtbl) as *mut c_void;
    let mut inner: *mut c_void = core::ptr::null_mut();
    let hr = create(inner_engine, &mut inner, &pcm as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX, flags, max_ratio, cbp, sends, effects);
    if hr < 0 {
        log("CreateSourceVoice", &format!("XMA2 {ch} ch @ {rate} Hz: host PCM voice creation failed hr={:#010x}", hr as u32));
        drop(Box::from_raw(v));
        return hr;
    }
    (*v).inner = inner;
    lock(&(*e).voices).push(v as usize);
    *pp = v as *mut c_void;
    log("CreateSourceVoice", &format!("XMA2 voice {:p}: {ch} ch @ {rate} Hz, streams {}, mask {:#x}, flags {flags:#x}, decoded in software into host PCM voice {:p}", v, { f.num_streams }, { f.channel_mask }, inner));
    S_OK
}

unsafe fn inner(this: *mut c_void) -> *mut c_void {
    (*(this as *mut XmaVoice)).inner
}

unsafe fn free_voice(v: *mut XmaVoice) {
    let inner = (*v).inner;
    if !inner.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) = core::mem::transmute(slot(inner, sv::DESTROY_VOICE));
        f(inner);
    }
    drop(Box::from_raw(v));
}

pub unsafe fn destroy_with_engine(p: *mut c_void) {
    free_voice(p as *mut XmaVoice);
}

unsafe extern "system" fn destroy_voice(this: *mut c_void) {
    let v = this as *mut XmaVoice;
    let e = (*v).engine;
    let mut list = lock(&(*e).voices);
    let pos = list.iter().position(|&x| x == this as usize);
    if let Some(i) = pos {
        list.swap_remove(i);
    }
    drop(list);
    if pos.is_some() {
        free_voice(v);
    }
}

unsafe extern "system" fn submit_source_buffer(this: *mut c_void, pbuf: *const XAUDIO2_BUFFER, wma: *const c_void) -> HRESULT {
    let v = this as *mut XmaVoice;
    if pbuf.is_null() {
        log("SubmitSourceBuffer", "Invalid NULL pointer 'pBuffer'");
        return XAUDIO2_E_INVALID_CALL;
    }
    if !wma.is_null() {
        log("SubmitSourceBuffer", "WMA parameters submitted to non-WMA voice");
        return XAUDIO2_E_INVALID_CALL;
    }
    let mut b = core::ptr::read_unaligned(pbuf);
    let data_ptr = b.p_audio_data;
    let bytes = b.audio_bytes as usize;
    let apu_ok = match crate::apu_address_valid(data_ptr as *const c_void, XMA_BYTES_PER_PACKET) {
        Ok(ok) => ok,
        Err(hr) => return hr,
    };
    let data: &[u8] = if apu_ok && bytes > 0 { core::slice::from_raw_parts(data_ptr, bytes) } else { &[] };
    if let Err(why) = validate_buffer(&b, data, apu_ok) {
        log("SubmitSourceBuffer", why);
        return XAUDIO2_E_INVALID_CALL;
    }
    console_loop_fill(&mut b);
    let ch = (*v).channels as usize;
    let mut pcm = Vec::new();
    {
        let mut d = lock(&(*v).dec);
        if d.need_reset {
            if let Err(err) = d.dec.reset() {
                log("SubmitSourceBuffer", &format!("XMA2 decoder reset failed: {err:?}"));
                return XAUDIO2_E_INVALID_CALL;
            }
            d.need_reset = false;
        }
        d.dec.feed(data, &mut pcm);
        if b.flags & XAUDIO2_END_OF_STREAM != 0 {
            d.need_reset = true;
        }
    }
    let mut frames = (pcm.len() / ch) as u32;
    if frames == 0 {
        log("SubmitSourceBuffer", &format!("XMA2 buffer of {bytes} bytes decoded to 0 samples; submitting one silent sample frame to keep the buffer queue and callbacks in order"));
        pcm = vec![0i16; ch];
        frames = 1;
    }
    let r = map_regions(&b, frames);
    if r.clamped {
        log(
            "SubmitSourceBuffer",
            &format!(
                "XMA2 regions clamped to {frames} decoded samples: play {}+{} -> {}+{}, loop {}+{}x{} -> {}+{}",
                { b.play_begin },
                { b.play_length },
                r.play_begin,
                r.play_length,
                { b.loop_begin },
                { b.loop_length },
                { b.loop_count },
                r.loop_begin,
                r.loop_length
            ),
        );
    }
    let game_ctx = b.p_context as usize;
    let audio = pcm.as_ptr() as *const u8;
    let audio_bytes = (pcm.len() * 2) as u32;
    let key = {
        let mut s = lock(&(*v).slots);
        s.next += 1;
        let key = s.next;
        s.live.insert(key, Slot { ctx: game_ctx, pcm: Some(pcm) });
        key
    };
    let ib = XAUDIO2_BUFFER {
        flags: b.flags,
        audio_bytes,
        p_audio_data: audio,
        play_begin: r.play_begin,
        play_length: r.play_length,
        loop_begin: r.loop_begin,
        loop_length: r.loop_length,
        loop_count: r.loop_count,
        p_context: key as *mut c_void,
    };
    let inner = (*v).inner;
    let f: unsafe extern "system" fn(*mut c_void, *const XAUDIO2_BUFFER, *const c_void) -> HRESULT = core::mem::transmute(slot(inner, sv::SUBMIT_SOURCE_BUFFER));
    let hr = f(inner, &ib, core::ptr::null());
    if hr < 0 {
        lock(&(*v).slots).live.remove(&key);
        log("SubmitSourceBuffer", &format!("host PCM voice rejected decoded XMA2 buffer ({frames} samples) hr={:#010x}", hr as u32));
    }
    hr
}

unsafe fn ctx_of(v: *mut XmaVoice, key: *mut c_void) -> *mut c_void {
    let s = lock(&(*v).slots);
    s.live.get(&(key as usize)).map(|x| x.ctx).unwrap_or(0) as *mut c_void
}

unsafe extern "system" fn flush_source_buffers(this: *mut c_void) -> HRESULT {
    let v = this as *mut XmaVoice;
    let inner = (*v).inner;
    let f: unsafe extern "system" fn(*mut c_void) -> HRESULT = core::mem::transmute(slot(inner, sv::FLUSH_SOURCE_BUFFERS));
    let hr = f(inner);
    if hr >= 0 {
        lock(&(*v).dec).need_reset = true;
    }
    hr
}

unsafe extern "system" fn get_state(this: *mut c_void, out: *mut XAUDIO2_VOICE_STATE, flags: u32) {
    let v = this as *mut XmaVoice;
    let inner = (*v).inner;
    let f: unsafe extern "system" fn(*mut c_void, *mut XAUDIO2_VOICE_STATE, u32) = core::mem::transmute(slot(inner, sv::GET_STATE));
    f(inner, out, flags);
    if out.is_null() {
        return;
    }
    let mut st = core::ptr::read_unaligned(out);
    let key = st.p_current_buffer_context;
    if !key.is_null() {
        st.p_current_buffer_context = ctx_of(v, key);
        core::ptr::write_unaligned(out, st);
    }
}

unsafe extern "system" fn set_source_sample_rate(this: *mut c_void, rate: u32) -> HRESULT {
    let v = this as *mut XmaVoice;
    let inner = (*v).inner;
    let f: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT = core::mem::transmute(slot(inner, sv::SET_SOURCE_SAMPLE_RATE));
    let hr = f(inner, rate);
    if hr >= 0 {
        let ch = (*v).channels as usize;
        match XmaDecoder::new(rate, ch) {
            Ok(d) => {
                let mut g = lock(&(*v).dec);
                g.dec = d;
                g.need_reset = false;
            }
            Err(err) => {
                log("SetSourceSampleRate", &format!("XMA2 decoder rejected {rate} Hz: {err:?}"));
                return XAUDIO2_E_INVALID_CALL;
            }
        }
    }
    hr
}

macro_rules! fwd {
    ($name:ident, $idx:expr, $ret:ty, ($($a:ident : $t:ty),*)) => {
        unsafe extern "system" fn $name(this: *mut c_void $(, $a: $t)*) -> $ret {
            let i = inner(this);
            let f: unsafe extern "system" fn(*mut c_void $(, $t)*) -> $ret = core::mem::transmute(slot(i, $idx));
            f(i $(, $a)*)
        }
    };
}

fwd!(get_voice_details, sv::GET_VOICE_DETAILS, (), (d: *mut XAUDIO2_VOICE_DETAILS));
fwd!(set_output_voices, sv::SET_OUTPUT_VOICES, HRESULT, (s: *const c_void));
fwd!(set_effect_chain, sv::SET_EFFECT_CHAIN, HRESULT, (c: *const c_void));
fwd!(enable_effect, sv::ENABLE_EFFECT, HRESULT, (i: u32, o: u32));
fwd!(disable_effect, sv::DISABLE_EFFECT, HRESULT, (i: u32, o: u32));
fwd!(get_effect_state, sv::GET_EFFECT_STATE, (), (i: u32, e: *mut i32));
fwd!(set_effect_parameters, sv::SET_EFFECT_PARAMETERS, HRESULT, (i: u32, p: *const c_void, n: u32, o: u32));
fwd!(get_effect_parameters, sv::GET_EFFECT_PARAMETERS, HRESULT, (i: u32, p: *mut c_void, n: u32));
fwd!(set_filter_parameters, sv::SET_FILTER_PARAMETERS, HRESULT, (p: *const c_void, o: u32));
fwd!(get_filter_parameters, sv::GET_FILTER_PARAMETERS, (), (p: *mut c_void));
fwd!(set_output_filter_parameters, sv::SET_OUTPUT_FILTER_PARAMETERS, HRESULT, (d: *mut c_void, p: *const c_void, o: u32));
fwd!(get_output_filter_parameters, sv::GET_OUTPUT_FILTER_PARAMETERS, (), (d: *mut c_void, p: *mut c_void));
fwd!(set_volume, sv::SET_VOLUME, HRESULT, (v: f32, o: u32));
fwd!(get_volume, sv::GET_VOLUME, (), (v: *mut f32));
fwd!(set_channel_volumes, sv::SET_CHANNEL_VOLUMES, HRESULT, (n: u32, v: *const f32, o: u32));
fwd!(get_channel_volumes, sv::GET_CHANNEL_VOLUMES, (), (n: u32, v: *mut f32));
fwd!(set_output_matrix, sv::SET_OUTPUT_MATRIX, HRESULT, (d: *mut c_void, s: u32, t: u32, m: *const f32, o: u32));
fwd!(get_output_matrix, sv::GET_OUTPUT_MATRIX, (), (d: *mut c_void, s: u32, t: u32, m: *mut f32));
fwd!(start, sv::START, HRESULT, (f: u32, o: u32));
fwd!(stop, sv::STOP, HRESULT, (f: u32, o: u32));
fwd!(discontinuity, sv::DISCONTINUITY, HRESULT, ());
fwd!(exit_loop, sv::EXIT_LOOP, HRESULT, (o: u32));
fwd!(set_frequency_ratio, sv::SET_FREQUENCY_RATIO, HRESULT, (r: f32, o: u32));
fwd!(get_frequency_ratio, sv::GET_FREQUENCY_RATIO, (), (r: *mut f32));

unsafe fn game_slot(v: *mut XmaVoice, i: usize) -> Option<Fp> {
    let g = (*v).game_cb;
    if g.is_null() {
        None
    } else {
        Some(slot(g, i))
    }
}

unsafe extern "system" fn cb_pass_start(this: *mut c_void, bytes: u32) {
    let v = from_cb(this);
    if let Some(f) = game_slot(v, cb::ON_VOICE_PROCESSING_PASS_START) {
        let f: unsafe extern "system" fn(*mut c_void, u32) = core::mem::transmute(f);
        f((*v).game_cb, bytes);
    }
}

unsafe extern "system" fn cb_pass_end(this: *mut c_void) {
    let v = from_cb(this);
    if let Some(f) = game_slot(v, cb::ON_VOICE_PROCESSING_PASS_END) {
        let f: unsafe extern "system" fn(*mut c_void) = core::mem::transmute(f);
        f((*v).game_cb);
    }
}

unsafe extern "system" fn cb_stream_end(this: *mut c_void) {
    let v = from_cb(this);
    if let Some(f) = game_slot(v, cb::ON_STREAM_END) {
        let f: unsafe extern "system" fn(*mut c_void) = core::mem::transmute(f);
        f((*v).game_cb);
    }
}

unsafe extern "system" fn cb_buffer_start(this: *mut c_void, key: *mut c_void) {
    let v = from_cb(this);
    if let Some(f) = game_slot(v, cb::ON_BUFFER_START) {
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(f);
        f((*v).game_cb, ctx_of(v, key));
    }
}

unsafe extern "system" fn cb_buffer_end(this: *mut c_void, key: *mut c_void) {
    let v = from_cb(this);
    let ctx = ctx_of(v, key);
    if let Some(f) = game_slot(v, cb::ON_BUFFER_END) {
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(f);
        f((*v).game_cb, ctx);
    }
    let mut s = lock(&(*v).slots);
    if let Some(sl) = s.live.get_mut(&(key as usize)) {
        sl.pcm = None;
    }
    s.ended.push_back(key as usize);
    while s.ended.len() > ENDED_KEEP {
        if let Some(old) = s.ended.pop_front() {
            s.live.remove(&old);
        }
    }
}

unsafe extern "system" fn cb_loop_end(this: *mut c_void, key: *mut c_void) {
    let v = from_cb(this);
    if let Some(f) = game_slot(v, cb::ON_LOOP_END) {
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(f);
        f((*v).game_cb, ctx_of(v, key));
    }
}

unsafe extern "system" fn cb_voice_error(this: *mut c_void, key: *mut c_void, hr: HRESULT) {
    let v = from_cb(this);
    log("OnVoiceError", &format!("XMA2 voice {:p} host error hr={:#010x}", v, hr as u32));
    if let Some(f) = game_slot(v, cb::ON_VOICE_ERROR) {
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void, HRESULT) = core::mem::transmute(f);
        f((*v).game_cb, ctx_of(v, key), hr);
    }
}

struct VTable<const N: usize>([Fp; N]);
unsafe impl<const N: usize> Sync for VTable<N> {}

static SOURCE_VTBL: VTable<{ sv::COUNT }> = VTable([
    get_voice_details as Fp,
    set_output_voices as Fp,
    set_effect_chain as Fp,
    enable_effect as Fp,
    disable_effect as Fp,
    get_effect_state as Fp,
    set_effect_parameters as Fp,
    get_effect_parameters as Fp,
    set_filter_parameters as Fp,
    get_filter_parameters as Fp,
    set_output_filter_parameters as Fp,
    get_output_filter_parameters as Fp,
    set_volume as Fp,
    get_volume as Fp,
    set_channel_volumes as Fp,
    get_channel_volumes as Fp,
    set_output_matrix as Fp,
    get_output_matrix as Fp,
    destroy_voice as Fp,
    start as Fp,
    stop as Fp,
    submit_source_buffer as Fp,
    flush_source_buffers as Fp,
    discontinuity as Fp,
    exit_loop as Fp,
    get_state as Fp,
    set_frequency_ratio as Fp,
    get_frequency_ratio as Fp,
    set_source_sample_rate as Fp,
]);

static CALLBACK_VTBL: VTable<{ cb::COUNT }> = VTable([
    cb_pass_start as Fp,
    cb_pass_end as Fp,
    cb_stream_end as Fp,
    cb_buffer_start as Fp,
    cb_buffer_end as Fp,
    cb_loop_end as Fp,
    cb_voice_error as Fp,
]);
