use crate::json::*;
use crate::stl::{wide, WString};
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

#[link(name = "runtimeobject")]
extern "system" {
    fn RoInitialize(kind: i32) -> i32;
    fn WindowsCreateString(src: *const u16, len: u32, out: *mut *mut c_void) -> i32;
    fn WindowsDeleteString(s: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: unsafe extern "system" fn(*mut *mut u8) -> i32) -> *mut c_void;
    fn TerminateProcess(process: *mut c_void, code: u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

fn ws(s: &str) -> WString {
    let mut w = WString::new();
    unsafe { WString::assign_cstr(&mut w, wide(s).as_ptr()) };
    w
}

fn text(w: *const WString) -> String {
    unsafe { String::from_utf16(WString::as_slice(w)).unwrap() }
}

unsafe fn hstring(s: &[u16]) -> *mut c_void {
    let mut h: *mut c_void = core::ptr::null_mut();
    assert_eq!(WindowsCreateString(s.as_ptr(), s.len() as u32, &mut h), 0);
    h
}

#[test]
fn module_factory_cache() {
    unsafe {
        RoInitialize(1);
        let mut f: *mut c_void = 1 as *mut c_void;
        let class = hstring(&wide("Windows.Data.Json.JsonObject")[..28]);
        assert_eq!(get_activation_factory(class, &mut f), CLASS_E_CLASSNOTAVAILABLE);
        assert!(f.is_null());
        assert_eq!(core::ptr::addr_of!(MODULE_CREATED).read(), 1);
        assert_eq!(core::ptr::addr_of!(MODULE).read(), &MODULE_TABLE as *const ModuleTable);
        let (first, last) = module_creators();
        assert_eq!(first, last);
        WindowsDeleteString(class);
        f = 1 as *mut c_void;
        assert_eq!(get_activation_factory(core::ptr::null_mut(), &mut f), E_INVALIDARG);
        assert!(f.is_null());
        let embedded = hstring(&[0x61, 0, 0x62]);
        assert_eq!(get_activation_factory(embedded, &mut f), E_INVALIDARG);
        WindowsDeleteString(embedded);
        let value = hstring(&wide("Windows.Data.Json.JsonValue")[..27]);
        assert_eq!(get_activation_factory(value, &mut f), CLASS_E_CLASSNOTAVAILABLE);
        WindowsDeleteString(value);
    }
}

#[test]
fn json_statics_helpers() {
    unsafe {
        let mut s: *mut c_void = core::ptr::null_mut();
        assert_eq!(get_json_value_statics(&mut s), CLASS_E_CLASSNOTAVAILABLE);
        assert!(s.is_null());
        assert_eq!(get_json_object_statics(&mut s), CLASS_E_CLASSNOTAVAILABLE);
        assert!(s.is_null());
        let mut object: *mut c_void = core::ptr::null_mut();
        assert_eq!(set_named_string(wide("SSHPublicKey").as_ptr(), wide("ssh-rsa AAAA").as_ptr(), &mut object), CLASS_E_CLASSNOTAVAILABLE);
        assert!(object.is_null());
    }
}

const CHILD_ENV: &str = "WDBASE_POST_SSH_CHILD";
const CHILD_PASS: u32 = 0x5a;
const CHILD_MISMATCH: u32 = 0x5b;

static CHILD_OUT: AtomicPtr<WString> = AtomicPtr::new(core::ptr::null_mut());
static CHILD_FLAG: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static CHILD_USER: AtomicPtr<WString> = AtomicPtr::new(core::ptr::null_mut());

unsafe extern "system" fn child_handler(pointers: *mut *mut u8) -> i32 {
    let record = *pointers;
    let code = *(record as *const u32);
    if code != STATUS_ACCESS_VIOLATION {
        return 0;
    }
    let flags = *(record.add(4) as *const u32);
    let count = *(record.add(0x18) as *const u32);
    let info = core::slice::from_raw_parts(record.add(0x20) as *const usize, 2);
    let out = CHILD_OUT.load(Ordering::SeqCst);
    let flag = CHILD_FLAG.load(Ordering::SeqCst);
    let user = CHILD_USER.load(Ordering::SeqCst);
    let checks = [flags & 1 == 0, count == 2, info == [0, 0], (*out).size == 0 && *WString::data(out) == 0, *flag == 0, text(user) == "administrator-user-name"];
    let failed = checks.iter().enumerate().fold(0u32, |m, (i, &c)| if c { m } else { m | 1 << (i + 8) });
    TerminateProcess(GetCurrentProcess(), if failed == 0 { CHILD_PASS } else { CHILD_MISMATCH | failed });
    0
}

#[test]
#[ignore]
fn post_ssh_public_key_child() {
    if std::env::var(CHILD_ENV).as_deref() != Ok("1") {
        return;
    }
    unsafe {
        RoInitialize(1);
        let mut r: Box<crate::rest::RestContext> = Box::new(core::mem::zeroed());
        crate::rest::rest_context_ctor(&mut *r, core::ptr::null(), 0);
        let (mut u, mut pw) = (ws("administrator-user-name"), ws("pw"));
        let (mut name, mut key, mut out) = (ws("devuser"), ws("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI key"), ws("a stale response long enough"));
        let mut authorized = 1u8;
        CHILD_OUT.store(&mut out, Ordering::SeqCst);
        CHILD_FLAG.store(&mut authorized, Ordering::SeqCst);
        CHILD_USER.store(&mut u, Ordering::SeqCst);
        AddVectoredExceptionHandler(1, child_handler);
        post_ssh_public_key(&mut *r, &mut u, &mut pw, &mut name, &mut key, &mut authorized, &mut out);
    }
}

#[test]
fn post_ssh_public_key_faults_before_post() {
    let exe = std::env::current_exe().unwrap();
    let status = std::process::Command::new(exe)
        .args(["json_tests::post_ssh_public_key_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code().map(|c| c as u32), Some(CHILD_PASS));
}
