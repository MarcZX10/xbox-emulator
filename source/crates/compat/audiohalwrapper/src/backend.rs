use core::ffi::c_void;
use std::sync::{Arc, OnceLock, RwLock};

pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const E_MOD_NOT_FOUND: i32 = 0x8007_007Eu32 as i32;

pub trait OpusDecoderCodec: Send {
    fn decode(&mut self, packet: &[u8], pcm: &mut [i16]) -> Result<usize, i32>;
}

pub trait OpusEncoderCodec: Send {
    fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize, i32>;
}

pub trait SilkDecoderCodec: Send {
    fn decode(&mut self, control: &mut [i32; 6], all_frames: bool, packet: &[u8], pcm: &mut [i16]) -> Result<usize, i32>;
}

pub trait SilkEncoderCodec: Send {
    fn encode(&mut self, control: &mut [i32; 8], pcm: &[i16], out: &mut [u8]) -> Result<usize, i32>;
}

pub trait CodecProvider: Send + Sync {
    fn opus_available(&self) -> bool;
    fn silk_available(&self) -> bool;
    fn opus_decoder(&self, rate: u32, channels: u32) -> Result<Box<dyn OpusDecoderCodec>, i32>;
    fn opus_encoder(&self, rate: u32, channels: u32, bitrate: u32, flags: u32) -> Result<Box<dyn OpusEncoderCodec>, i32>;
    fn silk_decoder(&self) -> Result<Box<dyn SilkDecoderCodec>, i32>;
    fn silk_encoder(&self) -> Result<Box<dyn SilkEncoderCodec>, i32>;
}

static OVERRIDE: RwLock<Option<Arc<dyn CodecProvider>>> = RwLock::new(None);
static DEFAULT: OnceLock<Arc<dyn CodecProvider>> = OnceLock::new();

pub fn set_codec_provider(p: Option<Arc<dyn CodecProvider>>) {
    *OVERRIDE.write().unwrap_or_else(|e| e.into_inner()) = p;
}

pub fn provider() -> Arc<dyn CodecProvider> {
    if let Some(p) = OVERRIDE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return p.clone();
    }
    DEFAULT.get_or_init(|| Arc::new(DynProvider::load())).clone()
}

type HMODULE = *mut c_void;

extern "system" {
    fn LoadLibraryW(name: *const u16) -> HMODULE;
    fn GetProcAddress(module: HMODULE, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut HMODULE) -> i32;
    fn GetModuleFileNameW(module: HMODULE, buf: *mut u16, len: u32) -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

fn own_dir() -> Option<String> {
    let mut h: HMODULE = core::ptr::null_mut();
    let ok = unsafe { GetModuleHandleExW(6, own_dir as *const c_void as *const u16, &mut h) };
    if ok == 0 {
        return None;
    }
    let mut buf = vec![0u16; 32768];
    let n = unsafe { GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..n]);
    let cut = path.rfind(['\\', '/']).map(|i| i + 1).unwrap_or(0);
    Some(path[..cut].to_string())
}

fn try_load(candidates: &[String]) -> Option<HMODULE> {
    for c in candidates {
        let w = wide(c);
        let h = unsafe { LoadLibraryW(w.as_ptr()) };
        if !h.is_null() {
            return Some(h);
        }
    }
    None
}

fn candidates(env: &str, names: &[&str]) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var(env) {
        if !p.is_empty() {
            v.push(p);
        }
    }
    let dir = own_dir();
    for n in names {
        if let Some(d) = &dir {
            v.push(format!("{d}{n}"));
        }
    }
    for n in names {
        v.push((*n).to_string());
    }
    v
}

unsafe fn sym(h: HMODULE, name: &[u8]) -> Option<*mut c_void> {
    let p = GetProcAddress(h, name.as_ptr());
    if p.is_null() { None } else { Some(p) }
}

pub const OPUS_APPLICATION_VOIP: i32 = 2048;
pub const OPUS_SET_BITRATE_REQUEST: i32 = 4002;
pub const OPUS_SET_VBR_REQUEST: i32 = 4006;
pub const OPUS_SET_SIGNAL_REQUEST: i32 = 4024;
pub const OPUS_SIGNAL_VOICE: i32 = 3001;

