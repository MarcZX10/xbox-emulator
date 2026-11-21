use crate::{Fix, Guid, Tables};
use core::ffi::c_void;
use std::sync::Mutex;

pub static SERIAL: Mutex<()> = Mutex::new(());

pub const IID_IPSFACTORYBUFFER: Guid = Guid { data1: 0xD5F569D0, data2: 0x593B, data3: 0x101A, data4: [0xB5, 0x69, 0x08, 0x00, 0x2B, 0x2D, 0xBF, 0x7A] };
pub const IID_IUNKNOWN: Guid = Guid { data1: 0, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
pub const S_OK: i32 = 0;
pub const S_FALSE: i32 = 1;
const HKEY_CLASSES_ROOT: isize = 0x8000_0000u32 as i32 as isize;
const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOverridePredefKey(key: isize, new: isize) -> i32;
    fn RegCreateKeyExW(key: isize, sub: *const u16, r: u32, class: *const u16, opt: u32, sam: u32, sa: *const c_void, out: *mut isize, disp: *mut u32) -> i32;
    fn RegOpenKeyExW(key: isize, sub: *const u16, opt: u32, sam: u32, out: *mut isize) -> i32;
    fn RegCloseKey(key: isize) -> i32;
    fn RegDeleteTreeW(key: isize, sub: *const u16) -> i32;
    fn RegQueryValueExW(key: isize, name: *const u16, r: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn guid_string(g: &Guid) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1, g.data2, g.data3, g.data4[0], g.data4[1], g.data4[2], g.data4[3], g.data4[4], g.data4[5], g.data4[6], g.data4[7]
    )
}

pub struct Console {
    pub module: *mut c_void,
}

impl Console {
    pub fn load(path: &str) -> Option<Console> {
        if !std::path::Path::new(path).is_file() {
            eprintln!("{path} absent, console oracle skipped");
            return None;
        }
        let h = unsafe { LoadLibraryW(wide(path).as_ptr()) };
        assert!(!h.is_null(), "load {path}");
        Some(Console { module: h })
    }
    pub unsafe fn sym<T: Copy>(&self, n: &str) -> T {
        let c = format!("{n}\0");
        let p = GetProcAddress(self.module, c.as_ptr());
        assert!(!p.is_null(), "{n}");
        core::mem::transmute_copy(&p)
    }
    pub unsafe fn proxy_dll_info(&self) -> (*const c_void, *const Guid) {
        let f: unsafe extern "system" fn(*mut *const c_void, *mut *const Guid) = self.sym("GetProxyDllInfo");
        let mut a = core::ptr::null();
        let mut b = core::ptr::null();
        f(&mut a, &mut b);
        (a, b)
    }
    pub unsafe fn get_class_object(&self, clsid: *const Guid, iid: *const Guid, ppv: *mut *mut c_void) -> i32 {
        let f: unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32 = self.sym("DllGetClassObject");
        f(clsid, iid, ppv)
    }
    pub unsafe fn can_unload_now(&self) -> i32 {
        let f: unsafe extern "system" fn() -> i32 = self.sym("DllCanUnloadNow");
        f()
    }
}

pub unsafe fn interfaces(pfl: *const c_void) -> Vec<(Guid, u32, bool)> {
    let mut out = Vec::new();
    let list = pfl as *const *const usize;
    let mut k = 0;
    while !(*list.add(k)).is_null() {
        let fi = *list.add(k);
        let proxies = *fi as *const *const usize;
        let stubs = *fi.add(1) as *const *const u8;
        let delegated = *fi.add(3) as *const *const Guid;
        let size = *((fi as *const u8).add(40) as *const u16) as usize;
        for j in 0..size {
            let piid = *(*proxies.add(j)).add(1) as *const Guid;
            let count = *((*stubs.add(j)).add(16) as *const u32);
            let base = !delegated.is_null() && !(*delegated.add(j)).is_null();
            out.push((*piid, count, base));
        }
        k += 1;
    }
    out
}

unsafe fn thunk_target(p: usize) -> usize {
    let b = p as *const u8;
    if *b == 0xFF && *b.add(1) == 0x25 {
        let disp = (b.add(2) as *const i32).read_unaligned() as isize;
        *((p as isize + 6 + disp) as *const usize)
    } else {
        p
    }
}

unsafe fn resolve(name: &str, dll: &str) -> usize {
    let h = LoadLibraryW(wide(dll).as_ptr());
    let c = format!("{name}\0");
    GetProcAddress(h, c.as_ptr()) as usize
}

