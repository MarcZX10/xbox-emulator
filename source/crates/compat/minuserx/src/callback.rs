use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::*;

#[link(name = "ole32")]
extern "system" {
    fn CoCreateFreeThreadedMarshaler(outer: *mut c_void, out: *mut *mut c_void) -> i32;
    fn CoTaskMemAlloc(n: usize) -> *mut c_void;
}

#[link(name = "runtimeobject")]
extern "system" {
    fn RoOriginateError(hr: i32, msg: *mut c_void) -> i32;
}

pub const OFF_MARSHAL: usize = 0x08;
pub const OFF_MARSHALLER: usize = 0x20;
pub const OFF_WEAK_SOURCE: usize = 0x28;
pub const OFF_CALLBACK: usize = 0x30;
pub const OFF_REFS: usize = 0x40;

const TAG: u64 = 1 << 63;
const MAX_REFS: u32 = 0x7fff_ffff;

#[repr(C)]
pub struct Callback {
    vtbl: *const InspectableVtbl,
    marshal_vtbl: *const MarshalVtbl,
    reserved: [usize; 2],
    marshaller: *mut c_void,
    weak_source_vtbl: *const WeakSourceVtbl,
    callback_vtbl: *const CallbackVtbl,
    reserved2: usize,
    refs: AtomicU64,
}

#[repr(C)]
pub struct InspectableVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_iids: unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut Guid) -> i32,
    pub get_runtime_class_name: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    pub get_trust_level: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
}

#[repr(C)]
pub struct MarshalVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_unmarshal_class: unsafe extern "system" fn(*mut c_void, *const Guid, *mut c_void, u32, *mut c_void, u32, *mut Guid) -> i32,
    pub get_marshal_size_max: unsafe extern "system" fn(*mut c_void, *const Guid, *mut c_void, u32, *mut c_void, u32, *mut u32) -> i32,
    pub marshal_interface: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut c_void, u32, *mut c_void, u32) -> i32,
    pub unmarshal_interface: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub release_marshal_data: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    pub disconnect_object: unsafe extern "system" fn(*mut c_void, u32) -> i32,
}

#[repr(C)]
pub struct WeakSourceVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_weak_reference: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
}

#[repr(C)]
pub struct CallbackVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub deliver: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, usize, isize) -> i32,
    pub refresh_cursor: unsafe extern "system" fn(*mut c_void) -> i32,
}

#[repr(C)]
pub struct WeakRefVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub resolve: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
}

#[repr(C)]
pub struct WeakRef {
    vtbl: *const WeakRefVtbl,
    reserved: u32,
    refs: AtomicU32,
    strong: AtomicU32,
    reserved2: u32,
    target: *mut Callback,
}

pub static INSPECTABLE_VTBL: InspectableVtbl = InspectableVtbl {
    query_interface: base_qi,
    add_ref: base_add_ref,
    release: base_release,
    get_iids,
    get_runtime_class_name,
    get_trust_level,
};

pub static MARSHAL_VTBL: MarshalVtbl = MarshalVtbl {
    query_interface: m_qi,
    add_ref: m_add_ref,
    release: m_release,
    get_unmarshal_class: m_get_unmarshal_class,
    get_marshal_size_max: m_get_marshal_size_max,
    marshal_interface: m_marshal_interface,
    unmarshal_interface: m_unmarshal_interface,
    release_marshal_data: m_release_marshal_data,
    disconnect_object: m_disconnect_object,
};

pub static WEAK_SOURCE_VTBL: WeakSourceVtbl = WeakSourceVtbl { query_interface: ws_qi, add_ref: ws_add_ref, release: ws_release, get_weak_reference };

pub static CALLBACK_VTBL: CallbackVtbl = CallbackVtbl { query_interface: cb_qi, add_ref: cb_add_ref, release: cb_release, deliver: cb_deliver, refresh_cursor: cb_refresh_cursor };

pub static WEAK_REF_VTBL: WeakRefVtbl = WeakRefVtbl { query_interface: wr_qi, add_ref: wr_add_ref, release: wr_release, resolve: wr_resolve };

pub const GET_IIDS: [Guid; 3] = [IID_IINSPECTABLE, IID_IWEAKREFERENCESOURCE, IID_CALLBACK];

