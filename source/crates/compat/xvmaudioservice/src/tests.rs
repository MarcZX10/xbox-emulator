use super::*;

#[link(name = "runtimeobject")]
extern "system" {
    fn WindowsCreateString(s: *const u16, len: u32, out: *mut HSTRING) -> i32;
    fn WindowsDeleteString(s: HSTRING) -> i32;
}

fn hstring(s: &[u16]) -> HSTRING {
    let mut h: HSTRING = core::ptr::null_mut();
    assert_eq!(unsafe { WindowsCreateString(s.as_ptr(), s.len() as u32, &mut h) }, 0);
    h
}

#[test]
fn dll_can_unload_now_with_no_module_objects() {
    assert_eq!(DllCanUnloadNow(), S_OK);
}

#[test]
fn activation_factory_table_is_empty_like_console() {
    unsafe {
        let mut out = 0x44usize as *mut c_void;
        assert_eq!(DllGetActivationFactory(core::ptr::null_mut(), &mut out), E_INVALIDARG);
        assert!(out.is_null());
        let name: Vec<u16> = "Windows.Xbox.Audio.Anything".encode_utf16().collect();
        let h = hstring(&name);
        out = 0x44usize as *mut c_void;
        assert_eq!(DllGetActivationFactory(h, &mut out), CLASS_E_CLASSNOTAVAILABLE);
        assert!(out.is_null());
        WindowsDeleteString(h);
        let embedded: Vec<u16> = vec![b'a' as u16, 0, b'b' as u16];
        let h = hstring(&embedded);
        assert_eq!(DllGetActivationFactory(h, &mut out), E_INVALIDARG);
        WindowsDeleteString(h);
    }
}
