use core::ffi::c_void;
use core::ptr::null_mut;

pub type Hkey = *mut c_void;

#[link(name = "kernel32")]
extern "system" {
    fn PackageFamilyNameFromFullName(full: *const u16, len: *mut u32, family: *mut u16) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenCurrentUser(sam: u32, out: *mut Hkey) -> i32;
    fn RegOpenKeyExW(key: Hkey, sub: *const u16, opts: u32, sam: u32, out: *mut Hkey) -> i32;
    fn RegQueryValueExW(key: Hkey, name: *const u16, res: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    fn RegCloseKey(key: Hkey) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct FileTime {
    pub low: u32,
    pub high: u32,
}

pub const ERROR_FILE_NOT_FOUND: i32 = 2;
pub const ERROR_INVALID_PARAMETER: i32 = 0x57;
pub const ERROR_NOT_FOUND: i32 = 0x490;
pub const ERROR_REGISTRY_CORRUPT_TYPE: i32 = 0x3d55;
pub const KEY_READ: u32 = 0x20019;
pub const REG_QWORD: u32 = 0xb;
pub const PATH_CAP: usize = 0x128;
pub const FAMILIES: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Families";

unsafe fn wlen(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn install_time_under(root: Hkey, prefix: &str, full: *const u16, out: *mut FileTime) -> i32 {
    if full.is_null() || out.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let mut family = [0u16; 65];
    let mut flen = family.len() as u32;
    let r = PackageFamilyNameFromFullName(full, &mut flen, family.as_mut_ptr());
    if r != 0 {
        return r;
    }
    let mut path: Vec<u16> = prefix.encode_utf16().collect();
    path.push(b'\\' as u16);
    path.extend_from_slice(&family[..wlen(family.as_ptr())]);
    path.push(b'\\' as u16);
    path.extend_from_slice(core::slice::from_raw_parts(full, wlen(full)));
    if path.len() >= PATH_CAP {
        return ERROR_INVALID_PARAMETER;
    }
    path.push(0);
    let mut key: Hkey = null_mut();
    let r = RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut key);
    if r != 0 {
        return if r == ERROR_FILE_NOT_FOUND { ERROR_NOT_FOUND } else { r };
    }
    let name: Vec<u16> = "InstallTime".encode_utf16().chain(Some(0)).collect();
    let mut ty = 0u32;
    let mut data = 0u64;
    let mut len = 8u32;
    let r = RegQueryValueExW(key, name.as_ptr(), null_mut(), &mut ty, &mut data as *mut u64 as *mut u8, &mut len);
    RegCloseKey(key);
    if r != 0 {
        return r;
    }
    if ty != REG_QWORD {
        return ERROR_REGISTRY_CORRUPT_TYPE;
    }
    (*out).low = data as u32;
    (*out).high = (data >> 32) as u32;
    0
}

#[no_mangle]
pub unsafe extern "system" fn GetPackageInstallTime(full: *const u16, out: *mut FileTime) -> i32 {
    if full.is_null() || out.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let mut user: Hkey = null_mut();
    let r = RegOpenCurrentUser(KEY_READ, &mut user);
    if r != 0 {
        return r;
    }
    let r = install_time_under(user, FAMILIES, full, out);
    RegCloseKey(user);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(key: Hkey, sub: *const u16, r: u32, class: *const u16, opts: u32, sam: u32, sa: *const c_void, out: *mut Hkey, disp: *mut u32) -> i32;
        fn RegSetValueExW(key: Hkey, name: *const u16, r: u32, ty: u32, data: *const u8, len: u32) -> i32;
        fn RegDeleteTreeW(key: Hkey, sub: *const u16) -> i32;
    }

    const HKCU: Hkey = 0x8000_0001usize as Hkey;
    const FULL: &str = "TomBox.Test_1.0.0.0_x64__8wekyb3d8bbwe";
    const FAMILY: &str = "TomBox.Test_8wekyb3d8bbwe";

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn seed(prefix: &str, ty: u32, data: &[u8]) {
        unsafe {
            let mut k: Hkey = null_mut();
            let sub = w(&format!("{prefix}\\{FAMILY}\\{FULL}"));
            assert_eq!(RegCreateKeyExW(HKCU, sub.as_ptr(), 0, null(), 0, 0xf003f, null(), &mut k, null_mut()), 0);
            assert_eq!(RegSetValueExW(k, w("InstallTime").as_ptr(), 0, ty, data.as_ptr(), data.len() as u32), 0);
            RegCloseKey(k);
        }
    }

    fn wipe(prefix: &str) {
        unsafe { RegDeleteTreeW(HKCU, w(prefix).as_ptr()) };
    }

    #[test]
    fn reads_qword_from_families_key() {
        let p = format!("Software\\TomBoxTest-{}-a", std::process::id());
        seed(&p, REG_QWORD, &0x01d9_1234_5678_9abcu64.to_le_bytes());
        let mut ft = FileTime::default();
        let r = unsafe { install_time_under(HKCU, &p, w(FULL).as_ptr(), &mut ft) };
        wipe(&p);
        assert_eq!(r, 0);
        assert_eq!(ft, FileTime { low: 0x5678_9abc, high: 0x01d9_1234 });
    }

    #[test]
    fn wrong_type_is_corrupt_type() {
        let p = format!("Software\\TomBoxTest-{}-b", std::process::id());
        seed(&p, 4, &7u32.to_le_bytes());
        let mut ft = FileTime { low: 9, high: 9 };
        let r = unsafe { install_time_under(HKCU, &p, w(FULL).as_ptr(), &mut ft) };
        wipe(&p);
        assert_eq!(r, ERROR_REGISTRY_CORRUPT_TYPE);
        assert_eq!(ft, FileTime { low: 9, high: 9 });
    }

    #[test]
    fn absent_key_is_not_found() {
        let p = format!("Software\\TomBoxTest-{}-c", std::process::id());
        let mut ft = FileTime::default();
        assert_eq!(unsafe { install_time_under(HKCU, &p, w(FULL).as_ptr(), &mut ft) }, ERROR_NOT_FOUND);
    }

    #[test]
    fn overlong_path_is_invalid_parameter() {
        let p = "x".repeat(PATH_CAP);
        let mut ft = FileTime::default();
        assert_eq!(unsafe { install_time_under(HKCU, &p, w(FULL).as_ptr(), &mut ft) }, ERROR_INVALID_PARAMETER);
    }
}