pub unsafe fn compare_with_console(t: &'static Tables, source_rva: u32, ours: *const c_void, console: &Console) {
    let ob = ours as usize - t.proxy_file_list;
    let cb = console.module as usize + source_rva as usize;
    let (cpfl, _) = console.proxy_dll_info();
    assert_eq!(cpfl as usize - cb, t.proxy_file_list, "proxy file list offset");
    let mut slot = vec![false; t.blob.len()];
    for &(off, fix) in t.fixups {
        slot[off..off + 8].iter_mut().for_each(|s| *s = true);
        let o = *((ob + off) as *const usize);
        let c = *((cb + off) as *const usize);
        match fix {
            Fix::Data(d) => {
                assert_eq!(o, ob + d, "our pointer at {off:#x}");
                assert_eq!(c, cb + d, "console pointer at {off:#x}");
            }
            Fix::Import(m, name) => {
                assert_eq!(o, resolve(name, t.dlls[m]), "{name} at {off:#x}");
                assert_eq!(thunk_target(c), o, "console thunk for {name} at {off:#x}");
            }
            Fix::StubRelease | Fix::StubRelease2 | Fix::Lookup(_) => assert_ne!(o, 0),
            Fix::Runtime => assert_eq!(o, 0),
        }
    }
    let ci = interfaces(cpfl);
    let oi = interfaces(ours);
    assert_eq!(oi, ci, "interface tables");
    for (off, s) in slot.iter().enumerate() {
        if !*s && off < t.rdata_len {
            let a = *((ob + off) as *const u8);
            let b = *((cb + off) as *const u8);
            if a != b && !is_iat(console, cb + off) {
                panic!("byte {off:#x} differs: ours {a:#x} console {b:#x}");
            }
        }
    }
}

unsafe fn is_iat(console: &Console, addr: usize) -> bool {
    let base = console.module as usize;
    let pe = base + *((base + 0x3c) as *const u32) as usize;
    let dd = pe + 24 + 112;
    let iat_rva = *((dd + 12 * 8) as *const u32) as usize;
    let iat_len = *((dd + 12 * 8 + 4) as *const u32) as usize;
    let lc_rva = *((dd + 10 * 8) as *const u32) as usize;
    let lc_len = *((base + lc_rva) as *const u32) as usize;
    (addr >= base + iat_rva && addr < base + iat_rva + iat_len) || (addr >= base + lc_rva && addr < base + lc_rva + lc_len)
}

#[repr(C)]
struct Outer {
    vtbl: *const [usize; 3],
    refs: u32,
}

unsafe extern "system" fn outer_qi(_this: *mut c_void, _iid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    *ppv = core::ptr::null_mut();
    0x8000_4002u32 as i32
}
unsafe extern "system" fn outer_addref(this: *mut Outer) -> u32 {
    (*this).refs += 1;
    (*this).refs
}
unsafe extern "system" fn outer_release(this: *mut Outer) -> u32 {
    (*this).refs -= 1;
    (*this).refs
}


pub unsafe fn release(p: *mut c_void) -> u32 {
    let vt = *(p as *const *const usize);
    let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*vt.add(2));
    f(p)
}

pub const SKIPPED: i32 = 1;

pub unsafe fn factory_results(factory: *mut c_void, iids: &[(Guid, bool)]) -> Vec<(i32, i32)> {
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(r: *mut c_void, model: u32) -> i32;
    }
    CoInitializeEx(core::ptr::null_mut(), 0);
    let vt = *(factory as *const *const usize);
    let create_proxy: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void, *mut *mut c_void) -> i32 = core::mem::transmute(*vt.add(3));
    let create_stub: unsafe extern "system" fn(*mut c_void, *const Guid, *mut c_void, *mut *mut c_void) -> i32 = core::mem::transmute(*vt.add(4));
    let mut out = Vec::new();
    let outer_vtbl = [outer_qi as *const () as usize, outer_addref as *const () as usize, outer_release as *const () as usize];
    for (iid, delegated) in iids {
        let mut outer = Outer { vtbl: &outer_vtbl, refs: 1 };
        let mut proxy = core::ptr::null_mut();
        let mut itf = core::ptr::null_mut();
        let hp = if *delegated { SKIPPED } else { create_proxy(factory, &mut outer as *mut Outer as *mut c_void, iid, &mut proxy, &mut itf) };
        if hp == S_OK {
            assert!(!proxy.is_null() && !itf.is_null());
            let ivt = *(itf as *const *const usize);
            assert_ne!(*ivt.add(3), 0, "proxy method slot for {}", guid_string(iid));
            release(itf);
            release(proxy);
        }
        let mut stub = core::ptr::null_mut();
        let hs = if *delegated { SKIPPED } else { create_stub(factory, iid, core::ptr::null_mut(), &mut stub) };
        if hs == S_OK {
            assert!(!stub.is_null());
            release(stub);
        }
        out.push((hp, hs));
    }
    out
}