type OpusEncCreate = unsafe extern "C" fn(i32, i32, i32, *mut i32) -> *mut c_void;
type OpusEncCtl = unsafe extern "C" fn(*mut c_void, i32, ...) -> i32;
type OpusEncode = unsafe extern "C" fn(*mut c_void, *const i16, i32, *mut u8, i32) -> i32;
type OpusDestroy = unsafe extern "C" fn(*mut c_void);
type OpusDecCreate = unsafe extern "C" fn(i32, i32, *mut i32) -> *mut c_void;
type OpusDecode = unsafe extern "C" fn(*mut c_void, *const u8, i32, *mut i16, i32, i32) -> i32;

#[derive(Clone, Copy)]
struct OpusApi {
    enc_create: OpusEncCreate,
    enc_ctl: OpusEncCtl,
    encode: OpusEncode,
    enc_destroy: OpusDestroy,
    dec_create: OpusDecCreate,
    decode: OpusDecode,
    dec_destroy: OpusDestroy,
}

impl OpusApi {
    fn load() -> Option<OpusApi> {
        let h = try_load(&candidates("TOMBOX_OPUS_DLL", &["opus.dll", "libopus.dll", "libopus-0.dll"]))?;
        unsafe {
            Some(OpusApi {
                enc_create: core::mem::transmute::<*mut c_void, OpusEncCreate>(sym(h, b"opus_encoder_create\0")?),
                enc_ctl: core::mem::transmute::<*mut c_void, OpusEncCtl>(sym(h, b"opus_encoder_ctl\0")?),
                encode: core::mem::transmute::<*mut c_void, OpusEncode>(sym(h, b"opus_encode\0")?),
                enc_destroy: core::mem::transmute::<*mut c_void, OpusDestroy>(sym(h, b"opus_encoder_destroy\0")?),
                dec_create: core::mem::transmute::<*mut c_void, OpusDecCreate>(sym(h, b"opus_decoder_create\0")?),
                decode: core::mem::transmute::<*mut c_void, OpusDecode>(sym(h, b"opus_decode\0")?),
                dec_destroy: core::mem::transmute::<*mut c_void, OpusDestroy>(sym(h, b"opus_decoder_destroy\0")?),
            })
        }
    }
}

type SkpSize = unsafe extern "C" fn(*mut i32) -> i32;
type SkpInitDec = unsafe extern "C" fn(*mut c_void) -> i32;
type SkpDecode = unsafe extern "C" fn(*mut c_void, *mut i32, i32, *const u8, i32, *mut i16, *mut i16) -> i32;
type SkpInitEnc = unsafe extern "C" fn(*mut c_void, *mut i32) -> i32;
type SkpEncode = unsafe extern "C" fn(*mut c_void, *const i32, *const i16, i32, *mut u8, *mut i16) -> i32;

#[derive(Clone, Copy)]
struct SilkApi {
    dec_size: SkpSize,
    init_dec: SkpInitDec,
    decode: SkpDecode,
    enc_size: SkpSize,
    init_enc: SkpInitEnc,
    encode: SkpEncode,
}

impl SilkApi {
    fn load() -> Option<SilkApi> {
        let h = try_load(&candidates("TOMBOX_SILK_DLL", &["SKP_SILK_SDK.dll", "silk.dll", "silk_sdk.dll"]))?;
        unsafe {
            Some(SilkApi {
                dec_size: core::mem::transmute::<*mut c_void, SkpSize>(sym(h, b"SKP_Silk_SDK_Get_Decoder_Size\0")?),
                init_dec: core::mem::transmute::<*mut c_void, SkpInitDec>(sym(h, b"SKP_Silk_SDK_InitDecoder\0")?),
                decode: core::mem::transmute::<*mut c_void, SkpDecode>(sym(h, b"SKP_Silk_SDK_Decode\0")?),
                enc_size: core::mem::transmute::<*mut c_void, SkpSize>(sym(h, b"SKP_Silk_SDK_Get_Encoder_Size\0")?),
                init_enc: core::mem::transmute::<*mut c_void, SkpInitEnc>(sym(h, b"SKP_Silk_SDK_InitEncoder\0")?),
                encode: core::mem::transmute::<*mut c_void, SkpEncode>(sym(h, b"SKP_Silk_SDK_Encode\0")?),
            })
        }
    }
}

