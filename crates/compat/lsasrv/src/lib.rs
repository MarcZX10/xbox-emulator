use core::ffi::c_void;
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};

pub const STATUS_SUCCESS: i32 = 0;
pub const STATUS_INVALID_PARAMETER: i32 = 0xC000_000Du32 as i32;
pub const STATUS_INVALID_SID: i32 = 0xC000_0078u32 as i32;
pub const EVENTLOG_AUDIT_SUCCESS: u32 = 8;
pub const EVENTLOG_AUDIT_FAILURE: u32 = 0x10;
pub const FIRST_SUBCATEGORY: u32 = 100;
pub const SUBCATEGORIES: usize = 64;

#[allow(clippy::declare_interior_mutable_const)]
const Z16: AtomicU16 = AtomicU16::new(0);
#[allow(clippy::declare_interior_mutable_const)]
const Z32: AtomicU32 = AtomicU32::new(0);

pub static POLICY: [AtomicU16; SUBCATEGORIES] = [Z16; SUBCATEGORIES];
pub static PER_USER_SUCCESS: [AtomicU32; SUBCATEGORIES] = [Z32; SUBCATEGORIES];
pub static PER_USER_ANY: [AtomicU32; SUBCATEGORIES] = [Z32; SUBCATEGORIES];

static DEFAULT_GUID: [u8; 16] = [0; 16];
static DEFAULT_SID: [u8; 68] = [0; 68];

#[link(name = "ntdll")]
extern "system" {
    fn RtlValidSid(sid: *const c_void) -> u8;
    fn RtlLengthSid(sid: *const c_void) -> u32;
}

fn slot(sub: u32) -> Option<usize> {
    let i = sub.wrapping_sub(FIRST_SUBCATEGORY) as usize;
    (i < SUBCATEGORIES).then_some(i)
}

fn policy(sub: u32) -> u16 {
    slot(sub).map_or(0, |i| POLICY[i].load(Ordering::Relaxed))
}

fn per_user(table: &[AtomicU32; SUBCATEGORIES], sub: u32) -> u32 {
    slot(sub).map_or(0, |i| table[i].load(Ordering::Relaxed))
}

#[no_mangle]
pub extern "system" fn LsaIUserProfileLoaded(_luid: *const c_void) -> u8 {
    0
}

#[no_mangle]
pub extern "system" fn LsapAdtAuditingEnabledBySubCategory(sub: u32, audit_type: u32) -> u8 {
    let p = policy(sub) as u8;
    match audit_type {
        EVENTLOG_AUDIT_SUCCESS => p & 1,
        EVENTLOG_AUDIT_FAILURE => p & 2,
        _ => 0,
    }
}

#[no_mangle]
pub extern "system" fn LsapAdtAuditingEnabledHint(sub: u32, audit_type: u32) -> u8 {
    let mut p = policy(sub) as u8;
    if per_user(&PER_USER_ANY, sub) != 0 {
        p = 3;
    }
    let m = if audit_type != EVENTLOG_AUDIT_SUCCESS { 2 } else { 1 };
    m & p
}

#[no_mangle]
pub unsafe extern "system" fn LsapAdtAuditingEnabledByLogonId(sub: u32, _logon_id: *const c_void, audit_type: u32, enabled: *mut u8) -> i32 {
    if policy(sub) == 0 && per_user(&PER_USER_SUCCESS, sub) == 0 {
        *enabled = 0;
        return STATUS_SUCCESS;
    }
    *enabled = LsapAdtAuditingEnabledBySubCategory(sub, audit_type);
    STATUS_SUCCESS
}

unsafe fn rd_u32(p: *const u8, off: usize) -> u32 {
    (p.add(off) as *const u32).read_unaligned()
}

unsafe fn sid_list_size(p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let n = rd_u32(p, 0);
    let arr = (p.add(8) as *const *const u8).read_unaligned();
    let mut size = (n.wrapping_add(1)) << 4;
    for i in 0..n as usize {
        let sid = (arr.add(8 + 16 * i) as *const *const c_void).read_unaligned();
        size = size.wrapping_add((RtlLengthSid(sid).wrapping_add(7)) & !7);
    }
    size
}

unsafe fn string_list_size(p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let n = rd_u32(p, 0);
    let arr = (p.add(8) as *const *const u8).read_unaligned();
    let mut size = n.wrapping_mul(24).wrapping_add(0x10);
    for i in 0..n as usize {
        let len = (arr.add(8 + 24 * i) as *const u16).read_unaligned() as u32;
        size = size.wrapping_add((len + 7) & !7);
    }
    size
}

#[repr(C)]
pub struct Entry {
    pub ty: u32,
    pub length: u32,
    pub data: [u64; 2],
    pub address: *const c_void,
}

