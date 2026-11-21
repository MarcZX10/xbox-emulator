use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use minuserx::*;

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, f: u32) -> i32;
    fn CoMarshalInterface(stm: *mut c_void, iid: *const Guid, unk: *mut c_void, ctx: u32, pctx: *mut c_void, flags: u32) -> i32;
    fn CoUnmarshalInterface(stm: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32;
    fn CreateStreamOnHGlobal(h: *mut c_void, del: i32, out: *mut *mut c_void) -> i32;
    fn CoTaskMemFree(p: *mut c_void);
}

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn mta() {
    unsafe { CoInitializeEx(null_mut(), 0) };
}

unsafe fn qi(p: *mut c_void, iid: &Guid) -> (i32, *mut c_void) {
    let mut o = 1usize as *mut c_void;
    let hr = query(p, iid, &mut o);
    if hr >= 0 {
        release(o);
    }
    (hr, o)
}

unsafe fn vt<T>(p: *mut c_void) -> &'static T {
    &**(p as *const *const T)
}

fn base(cb: *mut Callback) -> *mut c_void {
    cb as *mut c_void
}

fn off(cb: *mut Callback, o: usize) -> *mut c_void {
    (cb as usize + o) as *mut c_void
}

#[test]
fn query_interface_returns_the_console_interface_offsets() {
    let _g = serial();
    mta();
    unsafe {
        let cb = new_callback(true);
        let want = [
            (IID_IUNKNOWN, 0),
            (IID_IINSPECTABLE, 0),
            (IID_IAGILEOBJECT, OFF_MARSHAL),
            (IID_IMARSHAL, OFF_MARSHAL),
            (IID_IWEAKREFERENCESOURCE, OFF_WEAK_SOURCE),
            (IID_CALLBACK, OFF_CALLBACK),
        ];
        for from in [0, OFF_MARSHAL, OFF_WEAK_SOURCE, OFF_CALLBACK] {
            for (iid, o) in want {
                assert_eq!(qi(off(cb, from), &iid), (S_OK, off(cb, o)), "from +{from:#x}");
            }
            let other = guid(0x12345678, 1, 2, [0; 8]);
            assert_eq!(qi(off(cb, from), &other), (E_NOINTERFACE, null_mut()));
            assert_eq!(qi(off(cb, from), &IID_IWEAKREFERENCE), (E_NOINTERFACE, null_mut()));
        }
        assert_eq!(callback_iface(cb), off(cb, OFF_CALLBACK));
        assert_eq!(strong_count(cb), 1);
        assert_eq!(add_ref(off(cb, OFF_CALLBACK)), 2);
        assert_eq!(add_ref(off(cb, OFF_MARSHAL)), 3);
        assert_eq!(release(off(cb, OFF_WEAK_SOURCE)), 2);
        assert_eq!(release(base(cb)), 1);
        assert_eq!(release(base(cb)), 0);
    }
}

#[test]
fn iinspectable_methods_match_the_console() {
    let _g = serial();
    mta();
    unsafe {
        let cb = new_callback(true);
        let v: &InspectableVtbl = vt(base(cb));
        let mut n = 7u32;
        let mut ids: *mut Guid = 1usize as *mut Guid;
        assert_eq!((v.get_iids)(base(cb), &mut n, &mut ids), S_OK);
        assert_eq!(n, 3);
        assert_eq!(core::slice::from_raw_parts(ids, 3), &[IID_IINSPECTABLE, IID_IWEAKREFERENCESOURCE, IID_CALLBACK]);
        CoTaskMemFree(ids as *mut c_void);
        let mut name = 1usize as *mut c_void;
        assert_eq!((v.get_runtime_class_name)(base(cb), &mut name), E_NOTIMPL as i32);
        assert!(name.is_null());
        let mut trust = 9u32;
        assert_eq!((v.get_trust_level)(base(cb), &mut trust), E_NOTIMPL as i32);
        assert_eq!(trust, 9);
        release(base(cb));
    }
}

