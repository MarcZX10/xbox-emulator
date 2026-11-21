use crate::xapo_format::*;
use crate::Guid;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

pub const CLSID_LOUDNESS_METER: Guid = Guid(0x54e3_4cc6, 0x934c, 0x4dcc, [0xb5, 0x3d, 0x85, 0xc2, 0x08, 0x86, 0x44, 0xd3]);
pub const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_IXAPO: Guid = Guid(0xa410_b984, 0x9839, 0x4819, [0xa0, 0xbe, 0x28, 0x56, 0xae, 0x6b, 0x3a, 0xdb]);
pub const IID_IXAPO_PARAMETERS: Guid = Guid(0x26d9_5c66, 0x80f2, 0x499a, [0xad, 0x54, 0x5a, 0xe7, 0xf0, 0x1c, 0x6d, 0x98]);
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const XAPO_BUFFER_VALID: u32 = 1;
pub const REGISTRATION_FLAGS: u32 = 0x3f;
pub const PARAMETER_BLOCK_SIZE: usize = 0x30;
pub const RANGE_HIGH_FLOOR: f64 = -80.0;

const STAGE1: [f64; 5] = [1.53512485958697, -2.69169618940638, 1.19839281085285, -1.69065929318241, 0.73248077421585];
const STAGE2: [f64; 5] = [1.0, -2.0, 1.0, -1.99004745483398, 0.99007225036621];
const PAIR_WEIGHTS: [[f64; 2]; 4] = [[1.0, 1.0], [1.0, 0.0], [1.409_999_966_621_398_9, 1.409_999_966_621_398_9], [1.409_999_966_621_398_9, 1.409_999_966_621_398_9]];
const PHASE0: [f32; 12] = [-0.00170898438, 0.0109863281, -0.0196533203, 0.033203125, -0.0594482422, 0.137329102, 0.972167969, -0.102294922, 0.0476074219, -0.0266113281, 0.0148925781, -0.00830078125];
const PHASE1: [f32; 12] = [-0.0291748047, 0.029296875, -0.0517578125, 0.0891113281, -0.166503906, 0.465087891, 0.779785156, -0.200317383, 0.1015625, -0.0582275391, 0.0330810547, -0.0189208984];
const PHASE2: [f32; 12] = [-0.0189208984, 0.0330810547, -0.0582275391, 0.1015625, -0.200317383, 0.779785156, 0.465087891, -0.166503906, 0.0891113281, -0.0517578125, 0.029296875, -0.0291748047];
const PHASE3: [f32; 12] = [-0.00830078125, 0.0148925781, -0.0266113281, 0.0476074219, -0.102294922, 0.972167969, 0.137329102, -0.0594482422, 0.033203125, -0.0196533203, 0.0109863281, 0.00170898438];
const TRUE_PEAK_PHASES: [[f32; 12]; 4] = [PHASE0, PHASE1, PHASE2, PHASE3];
const HISTORY: usize = 11;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct LoudnessParameters {
    pub momentary: f64,
    pub short_term: f64,
    pub integrated: f64,
    pub range_high: f64,
    pub range_low: f64,
    pub true_peak: f32,
    pub reserved: u32,
}

#[repr(C, packed)]
pub struct RegistrationProperties {
    pub clsid: Guid,
    pub friendly_name: [u16; 256],
    pub copyright: [u16; 256],
    pub major: u32,
    pub minor: u32,
    pub flags: u32,
    pub min_input: u32,
    pub max_input: u32,
    pub min_output: u32,
    pub max_output: u32,
}

const fn wide256(s: &str) -> [u16; 256] {
    let b = s.as_bytes();
    let mut out = [0u16; 256];
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i] as u16;
        i += 1;
    }
    out
}

pub static REGISTRATION: RegistrationProperties = RegistrationProperties {
    clsid: CLSID_LOUDNESS_METER,
    friendly_name: wide256("Loudness Meter"),
    copyright: wide256("Copyright (c) Microsoft Corporation"),
    major: 1,
    minor: 0,
    flags: REGISTRATION_FLAGS,
    min_input: 1,
    max_input: 1,
    min_output: 1,
    max_output: 1,
};

#[repr(C, packed)]
pub struct LockParameters {
    pub format: *const u8,
    pub max_frames: u32,
}

#[repr(C)]
pub struct BufferParameters {
    pub buffer: *mut c_void,
    pub flags: u32,
    pub valid_frames: u32,
}