pub struct ScratchClassesRoot {
    key: isize,
    path: String,
}

impl ScratchClassesRoot {
    pub fn new() -> ScratchClassesRoot {
        let path = format!(r"Software\TomBoxTest\ndr-proxy-{}", unsafe { GetCurrentProcessId() });
        let mut key = 0;
        let r = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, wide(&path).as_ptr(), 0, core::ptr::null(), 0, 0xF003F, core::ptr::null(), &mut key, core::ptr::null_mut()) };
        assert_eq!(r, 0, "create scratch key");
        assert_eq!(unsafe { RegOverridePredefKey(HKEY_CLASSES_ROOT, key) }, 0, "override HKCR");
        let mut probe = 0;
        let r = unsafe { RegCreateKeyExW(HKEY_CLASSES_ROOT, wide("TomBoxProbe").as_ptr(), 0, core::ptr::null(), 0, 0xF003F, core::ptr::null(), &mut probe, core::ptr::null_mut()) };
        assert_eq!(r, 0);
        unsafe { RegCloseKey(probe) };
        let s = ScratchClassesRoot { key, path };
        assert!(s.exists("TomBoxProbe"), "HKCR writes must land in the scratch key");
        s
    }
    pub fn exists(&self, sub: &str) -> bool {
        let mut k = 0;
        let r = unsafe { RegOpenKeyExW(self.key, wide(sub).as_ptr(), 0, 0x20019, &mut k) };
        if r == 0 {
            unsafe { RegCloseKey(k) };
        }
        r == 0
    }
    pub fn default_value(&self, sub: &str) -> Option<String> {
        let mut k = 0;
        if unsafe { RegOpenKeyExW(self.key, wide(sub).as_ptr(), 0, 0x20019, &mut k) } != 0 {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = 0;
        let r = unsafe { RegQueryValueExW(k, core::ptr::null(), core::ptr::null_mut(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut len) };
        unsafe { RegCloseKey(k) };
        if r != 0 {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_string())
    }
}

impl Drop for ScratchClassesRoot {
    fn drop(&mut self) {
        unsafe {
            RegOverridePredefKey(HKEY_CLASSES_ROOT, 0);
            RegCloseKey(self.key);
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.path).as_ptr());
        }
    }
}

pub unsafe fn get_factory(get: unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32, clsid: &Guid) -> (i32, *mut c_void) {
    let mut f = core::ptr::null_mut();
    let hr = get(clsid, &IID_IPSFACTORYBUFFER, &mut f);
    (hr, f)
}

pub fn module_path(m: *mut c_void) -> String {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, n: u32) -> u32;
    }
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(m, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

pub fn current_module() -> *mut c_void {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
    }
    let mut h = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(6, crate::dll_can_unload_now as *const c_void, &mut h) };
    h
}

