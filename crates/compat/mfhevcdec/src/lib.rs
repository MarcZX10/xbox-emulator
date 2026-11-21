use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::Mutex;

pub mod bits;
pub mod d3d12dec;
pub mod dxva;
pub mod hevc;
pub mod mft;

pub const S_OK: i32 = 0;
pub const S_FALSE: i32 = 1;
pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;
pub const CLASS_E_NOAGGREGATION: i32 = 0x8004_0110u32 as i32;
pub const CLASS_E_CLASSNOTAVAILABLE: i32 = 0x8004_0111u32 as i32;

pub type Guid = [u8; 16];

pub const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Guid {
    let a = d1.to_le_bytes();
    let b = d2.to_le_bytes();
    let c = d3.to_le_bytes();
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7]]
}

pub const CLSID_MSH265_DECODER_MFT: Guid = guid(0x420a51a3, 0xd605, 0x430c, [0xb4, 0xfc, 0x45, 0x27, 0x4f, 0xa6, 0xc5, 0x62]);
pub const IID_IUNKNOWN: Guid = guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_ICLASSFACTORY: Guid = guid(1, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);

static OBJECTS: AtomicU32 = AtomicU32::new(0);
static FACTORY: AtomicPtr<Factory> = AtomicPtr::new(core::ptr::null_mut());
static FACTORY_LOCK: Mutex<()> = Mutex::new(());

pub fn module_increment() -> u32 {
    OBJECTS.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn module_decrement() -> u32 {
    OBJECTS.fetch_sub(1, Ordering::SeqCst) - 1
}

pub fn module_objects() -> u32 {
    OBJECTS.load(Ordering::SeqCst)
}

pub unsafe fn read_guid(p: *const c_void) -> Guid {
    *(p as *const Guid)
}

#[repr(C)]
struct FactoryVtbl {
    query_interface: unsafe extern "system" fn(*mut Factory, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Factory) -> u32,
    release: unsafe extern "system" fn(*mut Factory) -> u32,
    create_instance: unsafe extern "system" fn(*mut Factory, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    lock_server: unsafe extern "system" fn(*mut Factory, i32) -> i32,
}

#[repr(C)]
struct Factory {
    vtbl: &'static FactoryVtbl,
    refs: AtomicU32,
}

static FACTORY_VTBL: FactoryVtbl = FactoryVtbl {
    query_interface: factory_qi,
    add_ref: factory_add_ref,
    release: factory_release,
    create_instance: factory_create_instance,
    lock_server: factory_lock_server,
};

unsafe extern "system" fn factory_qi(this: *mut Factory, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = core::ptr::null_mut();
    let iid = *riid;
    if iid == IID_IUNKNOWN || iid == IID_ICLASSFACTORY {
        factory_add_ref(this);
        *ppv = this as *mut c_void;
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn factory_add_ref(this: *mut Factory) -> u32 {
    let n = (*this).refs.fetch_add(1, Ordering::SeqCst) + 1;
    if n == 2 {
        module_increment();
    }
    n
}

unsafe extern "system" fn factory_release(this: *mut Factory) -> u32 {
    let n = (*this).refs.fetch_sub(1, Ordering::SeqCst) - 1;
    if n == 0 {
        drop(Box::from_raw(this));
    } else if n == 1 {
        module_decrement();
    }
    n
}

unsafe extern "system" fn factory_create_instance(_this: *mut Factory, outer: *mut c_void, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = core::ptr::null_mut();
    if !outer.is_null() {
        ro_originate(CLASS_E_NOAGGREGATION, None);
        return CLASS_E_NOAGGREGATION;
    }
    mft::create_instance(riid, ppv)
}

unsafe extern "system" fn factory_lock_server(_this: *mut Factory, lock: i32) -> i32 {
    if lock != 0 {
        module_increment();
    } else {
        module_decrement();
    }
    S_OK
}

windows_targets::link!("combase.dll" "system" fn RoOriginateError(hr: i32, msg: *mut c_void) -> i32);
windows_targets::link!("combase.dll" "system" fn RoOriginateErrorW(hr: i32, len: u32, msg: *const u16) -> i32);
windows_targets::link!("combase.dll" "system" fn WindowsIsStringEmpty(s: *mut c_void) -> i32);
windows_targets::link!("combase.dll" "system" fn WindowsStringHasEmbeddedNull(s: *mut c_void, out: *mut i32) -> i32);

pub fn ro_originate(hr: i32, msg: Option<*mut c_void>) {
    unsafe {
        RoOriginateError(hr, msg.unwrap_or(core::ptr::null_mut()));
    }
}

fn cached_factory() -> *mut Factory {
    let _g = FACTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = FACTORY.load(Ordering::SeqCst);
    if !f.is_null() {
        return f;
    }
    let f = Box::into_raw(Box::new(Factory { vtbl: &FACTORY_VTBL, refs: AtomicU32::new(1) }));
    FACTORY.store(f, Ordering::SeqCst);
    f
}

fn release_cached_factory() {
    let _g = FACTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = FACTORY.swap(core::ptr::null_mut(), Ordering::SeqCst);
    if !f.is_null() {
        unsafe { factory_release(f) };
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const Guid, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    *ppv = core::ptr::null_mut();
    if *clsid != CLSID_MSH265_DECODER_MFT {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let f = cached_factory();
    factory_qi(f, riid, ppv)
}

#[no_mangle]
pub unsafe extern "system" fn DllCanUnloadNow() -> i32 {
    if module_objects() != 0 {
        return S_FALSE;
    }
    release_cached_factory();
    if module_objects() == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

const ACTIVATABLE_CLASS_ID: &str = "activatibleClassId";

#[no_mangle]
pub unsafe extern "system" fn DllGetActivationFactory(class_id: *mut c_void, factory: *mut *mut c_void) -> i32 {
    *factory = core::ptr::null_mut();
    let mut embedded = 0i32;
    if WindowsIsStringEmpty(class_id) != 0 || WindowsStringHasEmbeddedNull(class_id, &mut embedded) < 0 || embedded == 1 {
        let msg: Vec<u16> = ACTIVATABLE_CLASS_ID.encode_utf16().collect();
        RoOriginateErrorW(E_INVALIDARG, msg.len() as u32, msg.as_ptr());
        return E_INVALIDARG;
    }
    RoOriginateError(CLASS_E_CLASSNOTAVAILABLE, class_id);
    CLASS_E_CLASSNOTAVAILABLE
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> i32 {
    E_FAIL
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> i32 {
    S_OK
}
