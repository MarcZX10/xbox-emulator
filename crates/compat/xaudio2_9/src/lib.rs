#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

pub mod engine;
pub mod ffi;
pub mod xma_stream;
pub mod xma_voice;

pub mod exports {
    include!(concat!(env!("OUT_DIR"), "/ordinals.rs"));
}

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use ffi::*;
use std::sync::OnceLock;

type HMODULE = *mut c_void;

extern "system" {
    fn LoadLibraryW(name: *const u16) -> HMODULE;
    fn GetModuleHandleW(name: *const u16) -> HMODULE;
    fn GetProcAddress(m: HMODULE, name: *const u8) -> *mut c_void;
    fn GetSystemDirectoryW(buf: *mut u16, size: u32) -> u32;
    fn GetModuleFileNameW(m: HMODULE, buf: *mut u16, size: u32) -> u32;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut HMODULE) -> i32;
    fn GetLastError() -> u32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoIncrementMTAUsage(cookie: *mut *mut c_void) -> HRESULT;
}

static MTA: OnceLock<HRESULT> = OnceLock::new();

pub fn ensure_console_mta() -> HRESULT {
    *MTA.get_or_init(|| {
        let mut cookie: *mut c_void = core::ptr::null_mut();
        let hr = unsafe { CoIncrementMTAUsage(&mut cookie) };
        log(
            "host",
            &format!(
                "CoIncrementMTAUsage hr={:#010x}: console titles call CoCreateInstance without CoInitializeEx (implicit MTA), so the host engine's MMDevice activation gets the same process-wide MTA",
                hr as u32
            ),
        );
        hr
    })
}

const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 4;
const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 2;

fn log(func: &str, detail: &str) {
    compat_common::record_raw("xaudio2_9", func, detail);
}

