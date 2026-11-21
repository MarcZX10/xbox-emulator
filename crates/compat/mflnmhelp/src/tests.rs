use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(key: isize, sub: *const u16, r: u32, class: *const u16, opt: u32, sam: u32, sa: *const c_void, out: *mut isize, disp: *mut u32) -> i32;
    fn RegSetValueExW(key: isize, name: *const u16, r: u32, ty: u32, data: *const u8, cb: u32) -> i32;
    #[link_name = "RegDeleteTreeW"]
    fn PcRegDeleteTreeW(key: isize, sub: *const u16) -> i32;
}

const HKCU: isize = 0x8000_0001u32 as i32 as isize;

#[test]
fn crypt_acquire_context_writes_console_handle() {
    let mut h = 0usize;
    assert_eq!(unsafe { CryptAcquireContextA(&mut h, core::ptr::null(), core::ptr::null(), 1, 0) }, 1);
    assert_eq!(h, CONSOLE_PROV_HANDLE);
    assert_eq!(unsafe { CryptAcquireContextA(core::ptr::null_mut(), core::ptr::null(), core::ptr::null(), 1, 0) }, 0);
}

#[test]
fn crypt_gen_random_fills_buffer() {
    let mut a = [0u8; 64];
    assert_eq!(unsafe { CryptGenRandom(6, 64, a.as_mut_ptr()) }, 1);
    assert!(a.iter().any(|&b| b != 0));
    assert_eq!(unsafe { CryptGenRandom(6, 0, a.as_mut_ptr()) }, 0);
    assert_eq!(unsafe { CryptGenRandom(6, 4, core::ptr::null_mut()) }, 0);
}

#[test]
fn crypt_release_context_is_true() {
    assert_eq!(CryptReleaseContext(6, 0), 1);
}

#[test]
fn get_profile_int_returns_default() {
    assert_eq!(GetProfileIntW(core::ptr::null(), core::ptr::null(), 42), 42);
    assert_eq!(GetProfileIntW(core::ptr::null(), core::ptr::null(), -1), u32::MAX);
}

#[test]
fn global_free_returns_null() {
    let mut x = 5u32;
    assert!(GlobalFree((&mut x as *mut u32).cast()).is_null());
}

#[test]
fn global_lock_returns_null() {
    let mut x = 5u32;
    assert!(GlobalLock((&mut x as *mut u32).cast()).is_null());
}

#[test]
fn global_unlock_returns_false() {
    assert_eq!(GlobalUnlock(core::ptr::null_mut()), 0);
}

#[test]
fn prop_variant_to_variant_writes_nothing() {
    let src = [0x1234_5678u64; 3];
    let mut dst = [0xaaaa_aaaa_aaaa_aaaau64; 3];
    unsafe { PropVariantToVariant(src.as_ptr().cast(), dst.as_mut_ptr().cast()) };
    assert_eq!(dst, [0xaaaa_aaaa_aaaa_aaaau64; 3]);
}

#[test]
fn reg_delete_tree_not_implemented() {
    assert_eq!(RegDeleteTreeW(HKCU, wide("x").as_ptr()), ERROR_CALL_NOT_IMPLEMENTED);
}

#[test]
fn reg_get_value_reads_through_subkey_and_open_key() {
    let path = wide("Software\\TomBoxTest\\mflnmhelp");
    let name = wide("v");
    let mut k = 0isize;
    unsafe {
        assert_eq!(RegCreateKeyExW(HKCU, path.as_ptr(), 0, core::ptr::null(), 0, 0xf003f, core::ptr::null(), &mut k, core::ptr::null_mut()), 0);
        let v = 0x1122_3344u32.to_le_bytes();
        assert_eq!(RegSetValueExW(k, name.as_ptr(), 0, 4, v.as_ptr(), 4), 0);
        let (mut ty, mut d, mut cb) = (0u32, 0u32, 4u32);
        assert_eq!(RegGetValueW(HKCU, path.as_ptr(), name.as_ptr(), 0xffff, &mut ty, (&mut d as *mut u32).cast(), &mut cb), 0);
        assert_eq!((ty, d, cb), (4, 0x1122_3344, 4));
        d = 0;
        let empty = [0u16];
        assert_eq!(RegGetValueW(k, empty.as_ptr(), name.as_ptr(), 0, &mut ty, (&mut d as *mut u32).cast(), &mut cb), 0);
        assert_eq!(d, 0x1122_3344);
        d = 0;
        assert_eq!(RegGetValueW(k, core::ptr::null(), name.as_ptr(), 0, &mut ty, (&mut d as *mut u32).cast(), &mut cb), 0);
        assert_eq!(d, 0x1122_3344);
        assert_eq!(RegGetValueW(HKCU, wide("Software\\TomBoxTest\\nope_missing").as_ptr(), name.as_ptr(), 0, &mut ty, core::ptr::null_mut(), &mut cb), 2);
        RegCloseKey(k);
        PcRegDeleteTreeW(HKCU, path.as_ptr());
    }
}