#[derive(Default)]
pub struct Meter {
    pub channels: u32,
    pub momentary_frames: u32,
    pub short_term_frames: u32,
    pub frames_per_ms: u32,
    pub momentary_fill: u32,
    pub short_term_fill: u32,
    pub momentary_start: u32,
    pub short_term_start: u32,
    pub ring: Option<Vec<Vec<f64>>>,
    pub range_low: f64,
    pub range_high: f64,
    pub short_term_sum: f64,
    pub short_term_count: u32,
    pub true_peak: f32,
    pub pairs: u32,
    pub pair_sums: Vec<[f64; 2]>,
    pub stage1: Vec<f64>,
    pub stage2: Vec<f64>,
    pub history: Vec<f32>,
    pub blocks: [LoudnessParameters; 3],
    pub block_index: u32,
    pub producer: Option<usize>,
    pub locked: bool,
}

impl Meter {
    fn set_all(&mut self, f: impl Fn(&mut LoudnessParameters)) {
        for b in self.blocks.iter_mut() {
            f(b);
        }
    }

    fn allocate(&mut self, pairs: u32) {
        let c = self.channels as usize;
        self.ring = Some(vec![vec![0.0; c]; self.short_term_frames as usize]);
        self.stage1 = vec![0.0; c * 2];
        self.stage2 = vec![0.0; c * 2];
        self.history = vec![0.0; c * HISTORY];
        self.pairs = pairs;
        self.pair_sums = vec![[0.0; 2]; pairs as usize];
    }

    fn reset_counters(&mut self) {
        self.momentary_fill = 0;
        self.short_term_fill = 0;
        self.momentary_start = 0;
        self.short_term_start = 0;
        self.range_low = 0.0;
        self.range_high = RANGE_HIGH_FLOOR;
        self.true_peak = 0.0;
    }

    fn free_buffers(&mut self) {
        self.ring = None;
        self.stage1 = Vec::new();
        self.stage2 = Vec::new();
        self.history = Vec::new();
        self.pair_sums = Vec::new();
    }

    pub fn lock(&mut self, channels: u32, rate: u32) {
        self.channels = channels;
        self.momentary_frames = rate * 400 / 1000;
        self.short_term_frames = rate * 3000 / 1000;
        self.frames_per_ms = rate / 1000;
        if self.ring.is_none() {
            self.allocate(((channels / 2) as f64 + 0.5) as u32);
        }
        self.reset_counters();
        self.locked = true;
    }

    pub fn unlock(&mut self) {
        self.free_buffers();
        self.locked = false;
    }

    pub fn reset(&mut self) {
        let integrated = self.short_term_sum / self.short_term_count as f64;
        self.set_all(|b| b.integrated = integrated);
        self.short_term_sum = 0.0;
        self.short_term_count = 0;
        if self.ring.is_some() {
            self.free_buffers();
            self.allocate((self.channels as f32 / 2.0 + 0.5) as u32);
            self.reset_counters();
        }
    }

    fn true_peak_of(&mut self, x: f32, ch: usize) {
        let c = self.channels as usize;
        let mut h = [0.0f32; HISTORY];
        for (k, v) in h.iter_mut().enumerate() {
            *v = self.history[k * c + ch];
        }
        let mut s = x;
        for f in TRUE_PEAK_PHASES.iter() {
            s *= f[0];
            for k in 1..12 {
                s += h[k - 1] * f[k];
            }
        }
        if s.abs() > self.true_peak.abs() {
            self.true_peak = s;
            self.set_all(|b| b.true_peak = s);
        }
    }

    fn push_history(&mut self, x: f32, ch: usize) {
        let c = self.channels as usize;
        for k in (1..HISTORY).rev() {
            self.history[k * c + ch] = self.history[(k - 1) * c + ch];
        }
        self.history[ch] = x;
    }

    fn filter(&mut self, src: *const f32, frames: u32) {
        let c = self.channels as usize;
        let mut base = self.short_term_start + self.short_term_fill;
        let per_ms = self.frames_per_ms as f64;
        let len = self.short_term_frames;
        for i in 0..frames {
            if base + i >= len {
                base = 0;
            }
            let mut slot = (base + i) as usize;
            if slot >= len as usize {
                slot %= len.max(1) as usize;
            }
            for ch in 0..c {
                if ch == 3 {
                    self.ring.as_mut().unwrap()[slot][ch] = 0.0;
                    continue;
                }
                let xf = unsafe { *src.add(i as usize * c + ch) };
                let x = xf as f64;
                let x1 = self.history[ch] as f64;
                let x2 = self.history[c + ch] as f64;
                let y1 = self.stage1[ch];
                let y2 = self.stage1[c + ch];
                let z1 = self.stage2[ch];
                let z2 = self.stage2[c + ch];
                self.true_peak_of(xf, ch);
                self.push_history(xf, ch);
                let y = STAGE1[0] * x + STAGE1[1] * x1 + STAGE1[2] * x2 - STAGE1[3] * y1 - STAGE1[4] * y2;
                self.stage1[c + ch] = self.stage1[ch];
                self.stage1[ch] = y;
                let z = STAGE2[0] * y + STAGE2[1] * y1 + STAGE2[2] * y2 - STAGE2[3] * z1 - STAGE2[4] * z2;
                self.stage2[c + ch] = self.stage2[ch];
                self.stage2[ch] = z;
                self.ring.as_mut().unwrap()[slot][ch] = z * z / per_ms;
            }
        }
    }