pub unsafe fn new_callback(with_ftm: bool) -> *mut Callback {
    let cb = Box::into_raw(Box::new(Callback {
        vtbl: &INSPECTABLE_VTBL,
        marshal_vtbl: &MARSHAL_VTBL,
        reserved: [0; 2],
        marshaller: null_mut(),
        weak_source_vtbl: &WEAK_SOURCE_VTBL,
        callback_vtbl: &CALLBACK_VTBL,
        reserved2: 0,
        refs: AtomicU64::new(1),
    }));
    if with_ftm {
        let mut t = null_mut();
        if CoCreateFreeThreadedMarshaler(null_mut(), &mut t) >= 0 && !t.is_null() {
            let mut m = null_mut();
            query(t, &IID_IMARSHAL, &mut m);
            (*cb).marshaller = m;
            release(t);
        }
    }
    cb
}

pub unsafe fn callback_iface(cb: *mut Callback) -> *mut c_void {
    (cb as *mut u8).add(OFF_CALLBACK) as *mut c_void
}

pub unsafe fn marshaller_of(cb: *mut Callback) -> *mut c_void {
    (*cb).marshaller
}

unsafe fn at(p: *mut c_void, off: usize) -> *mut Callback {
    (p as *mut u8).sub(off) as *mut Callback
}

unsafe fn iface(cb: *mut Callback, off: usize) -> *mut c_void {
    (cb as *mut u8).add(off) as *mut c_void
}

unsafe fn weak_of(v: u64) -> *mut WeakRef {
    (v << 1) as *mut WeakRef
}

pub unsafe fn strong_count(cb: *mut Callback) -> u32 {
    let v = (*cb).refs.load(Ordering::Acquire);
    if v & TAG != 0 {
        (*weak_of(v)).strong.load(Ordering::Acquire)
    } else {
        v as u32
    }
}

unsafe fn obj_add_ref(cb: *mut Callback) -> u32 {
    let r = &(*cb).refs;
    let mut v = r.load(Ordering::Acquire);
    loop {
        if v & TAG != 0 {
            let s = &(*weak_of(v)).strong;
            let mut c = s.load(Ordering::Acquire);
            loop {
                if c == MAX_REFS {
                    return MAX_REFS;
                }
                match s.compare_exchange(c, c + 1, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => return c + 1,
                    Err(n) => c = n,
                }
            }
        }
        if v as u32 == MAX_REFS {
            return MAX_REFS;
        }
        match r.compare_exchange(v, v + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return (v + 1) as u32,
            Err(n) => v = n,
        }
    }
}

unsafe fn obj_release(cb: *mut Callback) -> u32 {
    let r = &(*cb).refs;
    let mut v = r.load(Ordering::Acquire);
    let n = loop {
        if v & TAG != 0 {
            let s = &(*weak_of(v)).strong;
            let mut c = s.load(Ordering::Acquire);
            break loop {
                if c == MAX_REFS {
                    return MAX_REFS - 1;
                }
                match s.compare_exchange(c, c - 1, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => break c - 1,
                    Err(x) => c = x,
                }
            };
        }
        if v as u32 == MAX_REFS {
            return MAX_REFS - 1;
        }
        match r.compare_exchange(v, v - 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break (v - 1) as u32,
            Err(x) => v = x,
        }
    };
    if n == 0 {
        destroy(cb);
    }
    n
}

unsafe fn destroy(cb: *mut Callback) {
    let v = (*cb).refs.load(Ordering::Acquire);
    if v & TAG != 0 {
        wr_release(weak_of(v) as *mut c_void);
    }
    let m = (*cb).marshaller;
    if !m.is_null() {
        (*cb).marshaller = null_mut();
        release(m);
    }
    drop(Box::from_raw(cb));
}

unsafe fn obj_qi(cb: *mut Callback, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    *out = null_mut();
    let id = *iid;
    let off = if id == IID_IUNKNOWN || id == IID_IINSPECTABLE {
        0
    } else if id == IID_IAGILEOBJECT || id == IID_IMARSHAL {
        OFF_MARSHAL
    } else if id == IID_IWEAKREFERENCESOURCE {
        OFF_WEAK_SOURCE
    } else if id == IID_CALLBACK {
        OFF_CALLBACK
    } else {
        return E_NOINTERFACE;
    };
    let p = iface(cb, off);
    *out = p;
    add_ref(p);
    S_OK
}

