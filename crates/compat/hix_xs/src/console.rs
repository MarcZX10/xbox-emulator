use crate::kmd;
use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn HeapAlloc(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void;
    fn HeapFree(heap: *mut c_void, flags: u32, p: *mut c_void) -> i32;
    fn GetProcessHeap() -> *mut c_void;
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, tmpl: isize) -> isize;
    fn DeviceIoControl(h: isize, code: u32, inb: *const u8, inl: u32, outb: *mut u8, outl: u32, ret: *mut u32, ov: *mut c_void) -> i32;
    fn CloseHandle(h: isize) -> i32;
    fn DeleteFileW(name: *const u16) -> i32;
    fn GetFileAttributesW(name: *const u16) -> u32;
    fn CreateDirectoryW(name: *const u16, sa: *mut c_void) -> i32;
    fn FindFirstFileW(name: *const u16, data: *mut c_void) -> isize;
    fn CopyFile2(src: *const u16, dst: *const u16, params: *mut c_void) -> i32;
    fn SetLastError(e: u32);
}

extern "C" {
    fn fopen(name: *const u8, mode: *const u8) -> *mut c_void;
}

const ALTERED_PATH: u32 = 8;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const E_PATH_NOT_FOUND: i32 = 0x8007_0003u32 as i32;
const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;

pub static CONSOLE: console_image::ConsoleDll = console_image::ConsoleDll::new("hix_xs.dll", "TOMBOX_HIX_XS_CONSOLE", "hix_xs_console.dll", "InitializeHix", &[]);

pub fn own() -> usize {
    own as *const () as usize
}

pub fn scratch_root() -> PathBuf {
    if let Some(p) = std::env::var_os("TOMBOX_SCRATCH_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let t = compat_common::gdk::title();
    let id = if t.identity.is_empty() { "default".to_string() } else { t.identity.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '_', "_") };
    base.join("TomBox").join("scratch").join(id)
}

pub enum Mapped {
    Host(String),
    Title(PathBuf),
    Refused,
}

