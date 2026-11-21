use core::ffi::c_void;

pub const ERROR_INVALID_NAME: u32 = 123;
pub const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
pub const FSCTL_PIPE_WAIT: u32 = 0x0011_0018;
pub const SUSPEND_EXEMPTIONS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\SuspendExemptions";

const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;
const KEY_ENUMERATE_SUB_KEYS: u32 = 8;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut c_void,
    name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_qos: *mut c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtOpenFile(h: *mut *mut c_void, access: u32, oa: *mut ObjectAttributes, iosb: *mut IoStatusBlock, share: u32, options: u32) -> i32;
    fn NtFsControlFile(
        h: *mut c_void,
        event: *mut c_void,
        apc: *mut c_void,
        ctx: *mut c_void,
        iosb: *mut IoStatusBlock,
        code: u32,
        input: *mut c_void,
        input_len: u32,
        output: *mut c_void,
        output_len: u32,
    ) -> i32;
    fn NtClose(h: *mut c_void) -> i32;
    fn RtlSetLastWin32ErrorAndNtStatusFromNtStatus(status: i32);
}

#[link(name = "kernel32")]
extern "system" {
    fn SetLastError(e: u32);
    fn GetCurrentPackageFullName(len: *mut u32, name: *mut u16) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(key: isize, sub: *const u16, options: u32, sam: u32, out: *mut isize) -> i32;
    fn RegEnumKeyExW(
        key: isize,
        index: u32,
        name: *mut u16,
        len: *mut u32,
        reserved: *mut u32,
        class: *mut u16,
        class_len: *mut u32,
        time: *mut u64,
    ) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}

static END_POINT_NAMESPACE: [u16; 10] = wstr(r"\SEVPipe\");
static LOCAL_EPMAPPER: [u16; 19] = wstr(r"\SEVPipe\epmapperX");
static REMOTE_EPMAPPER: [u16; 18] = wstr(r"\SEVPipe\epmapper");
static LOCAL_ALIAS: [u16; 9] = wstr("XBOX-ERA");
static REMOTE_ALIAS: [u16; 9] = wstr("XBOX-SRA");

const fn wstr<const N: usize>(s: &str) -> [u16; N] {
    let b = s.as_bytes();
    let mut out = [0u16; N];
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i] as u16;
        i += 1;
    }
    out
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

unsafe fn wlen(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

#[no_mangle]
pub extern "system" fn GetEndPointMapperCrossVmPipeNamespace() -> *const u16 {
    END_POINT_NAMESPACE.as_ptr()
}

#[no_mangle]
pub extern "system" fn GetLocalEndPointMapperCrossVmPipeName() -> *const u16 {
    LOCAL_EPMAPPER.as_ptr()
}

#[no_mangle]
pub extern "system" fn GetRemoteEndPointMapperCrossVmPipeName() -> *const u16 {
    REMOTE_EPMAPPER.as_ptr()
}

#[no_mangle]
pub extern "system" fn GetLocalVmAliasName() -> *const u16 {
    LOCAL_ALIAS.as_ptr()
}

#[no_mangle]
pub extern "system" fn GetRemoteVmAliasName() -> *const u16 {
    REMOTE_ALIAS.as_ptr()
}

#[no_mangle]
pub extern "system" fn GetLocalVmAliasNameLength() -> u32 {
    9
}

#[no_mangle]
pub extern "system" fn GetRemoteVmAliasNameLength() -> u32 {
    9
}

#[no_mangle]
pub extern "system" fn IsProcessRestrictedToSingleApartment() -> u32 {
    1
}

#[no_mangle]
pub extern "system" fn RemoteWinRTActivation() -> u32 {
    1
}

#[no_mangle]
pub extern "system" fn WinRTPerMachineSingleInstancing() -> u32 {
    1
}

#[no_mangle]
pub extern "system" fn ValidateRemoteVmNamedPipeEndpoint() -> u32 {
    0
}

#[no_mangle]
pub extern "system" fn OnInitializationOfSingleThreadedApartment() -> ! {
    unsafe { core::arch::asm!("int 0x29", in("ecx") 7u32, options(noreturn, nostack)) }
}

#[no_mangle]
pub unsafe extern "system" fn IsPackagedAppExemptFromSuspend(name: *const u16) -> i32 {
    let path = wide(SUSPEND_EXEMPTIONS);
    let mut key = 0isize;
    if RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_ENUMERATE_SUB_KEYS, &mut key) != 0 {
        return 0;
    }
    let mut buf = [0u16; 128];
    let want: &[u16] = if name.is_null() { &[] } else { core::slice::from_raw_parts(name, wlen(name)) };
    let mut found = 0;
    let mut index = 0u32;
    loop {
        let mut len = 0x7fu32;
        let r = RegEnumKeyExW(
            key,
            index,
            buf.as_mut_ptr(),
            &mut len,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        if r != 0 {
            break;
        }
        index += 1;
        let got = &buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())];
        if !name.is_null() && got == want {
            found = 1;
            break;
        }
    }
    RegCloseKey(key);
    found
}

#[no_mangle]
pub unsafe extern "system" fn IsCurrentProcessExemptFromSuspend() -> i32 {
    let mut buf = [0u16; 128];
    let mut len = 0x7fu32;
    GetCurrentPackageFullName(&mut len, buf.as_mut_ptr());
    IsPackagedAppExemptFromSuspend(buf.as_ptr())
}