#[test]
fn imarshal_is_the_free_threaded_marshaler_behind_the_object() {
    let _g = serial();
    mta();
    unsafe {
        let cb = new_callback(true);
        assert!(!marshaller_of(cb).is_null());
        let m = off(cb, OFF_MARSHAL);
        let v: &MarshalVtbl = vt(m);
        let mut cls = [0u8; 16];
        assert_eq!((v.get_unmarshal_class)(m, &IID_CALLBACK, off(cb, OFF_CALLBACK), 3, null_mut(), 0, &mut cls), S_OK);
        assert_eq!(cls, CLSID_INPROC_FREE_MARSHALER);
        let mut size = 0u32;
        assert_eq!((v.get_marshal_size_max)(m, &IID_CALLBACK, off(cb, OFF_CALLBACK), 3, null_mut(), 0, &mut size), S_OK);
        assert!(size > 0);
        let mut stm = null_mut();
        assert_eq!(CreateStreamOnHGlobal(null_mut(), 1, &mut stm), S_OK);
        assert_eq!(CoMarshalInterface(stm, &IID_CALLBACK, base(cb), 3, null_mut(), 0), S_OK);
        let seek: unsafe extern "system" fn(*mut c_void, i64, u32, *mut u64) -> i32 = core::mem::transmute(vcall(stm, 5));
        assert_eq!(seek(stm, 0, 0, null_mut()), S_OK);
        let mut out = null_mut();
        assert_eq!(CoUnmarshalInterface(stm, &IID_CALLBACK, &mut out), S_OK);
        assert_eq!(out, off(cb, OFF_CALLBACK));
        release(out);
        release(stm);
        assert_eq!(strong_count(cb), 1);
        release(base(cb));
    }
}

#[test]
fn imarshal_without_a_marshaler_returns_e_outofmemory() {
    let _g = serial();
    unsafe {
        let cb = new_callback(false);
        assert!(marshaller_of(cb).is_null());
        assert_eq!(qi(base(cb), &IID_IMARSHAL), (S_OK, off(cb, OFF_MARSHAL)));
        let m = off(cb, OFF_MARSHAL);
        let v: &MarshalVtbl = vt(m);
        let mut cls = [0u8; 16];
        let mut size = 0u32;
        let mut out = null_mut();
        assert_eq!((v.get_unmarshal_class)(m, &IID_CALLBACK, null_mut(), 3, null_mut(), 0, &mut cls), E_OUTOFMEMORY);
        assert_eq!((v.get_marshal_size_max)(m, &IID_CALLBACK, null_mut(), 3, null_mut(), 0, &mut size), E_OUTOFMEMORY);
        assert_eq!((v.marshal_interface)(m, null_mut(), &IID_CALLBACK, null_mut(), 3, null_mut(), 0), E_OUTOFMEMORY);
        assert_eq!((v.unmarshal_interface)(m, null_mut(), &IID_CALLBACK, &mut out), E_OUTOFMEMORY);
        assert_eq!((v.release_marshal_data)(m, null_mut()), E_OUTOFMEMORY);
        assert_eq!((v.disconnect_object)(m, 0), E_OUTOFMEMORY);
        release(base(cb));
    }
}

