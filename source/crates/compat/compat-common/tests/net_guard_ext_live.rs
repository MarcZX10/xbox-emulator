use std::ffi::c_void;

#[link(name = "ws2_32")]
extern "system" {
    fn WSAStartup(v: u16, data: *mut u8) -> i32;
    fn socket(af: i32, ty: i32, proto: i32) -> usize;
    fn closesocket(s: usize) -> i32;
    fn WSAIoctl(s: usize, code: u32, inb: *const u8, inlen: u32, outb: *mut u8, outlen: u32, ret: *mut u32, ov: *mut c_void, r: *const c_void) -> i32;
    fn WSAGetLastError() -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(n: *const u8) -> *mut c_void;
    fn LoadLibraryA(n: *const u8) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, n: *const u8) -> *const u8;
    fn GetModuleHandleExW(flags: u32, addr: *const c_void, out: *mut *mut c_void) -> i32;
}

const CONNECTEX: [u8; 16] = [0xb9, 0x07, 0xa2, 0x25, 0xf3, 0xdd, 0x60, 0x46, 0x8e, 0xe9, 0x76, 0xe5, 0x8c, 0x74, 0x06, 0x3e];

fn patched(module: &[u8], func: &[u8]) -> bool {
    unsafe {
        let mut m = GetModuleHandleA(module.as_ptr());
        if m.is_null() {
            m = LoadLibraryA(module.as_ptr());
        }
        let p = GetProcAddress(m, func.as_ptr());
        !p.is_null() && *p == 0x48 && *p.add(1) == 0xB8 && *p.add(10) == 0xFF && *p.add(11) == 0xE0
    }
}

fn in_this_exe(p: usize) -> bool {
    unsafe {
        let mut m = core::ptr::null_mut();
        let ok = GetModuleHandleExW(6, p as *const c_void, &mut m);
        ok != 0 && m == GetModuleHandleA(core::ptr::null())
    }
}

#[test]
fn literal_ip_paths_are_guarded_after_install_without_touching_the_network() {
    std::env::remove_var("TOMBOX_NET_BACKEND");
    std::env::remove_var("TOMBOX_ALLOW_NETWORK");
    std::env::set_var("TOMBOX_NET_REDIRECT", "127.0.0.1");
    compat_common::net_guard::ensure();
    for (m, f) in [
        (&b"ws2_32.dll\0"[..], &b"WSAIoctl\0"[..]),
        (b"ws2_32.dll\0", b"WSASendMsg\0"),
        (b"ws2_32.dll\0", b"WSAConnectByList\0"),
        (b"ws2_32.dll\0", b"WSAJoinLeaf\0"),
        (b"ws2_32.dll\0", b"gethostbyaddr\0"),
        (b"ws2_32.dll\0", b"getnameinfo\0"),
        (b"ws2_32.dll\0", b"GetNameInfoW\0"),
        (b"dnsapi.dll\0", b"DnsQuery_W\0"),
        (b"dnsapi.dll\0", b"DnsQueryEx\0"),
    ] {
        assert!(patched(m, f), "{} not patched", String::from_utf8_lossy(&f[..f.len() - 1]));
    }
    unsafe {
        let mut data = [0u8; 512];
        assert_eq!(WSAStartup(0x0202, data.as_mut_ptr()), 0);
        let s = socket(2, 1, 6);
        assert_ne!(s, !0usize);
        let mut fp = 0usize;
        let mut ret = 0u32;
        let rc = WSAIoctl(s, 0xC800_0006, CONNECTEX.as_ptr(), 16, &mut fp as *mut usize as *mut u8, 8, &mut ret, core::ptr::null_mut(), core::ptr::null());
        assert_eq!(rc, 0);
        assert!(in_this_exe(fp), "ConnectEx pointer {fp:#x} is not the net_guard wrapper");
        let cx: unsafe extern "system" fn(usize, *const u8, i32, *mut c_void, u32, *mut u32, *mut c_void) -> i32 = core::mem::transmute(fp);
        let mut sa = [0u8; 16];
        sa[0] = 2;
        sa[2..4].copy_from_slice(&443u16.to_be_bytes());
        sa[4..8].copy_from_slice(&[203, 0, 113, 9]);
        assert_eq!(cx(s, sa.as_ptr(), 16, core::ptr::null_mut(), 0, core::ptr::null_mut(), core::ptr::null_mut()), 0);
        assert_eq!(WSAGetLastError(), 10051);
        sa[4..8].copy_from_slice(&[127, 0, 0, 1]);
        assert_eq!(cx(s, sa.as_ptr(), 16, core::ptr::null_mut(), 0, core::ptr::null_mut(), core::ptr::null_mut()), 0);
        assert_ne!(WSAGetLastError(), 10051, "loopback ConnectEx must reach the real function");
        closesocket(s);

        let gni: unsafe extern "system" fn(*const u8, i32, *mut u8, u32, *mut u8, u32, i32) -> i32 =
            core::mem::transmute(GetProcAddress(GetModuleHandleA(b"ws2_32.dll\0".as_ptr()), b"getnameinfo\0".as_ptr()));
        sa[4..8].copy_from_slice(&[93, 184, 216, 34]);
        let mut host = [0u8; 128];
        assert_eq!(gni(sa.as_ptr(), 16, host.as_mut_ptr(), 128, core::ptr::null_mut(), 0, 0), 0);
        let h = std::ffi::CStr::from_ptr(host.as_ptr() as *const i8).to_string_lossy().into_owned();
        assert_eq!(h, "93.184.216.34");
        assert_eq!(gni(sa.as_ptr(), 16, host.as_mut_ptr(), 128, core::ptr::null_mut(), 0, 4), 11001);

        let dq: unsafe extern "system" fn(*const u16, u16, u32, *mut c_void, *mut *mut c_void, *mut c_void) -> i32 =
            core::mem::transmute(GetProcAddress(GetModuleHandleA(b"dnsapi.dll\0".as_ptr()), b"DnsQuery_W\0".as_ptr()));
        let name: Vec<u16> = "www.epicgames.com".encode_utf16().chain(Some(0)).collect();
        let mut res = core::ptr::null_mut();
        assert_eq!(dq(name.as_ptr(), 1, 0, core::ptr::null_mut(), &mut res, core::ptr::null_mut()), 9003);
        assert!(res.is_null());
    }
}
