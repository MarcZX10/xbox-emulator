use core::ffi::c_void;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    Data(usize),
    Import(usize, &'static str),
    StubRelease,
    StubRelease2,
    Lookup(usize),
    Runtime,
}

pub struct Tables {
    pub blob: &'static [u8],
    pub proxy_file_list: usize,
    pub clsid: usize,
    pub factory: usize,
    pub files: &'static [usize],
    pub dlls: &'static [&'static str],
    pub fixups: &'static [(usize, Fix)],
    pub rdata_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const CLASS_E_CLASSNOTAVAILABLE: i32 = 0x8004_0111u32 as i32;
pub const MAX_FILES: usize = 32;

#[link(name = "rpcrt4")]
extern "system" {
    fn NdrDllGetClassObject(clsid: *const Guid, iid: *const Guid, ppv: *mut *mut c_void, files: *const c_void, pclsid: *const Guid, factory: *mut c_void) -> i32;
    fn NdrDllCanUnloadNow(factory: *mut c_void) -> i32;
    fn NdrDllRegisterProxy(module: *mut c_void, files: *const c_void, pclsid: *const Guid) -> i32;
    fn NdrDllUnregisterProxy(module: *mut c_void, files: *const c_void, pclsid: *const Guid) -> i32;
    fn NdrCStdStubBuffer_Release(this: *mut c_void, factory: *mut c_void) -> u32;
    fn NdrCStdStubBuffer2_Release(this: *mut c_void, factory: *mut c_void) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
}

pub struct Image {
    base: usize,
    tables: &'static Tables,
}

static IMAGE: OnceLock<Result<Image, String>> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn build(t: &'static Tables) -> Result<Image, String> {
    if t.files.len() > MAX_FILES {
        return Err(format!("{} proxy files, at most {MAX_FILES} lookup routines", t.files.len()));
    }
    let words = t.blob.len().div_ceil(8);
    let mut mem: Vec<u64> = vec![0; words];
    let bytes = unsafe { core::slice::from_raw_parts_mut(mem.as_mut_ptr() as *mut u8, words * 8) };
    bytes[..t.blob.len()].copy_from_slice(t.blob);
    let base = mem.as_mut_ptr() as usize;
    let mut modules = Vec::with_capacity(t.dlls.len());
    for d in t.dlls {
        let h = unsafe { LoadLibraryW(wide(d).as_ptr()) };
        if h.is_null() {
            return Err(format!("{d} not loadable"));
        }
        modules.push(h);
    }
    for &(off, fix) in t.fixups {
        let v = match fix {
            Fix::Data(o) => base + o,
            Fix::Import(m, name) => {
                let c = format!("{name}\0");
                let f = unsafe { GetProcAddress(modules[m], c.as_ptr()) };
                if f.is_null() {
                    return Err(format!("{}!{name} not exported", t.dlls[m]));
                }
                f as usize
            }
            Fix::StubRelease => stub_release as *const () as usize,
            Fix::StubRelease2 => stub_release2 as *const () as usize,
            Fix::Lookup(n) => LOOKUPS[n] as usize,
            Fix::Runtime => 0,
        };
        bytes[off..off + 8].copy_from_slice(&(v as u64).to_le_bytes());
    }
    core::mem::forget(mem);
    Ok(Image { base, tables: t })
}

pub fn image(t: &'static Tables) -> Result<&'static Image, &'static str> {
    match IMAGE.get_or_init(|| build(t)) {
        Ok(i) if core::ptr::eq(i.tables, t) => Ok(i),
        Ok(_) => Err("a different proxy table is already loaded in this module"),
        Err(e) => Err(e.as_str()),
    }
}

fn loaded() -> &'static Image {
    match IMAGE.get() {
        Some(Ok(i)) => i,
        _ => panic!("proxy tables used before they were built"),
    }
}

fn must(t: &'static Tables) -> &'static Image {
    match image(t) {
        Ok(i) => i,
        Err(e) => panic!("console proxy tables: {e}"),
    }
}

impl Image {
    pub fn at(&self, off: usize) -> *mut c_void {
        (self.base + off) as *mut c_void
    }
    pub fn proxy_file_list(&self) -> *const c_void {
        self.at(self.tables.proxy_file_list)
    }
    pub fn clsid(&self) -> *const Guid {
        self.at(self.tables.clsid) as *const Guid
    }
    pub fn factory(&self) -> *mut c_void {
        self.at(self.tables.factory)
    }
    pub fn file(&self, n: usize) -> *const usize {
        self.at(self.tables.files[n]) as *const usize
    }
}

unsafe extern "system" fn stub_release(this: *mut c_void) -> u32 {
    NdrCStdStubBuffer_Release(this, loaded().factory())
}

unsafe extern "system" fn stub_release2(this: *mut c_void) -> u32 {
    NdrCStdStubBuffer2_Release(this, loaded().factory())
}

pub unsafe fn lookup_in(file: *const usize, iid: *const Guid, index: *mut i32) -> i32 {
    let proxies = *file as *const usize;
    let size = *((file as *const u8).add(40) as *const u16) as usize;
    for i in 0..size {
        let vtbl = *proxies.add(i) as *const usize;
        let piid = *vtbl.add(1) as *const Guid;
        if *piid == *iid {
            *index = i as i32;
            return 1;
        }
    }
    0
}

macro_rules! lookups {
    ($($n:literal)*) => {
        static LOOKUPS: [unsafe extern "system" fn(*const Guid, *mut i32) -> i32; MAX_FILES] = [$({
            unsafe extern "system" fn f(iid: *const Guid, index: *mut i32) -> i32 {
                lookup_in(loaded().file($n), iid, index)
            }
            f
        }),*];
    };
}

lookups!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31);

fn this_module() -> *mut c_void {
    let mut h = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(6, this_module as *const c_void, &mut h) };
    h
}

pub unsafe fn get_proxy_dll_info(t: &'static Tables, info: *mut *const c_void, id: *mut *const Guid) {
    let i = must(t);
    *info = i.proxy_file_list();
    *id = i.clsid();
}

pub unsafe fn dll_get_class_object(t: &'static Tables, clsid: *const Guid, iid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    let i = must(t);
    NdrDllGetClassObject(clsid, iid, ppv, i.proxy_file_list(), i.clsid(), i.factory())
}

pub unsafe fn dll_can_unload_now(t: &'static Tables) -> i32 {
    NdrDllCanUnloadNow(must(t).factory())
}

pub unsafe fn dll_register_server(t: &'static Tables) -> i32 {
    let i = must(t);
    NdrDllRegisterProxy(this_module(), i.proxy_file_list(), i.clsid())
}

pub unsafe fn dll_unregister_server(t: &'static Tables) -> i32 {
    let i = must(t);
    NdrDllUnregisterProxy(this_module(), i.proxy_file_list(), i.clsid())
}

#[cfg(feature = "testing")]
pub mod testing;
