use core::ffi::c_void;
use remotewinrt::*;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, n: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn SetLastError(e: u32);
}

const DLLS: [&str; 2] = [
    r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\RemoteWinRT.dll",
    r"D:\TomBox\input\trackmania\EmbeddedXvd\Windows\System32\RemoteWinRT.dll",
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn consoles() -> Vec<*mut c_void> {
    let mut v = Vec::new();
    for d in DLLS {
        if std::path::Path::new(d).is_file() {
            let h = unsafe { LoadLibraryW(wide(d).as_ptr()) };
            assert!(!h.is_null(), "load {d}");
            v.push(h);
        } else {
            eprintln!("{d} absent, oracle skipped");
        }
    }
    v
}

unsafe fn sym<T: Copy>(h: *mut c_void, n: &str) -> T {
    let c = format!("{n}\0");
    let p = GetProcAddress(h, c.as_ptr());
    assert!(!p.is_null(), "{n}");
    core::mem::transmute_copy(&p)
}

unsafe fn ws(p: *const u16) -> Vec<u16> {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n).to_vec()
}

#[test]
fn constant_exports_match_console() {
    type Ws = unsafe extern "system" fn() -> *const u16;
    type U = unsafe extern "system" fn() -> u32;
    for h in consoles() {
        unsafe {
            for (n, ours) in [
                ("GetEndPointMapperCrossVmPipeNamespace", GetEndPointMapperCrossVmPipeNamespace as Ws),
                ("GetLocalEndPointMapperCrossVmPipeName", GetLocalEndPointMapperCrossVmPipeName as Ws),
                ("GetRemoteEndPointMapperCrossVmPipeName", GetRemoteEndPointMapperCrossVmPipeName as Ws),
                ("GetLocalVmAliasName", GetLocalVmAliasName as Ws),
                ("GetRemoteVmAliasName", GetRemoteVmAliasName as Ws),
            ] {
                assert_eq!(ws(sym::<Ws>(h, n)()), ws(ours()), "{n}");
            }
            for (n, ours) in [
                ("GetLocalVmAliasNameLength", GetLocalVmAliasNameLength as U),
                ("GetRemoteVmAliasNameLength", GetRemoteVmAliasNameLength as U),
                ("IsProcessRestrictedToSingleApartment", IsProcessRestrictedToSingleApartment as U),
                ("RemoteWinRTActivation", RemoteWinRTActivation as U),
                ("WinRTPerMachineSingleInstancing", WinRTPerMachineSingleInstancing as U),
                ("ValidateRemoteVmNamedPipeEndpoint", ValidateRemoteVmNamedPipeEndpoint as U),
            ] {
                assert_eq!(sym::<U>(h, n)(), ours(), "{n}");
            }
        }
    }
}

#[test]
fn wait_remote_pipe_errors_match_console() {
    type W = unsafe extern "system" fn(*const u16, u32) -> i32;
    type E = unsafe extern "system" fn() -> i32;
    let cases = [
        r"pipe\x",
        r"\\.\nodevice",
        r"\\server\pipe\x",
        r"\\.\pipe\tombox-svc-no-such-pipe-7f3a",
        r"\\.\nosuchdevice-tombox\x",
        r"\\.\pipe",
    ];
    for h in consoles() {
        unsafe {
            let theirs: W = sym(h, "WaitRemotePipe");
            for c in cases {
                let w = wide(c);
                SetLastError(0);
                let a = theirs(w.as_ptr(), 10);
                let ea = GetLastError();
                SetLastError(0);
                let b = WaitRemotePipe(w.as_ptr(), 10);
                let eb = GetLastError();
                assert_eq!((a, ea), (b, eb), "{c}");
            }
            let ex: E = sym(h, "IsCurrentProcessExemptFromSuspend");
            assert_eq!(ex(), IsCurrentProcessExemptFromSuspend());
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateNamedPipeW(n: *const u16, open: u32, mode: u32, max: u32, out: u32, inb: u32, t: u32, sa: *mut c_void) -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn WaitNamedPipeW(n: *const u16, t: u32) -> i32;
}

#[test]
fn wait_remote_pipe_on_listening_pipe_matches_console() {
    type W = unsafe extern "system" fn(*const u16, u32) -> i32;
    let name = format!(r"\\.\pipe\tombox-svc-oracle-{}", std::process::id());
    let w = wide(&name);
    unsafe {
        let h = CreateNamedPipeW(w.as_ptr(), 3, 0, 1, 512, 512, 0, core::ptr::null_mut());
        assert!(h as isize != -1, "create {} {:?}", GetLastError(), String::from_utf16_lossy(&w));
        SetLastError(0);
        let pc = WaitNamedPipeW(w.as_ptr(), 100);
        let epc = GetLastError();
        for c in consoles() {
            let theirs: W = sym(c, "WaitRemotePipe");
            SetLastError(0);
            let a = theirs(w.as_ptr(), 100);
            let ea = GetLastError();
            SetLastError(0);
            let b = WaitRemotePipe(w.as_ptr(), 100);
            let eb = GetLastError();
            eprintln!("pc {pc} {epc} console {a} {ea} ours {b} {eb}");
            assert_eq!((a, ea), (b, eb));
        }
        CloseHandle(h);
    }
}