unsafe extern "system" fn base_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    obj_qi(at(this, 0), iid, out)
}

unsafe extern "system" fn base_add_ref(this: *mut c_void) -> u32 {
    obj_add_ref(at(this, 0))
}

unsafe extern "system" fn base_release(this: *mut c_void) -> u32 {
    obj_release(at(this, 0))
}

unsafe extern "system" fn get_iids(_this: *mut c_void, count: *mut u32, iids: *mut *mut Guid) -> i32 {
    *iids = null_mut();
    *count = 0;
    let p = CoTaskMemAlloc(core::mem::size_of_val(&GET_IIDS)) as *mut Guid;
    if p.is_null() {
        return E_OUTOFMEMORY;
    }
    core::ptr::copy_nonoverlapping(GET_IIDS.as_ptr(), p, GET_IIDS.len());
    *count = GET_IIDS.len() as u32;
    *iids = p;
    S_OK
}

unsafe extern "system" fn get_runtime_class_name(_this: *mut c_void, out: *mut *mut c_void) -> i32 {
    *out = null_mut();
    RoOriginateError(E_NOTIMPL as i32, null_mut());
    E_NOTIMPL as i32
}

unsafe extern "system" fn get_trust_level(_this: *mut c_void, _out: *mut u32) -> i32 {
    RoOriginateError(E_NOTIMPL as i32, null_mut());
    E_NOTIMPL as i32
}

unsafe extern "system" fn m_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    obj_qi(at(this, OFF_MARSHAL), iid, out)
}

unsafe extern "system" fn m_add_ref(this: *mut c_void) -> u32 {
    obj_add_ref(at(this, OFF_MARSHAL))
}

unsafe extern "system" fn m_release(this: *mut c_void) -> u32 {
    obj_release(at(this, OFF_MARSHAL))
}

unsafe fn ftm(this: *mut c_void) -> *mut c_void {
    (*at(this, OFF_MARSHAL)).marshaller
}

unsafe extern "system" fn m_get_unmarshal_class(this: *mut c_void, riid: *const Guid, pv: *mut c_void, ctx: u32, pctx: *mut c_void, flags: u32, cid: *mut Guid) -> i32 {
    let m = ftm(this);
    if m.is_null() {
        return E_OUTOFMEMORY;
    }
    ((*(*(m as *const *const MarshalVtbl))).get_unmarshal_class)(m, riid, pv, ctx, pctx, flags, cid)
}

unsafe extern "system" fn m_get_marshal_size_max(this: *mut c_void, riid: *const Guid, pv: *mut c_void, ctx: u32, pctx: *mut c_void, flags: u32, size: *mut u32) -> i32 {
    let m = ftm(this);
    if m.is_null() {
        return E_OUTOFMEMORY;
    }
    ((*(*(m as *const *const MarshalVtbl))).get_marshal_size_max)(m, riid, pv, ctx, pctx, flags, size)
}

unsafe extern "system" fn m_marshal_interface(this: *mut c_void, stm: *mut c_void, riid: *const Guid, pv: *mut c_void, ctx: u32, pctx: *mut c_void, flags: u32) -> i32 {
    let m = ftm(this);
    if m.is_null() {
        return E_OUTOFMEMORY;
    }
    ((*(*(m as *const *const MarshalVtbl))).marshal_interface)(m, stm, riid, pv, ctx, pctx, flags)
}

unsafe extern "system" fn m_unmarshal_interface(this: *mut c_void, stm: *mut c_void, riid: *const Guid, out: *mut *mut c_void) -> i32 {
    let m = ftm(this);
    if m.is_null() {
        return E_OUTOFMEMORY;
    }
    ((*(*(m as *const *const MarshalVtbl))).unmarshal_interface)(m, stm, riid, out)
}

unsafe extern "system" fn m_release_marshal_data(this: *mut c_void, stm: *mut c_void) -> i32 {
    let m = ftm(this);
    if m.is_null() {
        return E_OUTOFMEMORY;
    }
    ((*(*(m as *const *const MarshalVtbl))).release_marshal_data)(m, stm)
}

unsafe extern "system" fn m_disconnect_object(this: *mut c_void, reserved: u32) -> i32 {
    let m = ftm(this);
    if m.is_null() {
        return E_OUTOFMEMORY;
    }
    ((*(*(m as *const *const MarshalVtbl))).disconnect_object)(m, reserved)
}