pub fn pipe_wait_timeout(timeout: u32) -> Option<i64> {
    match timeout {
        0 => None,
        u32::MAX => Some(i64::MIN),
        t => Some((t as i64).wrapping_mul(-10_000)),
    }
}

#[no_mangle]
pub unsafe extern "system" fn WaitRemotePipe(name: *const u16, timeout: u32) -> i32 {
    let bs = b'\\' as u16;
    if name.is_null() || *name != bs || *name.add(1) != bs || *name.add(2) != b'.' as u16 || *name.add(3) != bs {
        SetLastError(ERROR_INVALID_NAME);
        return 0;
    }
    let mut sep = 4usize;
    loop {
        let c = *name.add(sep);
        if c == bs {
            break;
        }
        if c == 0 {
            SetLastError(ERROR_INVALID_NAME);
            return 0;
        }
        sep += 1;
    }
    if (sep - 3) * 2 + 0x18 > 0xffff {
        SetLastError(ERROR_INVALID_NAME);
        return 0;
    }
    let mut device: Vec<u16> = r"\DosDevices\".encode_utf16().collect();
    device.extend_from_slice(core::slice::from_raw_parts(name.add(4), sep - 3));
    let pipe = core::slice::from_raw_parts(name.add(sep + 1), wlen(name.add(sep + 1)));
    let name_bytes = pipe.len() * 2;
    if name_bytes > 0xffff {
        SetLastError(ERROR_INVALID_NAME);
        return 0;
    }
    let bytes = (device.len() * 2) as u16;
    let mut us = UnicodeString { length: bytes, maximum_length: bytes, buffer: device.as_mut_ptr() };
    let mut oa = ObjectAttributes {
        length: core::mem::size_of::<ObjectAttributes>() as u32,
        root: core::ptr::null_mut(),
        name: &mut us,
        attributes: 0x40,
        security_descriptor: core::ptr::null_mut(),
        security_qos: core::ptr::null_mut(),
    };
    let mut iosb = IoStatusBlock { status: 0, information: 0 };
    let mut h: *mut c_void = core::ptr::null_mut();
    let st = NtOpenFile(&mut h, 0x0010_0080, &mut oa, &mut iosb, 3, 0x20);
    drop(device);
    if st < 0 {
        RtlSetLastWin32ErrorAndNtStatusFromNtStatus(st);
        return 0;
    }
    let name_len = name_bytes as u16 as usize;
    let mut wait = vec![0u8; name_len + 0xe];
    if let Some(t) = pipe_wait_timeout(timeout) {
        wait[..8].copy_from_slice(&t.to_le_bytes());
        wait[0xc] = 1;
    }
    wait[8..12].copy_from_slice(&(name_len as u32).to_le_bytes());
    for (i, c) in pipe.iter().enumerate().take(name_len / 2) {
        wait[0xe + 2 * i..0x10 + 2 * i].copy_from_slice(&c.to_le_bytes());
    }
    let st = NtFsControlFile(
        h,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        &mut iosb,
        FSCTL_PIPE_WAIT,
        wait.as_mut_ptr() as *mut c_void,
        (name_len + 0xe) as u32,
        core::ptr::null_mut(),
        0,
    );
    NtClose(h);
    if st < 0 {
        RtlSetLastWin32ErrorAndNtStatusFromNtStatus(st);
        return 0;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORTS: &str = include_str!("../exports.txt");

    unsafe fn s(p: *const u16) -> String {
        String::from_utf16_lossy(core::slice::from_raw_parts(p, wlen(p)))
    }

    #[test]
    fn export_table_matches_reference_dll() {
        xone_exports::check_crate(env!("CARGO_MANIFEST_DIR"), "RemoteWinRT.dll", EXPORTS);
    }

    #[test]
    fn built_dll_has_reference_ordinals() {
        xone_exports::check_built(env!("CARGO_MANIFEST_DIR"), "remotewinrt.dll", EXPORTS);
    }

    #[test]
    fn names_match_console_strings() {
        unsafe {
            assert_eq!(s(GetEndPointMapperCrossVmPipeNamespace()), r"\SEVPipe\");
            assert_eq!(s(GetLocalEndPointMapperCrossVmPipeName()), r"\SEVPipe\epmapperX");
            assert_eq!(s(GetRemoteEndPointMapperCrossVmPipeName()), r"\SEVPipe\epmapper");
            assert_eq!(s(GetLocalVmAliasName()), "XBOX-ERA");
            assert_eq!(s(GetRemoteVmAliasName()), "XBOX-SRA");
        }
        assert_eq!(GetLocalVmAliasNameLength(), 9);
        assert_eq!(GetRemoteVmAliasNameLength(), 9);
        assert_eq!(IsProcessRestrictedToSingleApartment(), 1);
        assert_eq!(RemoteWinRTActivation(), 1);
        assert_eq!(WinRTPerMachineSingleInstancing(), 1);
        assert_eq!(ValidateRemoteVmNamedPipeEndpoint(), 0);
    }

    #[test]
    fn timeout_encoding_matches_console() {
        assert_eq!(pipe_wait_timeout(0), None);
        assert_eq!(pipe_wait_timeout(u32::MAX), Some(i64::MIN));
        assert_eq!(pipe_wait_timeout(250), Some(-2_500_000));
    }
}