#[test]
fn reg_notify_change_not_implemented() {
    assert_eq!(RegNotifyChangeKeyValue(HKCU, 1, 4, 0, 1), ERROR_CALL_NOT_IMPLEMENTED);
}

#[test]
fn variant_change_type_not_implemented() {
    assert_eq!(VariantChangeType(core::ptr::null_mut(), core::ptr::null(), 0, 3), E_NOTIMPL);
}

static RELEASES: AtomicU32 = AtomicU32::new(0);
static CLEARS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn release(_t: *mut c_void) -> u32 {
    RELEASES.fetch_add(1, Ordering::SeqCst);
    0
}

unsafe extern "system" fn record_clear(_t: *mut c_void, _p: *mut c_void) -> i32 {
    CLEARS.fetch_add(1, Ordering::SeqCst);
    0
}

#[repr(C)]
struct Var {
    vt: u16,
    pad: [u16; 3],
    a: *mut c_void,
    b: *mut c_void,
}

fn var(vt: u16, a: *mut c_void, b: *mut c_void) -> Var {
    Var { vt, pad: [0; 3], a, b }
}

#[test]
fn variant_clear_follows_console_body() {
    let vtbl: [usize; 5] = [0, 0, release as *const () as usize, 0, record_clear as *const () as usize];
    let mut obj = vtbl.as_ptr();
    let o = (&mut obj as *mut *const usize).cast::<c_void>();
    unsafe {
        let mut v = var(3, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), 0);
        assert_eq!(v.vt, 0);
        let before = RELEASES.load(Ordering::SeqCst);
        let mut v = var(13, o, core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), 0);
        let mut v = var(9, o, core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), 0);
        assert_eq!(v.vt, 0);
        assert_eq!(RELEASES.load(Ordering::SeqCst), before + 2);
        let mut v = var(36, core::ptr::null_mut(), o);
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), 0);
        assert_eq!(RELEASES.load(Ordering::SeqCst), before + 3);
        assert_eq!(CLEARS.load(Ordering::SeqCst), 0);
        let mut v = var(8, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), E_NOTIMPL);
        assert_eq!(v.vt, 8);
        let mut v = var(72, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), E_NOTIMPL);
        let mut v = var(0x2003, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), E_NOTIMPL);
        let mut v = var(0x4003, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), 0);
        assert_eq!(v.vt, 0);
        let mut v = var(15, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), DISP_E_BADVARTYPE);
        let mut v = var(0x1003, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), DISP_E_BADVARTYPE);
        let mut v = var(30, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), DISP_E_BADVARTYPE);
        let mut v = var(0x4009, o, core::ptr::null_mut());
        assert_eq!(VariantClear((&mut v as *mut Var).cast()), 0);
        assert_eq!(RELEASES.load(Ordering::SeqCst), before + 3);
    }
}

#[test]
fn variant_init_sets_empty() {
    let mut v = var(9, core::ptr::null_mut(), core::ptr::null_mut());
    unsafe { VariantInit((&mut v as *mut Var).cast()) };
    assert_eq!(v.vt, 0);
}

#[test]
fn time_get_time_tracks_tick_count() {
    let a = unsafe { GetTickCount() };
    let t = unsafe { timeGetTime() };
    let b = unsafe { GetTickCount() };
    assert!(t >= a && t <= b);
}