pub struct DynProvider {
    opus: Option<OpusApi>,
    silk: Option<SilkApi>,
}

impl DynProvider {
    pub fn load() -> DynProvider {
        DynProvider { opus: OpusApi::load(), silk: SilkApi::load() }
    }
}

pub fn opus_frame_ok(rate: u32, frame: usize) -> bool {
    let unit = (rate / 400) as usize;
    unit != 0 && frame % unit == 0 && matches!(frame / unit, 1 | 2 | 4 | 8 | 16 | 24 | 32 | 40 | 48)
}

pub fn opus_padded_frame(rate: u32, frame: usize) -> Option<usize> {
    let unit = (rate / 400) as usize;
    [1usize, 2, 4, 8, 16, 24, 32, 40, 48].iter().map(|m| m * unit).find(|&f| f >= frame && f > 0)
}

struct DynOpusEnc {
    api: OpusApi,
    st: *mut c_void,
    rate: u32,
    channels: usize,
}

unsafe impl Send for DynOpusEnc {}

impl Drop for DynOpusEnc {
    fn drop(&mut self) {
        unsafe { (self.api.enc_destroy)(self.st) }
    }
}

impl OpusEncoderCodec for DynOpusEnc {
    fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize, i32> {
        let frame = pcm.len() / self.channels;
        let target = opus_padded_frame(self.rate, frame).ok_or(E_INVALIDARG)?;
        let mut padded;
        let src: &[i16] = if target == frame && pcm.len() == frame * self.channels {
            pcm
        } else {
            padded = vec![0i16; target * self.channels];
            padded[..frame * self.channels].copy_from_slice(&pcm[..frame * self.channels]);
            &padded
        };
        let n = unsafe { (self.api.encode)(self.st, src.as_ptr(), target as i32, out.as_mut_ptr(), out.len() as i32) };
        if n < 0 { Err(E_FAIL) } else { Ok(n as usize) }
    }
}

struct DynOpusDec {
    api: OpusApi,
    st: *mut c_void,
    channels: usize,
    last_frame: usize,
}

unsafe impl Send for DynOpusDec {}

impl Drop for DynOpusDec {
    fn drop(&mut self) {
        unsafe { (self.api.dec_destroy)(self.st) }
    }
}

impl OpusDecoderCodec for DynOpusDec {
    fn decode(&mut self, packet: &[u8], pcm: &mut [i16]) -> Result<usize, i32> {
        let cap = pcm.len() / self.channels;
        let (ptr, len, frame) = if packet.is_empty() {
            (core::ptr::null(), 0, self.last_frame.min(cap))
        } else {
            (packet.as_ptr(), packet.len() as i32, cap)
        };
        let n = unsafe { (self.api.decode)(self.st, ptr, len, pcm.as_mut_ptr(), frame as i32, 0) };
        if n < 0 {
            return Err(E_FAIL);
        }
        if n > 0 {
            self.last_frame = n as usize;
        }
        Ok(n as usize * self.channels)
    }
}

struct DynSilkDec {
    api: SilkApi,
    st: Vec<u64>,
}

impl SilkDecoderCodec for DynSilkDec {
    fn decode(&mut self, control: &mut [i32; 6], all_frames: bool, packet: &[u8], pcm: &mut [i16]) -> Result<usize, i32> {
        let lost = if packet.is_empty() { 1 } else { 0 };
        let step = (control[0].max(8000) as usize) / 50;
        let mut total = 0usize;
        loop {
            if pcm.len() - total < step {
                break;
            }
            let mut n: i16 = 0;
            let r = unsafe {
                (self.api.decode)(
                    self.st.as_mut_ptr() as *mut c_void,
                    control.as_mut_ptr(),
                    lost,
                    packet.as_ptr(),
                    packet.len() as i32,
                    pcm[total..].as_mut_ptr(),
                    &mut n,
                )
            };
            if r != 0 {
                return Err(E_FAIL);
            }
            total += n.max(0) as usize;
            if !all_frames || control[3] == 0 || n <= 0 {
                break;
            }
        }
        Ok(total)
    }
}