    pub fn block_loudness(&mut self, start: u32, frames: u32) -> f64 {
        let div = frames as f64 / self.frames_per_ms as f64;
        for s in self.pair_sums.iter_mut() {
            *s = [0.0; 2];
        }
        let mut acc = [0.0f64; 2];
        let mut off = start;
        let len = self.short_term_frames;
        let odd_channels = self.channels % 2 != 0;
        for i in 0..self.pairs as usize {
            let odd = odd_channels && i == self.pairs as usize - 1;
            let mut sum = [0.0f64; 2];
            for j in 0..frames {
                if off + j >= len {
                    off = 0;
                }
                let e = &self.ring.as_ref().unwrap()[(off + j) as usize];
                let a = e.get(2 * i).copied().unwrap_or(0.0);
                let b = if odd { 0.0 } else { e.get(2 * i + 1).copied().unwrap_or(0.0) };
                sum = [sum[0] + a, sum[1] + b];
            }
            let w = PAIR_WEIGHTS[i.min(PAIR_WEIGHTS.len() - 1)];
            let v = [sum[0] / div * w[0], sum[1] / div * w[1]];
            self.pair_sums[i] = v;
            acc = [acc[0] + v[0], acc[1] + v[1]];
        }
        let l = (acc[0] + acc[1]).log10() * 10.0 + -0.691;
        if self.range_low > l {
            self.range_low = l;
            self.set_all(|b| b.range_low = l);
        }
        if l > self.range_high {
            self.range_high = l;
            self.set_all(|b| b.range_high = l);
        }
        l
    }

    pub fn process(&mut self, src: *const f32, frames: u32) {
        let mut rem = 0;
        if self.short_term_fill + frames > self.short_term_frames {
            self.filter(src, self.short_term_frames - self.short_term_fill);
            self.short_term_fill = self.short_term_frames;
            rem = frames;
        } else {
            self.filter(src, frames);
            self.short_term_fill += frames;
            self.momentary_fill += frames;
        }
        let wrap = self.channels.wrapping_mul(self.short_term_frames);
        if self.momentary_fill >= self.momentary_frames {
            let m = self.block_loudness(self.momentary_start, self.momentary_frames);
            self.set_all(|b| b.momentary = m);
            let hop = (self.momentary_frames as f64 * 0.25) as u32;
            self.momentary_fill = self.momentary_fill.wrapping_sub(hop);
            self.momentary_start = self.momentary_start.wrapping_add(hop);
            if self.momentary_start > wrap {
                self.momentary_start -= wrap;
            }
        }
        if self.short_term_fill >= self.short_term_frames {
            let s = self.block_loudness(self.short_term_start, self.short_term_frames);
            self.set_all(|b| b.short_term = s);
            self.short_term_sum += s;
            self.short_term_count += 1;
            let hop = (self.short_term_frames as f64 * 0.25) as u32;
            self.short_term_fill = self.short_term_fill.wrapping_sub(hop);
            self.short_term_start = self.short_term_start.wrapping_add(hop);
            if self.short_term_start > wrap {
                self.short_term_start -= wrap;
            }
        }
        if rem > 0 {
            let src = unsafe { src.add((frames - rem) as usize) };
            self.filter(src, rem);
            self.momentary_fill = self.momentary_fill.wrapping_add(rem);
            self.short_term_fill = rem;
        }
    }

    pub fn get_parameters(&self) -> LoudnessParameters {
        self.blocks[(self.block_index ^ 1) as usize]
    }

    pub fn set_parameters(&mut self, p: &[u8]) {
        let producer = *self.producer.get_or_insert(0);
        let n = p.len().min(PARAMETER_BLOCK_SIZE);
        for target in [producer, 2] {
            let dst = &mut self.blocks[target] as *mut LoudnessParameters as *mut u8;
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), dst, n) };
        }
    }
}