#[test]
fn weak_reference_resolves_while_alive_and_is_null_after_release() {
    let _g = serial();
    unsafe {
        let cb = new_callback(false);
        let ws = off(cb, OFF_WEAK_SOURCE);
        let v: &WeakSourceVtbl = vt(ws);
        let mut w = null_mut();
        assert_eq!((v.get_weak_reference)(ws, &mut w), S_OK);
        assert!(!w.is_null());
        let mut w2 = null_mut();
        assert_eq!((v.get_weak_reference)(ws, &mut w2), S_OK);
        assert_eq!(w, w2);
        assert_eq!(release(w2), 2);
        assert_eq!(qi(w, &IID_IWEAKREFERENCE), (S_OK, w));
        assert_eq!(qi(w, &IID_IUNKNOWN), (S_OK, w));
        assert_eq!(qi(w, &IID_CALLBACK), (E_NOINTERFACE, null_mut()));
        assert_eq!(strong_count(cb), 1);
        assert_eq!(add_ref(base(cb)), 2);
        assert_eq!(release(base(cb)), 1);
        let rv: &WeakRefVtbl = vt(w);
        let mut o = null_mut();
        assert_eq!((rv.resolve)(w, &IID_CALLBACK, &mut o), S_OK);
        assert_eq!(o, off(cb, OFF_CALLBACK));
        assert_eq!(strong_count(cb), 2);
        release(o);
        let mut bad = 1usize as *mut c_void;
        assert_eq!((rv.resolve)(w, &IID_IWEAKREFERENCE, &mut bad), E_NOINTERFACE);
        assert!(bad.is_null());
        assert_eq!(strong_count(cb), 1);
        assert_eq!(release(base(cb)), 0);
        let mut dead = 1usize as *mut c_void;
        assert_eq!((rv.resolve)(w, &IID_CALLBACK, &mut dead), S_OK);
        assert!(dead.is_null());
        assert_eq!(release(w), 0);
    }
}

#[repr(C)]
struct FakeHooks {
    vtbl: *const [usize; 5],
    refreshes: AtomicUsize,
    attached: AtomicUsize,
}

unsafe extern "system" fn fh_qi(_t: *mut c_void, _i: *const Guid, o: *mut *mut c_void) -> i32 {
    *o = null_mut();
    E_NOINTERFACE
}
unsafe extern "system" fn fh_ref(_t: *mut c_void) -> u32 {
    1
}
unsafe extern "system" fn fh_attach(t: *mut FakeHooks, cb: *mut c_void) -> i32 {
    add_ref(cb);
    let old = (*t).attached.swap(cb as usize, Ordering::SeqCst);
    if old != 0 {
        release(old as *mut c_void);
    }
    S_OK
}
unsafe extern "system" fn fh_refresh(t: *mut FakeHooks) -> i32 {
    (*t).refreshes.fetch_add(1, Ordering::SeqCst);
    E_OUTOFMEMORY
}

fn fake() -> &'static FakeHooks {
    let v: &'static [usize; 5] = Box::leak(Box::new([fh_qi as *const () as usize, fh_ref as *const () as usize, fh_ref as *const () as usize, fh_attach as *const () as usize, fh_refresh as *const () as usize]));
    Box::leak(Box::new(FakeHooks { vtbl: v, refreshes: AtomicUsize::new(0), attached: AtomicUsize::new(0) }))
}

#[test]
fn set_hooks_hands_the_callback_interface_at_0x30_and_refresh_uses_slot_4() {
    let _g = serial();
    reset_for_tests();
    let f = fake();
    unsafe {
        assert_eq!(mux_MinUserXP_SetHooks(f as *const FakeHooks as *mut c_void), S_OK);
        let cb = f.attached.load(Ordering::SeqCst) as *mut c_void;
        assert_eq!(qi(cb, &IID_CALLBACK), (S_OK, cb));
        assert_eq!(qi(cb, &IID_IUNKNOWN).1 as usize + OFF_CALLBACK, cb as usize);
        let v: &CallbackVtbl = vt(cb);
        assert_eq!((v.refresh_cursor)(cb), E_OUTOFMEMORY);
        assert_eq!(f.refreshes.load(Ordering::SeqCst), 1);
        mux_MinUserXP_SetHooks(null_mut());
        assert_eq!((v.refresh_cursor)(cb), S_OK);
        assert_eq!(f.refreshes.load(Ordering::SeqCst), 1);
    }
    reset_for_tests();
}