struct DynSilkEnc {
    api: SilkApi,
    st: Vec<u64>,
}

impl SilkEncoderCodec for DynSilkEnc {
    fn encode(&mut self, control: &mut [i32; 8], pcm: &[i16], out: &mut [u8]) -> Result<usize, i32> {
        let mut n: i16 = out.len().min(i16::MAX as usize) as i16;
        let r = unsafe {
            (self.api.encode)(
                self.st.as_mut_ptr() as *mut c_void,
                control.as_ptr(),
                pcm.as_ptr(),
                pcm.len() as i32,
                out.as_mut_ptr(),
                &mut n,
            )
        };
        if r != 0 { Err(E_FAIL) } else { Ok(n.max(0) as usize) }
    }
}

fn state_buf(bytes: i32) -> Result<Vec<u64>, i32> {
    if bytes <= 0 || bytes > (64 << 20) {
        return Err(E_FAIL);
    }
    Ok(vec![0u64; (bytes as usize).div_ceil(8)])
}

impl CodecProvider for DynProvider {
    fn opus_available(&self) -> bool {
        self.opus.is_some()
    }
    fn silk_available(&self) -> bool {
        self.silk.is_some()
    }
    fn opus_decoder(&self, rate: u32, channels: u32) -> Result<Box<dyn OpusDecoderCodec>, i32> {
        let api = self.opus.ok_or(E_MOD_NOT_FOUND)?;
        let mut err = 0;
        let st = unsafe { (api.dec_create)(rate as i32, channels as i32, &mut err) };
        if st.is_null() || err != 0 {
            return Err(E_FAIL);
        }
        Ok(Box::new(DynOpusDec { api, st, channels: channels as usize, last_frame: (rate / 50) as usize }))
    }
    fn opus_encoder(&self, rate: u32, channels: u32, bitrate: u32, flags: u32) -> Result<Box<dyn OpusEncoderCodec>, i32> {
        let api = self.opus.ok_or(E_MOD_NOT_FOUND)?;
        let mut err = 0;
        let st = unsafe { (api.enc_create)(rate as i32, channels as i32, OPUS_APPLICATION_VOIP, &mut err) };
        if st.is_null() || err != 0 {
            return Err(E_FAIL);
        }
        let enc = DynOpusEnc { api, st, rate, channels: channels as usize };
        unsafe {
            (api.enc_ctl)(st, OPUS_SET_BITRATE_REQUEST, bitrate as i32);
            (api.enc_ctl)(st, OPUS_SET_SIGNAL_REQUEST, OPUS_SIGNAL_VOICE);
            (api.enc_ctl)(st, OPUS_SET_VBR_REQUEST, if flags != 0 { 0i32 } else { 1i32 });
        }
        Ok(Box::new(enc))
    }
    fn silk_decoder(&self) -> Result<Box<dyn SilkDecoderCodec>, i32> {
        let api = self.silk.ok_or(E_MOD_NOT_FOUND)?;
        let mut size = 0;
        if unsafe { (api.dec_size)(&mut size) } != 0 {
            return Err(E_FAIL);
        }
        let mut st = state_buf(size)?;
        if unsafe { (api.init_dec)(st.as_mut_ptr() as *mut c_void) } != 0 {
            return Err(E_FAIL);
        }
        Ok(Box::new(DynSilkDec { api, st }))
    }
    fn silk_encoder(&self) -> Result<Box<dyn SilkEncoderCodec>, i32> {
        let api = self.silk.ok_or(E_MOD_NOT_FOUND)?;
        let mut size = 0;
        if unsafe { (api.enc_size)(&mut size) } != 0 {
            return Err(E_FAIL);
        }
        let mut st = state_buf(size)?;
        let mut status = [0i32; 8];
        if unsafe { (api.init_enc)(st.as_mut_ptr() as *mut c_void, status.as_mut_ptr()) } != 0 {
            return Err(E_FAIL);
        }
        Ok(Box::new(DynSilkEnc { api, st }))
    }
}