#[repr(C)]
pub struct XapoVtbl {
    pub query_interface: unsafe extern "system" fn(*mut LoudnessXapo, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut LoudnessXapo) -> u32,
    pub release: unsafe extern "system" fn(*mut LoudnessXapo) -> u32,
    pub get_registration_properties: unsafe extern "system" fn(*mut LoudnessXapo, *mut *mut RegistrationProperties) -> i32,
    pub is_input_format_supported: unsafe extern "system" fn(*mut LoudnessXapo, *const u8, *const u8, *mut *mut u8) -> i32,
    pub is_output_format_supported: unsafe extern "system" fn(*mut LoudnessXapo, *const u8, *const u8, *mut *mut u8) -> i32,
    pub initialize: unsafe extern "system" fn(*mut LoudnessXapo, *const c_void, u32) -> i32,
    pub reset: unsafe extern "system" fn(*mut LoudnessXapo),
    pub lock_for_process: unsafe extern "system" fn(*mut LoudnessXapo, u32, *const LockParameters, u32, *const LockParameters) -> i32,
    pub unlock_for_process: unsafe extern "system" fn(*mut LoudnessXapo),
    pub process: unsafe extern "system" fn(*mut LoudnessXapo, u32, *const BufferParameters, u32, *mut BufferParameters, i32),
    pub calc_input_frames: unsafe extern "system" fn(*mut LoudnessXapo, u32) -> u32,
    pub calc_output_frames: unsafe extern "system" fn(*mut LoudnessXapo, u32) -> u32,
}

#[repr(C)]
pub struct XapoParametersVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub set_parameters: unsafe extern "system" fn(*mut c_void, *const c_void, u32),
    pub get_parameters: unsafe extern "system" fn(*mut c_void, *mut c_void, u32),
}

#[repr(C)]
pub struct LoudnessXapo {
    pub vtbl: *const XapoVtbl,
    pub params_vtbl: *const XapoParametersVtbl,
    refs: AtomicU32,
    pub meter: Mutex<Meter>,
}

static XAPO_VTBL: XapoVtbl = XapoVtbl {
    query_interface,
    add_ref,
    release,
    get_registration_properties,
    is_input_format_supported,
    is_output_format_supported,
    initialize,
    reset,
    lock_for_process,
    unlock_for_process,
    process,
    calc_input_frames,
    calc_output_frames,
};

static PARAMS_VTBL: XapoParametersVtbl = XapoParametersVtbl {
    query_interface: params_query_interface,
    add_ref: params_add_ref,
    release: params_release,
    set_parameters,
    get_parameters,
};

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemAlloc(size: usize) -> *mut c_void;
    fn CoTaskMemFree(p: *mut c_void);
}

fn meter(x: &LoudnessXapo) -> std::sync::MutexGuard<'_, Meter> {
    x.meter.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn from_params(p: *mut c_void) -> *mut LoudnessXapo {
    (p as *mut u8).sub(core::mem::offset_of!(LoudnessXapo, params_vtbl)) as *mut LoudnessXapo
}

pub fn create() -> *mut LoudnessXapo {
    Box::into_raw(Box::new(LoudnessXapo {
        vtbl: &XAPO_VTBL,
        params_vtbl: &PARAMS_VTBL,
        refs: AtomicU32::new(1),
        meter: Mutex::new(Meter::default()),
    }))
}