pub fn map_path(p: &str) -> Mapped {
    let s = p.strip_prefix(r"\\?\").unwrap_or(p).replace('/', "\\");
    let b = s.as_bytes();
    if b.len() < 2 || b[1] != b':' {
        return Mapped::Host(p.to_string());
    }
    let drive = (b[0] as char).to_ascii_lowercase();
    if drive != 'd' && drive != 't' {
        return Mapped::Host(p.to_string());
    }
    let tail = s[2..].trim_start_matches('\\');
    if tail.split('\\').any(|c| c == "..") {
        return Mapped::Refused;
    }
    let root = scratch_root().join(drive.to_string());
    if std::fs::create_dir_all(&root).is_err() {
        return Mapped::Refused;
    }
    Mapped::Title(if tail.is_empty() { root } else { root.join(tail) })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn from_wide(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    Some(String::from_utf16_lossy(core::slice::from_raw_parts(p, n)))
}

unsafe fn remap_w(p: *const u16) -> Result<Option<Vec<u16>>, ()> {
    let Some(s) = from_wide(p) else { return Ok(None) };
    match map_path(&s) {
        Mapped::Host(_) => Ok(None),
        Mapped::Title(t) => Ok(Some(wide(&t.to_string_lossy()))),
        Mapped::Refused => Err(()),
    }
}

macro_rules! with_path {
    ($p:expr, $fail:expr, |$q:ident| $body:expr) => {
        match remap_w($p) {
            Err(()) => {
                SetLastError(ERROR_PATH_NOT_FOUND);
                $fail
            }
            Ok(Some(v)) => {
                let $q = v.as_ptr();
                $body
            }
            Ok(None) => {
                let $q = $p;
                $body
            }
        }
    };
}

unsafe extern "system" fn h_create_file(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, tmpl: isize) -> isize {
    if from_wide(name).is_some_and(|n| kmd::is_device(&n)) {
        kmd::region0();
        kmd::region1();
        return kmd::PSEUDO;
    }
    with_path!(name, -1, |q| CreateFileW(q, access, share, sa, disp, flags, tmpl))
}

unsafe extern "system" fn h_ioctl(h: isize, code: u32, inb: *const u8, inl: u32, outb: *mut u8, outl: u32, ret: *mut u32, ov: *mut c_void) -> i32 {
    if h == kmd::PSEUDO {
        return kmd::ioctl(code, inb, inl, outb, outl, ret);
    }
    DeviceIoControl(h, code, inb, inl, outb, outl, ret, ov)
}

unsafe extern "system" fn h_close(h: isize) -> i32 {
    if h == kmd::PSEUDO {
        return 1;
    }
    CloseHandle(h)
}

unsafe extern "system" fn h_delete(name: *const u16) -> i32 {
    with_path!(name, 0, |q| DeleteFileW(q))
}

unsafe extern "system" fn h_attrs(name: *const u16) -> u32 {
    with_path!(name, INVALID_FILE_ATTRIBUTES, |q| GetFileAttributesW(q))
}

unsafe extern "system" fn h_mkdir(name: *const u16, sa: *mut c_void) -> i32 {
    with_path!(name, 0, |q| CreateDirectoryW(q, sa))
}

unsafe extern "system" fn h_find(name: *const u16, data: *mut c_void) -> isize {
    with_path!(name, -1, |q| FindFirstFileW(q, data))
}

unsafe extern "system" fn h_copy(src: *const u16, dst: *const u16, params: *mut c_void) -> i32 {
    with_path!(src, E_PATH_NOT_FOUND, |s| with_path!(dst, E_PATH_NOT_FOUND, |d| CopyFile2(s, d, params)))
}

unsafe extern "C" fn h_fopen(name: *const u8, mode: *const u8) -> *mut c_void {
    if name.is_null() {
        return fopen(name, mode);
    }
    let s = core::ffi::CStr::from_ptr(name as *const core::ffi::c_char).to_string_lossy().into_owned();
    match map_path(&s) {
        Mapped::Host(_) => fopen(name, mode),
        Mapped::Title(t) => {
            let z: Vec<u8> = t.to_string_lossy().bytes().chain(Some(0)).collect();
            fopen(z.as_ptr(), mode)
        }
        Mapped::Refused => core::ptr::null_mut(),
    }
}

unsafe extern "system" fn xmem_alloc(size: usize, _attrs: u64) -> *mut c_void {
    HeapAlloc(GetProcessHeap(), 0, size.max(1))
}

unsafe extern "system" fn xmem_free(p: *mut c_void, _attrs: u64) {
    if !p.is_null() {
        HeapFree(GetProcessHeap(), 0, p);
    }
}

fn hooks() -> Vec<(&'static str, usize)> {
    vec![
        ("CreateFileW", h_create_file as *const () as usize),
        ("DeviceIoControl", h_ioctl as *const () as usize),
        ("CloseHandle", h_close as *const () as usize),
        ("NtDirectGraphicsCall", kmd::direct_graphics_call as *const () as usize),
        ("DeleteFileW", h_delete as *const () as usize),
        ("GetFileAttributesW", h_attrs as *const () as usize),
        ("CreateDirectoryW", h_mkdir as *const () as usize),
        ("FindFirstFileW", h_find as *const () as usize),
        ("CopyFile2", h_copy as *const () as usize),
        ("_o_fopen", h_fopen as *const () as usize),
    ]
}

unsafe fn rd32(p: *const u8) -> u32 {
    (p as *const u32).read_unaligned()
}

pub unsafe fn imports(base: *const u8) -> Vec<(String, String, *mut usize)> {
    let pe = base.add(rd32(base.add(0x3c)) as usize);
    let dir = rd32(pe.add(24 + 112 + 8)) as usize;
    let mut out = Vec::new();
    if dir == 0 {
        return out;
    }
    let mut d = base.add(dir);
    while rd32(d.add(12)) != 0 {
        let dll = core::ffi::CStr::from_ptr(base.add(rd32(d.add(12)) as usize) as *const core::ffi::c_char).to_string_lossy().into_owned();
        let oft = rd32(d) as usize;
        let ft = base.add(rd32(d.add(16)) as usize) as *mut usize;
        if oft != 0 {
            let names = base.add(oft) as *const u64;
            let mut i = 0;
            while *names.add(i) != 0 {
                let v = *names.add(i);
                if v >> 63 == 0 {
                    let n = core::ffi::CStr::from_ptr(base.add((v & 0xffff_ffff) as usize + 2) as *const core::ffi::c_char).to_string_lossy().into_owned();
                    out.push((dll.clone(), n, ft.add(i)));
                }
                i += 1;
            }
        }
        d = d.add(20);
    }
    out
}

unsafe fn patch(slot: *mut usize, v: usize) -> bool {
    let mut old = 0u32;
    if VirtualProtect(slot as *mut c_void, 8, 4, &mut old) == 0 {
        return false;
    }
    *slot = v;
    VirtualProtect(slot as *mut c_void, 8, old, &mut old);
    true
}

unsafe fn prepare_xmem(dir: &std::path::Path) -> bool {
    if !GetModuleHandleW(wide("xmem.dll").as_ptr()).is_null() {
        return true;
    }
    let x = LoadLibraryExW(wide(&dir.join("xmem.dll").to_string_lossy()).as_ptr(), core::ptr::null_mut(), ALTERED_PATH);
    if x.is_null() {
        return false;
    }
    let f = GetProcAddress(x, b"XMemSetAllocationHooks\0".as_ptr());
    if f.is_null() {
        return false;
    }
    let set: unsafe extern "system" fn(usize, usize) -> i32 = core::mem::transmute(f);
    set(xmem_alloc as *const () as usize, xmem_free as *const () as usize);
    true
}

unsafe fn open() -> usize {
    for c in CONSOLE.candidates(own()) {
        if !c.is_file() {
            continue;
        }
        let Some(dir) = c.parent() else { continue };
        if !prepare_xmem(dir) {
            continue;
        }
        let h = LoadLibraryExW(wide(&c.to_string_lossy()).as_ptr(), core::ptr::null_mut(), ALTERED_PATH) as usize;
        if h == 0 || console_image::symbol(h, CONSOLE.probe).is_none() {
            continue;
        }
        let table = hooks();
        let mut ok = true;
        for (_, name, slot) in imports(h as *const u8) {
            if let Some((_, v)) = table.iter().find(|(n, _)| *n == name) {
                ok &= patch(slot, *v);
            }
        }
        if !ok {
            continue;
        }
        compat_common::record_raw("hix_xs.dll", "console", &format!("console hix_xs {} bound to the in-process XgsKmd10, d:\\ and t:\\ under {}", c.display(), scratch_root().display()));
        return h;
    }
    compat_common::record_raw("hix_xs.dll", "console", "no console hix_xs.dll found (TOMBOX_HIX_XS_CONSOLE, hix_xs_console.dll beside this DLL, or a title EmbeddedXvd dump)");
    0
}

pub fn module() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(|| unsafe { open() })
}

pub fn proc(name: &str) -> Option<*mut c_void> {
    console_image::symbol(module(), name)
}
