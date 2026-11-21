use super::*;

const CONSOLE: &str = r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\gamecoreclient.dll";

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

#[link(name = "runtimeobject")]
extern "system" {
    fn WindowsCreateString(s: *const u16, len: u32, out: *mut Hstring) -> i32;
    fn WindowsDeleteString(s: Hstring) -> i32;
    fn GetRestrictedErrorInfo(info: *mut *mut c_void) -> i32;
}

#[link(name = "oleaut32")]
extern "system" {
    fn SysFreeString(s: *mut u16);
    fn SysStringLen(s: *mut u16) -> u32;
}

struct Console {
    module: *mut c_void,
}

impl Console {
    fn load() -> Option<Console> {
        if !std::path::Path::new(CONSOLE).is_file() {
            eprintln!("{CONSOLE} absent, oracle skipped");
            return None;
        }
        let w: Vec<u16> = CONSOLE.encode_utf16().chain(Some(0)).collect();
        let h = unsafe { LoadLibraryW(w.as_ptr()) };
        assert!(!h.is_null(), "console gamecoreclient.dll must load on the PC for the oracle");
        Some(Console { module: h })
    }
    unsafe fn sym<T: Copy>(&self, n: &str) -> T {
        let c = format!("{n}\0");
        let p = GetProcAddress(self.module, c.as_ptr());
        assert!(!p.is_null(), "{n}");
        core::mem::transmute_copy(&p)
    }
}

fn hstring(s: &[u16]) -> Hstring {
    let mut h = core::ptr::null_mut();
    assert_eq!(unsafe { WindowsCreateString(s.as_ptr(), s.len() as u32, &mut h) }, S_OK);
    h
}

fn take_error_message() -> Option<(i32, String)> {
    let mut info = core::ptr::null_mut();
    if unsafe { GetRestrictedErrorInfo(&mut info) } != S_OK || info.is_null() {
        return None;
    }
    unsafe {
        let vt = *(info as *const *const usize);
        let details: unsafe extern "system" fn(*mut c_void, *mut *mut u16, *mut i32, *mut *mut u16, *mut *mut u16) -> i32 = core::mem::transmute(*vt.add(3));
        let mut d = core::ptr::null_mut();
        let mut e = 0;
        let mut r = core::ptr::null_mut();
        let mut sid = core::ptr::null_mut();
        assert_eq!(details(info, &mut d, &mut e, &mut r, &mut sid), S_OK);
        let msg = if r.is_null() { String::new() } else { String::from_utf16_lossy(core::slice::from_raw_parts(r, SysStringLen(r) as usize)) };
        for b in [d, r, sid] {
            if !b.is_null() {
                SysFreeString(b);
            }
        }
        let release: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*vt.add(2));
        release(info);
        Some((e, msg))
    }
}

type Activation = unsafe extern "system" fn(Hstring, *mut *mut c_void) -> i32;
type ClassObject = unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32;

fn activation_cases(f: Activation) -> Vec<(i32, bool, Option<(i32, String)>)> {
    let name: Vec<u16> = "Windows.Xbox.GameCore.Client".encode_utf16().collect();
    let embedded: Vec<u16> = "Game\0Core".encode_utf16().collect();
    let mut out = Vec::new();
    for s in [core::ptr::null_mut(), hstring(&name), hstring(&embedded)] {
        take_error_message();
        let mut factory = 0x1234usize as *mut c_void;
        let hr = unsafe { f(s, &mut factory) };
        out.push((hr, factory.is_null(), take_error_message()));
        if !s.is_null() {
            unsafe { WindowsDeleteString(s) };
        }
    }
    out
}

#[test]
fn dll_get_activation_factory_has_no_activatable_classes() {
    let ours = activation_cases(DllGetActivationFactory);
    assert_eq!(ours[0].0, E_INVALIDARG);
    assert_eq!(ours[1].0, CLASS_E_CLASSNOTAVAILABLE);
    assert_eq!(ours[2].0, E_INVALIDARG);
    assert!(ours.iter().all(|r| r.1), "factory is cleared on every path");
    if let Some((e, msg)) = &ours[0].2 {
        assert_eq!(*e, E_INVALIDARG);
        assert_eq!(msg, ACTIVATION_ARG_NAME);
    }
    if let Some(c) = Console::load() {
        let theirs = activation_cases(unsafe { c.sym::<Activation>("DllGetActivationFactory") });
        assert_eq!(ours, theirs);
    }
}

fn class_cases(f: ClassObject) -> Vec<(i32, bool)> {
    let unknown = Guid { data1: 0x0BADC0DE, data2: 1, data3: 2, data4: [3, 4, 5, 6, 7, 8, 9, 10] };
    let iid_class_factory = Guid { data1: 1, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
    let mut ppv = 0x1234usize as *mut c_void;
    let hr = unsafe { f(&unknown, &iid_class_factory, &mut ppv) };
    vec![(hr, ppv.is_null())]
}

#[test]
fn dll_get_class_object_rejects_unknown_clsids_like_console() {
    let ours = class_cases(DllGetClassObject);
    assert_eq!(ours, vec![(CLASS_E_CLASSNOTAVAILABLE, true)]);
    let mut ppv = 0x1234usize as *mut c_void;
    let iid = Guid { data1: 1, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
    for c in COM_CLASSES {
        assert_eq!(unsafe { DllGetClassObject(&c, &iid, &mut ppv) }, E_NOTIMPL);
        assert!(ppv.is_null());
    }
    if let Some(c) = Console::load() {
        assert_eq!(class_cases(unsafe { c.sym::<ClassObject>("DllGetClassObject") }), ours);
    }
}

#[test]
fn dll_can_unload_now_tracks_module_objects() {
    assert_eq!(DllCanUnloadNow(), S_OK);
    OBJECT_COUNT.fetch_add(1, Ordering::AcqRel);
    assert_eq!(DllCanUnloadNow(), S_FALSE);
    OBJECT_COUNT.fetch_sub(1, Ordering::AcqRel);
    assert_eq!(DllCanUnloadNow(), S_OK);
    if let Some(c) = Console::load() {
        let f = unsafe { c.sym::<unsafe extern "system" fn() -> i32>("DllCanUnloadNow") };
        assert_eq!(unsafe { f() }, S_OK);
    }
}
