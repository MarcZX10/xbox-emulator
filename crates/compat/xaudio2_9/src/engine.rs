use crate::ffi::*;
use crate::xma_voice;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

pub const CONSOLE_MIX_RATE: u32 = 48000;

#[repr(C)]
pub struct Engine {
    vtbl: *const Fp,
    ext_vtbl: *const Fp,
    refs: AtomicU32,
    pub inner: *mut c_void,
    inner_ext: *mut c_void,
    pub voices: Mutex<Vec<usize>>,
    pub flags: u32,
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

type QiFn = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
type RefFn = unsafe extern "system" fn(*mut c_void) -> u32;

fn ext_offset() -> usize {
    core::mem::offset_of!(Engine, ext_vtbl)
}

unsafe fn from_ext(p: *mut c_void) -> *mut Engine {
    (p as *mut u8).sub(ext_offset()) as *mut Engine
}

pub unsafe fn wrap(inner: *mut c_void, flags: u32) -> *mut c_void {
    let mut inner_ext: *mut c_void = core::ptr::null_mut();
    let qi: QiFn = core::mem::transmute(slot(inner, xa::QUERY_INTERFACE));
    if qi(inner, &IID_IXAUDIO2_EXTENSION, &mut inner_ext) < 0 {
        inner_ext = core::ptr::null_mut();
    }
    let e = Box::new(Engine {
        vtbl: ENGINE_VTBL.as_ptr(),
        ext_vtbl: EXT_VTBL.as_ptr(),
        refs: AtomicU32::new(1),
        inner,
        inner_ext,
        voices: Mutex::new(Vec::new()),
        flags,
    });
    Box::into_raw(e) as *mut c_void
}

unsafe extern "system" fn query_interface(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    if riid.is_null() {
        *out = core::ptr::null_mut();
        return E_POINTER;
    }
    let e = this as *mut Engine;
    let id = *riid;
    if id == IID_IUNKNOWN || id == IID_IXAUDIO2 {
        add_ref(this);
        *out = this;
        return S_OK;
    }
    if id == IID_IXAUDIO2_EXTENSION {
        if (*e).inner_ext.is_null() {
            *out = core::ptr::null_mut();
            return E_NOINTERFACE;
        }
        add_ref(this);
        *out = (this as *mut u8).add(ext_offset()) as *mut c_void;
        return S_OK;
    }
    let qi: QiFn = core::mem::transmute(slot((*e).inner, xa::QUERY_INTERFACE));
    qi((*e).inner, riid, out)
}

unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    (*(this as *mut Engine)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let e = this as *mut Engine;
    let n = (*e).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        let voices: Vec<usize> = std::mem::take(&mut *(*e).voices.lock().unwrap_or_else(|p| p.into_inner()));
        for v in voices {
            xma_voice::destroy_with_engine(v as *mut c_void);
        }
        if !(*e).inner_ext.is_null() {
            let r: RefFn = core::mem::transmute(slot((*e).inner_ext, xa::RELEASE));
            r((*e).inner_ext);
        }
        let r: RefFn = core::mem::transmute(slot((*e).inner, xa::RELEASE));
        r((*e).inner);
        drop(Box::from_raw(e));
    }
    n
}

unsafe extern "system" fn register_for_callbacks(this: *mut c_void, cb: *mut c_void) -> HRESULT {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT = core::mem::transmute(slot(inner, xa::REGISTER_FOR_CALLBACKS));
    f(inner, cb)
}

unsafe extern "system" fn unregister_for_callbacks(this: *mut c_void, cb: *mut c_void) {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(slot(inner, xa::UNREGISTER_FOR_CALLBACKS));
    f(inner, cb)
}

pub type CreateSourceFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *const WAVEFORMATEX, u32, f32, *mut c_void, *const c_void, *const c_void) -> HRESULT;

unsafe extern "system" fn create_source_voice(
    this: *mut c_void,
    pp: *mut *mut c_void,
    fmt: *const WAVEFORMATEX,
    flags: u32,
    max_ratio: f32,
    cb: *mut c_void,
    sends: *const c_void,
    effects: *const c_void,
) -> HRESULT {
    let e = this as *mut Engine;
    if !pp.is_null() && !fmt.is_null() && core::ptr::read_unaligned(core::ptr::addr_of!((*fmt).w_format_tag)) == WAVE_FORMAT_XMA2 {
        return xma_voice::create(e, pp, fmt, flags, max_ratio, cb, sends, effects);
    }
    let inner = (*e).inner;
    let f: CreateSourceFn = core::mem::transmute(slot(inner, xa::CREATE_SOURCE_VOICE));
    f(inner, pp, fmt, flags, max_ratio, cb, sends, effects)
}