#[macro_export]
macro_rules! proxy_dll_tests {
    ($console:expr, $clsid:expr) => {
        use core::ffi::c_void;
        use ndr_proxy::testing::*;
        use ndr_proxy::Guid;

        const CONSOLE: &str = $console;
        const EXPECTED_CLSID: Guid = $clsid;
        const WRONG_CLSID: Guid = Guid { data1: 0x1234_5678, data2: 1, data3: 2, data4: [3, 4, 5, 6, 7, 8, 9, 10] };

        fn ours_pfl() -> *const c_void {
            let mut pfl = core::ptr::null();
            let mut id = core::ptr::null();
            unsafe { super::GetProxyDllInfo(&mut pfl, &mut id) };
            pfl
        }

        #[test]
        fn get_proxy_dll_info_matches_console_tables() {
            let mut pfl = core::ptr::null();
            let mut id: *const Guid = core::ptr::null();
            unsafe { super::GetProxyDllInfo(&mut pfl, &mut id) };
            assert_eq!(unsafe { *id }, EXPECTED_CLSID);
            let list = pfl as *const *const c_void;
            let n = (0..).take_while(|&k| !unsafe { *list.add(k) }.is_null()).count();
            assert_eq!(n, super::FILE_COUNT);
            if let Some(c) = Console::load(CONSOLE) {
                let (_, cid) = unsafe { c.proxy_dll_info() };
                assert_eq!(unsafe { *cid }, EXPECTED_CLSID);
                unsafe { compare_with_console(&super::TABLES, super::SOURCE_RVA, pfl, &c) };
            }
        }

        #[test]
        fn dll_get_class_object_builds_every_proxy_and_stub() {
            let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            let iids: Vec<(Guid, bool)> = unsafe { interfaces(ours_pfl()) }.into_iter().map(|(g, _, d)| (g, d)).collect();
            assert!(!iids.is_empty());
            let (hr, f) = unsafe { get_factory(super::DllGetClassObject, &EXPECTED_CLSID) };
            assert_eq!(hr, S_OK);
            let ours = unsafe { factory_results(f, &iids) };
            unsafe { release(f) };
            let sync = ours.iter().filter(|(hp, hs)| *hp == S_OK && *hs == S_OK).count();
            assert!(sync > 0, "synchronous interfaces must build proxies and stubs: {ours:?}");
            for (hp, hs) in &ours {
                assert!(*hp == S_OK || *hp == SKIPPED || *hp == 0x8000_4002u32 as i32, "{hp:#x}");
                assert!(*hs == S_OK || *hs == SKIPPED || *hs == 0x8000_4002u32 as i32, "{hs:#x}");
            }
            let (bad, bf) = unsafe { get_factory(super::DllGetClassObject, &WRONG_CLSID) };
            assert_eq!(bad, ndr_proxy::CLASS_E_CLASSNOTAVAILABLE);
            assert!(bf.is_null());
            if let Some(c) = Console::load(CONSOLE) {
                let get = unsafe { c.sym::<unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32>("DllGetClassObject") };
                let (chr, cf) = unsafe { get_factory(get, &EXPECTED_CLSID) };
                assert_eq!(chr, S_OK);
                let theirs = unsafe { factory_results(cf, &iids) };
                unsafe { release(cf) };
                assert_eq!(ours, theirs);
                let (cbad, _) = unsafe { get_factory(get, &WRONG_CLSID) };
                assert_eq!(cbad, bad);
            }
        }

        #[test]
        fn dll_can_unload_now_follows_factory_references() {
            let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            let seq = |get: unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32, can: &dyn Fn() -> i32| {
                let a = can();
                let (hr, f) = unsafe { get_factory(get, &EXPECTED_CLSID) };
                assert_eq!(hr, S_OK);
                let b = can();
                unsafe { release(f) };
                let c = can();
                (a, b, c)
            };
            let ours = seq(super::DllGetClassObject, &|| unsafe { super::DllCanUnloadNow() });
            assert_eq!(ours, (S_OK, S_FALSE, S_OK));
            if let Some(c) = Console::load(CONSOLE) {
                let get = unsafe { c.sym::<unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32>("DllGetClassObject") };
                let theirs = seq(get, &|| unsafe { c.can_unload_now() });
                assert_eq!(theirs, ours);
            }
        }

        #[test]
        fn dll_register_server_writes_clsid_and_interface_keys() {
            let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            let root = ScratchClassesRoot::new();
            assert_eq!(unsafe { super::DllRegisterServer() }, S_OK);
            let cls = guid_string(&EXPECTED_CLSID);
            let server = root.default_value(&format!(r"CLSID\{cls}\InProcServer32"));
            assert_eq!(server.map(|s| s.to_lowercase()), Some(module_path(current_module()).to_lowercase()));
            let iids: Vec<(Guid, bool)> = unsafe { interfaces(ours_pfl()) }.into_iter().map(|(g, _, d)| (g, d)).collect();
            let (hr, f) = unsafe { get_factory(super::DllGetClassObject, &EXPECTED_CLSID) };
            assert_eq!(hr, S_OK);
            let built = unsafe { factory_results(f, &iids) };
            unsafe { release(f) };
            for (iid, _) in iids.iter().zip(built).filter(|(_, r)| r.1 == S_OK) {
                let k = format!(r"Interface\{}\ProxyStubClsid32", guid_string(&iid.0));
                assert_eq!(root.default_value(&k).map(|s| s.to_uppercase()), Some(cls.clone()), "{k}");
            }
            assert_eq!(unsafe { super::DllUnregisterServer() }, S_OK);
        }

        #[test]
        fn dll_unregister_server_removes_interface_keys() {
            let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            let root = ScratchClassesRoot::new();
            assert_eq!(unsafe { super::DllRegisterServer() }, S_OK);
            let iids = unsafe { interfaces(ours_pfl()) };
            let first = format!(r"Interface\{}", guid_string(&iids[0].0));
            assert!(root.exists(&first));
            assert_eq!(unsafe { super::DllUnregisterServer() }, S_OK);
            for (iid, _, _) in iids {
                assert!(!root.exists(&format!(r"Interface\{}", guid_string(&iid))), "{}", guid_string(&iid));
            }
        }
    };
}