pub unsafe extern "system" fn init_parameters_worker(
    array: *mut u8,
    category: u32,
    audit_id: u32,
    flat_sub: u16,
    ty: u16,
    count: u16,
    va: *const u64,
) -> i32 {
    let count = count as usize;
    core::ptr::write_bytes(array, 0, count * 32 + 0x18);
    (array as *mut u32).write_unaligned(category);
    (array.add(4) as *mut u32).write_unaligned(audit_id);
    (array.add(0x10) as *mut u16).write_unaligned(flat_sub);
    (array.add(0x12) as *mut u16).write_unaligned(ty);
    (array.add(8) as *mut u32).write_unaligned(count as u32);
    let mut va = va;
    let mut next = || {
        let v = *va;
        va = va.add(1);
        v
    };
    for i in 0..count {
        let e = &mut *(array.add(0x18 + 32 * i) as *mut Entry);
        let t = next() as u32 as i32;
        e.ty = t as u32;
        match t {
            1 | 2 => {
                let p = next() as *const u16;
                if p.is_null() {
                    e.ty = 0;
                } else {
                    e.length = *p as u32 + 0x10;
                    e.address = p as *const c_void;
                }
            }
            3 | 10 | 0x15 | 0x1b => {
                e.length = 4;
                e.data[0] = next() as u32 as u64;
            }
            4 | 0x21 => {
                let mut p = next() as *const c_void;
                if p.is_null() {
                    p = DEFAULT_SID.as_ptr() as *const c_void;
                } else if RtlValidSid(p) == 0 {
                    return STATUS_INVALID_SID;
                }
                e.length = RtlLengthSid(p);
                e.address = p;
            }
            5 | 0xc | 0xe | 0xf | 0x12 | 0x16 | 0x1a => {
                e.length = 8;
                e.data[0] = next();
            }
            7 => {
                e.length = 4;
                e.data[0] = next() as u32 as u64;
                e.data[1] = next() as u16 as u64;
            }
            8 => {
                let p = next() as *const u32;
                if !p.is_null() {
                    let n = *p;
                    if n != 0 && n - 1 > 0x41 {
                        return STATUS_INVALID_PARAMETER;
                    }
                }
                e.address = p as *const c_void;
                e.length = if p.is_null() {
                    0
                } else if *p == 0 {
                    8
                } else {
                    *p * 12 + 8
                };
            }
            9 => {
                e.address = next() as *const c_void;
                e.length = (next() as u32).wrapping_mul(24);
                e.data[1] = next() as u32 as u64;
            }
            0xb => {
                e.length = 8;
                e.data[0] = next();
            }
            0xd => {
                let p = next() as *const c_void;
                e.address = if p.is_null() { DEFAULT_GUID.as_ptr() as *const c_void } else { p };
                e.length = 0x10;
            }
            0x10 => {
                let p = next() as *const u8;
                if p.is_null() {
                    e.ty = 0;
                } else {
                    e.address = p as *const c_void;
                    e.length = string_list_size(p);
                }
            }
            0x11 => {
                let p = next() as *const u8;
                if p.is_null() {
                    e.ty = 0;
                } else {
                    e.address = p as *const c_void;
                    e.length = sid_list_size(p);
                }
            }
            0x13 => {
                e.length = 8;
                e.data[0] = next() as u32 as u64;
                e.data[1] = next() as u32 as u64;
            }
            0x17 | 0x1c => {
                let p = next() as *const u16;
                e.address = p as *const c_void;
                if !p.is_null() {
                    e.length = if *p == 0x17 { 0x1c } else { 0x10 };
                }
            }
            _ => {}
        }
    }
    STATUS_SUCCESS
}

core::arch::global_asm!(
    ".globl LsapAdtInitParametersArray",
    "LsapAdtInitParametersArray:",
    "sub rsp, 0x48",
    "lea rax, [rsp + 0x80]",
    "mov [rsp + 0x30], rax",
    "movzx eax, word ptr [rsp + 0x78]",
    "mov [rsp + 0x28], rax",
    "movzx eax, word ptr [rsp + 0x70]",
    "mov [rsp + 0x20], rax",
    "call {w}",
    "add rsp, 0x48",
    "ret",
    w = sym init_parameters_worker,
);