unsafe extern "system" fn ws_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    obj_qi(at(this, OFF_WEAK_SOURCE), iid, out)
}

unsafe extern "system" fn ws_add_ref(this: *mut c_void) -> u32 {
    obj_add_ref(at(this, OFF_WEAK_SOURCE))
}

unsafe extern "system" fn ws_release(this: *mut c_void) -> u32 {
    obj_release(at(this, OFF_WEAK_SOURCE))
}

unsafe extern "system" fn get_weak_reference(this: *mut c_void, out: *mut *mut c_void) -> i32 {
    let cb = at(this, OFF_WEAK_SOURCE);
    *out = null_mut();
    let r = &(*cb).refs;
    let mut v = r.load(Ordering::Acquire);
    if v & TAG != 0 {
        let w = weak_of(v);
        wr_add_ref(w as *mut c_void);
        *out = w as *mut c_void;
        return S_OK;
    }
    let w = Box::into_raw(Box::new(WeakRef { vtbl: &WEAK_REF_VTBL, reserved: 0, refs: AtomicU32::new(2), strong: AtomicU32::new(v as u32), reserved2: 0, target: cb }));
    let tagged = ((w as u64) >> 1) | TAG;
    loop {
        (*w).strong.store(v as u32, Ordering::Release);
        match r.compare_exchange(v, tagged, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                *out = w as *mut c_void;
                return S_OK;
            }
            Err(n) if n & TAG == 0 => v = n,
            Err(n) => {
                drop(Box::from_raw(w));
                let e = weak_of(n);
                wr_add_ref(e as *mut c_void);
                *out = e as *mut c_void;
                return S_OK;
            }
        }
    }
}

unsafe extern "system" fn cb_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    obj_qi(at(this, OFF_CALLBACK), iid, out)
}

unsafe extern "system" fn cb_add_ref(this: *mut c_void) -> u32 {
    obj_add_ref(at(this, OFF_CALLBACK))
}

unsafe extern "system" fn cb_release(this: *mut c_void) -> u32 {
    obj_release(at(this, OFF_CALLBACK))
}

unsafe extern "system" fn cb_deliver(_this: *mut c_void, hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> i32 {
    let hr = crate::queue::post(hwnd, msg, wparam, lparam);
    if hr < 0 {
        hr
    } else {
        S_OK
    }
}

unsafe extern "system" fn cb_refresh_cursor(_this: *mut c_void) -> i32 {
    let mut h = null_mut();
    let hr = get_hooks(&mut h);
    if hr < 0 {
        return hr;
    }
    if h.is_null() {
        return S_OK;
    }
    let r = hooks_refresh(h);
    release(h);
    if r < 0 {
        r
    } else {
        S_OK
    }
}

unsafe extern "system" fn wr_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    *out = null_mut();
    let id = *iid;
    if id == IID_IUNKNOWN || id == IID_IWEAKREFERENCE {
        *out = this;
        wr_add_ref(this);
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn wr_add_ref(this: *mut c_void) -> u32 {
    let r = &(*(this as *mut WeakRef)).refs;
    let mut c = r.load(Ordering::Acquire);
    loop {
        if c == MAX_REFS {
            return MAX_REFS;
        }
        match r.compare_exchange(c, c + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return c + 1,
            Err(n) => c = n,
        }
    }
}

unsafe extern "system" fn wr_release(this: *mut c_void) -> u32 {
    let w = this as *mut WeakRef;
    let r = &(*w).refs;
    let mut c = r.load(Ordering::Acquire);
    let n = loop {
        if c == MAX_REFS {
            return MAX_REFS - 1;
        }
        match r.compare_exchange(c, c - 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break c - 1,
            Err(x) => c = x,
        }
    };
    if n == 0 {
        drop(Box::from_raw(w));
    }
    n
}

unsafe extern "system" fn wr_resolve(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    let w = this as *mut WeakRef;
    *out = null_mut();
    let s = &(*w).strong;
    let mut c = s.load(Ordering::Acquire);
    loop {
        if c == 0 {
            return S_OK;
        }
        match s.compare_exchange(c, c + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(n) => c = n,
        }
    }
    let t = (*w).target;
    let hr = obj_qi(t, iid, out);
    obj_release(t);
    hr
}
