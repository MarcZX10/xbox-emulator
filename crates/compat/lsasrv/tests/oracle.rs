use core::ffi::c_void;
use lsasrv::*;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, n: *const u8) -> *mut c_void;
}

const LSAADT: &str = r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\lsaadt.dll";

fn console() -> Option<*mut c_void> {
    if !std::path::Path::new(LSAADT).is_file() {
        eprintln!("{LSAADT} absent, oracle skipped");
        return None;
    }
    let w: Vec<u16> = LSAADT.encode_utf16().chain(Some(0)).collect();
    let h = unsafe { LoadLibraryW(w.as_ptr()) };
    assert!(!h.is_null());
    Some(h)
}

unsafe fn ord(h: *mut c_void, n: usize) -> *mut c_void {
    let p = GetProcAddress(h, n as *const u8);
    assert!(!p.is_null(), "ordinal {n}");
    p
}

type Init = unsafe extern "C" fn(*mut u8, u32, u32, u16, u16, u16, ...) -> i32;
type Sub = unsafe extern "system" fn(u32, u32) -> u8;
type ByLogon = unsafe extern "system" fn(u32, *const c_void, u32, *mut u8) -> i32;

#[test]
fn empty_policy_answers_match_console_lsaadt() {
    let Some(h) = console() else { return };
    unsafe {
        let by_sub: Sub = core::mem::transmute(ord(h, 2));
        let hint: Sub = core::mem::transmute(ord(h, 3));
        let by_logon: ByLogon = core::mem::transmute(ord(h, 1));
        for sub in [100u32, 101, 120, 163] {
            for t in [8u32, 0x10, 4] {
                assert_eq!(by_sub(sub, t), LsapAdtAuditingEnabledBySubCategory(sub, t), "{sub} {t}");
                assert_eq!(hint(sub, t), LsapAdtAuditingEnabledHint(sub, t), "{sub} {t}");
                let luid = [0x3e7u32, 0];
                let (mut a, mut b) = (9u8, 9u8);
                let ra = by_logon(sub, luid.as_ptr() as *const c_void, t, &mut a);
                let rb = LsapAdtAuditingEnabledByLogonId(sub, luid.as_ptr() as *const c_void, t, &mut b);
                assert_eq!((ra, a), (rb, b), "{sub} {t}");
            }
        }
    }
}

#[test]
fn init_parameters_array_matches_console_lsaadt() {
    let Some(h) = console() else { return };
    let theirs: Init = unsafe { core::mem::transmute(ord(h, 4)) };
    let s: Vec<u16> = "object".encode_utf16().collect();
    let us: [u64; 2] = [12 | (12 << 16), s.as_ptr() as u64];
    let sid: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 2, 0, 0];
    let privs: [u32; 7] = [2, 1, 2, 3, 4, 5, 6];
    let guid = [3u8; 16];
    let sa: [u16; 14] = [0x17, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let objs = [0u8; 48];
    let run = |f: Init, out: &mut Vec<u8>| unsafe {
        f(
            out.as_mut_ptr(),
            5u32,
            4663u32,
            0x19,
            1,
            15,
            1u64,
            us.as_ptr(),
            2u64,
            us.as_ptr(),
            3u64,
            77u64,
            4u64,
            sid.as_ptr(),
            5u64,
            0x1122334455u64,
            7u64,
            0x1f01ffu64,
            9u64,
            8u64,
            privs.as_ptr(),
            9u64,
            objs.as_ptr(),
            2u64,
            6u64,
            0xau64,
            0xabcdu64,
            0xdu64,
            guid.as_ptr(),
            0x13u64,
            0x10u64,
            0x200u64,
            0x17u64,
            sa.as_ptr(),
            0x15u64,
            0x30u64,
            0x6u64,
            0x14u64,
        )
    };
    let mut a = vec![0xeeu8; 0x18 + 32 * 15];
    let mut b = vec![0xeeu8; 0x18 + 32 * 15];
    let ra = run(theirs, &mut a);
    let rb = run(LsapAdtInitParametersArray, &mut b);
    assert_eq!(ra, rb);
    assert_eq!(a, b);
    let bad = [9u8; 16];
    let mut a = vec![0u8; 0x18 + 32];
    let mut b = vec![0u8; 0x18 + 32];
    let ra = unsafe { theirs(a.as_mut_ptr(), 1u32, 2u32, 0, 0, 1, 4u64, bad.as_ptr()) };
    let rb = unsafe { LsapAdtInitParametersArray(b.as_mut_ptr(), 1u32, 2u32, 0, 0, 1, 4u64, bad.as_ptr()) };
    assert_eq!((ra, &a), (rb, &b));
}