fn hresult_from_win32(e: u32) -> HRESULT {
    if e == 0 {
        0x8000_4005u32 as i32
    } else {
        ((e & 0xFFFF) | 0x8007_0000) as i32
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn module_path(m: HMODULE) -> String {
    let mut buf = vec![0u16; 32768];
    let n = unsafe { GetModuleFileNameW(m, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

pub fn self_module() -> HMODULE {
    let mut h: HMODULE = core::ptr::null_mut();
    unsafe {
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, self_module as *const c_void as *const u16, &mut h);
    }
    h
}

pub struct Host {
    pub module: usize,
    pub path: String,
    create_with_version_info: usize,
    create_with_shared_contexts: usize,
    create_audio_reverb: usize,
    create_audio_reverb_v2_8: usize,
    create_audio_volume_meter: usize,
    create_fx: usize,
    x3d_calculate: usize,
    x3d_initialize: usize,
}

static HOST: OnceLock<Result<Host, HRESULT>> = OnceLock::new();

pub fn system_xaudio2_path() -> String {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let dir = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    format!("{dir}\\XAudio2_9.dll")
}

pub fn host() -> Result<&'static Host, HRESULT> {
    HOST.get_or_init(|| {
        ensure_console_mta();
        let path = system_xaudio2_path();
        let m = unsafe { LoadLibraryW(wide(&path).as_ptr()) };
        if m.is_null() {
            let hr = hresult_from_win32(unsafe { GetLastError() });
            log("host", &format!("LoadLibraryW({path}) failed hr={:#010x}", hr as u32));
            return Err(hr);
        }
        let me = self_module();
        if m == me {
            log("host", &format!("{path} resolved to this compat module itself; refusing to recurse"));
            return Err(0x8000_4005u32 as i32);
        }
        let sym = |n: &[u8]| unsafe { GetProcAddress(m, n.as_ptr()) } as usize;
        let h = Host {
            module: m as usize,
            path: module_path(m),
            create_with_version_info: sym(b"XAudio2CreateWithVersionInfo\0"),
            create_with_shared_contexts: sym(b"XAudio2CreateWithSharedContexts\0"),
            create_audio_reverb: sym(b"CreateAudioReverb\0"),
            create_audio_reverb_v2_8: sym(b"CreateAudioReverbV2_8\0"),
            create_audio_volume_meter: sym(b"CreateAudioVolumeMeter\0"),
            create_fx: sym(b"CreateFX\0"),
            x3d_calculate: sym(b"X3DAudioCalculate\0"),
            x3d_initialize: sym(b"X3DAudioInitialize\0"),
        };
        log("host", &format!("compat module {} delegating to host {}", module_path(me), h.path));
        Ok(h)
    })
    .as_ref()
    .map_err(|e| *e)
}

fn missing(name: &str) -> HRESULT {
    log(name, "host XAudio2_9.dll does not export this function");
    0x8007_007Fu32 as i32
}

type ApuValidFn = unsafe extern "system" fn(*const c_void, u32) -> i32;
static APU_VALID: OnceLock<usize> = OnceLock::new();

pub fn apu_address_valid(p: *const c_void, align: u32) -> Result<bool, HRESULT> {
    let f = *APU_VALID.get_or_init(|| unsafe {
        let name = wide("AcpHal.dll");
        let mut m = GetModuleHandleW(name.as_ptr());
        if m.is_null() {
            m = LoadLibraryW(name.as_ptr());
        }
        if m.is_null() {
            log("SubmitSourceBuffer", "AcpHal.dll not loadable: XMA buffers cannot be checked as ApuAlloc memory");
            return 0;
        }
        let f = GetProcAddress(m, b"ApuIsVirtualAddressValid\0".as_ptr()) as usize;
        log("SubmitSourceBuffer", &format!("APU address checks via {} ApuIsVirtualAddressValid={:#x}", module_path(m), f));
        f
    });
    if f == 0 {
        return Err(0x8007_007Eu32 as i32);
    }
    let f: ApuValidFn = unsafe { core::mem::transmute(f) };
    Ok(unsafe { f(p, align) } != 0)
}

static CREATE_LOGS: AtomicU32 = AtomicU32::new(0);

pub fn console_processor(processor: u32) -> u32 {
    if processor == 0 {
        XAUDIO2_CONSOLE_DEFAULT_PROCESSOR
    } else {
        processor
    }
}

type CreateVersionFn = unsafe extern "system" fn(*mut *mut c_void, u32, u32, u32) -> HRESULT;
type CreateSharedFn = unsafe extern "system" fn(*mut *mut c_void, u32, u32, *mut c_void) -> HRESULT;

const NTDDI_WIN10: u32 = 0x0A00_0000;

unsafe fn finish_create(func: &str, pp: *mut *mut c_void, flags: u32, processor: u32, used: u32, hr: HRESULT) -> HRESULT {
    let mut wrapped = false;
    if hr >= 0 && flags & XAUDIO2_DO_NOT_USE_SHAPE == 0 {
        *pp = engine::wrap(*pp, flags);
        wrapped = true;
    }
    if CREATE_LOGS.fetch_add(1, Ordering::Relaxed) < 64 {
        log(
            func,
            &format!(
                "flags={flags:#x} processor={processor:#x}->{used:#x} hr={:#010x} engine={:p} {}",
                hr as u32,
                if pp.is_null() { core::ptr::null_mut() } else { *pp },
                if hr < 0 {
                    "failed"
                } else if wrapped {
                    "SHAPE enabled: wrapped for XMA2 source voices"
                } else {
                    "SHAPE disabled (flag 0x400): host engine returned as is"
                }
            ),
        );
    }
    hr
}

unsafe fn create_version(func: &str, pp: *mut *mut c_void, flags: u32, processor: u32, ntddi: u32) -> HRESULT {
    let h = match host() {
        Ok(h) => h,
        Err(hr) => return hr,
    };
    if h.create_with_version_info == 0 {
        return missing(func);
    }
    let used = console_processor(processor);
    let f: CreateVersionFn = core::mem::transmute(h.create_with_version_info);
    let hr = f(pp, flags, used, ntddi);
    finish_create(func, pp, flags, processor, used, hr)
}

#[no_mangle]
pub unsafe extern "system" fn XAudio2Create(pp: *mut *mut c_void, flags: u32, processor: u32) -> HRESULT {
    create_version("XAudio2Create", pp, flags, processor, NTDDI_WIN10)
}

#[no_mangle]
pub unsafe extern "system" fn XAudio2CreateWithVersionInfo(pp: *mut *mut c_void, flags: u32, processor: u32, ntddi: u32) -> HRESULT {
    create_version("XAudio2CreateWithVersionInfo", pp, flags, processor, ntddi)
}

#[no_mangle]
pub unsafe extern "system" fn XAudio2CreateWithSharedContexts(pp: *mut *mut c_void, flags: u32, processor: u32, shared: *mut c_void) -> HRESULT {
    let h = match host() {
        Ok(h) => h,
        Err(hr) => return hr,
    };
    if h.create_with_shared_contexts == 0 {
        return missing("XAudio2CreateWithSharedContexts");
    }
    let used = console_processor(processor);
    let f: CreateSharedFn = core::mem::transmute(h.create_with_shared_contexts);
    let hr = f(pp, flags, used, shared);
    finish_create("XAudio2CreateWithSharedContexts", pp, flags, processor, used, hr)
}

type ReverbFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;

#[no_mangle]
pub unsafe extern "system" fn CreateAudioReverb(out: *mut *mut c_void) -> HRESULT {
    match host() {
        Ok(h) if h.create_audio_reverb != 0 => core::mem::transmute::<usize, ReverbFn>(h.create_audio_reverb)(out),
        Ok(_) => missing("CreateAudioReverb"),
        Err(hr) => hr,
    }
}

#[no_mangle]
pub unsafe extern "system" fn CreateAudioReverbV2_8(out: *mut *mut c_void) -> HRESULT {
    match host() {
        Ok(h) if h.create_audio_reverb_v2_8 != 0 => core::mem::transmute::<usize, ReverbFn>(h.create_audio_reverb_v2_8)(out),
        Ok(_) => missing("CreateAudioReverbV2_8"),
        Err(hr) => hr,
    }
}

#[no_mangle]
pub unsafe extern "system" fn CreateAudioVolumeMeter(out: *mut *mut c_void) -> HRESULT {
    match host() {
        Ok(h) if h.create_audio_volume_meter != 0 => core::mem::transmute::<usize, ReverbFn>(h.create_audio_volume_meter)(out),
        Ok(_) => missing("CreateAudioVolumeMeter"),
        Err(hr) => hr,
    }
}

type CreateFxFn = unsafe extern "system" fn(*const GUID, *mut *mut c_void, *const c_void, u32) -> HRESULT;

pub const CONSOLE_XAPOFX_CLSIDS: [GUID; 4] = [
    GUID { data1: 0xF5E0_1117, data2: 0xD6C4, data3: 0x485A, data4: [0xA3, 0xF5, 0x69, 0x51, 0x96, 0xF3, 0xDB, 0xFA] },
    GUID { data1: 0xC413_7916, data2: 0x2BE1, data3: 0x46FD, data4: [0x85, 0x99, 0x44, 0x15, 0x36, 0xF4, 0x98, 0x56] },
    GUID { data1: 0x7D9A_CA56, data2: 0xCB68, data3: 0x4807, data4: [0xB6, 0x32, 0xB1, 0x37, 0x35, 0x2E, 0x85, 0x96] },
    GUID { data1: 0x5039_D740, data2: 0xF736, data3: 0x449A, data4: [0x84, 0xD3, 0xA5, 0x62, 0x02, 0x55, 0x7B, 0x87] },
];

static PLATFORM_FX: OnceLock<usize> = OnceLock::new();

fn platform_fx_factory() -> usize {
    *PLATFORM_FX.get_or_init(|| unsafe {
        let name = wide("GCXAudioExt.dll");
        let mut m = GetModuleHandleW(name.as_ptr());
        if m.is_null() {
            m = LoadLibraryW(name.as_ptr());
        }
        if m.is_null() {
            log("CreateFX", "GCXAudioExt.dll (ext-ms-win-xaudio-platform-l1-1-0) not loadable: platform XAPOs unavailable");
            return 0;
        }
        GetProcAddress(m, b"PlatformSpecificXAPOFXFactory\0".as_ptr()) as usize
    })
}

#[no_mangle]
pub unsafe extern "system" fn CreateFX(clsid: *const GUID, out: *mut *mut c_void, init: *const c_void, init_bytes: u32) -> HRESULT {
    if !clsid.is_null() && !CONSOLE_XAPOFX_CLSIDS.contains(&*clsid) {
        let f = platform_fx_factory();
        if f == 0 {
            return 0x8007_000Eu32 as i32;
        }
        let f: CreateFxFn = core::mem::transmute(f);
        let hr = f(clsid, out, init, init_bytes);
        if hr < 0 {
            log("CreateFX", &format!("clsid={:?} GCXAudioExt PlatformSpecificXAPOFXFactory hr={:#010x}", *clsid, hr as u32));
        }
        return hr;
    }
    let h = match host() {
        Ok(h) => h,
        Err(hr) => return hr,
    };
    if h.create_fx == 0 {
        return missing("CreateFX");
    }
    let f: CreateFxFn = core::mem::transmute(h.create_fx);
    let hr = f(clsid, out, init, init_bytes);
    if hr < 0 {
        let id = if clsid.is_null() { None } else { Some(*clsid) };
        log("CreateFX", &format!("clsid={id:?} hr={:#010x}", hr as u32));
    }
    hr
}

type X3dInitFn = unsafe extern "system" fn(u32, f32, *mut u8) -> HRESULT;
type X3dCalcFn = unsafe extern "system" fn(*const u8, *const c_void, *const c_void, u32, *mut c_void);

#[no_mangle]
pub unsafe extern "system" fn X3DAudioInitialize(mask: u32, speed_of_sound: f32, instance: *mut u8) -> HRESULT {
    match host() {
        Ok(h) if h.x3d_initialize != 0 => core::mem::transmute::<usize, X3dInitFn>(h.x3d_initialize)(mask, speed_of_sound, instance),
        Ok(_) => missing("X3DAudioInitialize"),
        Err(hr) => hr,
    }
}

static X3D_FAIL_LOGS: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
pub unsafe extern "system" fn X3DAudioCalculate(instance: *const u8, listener: *const c_void, emitter: *const c_void, flags: u32, dsp: *mut c_void) {
    match host() {
        Ok(h) if h.x3d_calculate != 0 => core::mem::transmute::<usize, X3dCalcFn>(h.x3d_calculate)(instance, listener, emitter, flags, dsp),
        _ => {
            if X3D_FAIL_LOGS.fetch_add(1, Ordering::Relaxed) < 8 {
                log("X3DAudioCalculate", "host X3DAudioCalculate unavailable: DSP settings not computed");
            }
        }
    }
}

pub use engine::is_wrapped as engine_is_wrapped;
pub use xma_voice::is_xma_voice;