unsafe extern "system" fn query_interface(this: *mut LoudnessXapo, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    if *iid == IID_IXAPO_PARAMETERS {
        *out = if this.is_null() { core::ptr::null_mut() } else { core::ptr::addr_of_mut!((*this).params_vtbl) as *mut c_void };
        add_ref(this);
        return 0;
    }
    if *iid == IID_IXAPO || *iid == IID_IUNKNOWN {
        *out = this as *mut c_void;
        add_ref(this);
        return 0;
    }
    *out = core::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn add_ref(this: *mut LoudnessXapo) -> u32 {
    (*this).refs.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn release(this: *mut LoudnessXapo) -> u32 {
    let n = (*this).refs.fetch_sub(1, Ordering::SeqCst) - 1;
    if n == 0 {
        drop(Box::from_raw(this));
    }
    n
}

unsafe extern "system" fn get_registration_properties(_this: *mut LoudnessXapo, out: *mut *mut RegistrationProperties) -> i32 {
    let size = core::mem::size_of::<RegistrationProperties>();
    let p = CoTaskMemAlloc(size) as *mut RegistrationProperties;
    if p.is_null() {
        return E_OUTOFMEMORY;
    }
    core::ptr::copy_nonoverlapping(&REGISTRATION as *const RegistrationProperties as *const u8, p as *mut u8, size);
    *out = p;
    0
}

unsafe fn format_supported(other: *const u8, requested: *const u8, supported: *mut *mut u8) -> i32 {
    if validate_float_format(other as *mut u8, false) < 0 || !wave_format_valid(requested) {
        return E_INVALIDARG;
    }
    let overwrite = !supported.is_null();
    let fmt = if overwrite {
        let c = clone_as_extensible(requested);
        if c.is_null() {
            return E_OUTOFMEMORY;
        }
        c
    } else {
        requested as *mut u8
    };
    let mut hr = validate_float_format(fmt, overwrite);
    if hr >= 0 {
        hr = match_formats(REGISTRATION_FLAGS, other, fmt, overwrite);
    }
    if overwrite {
        if hr == XAPO_E_FORMAT_UNSUPPORTED {
            *supported = fmt;
        } else {
            CoTaskMemFree(fmt as *mut c_void);
        }
    }
    hr
}

unsafe extern "system" fn is_input_format_supported(_this: *mut LoudnessXapo, output: *const u8, requested: *const u8, supported: *mut *mut u8) -> i32 {
    format_supported(output, requested, supported)
}

unsafe extern "system" fn is_output_format_supported(_this: *mut LoudnessXapo, input: *const u8, requested: *const u8, supported: *mut *mut u8) -> i32 {
    format_supported(input, requested, supported)
}

unsafe extern "system" fn initialize(_this: *mut LoudnessXapo, _data: *const c_void, _size: u32) -> i32 {
    0
}

unsafe extern "system" fn reset(this: *mut LoudnessXapo) {
    meter(&*this).reset();
}

unsafe extern "system" fn lock_for_process(this: *mut LoudnessXapo, _in_count: u32, input: *const LockParameters, _out_count: u32, _output: *const LockParameters) -> i32 {
    let f = core::ptr::read_unaligned(core::ptr::addr_of!((*input).format));
    let channels = core::ptr::read_unaligned(f.add(2) as *const u16) as u32;
    let rate = core::ptr::read_unaligned(f.add(4) as *const u32);
    meter(&*this).lock(channels, rate);
    0
}

unsafe extern "system" fn unlock_for_process(this: *mut LoudnessXapo) {
    meter(&*this).unlock();
}

unsafe extern "system" fn process(this: *mut LoudnessXapo, _in_count: u32, input: *const BufferParameters, _out_count: u32, output: *mut BufferParameters, enabled: i32) {
    let i = &*input;
    let o = &mut *output;
    if i.flags == XAPO_BUFFER_VALID {
        let frames = i.valid_frames;
        if o.buffer != i.buffer {
            core::ptr::copy(i.buffer as *const u8, o.buffer as *mut u8, frames as usize * 4);
        }
        if enabled != 0 {
            meter(&*this).process(i.buffer as *const f32, frames);
        }
    }
    o.valid_frames = i.valid_frames;
    o.flags = i.flags;
}

unsafe extern "system" fn calc_input_frames(_this: *mut LoudnessXapo, frames: u32) -> u32 {
    frames
}

unsafe extern "system" fn calc_output_frames(_this: *mut LoudnessXapo, frames: u32) -> u32 {
    frames
}

unsafe extern "system" fn params_query_interface(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    query_interface(from_params(this), iid, out)
}

unsafe extern "system" fn params_add_ref(this: *mut c_void) -> u32 {
    add_ref(from_params(this))
}

unsafe extern "system" fn params_release(this: *mut c_void) -> u32 {
    release(from_params(this))
}

unsafe extern "system" fn set_parameters(this: *mut c_void, p: *const c_void, size: u32) {
    let x = &*from_params(this);
    let bytes = core::slice::from_raw_parts(p as *const u8, size as usize);
    meter(x).set_parameters(bytes);
}

unsafe extern "system" fn get_parameters(this: *mut c_void, p: *mut c_void, size: u32) {
    let x = &*from_params(this);
    let v = meter(x).get_parameters();
    let n = (size as usize).min(PARAMETER_BLOCK_SIZE);
    core::ptr::copy_nonoverlapping(&v as *const LoudnessParameters as *const u8, p as *mut u8, n);
}

#[no_mangle]
pub unsafe extern "system" fn PlatformSpecificXAPOFXFactory(clsid: *const Guid, out: *mut *mut c_void, _init: *const c_void, _init_bytes: u32) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if clsid.is_null() || *clsid != CLSID_LOUDNESS_METER {
        return E_INVALIDARG;
    }
    *out = create() as *mut c_void;
    0
}
