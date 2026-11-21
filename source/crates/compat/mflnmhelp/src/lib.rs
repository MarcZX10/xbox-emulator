use core::ffi::c_void;

pub const ERROR_CALL_NOT_IMPLEMENTED: i32 = 0x78;
pub const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
pub const DISP_E_BADVARTYPE: i32 = 0x8002_0008u32 as i32;
pub const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;
pub const CONSOLE_PROV_HANDLE: usize = 6;
pub const RECORD_FREE_ATTRIBUTES: u64 = 0x0482_0000;

const VT_BSTR: u16 = 8;
const VT_DISPATCH: u16 = 9;
const VT_UNKNOWN: u16 = 13;
const VT_RECORD: u16 = 36;
const VT_CLSID: u16 = 72;
const VT_ARRAY: u16 = 0x2000;
const VALID_VT_MASK: u64 = 0x10_00ff_7ffc;

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyW(key: isize, sub: *const u16, out: *mut isize) -> i32;
    fn RegQueryValueExW(key: isize, name: *const u16, reserved: *mut u32, ty: *mut u32, data: *mut u8, cb: *mut u32) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(alg: *mut c_void, buf: *mut u8, len: u32, flags: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
}

#[no_mangle]
pub unsafe extern "system" fn CryptAcquireContextA(prov: *mut usize, _container: *const u8, _provider: *const u8, _ty: u32, _flags: u32) -> i32 {
    if prov.is_null() {
        return 0;
    }
    *prov = CONSOLE_PROV_HANDLE;
    1
}

#[no_mangle]
pub unsafe extern "system" fn CryptGenRandom(_prov: usize, len: u32, buf: *mut u8) -> i32 {
    if buf.is_null() || len == 0 {
        return 0;
    }
    (BCryptGenRandom(core::ptr::null_mut(), buf, len, BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0) as i32
}

#[no_mangle]
pub extern "system" fn CryptReleaseContext(_prov: usize, _flags: u32) -> i32 {
    1
}

#[no_mangle]
pub extern "system" fn GetProfileIntW(_app: *const u16, _key: *const u16, default: i32) -> u32 {
    default as u32
}

#[no_mangle]
pub extern "system" fn GlobalFree(_mem: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "system" fn GlobalLock(_mem: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "system" fn GlobalUnlock(_mem: *mut c_void) -> i32 {
    0
}

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(".globl PropVariantToVariant", "PropVariantToVariant:", "ret");

#[cfg(target_arch = "x86_64")]
extern "system" {
    pub fn PropVariantToVariant(pv: *const c_void, var: *mut c_void) -> i32;
}

#[no_mangle]
pub extern "system" fn RegDeleteTreeW(_key: isize, _sub: *const u16) -> i32 {
    ERROR_CALL_NOT_IMPLEMENTED
}

#[no_mangle]
pub unsafe extern "system" fn RegGetValueW(key: isize, sub: *const u16, value: *const u16, _flags: u32, ty: *mut u32, data: *mut c_void, cb: *mut u32) -> i32 {
    if !sub.is_null() && *sub != 0 {
        let mut k: isize = 0;
        let r = RegOpenKeyW(key, sub, &mut k);
        if r != 0 {
            return r;
        }
        let r = RegQueryValueExW(k, value, core::ptr::null_mut(), ty, data.cast(), cb);
        RegCloseKey(k);
        return r;
    }
    RegQueryValueExW(key, value, core::ptr::null_mut(), ty, data.cast(), cb)
}

#[no_mangle]
pub extern "system" fn RegNotifyChangeKeyValue(_key: isize, _subtree: i32, _filter: u32, _event: isize, _async: i32) -> i32 {
    ERROR_CALL_NOT_IMPLEMENTED
}

#[no_mangle]
pub extern "system" fn VariantChangeType(_dst: *mut c_void, _src: *const c_void, _flags: u16, _vt: u16) -> i32 {
    E_NOTIMPL
}

unsafe fn vcall(obj: *mut c_void, slot: usize) {
    let vt = *(obj as *const *const usize);
    let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*vt.add(slot));
    f(obj);
}

unsafe fn vcall2(obj: *mut c_void, slot: usize, a: *mut c_void) {
    let vt = *(obj as *const *const usize);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 = core::mem::transmute(*vt.add(slot));
    f(obj, a);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub unsafe fn xmem_free(p: *mut c_void, attributes: u64) -> bool {
    if p.is_null() {
        return true;
    }
    let name = wide("xmem.dll");
    let mut m = GetModuleHandleW(name.as_ptr());
    if m == 0 {
        m = LoadLibraryW(name.as_ptr());
    }
    if m == 0 {
        return false;
    }
    let f = GetProcAddress(m, b"XMemFree\0".as_ptr());
    if f.is_null() {
        return false;
    }
    let f: unsafe extern "system" fn(*mut c_void, u64) = core::mem::transmute(f);
    f(p, attributes);
    true
}

#[no_mangle]
pub unsafe extern "system" fn VariantClear(var: *mut c_void) -> i32 {
    let vtp = var as *mut u16;
    let vt = *vtp;
    if vt >= VT_BSTR {
        if vt & 0x9fff == VT_CLSID {
            return E_NOTIMPL;
        }
        let flags = vt & 0x6000;
        if vt >= 0xc {
            let base = vt & 0x9fff;
            if base > VT_RECORD || VALID_VT_MASK & (1u64 << base) == 0 {
                return DISP_E_BADVARTYPE;
            }
        }
        let p8 = *((var as *mut u8).add(8) as *mut *mut c_void);
        let p16 = *((var as *mut u8).add(16) as *mut *mut c_void);
        match vt {
            VT_BSTR => return E_NOTIMPL,
            VT_DISPATCH | VT_UNKNOWN => {
                if !p8.is_null() {
                    vcall(p8, 2);
                }
            }
            VT_RECORD => {
                if !p16.is_null() {
                    if !p8.is_null() {
                        vcall2(p16, 4, p8);
                    }
                    vcall(p16, 2);
                    xmem_free(p8, RECORD_FREE_ATTRIBUTES);
                }
            }
            _ => {
                if flags == VT_ARRAY {
                    return E_NOTIMPL;
                }
            }
        }
    }
    *vtp = 0;
    0
}

#[no_mangle]
pub unsafe extern "system" fn VariantInit(var: *mut c_void) {
    *(var as *mut u16) = 0;
}

#[no_mangle]
pub unsafe extern "system" fn timeGetTime() -> u32 {
    GetTickCount()
}

#[cfg(test)]
mod tests;
