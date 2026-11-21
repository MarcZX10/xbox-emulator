use crate::crt::_wcsnicmp;
use crate::stl::{operator_delete, try_operator_new, wcslen, AString, WString};
use core::ffi::c_void;

pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
const CP_UTF8: u32 = 65001;

#[link(name = "kernel32")]
extern "system" {
    fn MultiByteToWideChar(cp: u32, flags: u32, src: *const u8, cb: i32, dst: *mut u16, cch: i32) -> i32;
    fn WideCharToMultiByte(cp: u32, flags: u32, src: *const u16, cch: i32, dst: *mut u8, cb: i32, default: *const u8, used: *mut i32) -> i32;
    fn RaiseException(code: u32, flags: u32, n: u32, args: *const usize);
}

#[repr(C)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

#[repr(C)]
struct StringHeader {
    reserved: [usize; 3],
}

#[link(name = "runtimeobject")]
extern "system" {
    fn WindowsCreateStringReference(src: *const u16, len: u32, header: *mut StringHeader, out: *mut *mut c_void) -> i32;
    fn WindowsDeleteString(s: *mut c_void) -> i32;
    fn RoGetActivationFactory(class: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32;
}

pub const IID_ICRYPTOGRAPHIC_BUFFER_STATICS: Guid = Guid(0x320b_7e22, 0x3cb0, 0x4cdf, [0x86, 0x63, 0x1d, 0x28, 0x91, 0x00, 0x65, 0xeb]);

type Release = unsafe extern "system" fn(*mut c_void) -> u32;
type DecodeFromBase64String = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> i32;
type ConvertBinaryToString = unsafe extern "system" fn(*mut c_void, i32, *mut c_void, *mut *mut c_void) -> i32;

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn release(p: &mut *mut c_void) {
    let obj = *p;
    if !obj.is_null() {
        *p = core::ptr::null_mut();
        core::mem::transmute::<usize, Release>(slot(obj, 2))(obj);
    }
}

#[export_name = "?SubStringInsensitive@@YAPEBGPEBG0@Z"]
pub unsafe extern "C" fn sub_string_insensitive(haystack: *const u16, needle: *const u16) -> *const u16 {
    if needle.is_null() || *needle == 0 || haystack.is_null() || *haystack == 0 {
        return core::ptr::null();
    }
    let n = wcslen(needle);
    let mut p = haystack;
    while *p != 0 {
        if _wcsnicmp(p, needle, n) == 0 {
            return p;
        }
        p = p.add(1);
    }
    core::ptr::null()
}

#[export_name = "?ConvertUtf8ToWide@@YAJPEBDKAEAV?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@@Z"]
pub unsafe extern "C" fn convert_utf8_to_wide(src: *const u8, cb: u32, out: *mut WString) -> i32 {
    WString::clear(out);
    if src.is_null() || cb == 0 || *src == 0 {
        return 0;
    }
    let n = MultiByteToWideChar(CP_UTF8, 0, src, cb as i32, core::ptr::null_mut(), 0) as isize as usize;
    let bytes = n.checked_mul(2).unwrap_or(usize::MAX);
    let tmp = try_operator_new(bytes) as *mut u16;
    if tmp.is_null() {
        return E_OUTOFMEMORY;
    }
    MultiByteToWideChar(CP_UTF8, 0, src, cb as i32, tmp, n as i32);
    WString::assign(out, tmp, n);
    operator_delete(tmp as *mut u8);
    0
}

#[export_name = "?ConvertWideToUtf8@@YAJPEBGKAEAV?$basic_string@DU?$char_traits@D@std@@V?$allocator@D@2@@std@@@Z"]
pub unsafe extern "C" fn convert_wide_to_utf8(src: *const u16, cch: u32, out: *mut AString) -> i32 {
    AString::clear(out);
    if src.is_null() || cch == 0 || *src == 0 {
        return 0;
    }
    let n = WideCharToMultiByte(CP_UTF8, 0, src, cch as i32, core::ptr::null_mut(), 0, core::ptr::null(), core::ptr::null_mut()) as isize as usize;
    let tmp = try_operator_new(n);
    if tmp.is_null() {
        return E_OUTOFMEMORY;
    }
    WideCharToMultiByte(CP_UTF8, 0, src, cch as i32, tmp, n as i32, core::ptr::null(), core::ptr::null_mut());
    AString::assign(out, tmp, n);
    operator_delete(tmp);
    0
}

#[export_name = "?DecodeBase64String@@YAJPEBGPEAPEAUHSTRING__@@@Z"]
pub unsafe extern "C" fn decode_base64_string(base64: *const u16, out: *mut *mut c_void) -> i32 {
    if base64.is_null() {
        return E_INVALIDARG;
    }
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let mut result: *mut c_void = core::ptr::null_mut();
    let mut factory: *mut c_void = core::ptr::null_mut();
    let mut buffer: *mut c_void = core::ptr::null_mut();
    let mut header = StringHeader { reserved: [0; 3] };
    let mut class: *mut c_void = core::ptr::null_mut();
    let class_name = crate::w!("Windows.Security.Cryptography.CryptographicBuffer");
    if WindowsCreateStringReference(class_name.as_ptr(), 0x31, &mut header, &mut class) < 0 {
        RaiseException(0xc000_000d, 1, 0, core::ptr::null());
    }
    let mut hr = RoGetActivationFactory(class, &IID_ICRYPTOGRAPHIC_BUFFER_STATICS, &mut factory);
    if hr >= 0 {
        let decode: DecodeFromBase64String = core::mem::transmute(slot(factory, 13));
        let mut len = wcslen(base64);
        if len > u32::MAX as usize {
            len = u32::MAX as usize;
            RaiseException(0xc000_000d, 1, 0, core::ptr::null());
        }
        let mut input_header = StringHeader { reserved: [0; 3] };
        let mut input: *mut c_void = core::ptr::null_mut();
        WindowsCreateStringReference(base64, len as u32, &mut input_header, &mut input);
        hr = decode(factory, input, &mut buffer);
        if hr >= 0 {
            let convert: ConvertBinaryToString = core::mem::transmute(slot(factory, 16));
            hr = convert(factory, 0, buffer, &mut result);
            if hr >= 0 {
                *out = result;
                result = core::ptr::null_mut();
                hr = 0;
            }
        }
    }
    release(&mut buffer);
    release(&mut factory);
    if !result.is_null() {
        WindowsDeleteString(result);
    }
    hr
}