unsafe extern "system" fn create_submix_voice(this: *mut c_void, pp: *mut *mut c_void, ch: u32, rate: u32, flags: u32, stage: u32, sends: *const c_void, effects: *const c_void) -> HRESULT {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, u32, *const c_void, *const c_void) -> HRESULT =
        core::mem::transmute(slot(inner, xa::CREATE_SUBMIX_VOICE));
    f(inner, pp, ch, rate, flags, stage, sends, effects)
}

unsafe extern "system" fn create_mastering_voice(this: *mut c_void, pp: *mut *mut c_void, ch: u32, rate: u32, flags: u32, device: *const u16, effects: *const c_void, category: i32) -> HRESULT {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, *const u16, *const c_void, i32) -> HRESULT =
        core::mem::transmute(slot(inner, xa::CREATE_MASTERING_VOICE));
    let rate = if rate == 0 { CONSOLE_MIX_RATE } else { rate };
    f(inner, pp, ch, rate, flags, device, effects, category)
}

unsafe extern "system" fn start_engine(this: *mut c_void) -> HRESULT {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void) -> HRESULT = core::mem::transmute(slot(inner, xa::START_ENGINE));
    f(inner)
}

unsafe extern "system" fn stop_engine(this: *mut c_void) {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void) = core::mem::transmute(slot(inner, xa::STOP_ENGINE));
    f(inner)
}

unsafe extern "system" fn commit_changes(this: *mut c_void, set: u32) -> HRESULT {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT = core::mem::transmute(slot(inner, xa::COMMIT_CHANGES));
    f(inner, set)
}

unsafe extern "system" fn get_performance_data(this: *mut c_void, out: *mut c_void) {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(slot(inner, xa::GET_PERFORMANCE_DATA));
    f(inner, out)
}

unsafe extern "system" fn set_debug_configuration(this: *mut c_void, cfg: *const c_void, reserved: *mut c_void) {
    let inner = (*(this as *mut Engine)).inner;
    let f: unsafe extern "system" fn(*mut c_void, *const c_void, *mut c_void) = core::mem::transmute(slot(inner, xa::SET_DEBUG_CONFIGURATION));
    f(inner, cfg, reserved)
}

unsafe extern "system" fn ext_query_interface(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    query_interface(from_ext(this) as *mut c_void, riid, out)
}

unsafe extern "system" fn ext_add_ref(this: *mut c_void) -> u32 {
    add_ref(from_ext(this) as *mut c_void)
}

unsafe extern "system" fn ext_release(this: *mut c_void) -> u32 {
    release(from_ext(this) as *mut c_void)
}

unsafe extern "system" fn ext_get_processing_quantum(this: *mut c_void, num: *mut u32, den: *mut u32) {
    let ie = (*from_ext(this)).inner_ext;
    let f: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) = core::mem::transmute(slot(ie, ext::GET_PROCESSING_QUANTUM));
    f(ie, num, den)
}

unsafe extern "system" fn ext_get_processor(this: *mut c_void, out: *mut u32) {
    let ie = (*from_ext(this)).inner_ext;
    let f: unsafe extern "system" fn(*mut c_void, *mut u32) = core::mem::transmute(slot(ie, ext::GET_PROCESSOR));
    f(ie, out)
}

struct VTable<const N: usize>([Fp; N]);
unsafe impl<const N: usize> Sync for VTable<N> {}

impl<const N: usize> VTable<N> {
    const fn as_ptr(&'static self) -> *const Fp {
        self.0.as_ptr()
    }
}

static ENGINE_VTBL: VTable<{ xa::COUNT }> = VTable([
    query_interface as Fp,
    add_ref as Fp,
    release as Fp,
    register_for_callbacks as Fp,
    unregister_for_callbacks as Fp,
    create_source_voice as Fp,
    create_submix_voice as Fp,
    create_mastering_voice as Fp,
    start_engine as Fp,
    stop_engine as Fp,
    commit_changes as Fp,
    get_performance_data as Fp,
    set_debug_configuration as Fp,
]);

static EXT_VTBL: VTable<5> = VTable([
    ext_query_interface as Fp,
    ext_add_ref as Fp,
    ext_release as Fp,
    ext_get_processing_quantum as Fp,
    ext_get_processor as Fp,
]);

pub fn is_wrapped(p: *mut c_void) -> bool {
    !p.is_null() && unsafe { (*(p as *mut Engine)).vtbl } == ENGINE_VTBL.as_ptr()
}
