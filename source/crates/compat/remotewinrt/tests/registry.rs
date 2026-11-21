use remotewinrt::*;

#[link(name = "advapi32")]
extern "system" {
    fn RegOverridePredefKey(key: isize, new: isize) -> i32;
    fn RegCreateKeyExW(k: isize, sub: *const u16, r: u32, c: *mut u16, o: u32, sam: u32, sa: *mut u8, out: *mut isize, disp: *mut u32) -> i32;
    fn RegDeleteTreeW(k: isize, sub: *const u16) -> i32;
    fn RegCloseKey(k: isize) -> i32;
}

const HKCU: isize = 0x8000_0001u32 as i32 as isize;
const HKLM: isize = 0x8000_0002u32 as i32 as isize;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn create(root: isize, path: &str) -> isize {
    let mut k = 0;
    let r = unsafe { RegCreateKeyExW(root, wide(path).as_ptr(), 0, core::ptr::null_mut(), 0, 0xf003f, core::ptr::null_mut(), &mut k, core::ptr::null_mut()) };
    assert_eq!(r, 0, "create {path}");
    k
}

#[test]
fn suspend_exemptions_are_read_from_hklm() {
    let base = format!(r"Software\TomBoxTest\svc-remotewinrt-{}", std::process::id());
    let fake = create(HKCU, &base);
    unsafe {
        assert_eq!(RegOverridePredefKey(HKLM, fake), 0);
        let absent = IsPackagedAppExemptFromSuspend(wide("Title_1.0_x64__abc").as_ptr());
        let k = create(HKLM, &format!(r"{SUSPEND_EXEMPTIONS}\Title_1.0_x64__abc"));
        RegCloseKey(k);
        let k = create(HKLM, &format!(r"{SUSPEND_EXEMPTIONS}\Other_2.0_x64__def"));
        RegCloseKey(k);
        let hit = IsPackagedAppExemptFromSuspend(wide("Title_1.0_x64__abc").as_ptr());
        let other = IsPackagedAppExemptFromSuspend(wide("Other_2.0_x64__def").as_ptr());
        let miss = IsPackagedAppExemptFromSuspend(wide("Title_1.0_x64__ab").as_ptr());
        let case = IsPackagedAppExemptFromSuspend(wide("title_1.0_x64__abc").as_ptr());
        let unpackaged = IsCurrentProcessExemptFromSuspend();
        RegOverridePredefKey(HKLM, 0);
        RegCloseKey(fake);
        RegDeleteTreeW(HKCU, wide(&base).as_ptr());
        assert_eq!((absent, hit, other, miss, case, unpackaged), (0, 1, 1, 0, 0, 0));
    }
}