extern "C" {
    pub fn LsapAdtInitParametersArray(array: *mut u8, category: u32, audit_id: u32, flat_sub: u16, ty: u16, count: u16, ...) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORTS: &str = include_str!("../exports.txt");
    const CONSOLE: &str = r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\lsasrv.dll";

    #[test]
    fn export_table_matches_console_ordinals() {
        let ours = xone_exports::table(&xone_exports::parse(EXPORTS));
        let Ok(bytes) = std::fs::read(CONSOLE) else {
            eprintln!("{CONSOLE} absent");
            return;
        };
        let theirs = xone_exports::pe_exports(&bytes).unwrap();
        for (o, n) in &ours {
            assert!(theirs.contains(&(*o, n.clone())), "{n} @{o} not in console lsasrv");
        }
    }

    #[test]
    fn built_dll_has_console_ordinals() {
        xone_exports::check_built(env!("CARGO_MANIFEST_DIR"), "lsasrv.dll", EXPORTS);
    }

    #[test]
    fn profile_never_loaded_outside_lsa() {
        let luid = [0u32; 2];
        assert_eq!(LsaIUserProfileLoaded(luid.as_ptr() as *const c_void), 0);
    }

    #[test]
    fn policy_lookup_follows_lsaadt_tables() {
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(101, 8), 0);
        POLICY[5].store(1, Ordering::Relaxed);
        POLICY[6].store(2, Ordering::Relaxed);
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(105, 8), 1);
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(105, 0x10), 0);
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(106, 0x10), 2);
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(106, 4), 0);
        assert_eq!(LsapAdtAuditingEnabledHint(105, 8), 1);
        assert_eq!(LsapAdtAuditingEnabledHint(106, 8), 0);
        assert_eq!(LsapAdtAuditingEnabledHint(106, 0x10), 2);
        PER_USER_ANY[7].store(1, Ordering::Relaxed);
        assert_eq!(LsapAdtAuditingEnabledHint(107, 8), 1);
        assert_eq!(LsapAdtAuditingEnabledHint(107, 0x10), 2);
        let mut en = 7u8;
        unsafe {
            assert_eq!(LsapAdtAuditingEnabledByLogonId(110, core::ptr::null(), 8, &mut en), 0);
            assert_eq!(en, 0);
            assert_eq!(LsapAdtAuditingEnabledByLogonId(105, core::ptr::null(), 8, &mut en), 0);
            assert_eq!(en, 1);
        }
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(99, 8), 0);
        assert_eq!(LsapAdtAuditingEnabledBySubCategory(500, 8), 0);
    }

    #[test]
    fn init_parameters_array_builds_entries() {
        let mut arr = vec![0xccu8; 0x18 + 32 * 8];
        let s: Vec<u16> = "abc".encode_utf16().collect();
        let us: [u64; 2] = [6 | (6 << 16), s.as_ptr() as u64];
        let sid: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
        let guid = [7u8; 16];
        let st = unsafe {
            LsapAdtInitParametersArray(
                arr.as_mut_ptr(),
                2u32,
                4624u32,
                0x2a,
                3,
                8,
                1u64,
                us.as_ptr(),
                3u64,
                0x1234u64,
                4u64,
                sid.as_ptr(),
                4u64,
                core::ptr::null::<u8>(),
                7u64,
                0x10u64,
                5u64,
                0xdu64,
                core::ptr::null::<u8>(),
                1u64,
                core::ptr::null::<u8>(),
                0x13u64,
                9u64,
                10u64,
            )
        };
        assert_eq!(st, 0);
        let h = |o: usize| u32::from_le_bytes(arr[o..o + 4].try_into().unwrap());
        assert_eq!((h(0), h(4), h(8), h(0xc)), (2, 4624, 8, 0));
        assert_eq!(u16::from_le_bytes([arr[0x10], arr[0x11]]), 0x2a);
        assert_eq!(u16::from_le_bytes([arr[0x12], arr[0x13]]), 3);
        let e = |i: usize| unsafe { &*(arr.as_ptr().add(0x18 + 32 * i) as *const Entry) };
        assert_eq!((e(0).ty, e(0).length, e(0).address), (1, 0x16, us.as_ptr() as *const c_void));
        assert_eq!((e(1).ty, e(1).length, e(1).data[0]), (3, 4, 0x1234));
        assert_eq!((e(2).ty, e(2).length, e(2).address), (4, 12, sid.as_ptr() as *const c_void));
        assert_eq!((e(3).ty, e(3).length, e(3).address), (4, 8, DEFAULT_SID.as_ptr() as *const c_void));
        assert_eq!((e(4).ty, e(4).length, e(4).data), (7, 4, [0x10, 5]));
        assert_eq!((e(5).ty, e(5).length, e(5).address), (0xd, 0x10, DEFAULT_GUID.as_ptr() as *const c_void));
        assert_eq!((e(6).ty, e(6).length), (0, 0));
        assert_eq!((e(7).ty, e(7).length, e(7).data), (0x13, 8, [9, 10]));
        let _ = guid;
    }

    #[test]
    fn init_parameters_array_rejects_bad_sid_and_privileges() {
        let mut arr = vec![0u8; 0x18 + 32];
        let bad = [9u8; 16];
        assert_eq!(unsafe { LsapAdtInitParametersArray(arr.as_mut_ptr(), 1u32, 2u32, 0, 0, 1, 4u64, bad.as_ptr()) }, STATUS_INVALID_SID);
        let privs = [0x43u32, 0, 0, 0];
        assert_eq!(unsafe { LsapAdtInitParametersArray(arr.as_mut_ptr(), 1u32, 2u32, 0, 0, 1, 8u64, privs.as_ptr()) }, STATUS_INVALID_PARAMETER);
        let privs = [0x42u32, 0, 0, 0];
        assert_eq!(unsafe { LsapAdtInitParametersArray(arr.as_mut_ptr(), 1u32, 2u32, 0, 0, 1, 8u64, privs.as_ptr()) }, 0);
        let e = unsafe { &*(arr.as_ptr().add(0x18) as *const Entry) };
        assert_eq!(e.length, 0x42 * 12 + 8);
    }
}
