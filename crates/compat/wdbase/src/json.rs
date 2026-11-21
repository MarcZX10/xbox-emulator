use crate::stl::WString;
use crate::w;
use core::ffi::c_void;

pub const CLASS_E_CLASSNOTAVAILABLE: i32 = 0x8004_0111u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const STATUS_ACCESS_VIOLATION: u32 = 0xc000_0005;
const STATUS_INVALID_PARAMETER: u32 = 0xc000_000d;

#[repr(C)]
struct StringHeader {
    reserved: [usize; 3],
}

#[link(name = "runtimeobject")]
extern "system" {
    fn WindowsCreateStringReference(src: *const u16, len: u32, header: *mut StringHeader, out: *mut *mut c_void) -> i32;
    fn WindowsIsStringEmpty(s: *mut c_void) -> i32;
    fn WindowsStringHasEmbeddedNull(s: *mut c_void, has: *mut i32) -> i32;
    fn WindowsGetStringRawBuffer(s: *mut c_void, len: *mut u32) -> *const u16;
    fn RoOriginateError(hr: i32, message: *mut c_void) -> i32;
    fn RoOriginateErrorW(hr: i32, len: u32, message: *const u16) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn InitOnceExecuteOnce(once: *mut usize, callback: InitOnceCallback, parameter: *mut c_void, context: *mut *mut c_void) -> i32;
    fn RaiseException(code: u32, flags: u32, n: u32, args: *const usize);
}

type InitOnceCallback = unsafe extern "system" fn(*mut usize, *mut c_void, *mut *mut c_void) -> i32;

#[repr(C)]
pub struct ModuleTable {
    pub creator_first: usize,
    pub creator_last: usize,
}

pub static MODULE_TABLE: ModuleTable = ModuleTable { creator_first: 0, creator_last: 0 };

static mut MODULE_ONCE: usize = 0;
pub static mut MODULE: *const ModuleTable = core::ptr::null();
pub static mut MODULE_CREATED: u8 = 0;

unsafe extern "system" fn create_module(_: *mut usize, _: *mut c_void, _: *mut *mut c_void) -> i32 {
    MODULE_CREATED = 1;
    MODULE = &MODULE_TABLE;
    1
}

pub unsafe fn module_creators() -> (*const usize, *const usize) {
    let first = core::ptr::addr_of!((*MODULE).creator_first);
    let last = core::ptr::addr_of!((*MODULE).creator_last);
    (first.add(1), last)
}

pub unsafe fn get_activation_factory(class: *mut c_void, factory: *mut *mut c_void) -> i32 {
    InitOnceExecuteOnce(core::ptr::addr_of_mut!(MODULE_ONCE), create_module, core::ptr::null_mut(), core::ptr::null_mut());
    MODULE_CREATED = 1;
    *factory = core::ptr::null_mut();
    let mut embedded = 0i32;
    if WindowsIsStringEmpty(class) != 0 || WindowsStringHasEmbeddedNull(class, &mut embedded) < 0 || embedded == 1 {
        RoOriginateErrorW(E_INVALIDARG, 0x12, w!("activatibleClassId").as_ptr());
        return E_INVALIDARG;
    }
    WindowsGetStringRawBuffer(class, core::ptr::null_mut());
    RoOriginateError(CLASS_E_CLASSNOTAVAILABLE, class);
    CLASS_E_CLASSNOTAVAILABLE
}

unsafe fn class_reference(name: &[u16], header: *mut StringHeader) -> *mut c_void {
    let mut class: *mut c_void = core::ptr::null_mut();
    if WindowsCreateStringReference(name.as_ptr(), (name.len() - 1) as u32, header, &mut class) < 0 {
        RaiseException(STATUS_INVALID_PARAMETER, 1, 0, core::ptr::null());
    }
    class
}

pub unsafe fn get_json_value_statics(_statics: *mut *mut c_void) -> i32 {
    let mut header = StringHeader { reserved: [0; 3] };
    let class = class_reference(w!("Windows.Data.Json.JsonValue"), &mut header);
    let mut factory: *mut c_void = core::ptr::null_mut();
    get_activation_factory(class, &mut factory)
}

pub unsafe fn get_json_object_statics(_statics: *mut *mut c_void) -> i32 {
    let mut header = StringHeader { reserved: [0; 3] };
    let class = class_reference(w!("Windows.Data.Json.JsonObject"), &mut header);
    let mut factory: *mut c_void = core::ptr::null_mut();
    get_activation_factory(class, &mut factory)
}

pub unsafe fn set_named_string(_name: *const u16, _value: *const u16, _object: *mut *mut c_void) -> i32 {
    let mut statics: *mut c_void = core::ptr::null_mut();
    get_json_value_statics(&mut statics)
}

#[export_name = "?PostSshPublicKey@RestContext@@QEAAKV?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@0AEAV23@1AEA_N1@Z"]
pub unsafe extern "C" fn post_ssh_public_key(
    _this: *mut crate::rest::RestContext,
    _user: *mut WString,
    _password: *mut WString,
    ssh_user: *mut WString,
    public_key: *mut WString,
    authorized: *mut u8,
    out: *mut WString,
) -> u32 {
    WString::clear(out);
    *authorized = 0;
    let mut object: *mut c_void = core::ptr::null_mut();
    let mut header = StringHeader { reserved: [0; 3] };
    let class = class_reference(w!("Windows.Data.Json.JsonObject"), &mut header);
    let mut factory: *mut c_void = core::ptr::null_mut();
    get_activation_factory(class, &mut factory);
    set_named_string(w!("AuthorizeSSHUser").as_ptr(), WString::data(ssh_user), &mut object);
    set_named_string(w!("SSHPublicKey").as_ptr(), WString::data(public_key), &mut object);
    let info = [0usize, object as usize];
    loop {
        RaiseException(STATUS_ACCESS_VIOLATION, 0, 2, info.as_ptr());
    }
}
