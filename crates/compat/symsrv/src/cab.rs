use crate::store::{dir_of, fname_ext, sibling};
use crate::win::*;
use core::ffi::c_void;

#[repr(C)]
#[derive(Default)]
pub struct Erf {
    pub oper: i32,
    pub typ: i32,
    pub error: i32,
}

#[repr(C)]
pub struct FdiNotification {
    pub cb: i32,
    pub psz1: *mut u8,
    pub psz2: *mut u8,
    pub psz3: *mut u8,
    pub pv: *mut c_void,
    pub hf: isize,
    pub date: u16,
    pub time: u16,
    pub attribs: u16,
    pub set_id: u16,
    pub i_cabinet: u16,
    pub i_folder: u16,
    pub fdie: i32,
}

#[repr(C)]
pub struct Ccab {
    pub cb: u32,
    pub folder_thresh: u32,
    pub reserve_header: u32,
    pub reserve_folder: u32,
    pub reserve_data: u32,
    pub i_cab: i32,
    pub i_disk: i32,
    pub fail_on_incompressible: i32,
    pub set_id: u16,
    pub disk: [u8; 256],
    pub cab: [u8; 256],
    pub cab_path: [u8; 256],
}

type Notify = unsafe extern "C" fn(i32, *mut FdiNotification) -> isize;

#[link(name = "cabinet")]
extern "C" {
    fn FDICreate(alloc: *const (), free: *const (), open: *const (), read: *const (), write: *const (), close: *const (), seek: *const (), cpu: i32, erf: *mut Erf) -> *mut c_void;
    fn FDICopy(h: *mut c_void, cab: *const u8, path: *const u8, flags: i32, notify: Notify, dec: *mut c_void, user: *mut c_void) -> i32;
    fn FDIDestroy(h: *mut c_void) -> i32;
    fn FCICreate(erf: *mut Erf, placed: *const (), alloc: *const (), free: *const (), open: *const (), read: *const (), write: *const (), close: *const (), seek: *const (), delete: *const (), temp: *const (), ccab: *mut Ccab, pv: *mut c_void) -> *mut c_void;
    fn FCIAddFile(h: *mut c_void, src: *const u8, name: *const u8, exec: i32, next: *const (), status: *const (), info: *const (), comp: u16) -> i32;
    fn FCIFlushCabinet(h: *mut c_void, next_cab: i32, next: *const (), status: *const ()) -> i32;
    fn FCIDestroy(h: *mut c_void) -> i32;
}

#[link(name = "lz32")]
extern "system" {
    fn LZInit(h: i32) -> i32;
    fn LZCopy(a: i32, b: i32) -> i32;
    fn LZClose(h: i32);
}

#[link(name = "kernel32")]
extern "system" {
    fn GetFileTime(h: isize, c: *mut u64, a: *mut u64, w: *mut u64) -> i32;
    fn FileTimeToLocalFileTime(a: *const u64, b: *mut u64) -> i32;
    fn FileTimeToDosDateTime(f: *const u64, d: *mut u16, t: *mut u16) -> i32;
    fn GetFileAttributesA(p: *const u8) -> u32;
}

pub const SZDD: [u8; 8] = [0x53, 0x5a, 0x44, 0x44, 0x88, 0xf0, 0x27, 0x33];
pub const LZX21: u16 = 0x1503;

fn ansi(v: &[u16]) -> Vec<u8> {
    let mut a = w2a(v);
    a.push(0);
    a
}

unsafe extern "C" fn mem_alloc(cb: u32) -> *mut c_void {
    let l = std::alloc::Layout::from_size_align((cb as usize).max(1) + 16, 16).unwrap();
    let p = std::alloc::alloc(l);
    if p.is_null() {
        return p as *mut c_void;
    }
    (p as *mut usize).write(l.size());
    p.add(16) as *mut c_void
}

unsafe extern "C" fn mem_free(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let b = (p as *mut u8).sub(16);
    let n = (b as *const usize).read();
    std::alloc::dealloc(b, std::alloc::Layout::from_size_align(n, 16).unwrap());
}

unsafe fn open_a(path: *const u8, oflag: i32) -> isize {
    let access = if oflag & 3 == 0 { GENERIC_READ } else { GENERIC_READ | GENERIC_WRITE };
    let disp = if oflag & 0x100 != 0 {
        if oflag & 0x400 != 0 {
            1
        } else if oflag & 0x200 != 0 {
            2
        } else {
            4
        }
    } else if oflag & 0x200 != 0 {
        5
    } else {
        3
    };
    CreateFileA(path, access, 1, core::ptr::null(), disp, 0x80, 0)
}

unsafe extern "C" fn fdi_open(path: *const u8, oflag: i32, _mode: i32) -> isize {
    open_a(path, oflag)
}

unsafe extern "C" fn fdi_read(h: isize, p: *mut c_void, cb: u32) -> u32 {
    let mut n = 0u32;
    if ReadFile(h, p as *mut u8, cb, &mut n, core::ptr::null_mut()) == 0 {
        return u32::MAX;
    }
    n
}

unsafe extern "C" fn fdi_write(h: isize, p: *mut c_void, cb: u32) -> u32 {
    let mut n = 0u32;
    if WriteFile(h, p as *const u8, cb, &mut n, core::ptr::null_mut()) == 0 {
        return u32::MAX;
    }
    n
}

unsafe extern "C" fn fdi_close(h: isize) -> i32 {
    if CloseHandle(h) != 0 {
        0
    } else {
        -1
    }
}

unsafe extern "C" fn fdi_seek(h: isize, dist: i32, typ: i32) -> i32 {
    SetFilePointer(h, dist, core::ptr::null_mut(), typ as u32) as i32
}

struct Expand {
    out: Vec<u8>,
    code: u32,
}

unsafe extern "C" fn fdi_notify(kind: i32, n: *mut FdiNotification) -> isize {
    let ctx = &mut *((*n).pv as *mut Expand);
    match kind {
        2 => {
            let h = CreateFileA(ctx.out.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, core::ptr::null(), 2, 0x80, 0);
            if h == INVALID {
                ctx.code = GetLastError();
            }
            h
        }
        3 => {
            CloseHandle((*n).hf);
            ctx.code = 0;
            1
        }
        _ => 0,
    }
}

pub fn fdi_expand(src: &[u16], tmp: &[u16]) -> u32 {
    unsafe {
        let mut erf = Erf::default();
        let h = FDICreate(mem_alloc as *const (), mem_free as *const (), fdi_open as *const (), fdi_read as *const (), fdi_write as *const (), fdi_close as *const (), fdi_seek as *const (), 1, &mut erf);
        if h.is_null() {
            let e = GetLastError();
            return if e == 0 { 8 } else { e };
        }
        let name = ansi(&fname_ext(src));
        let mut dir = dir_of(src);
        if !dir.is_empty() {
            dir.push(b'\\' as u16);
        }
        let dir = ansi(&dir);
        let mut ctx = Expand { out: ansi(tmp), code: 2 };
        let ok = FDICopy(h, name.as_ptr(), dir.as_ptr(), 0, fdi_notify, core::ptr::null_mut(), &mut ctx as *mut Expand as *mut c_void);
        FDIDestroy(h);
        if ok != 0 {
            return ctx.code;
        }
        if ctx.code == 2 && erf.oper >= 2 && (erf.oper <= 4 || (erf.oper - 6) as u32 <= 1) {
            return 0x570;
        }
        ctx.code
    }
}

pub fn is_szdd(src: &[u16]) -> bool {
    std::fs::File::open(String::from_utf16_lossy(src))
        .and_then(|mut f| {
            let mut b = [0u8; 8];
            std::io::Read::read_exact(&mut f, &mut b).map(|_| b == SZDD)
        })
        .unwrap_or(false)
}

pub fn lz_error(r: i32) -> u32 {
    match r {
        0.. => 0,
        -1 | -3 => 0x1e,
        -2 | -4 => 0x1d,
        -6 | -5 => 8,
        -8 | -7 => 0xd,
        _ => 1,
    }
}

pub fn lz_expand(src: &[u16], dst: &[u16]) -> u32 {
    if !is_szdd(src) {
        return 0x570;
    }
    unsafe {
        let a = ansi(src);
        let b = ansi(dst);
        let hs = CreateFileA(a.as_ptr(), GENERIC_READ, 1, core::ptr::null(), 3, 0x80, 0);
        if hs == INVALID {
            return lz_error(-1);
        }
        let ls = LZInit(hs as i32);
        if ls < 0 {
            CloseHandle(hs);
            return lz_error(ls);
        }
        let hd = CreateFileA(b.as_ptr(), GENERIC_READ | GENERIC_WRITE, 1, core::ptr::null(), 1, 0x80, 0);
        let r = if hd == INVALID {
            -2
        } else {
            let ld = LZInit(hd as i32);
            if ld < 0 {
                CloseHandle(hd);
                ld
            } else {
                let r = LZCopy(ls, ld);
                LZClose(ld);
                if r < 0 {
                    r
                } else {
                    0
                }
            }
        };
        LZClose(ls);
        lz_error(r)
    }
}

pub fn expand(src: &[u16], dst: &[u16]) -> u32 {
    let tmp = sibling(dst, "download.error");
    let e = fdi_expand(src, &tmp);
    if e == 0 {
        unsafe {
            MoveFileW(z(&tmp).as_ptr(), z(dst).as_ptr());
        }
        return 0;
    }
    unsafe { DeleteFileW(z(&tmp).as_ptr()) };
    if is_szdd(src) {
        return lz_expand(src, dst);
    }
    e
}

unsafe extern "C" fn fci_placed(_c: *mut Ccab, _f: *const u8, _cb: i32, _cont: i32, _pv: *mut c_void) -> i32 {
    0
}

unsafe extern "C" fn fci_open(path: *const u8, oflag: i32, _mode: i32, err: *mut i32, _pv: *mut c_void) -> isize {
    let h = open_a(path, oflag);
    if h == INVALID {
        *err = GetLastError() as i32;
    }
    h
}

unsafe extern "C" fn fci_read(h: isize, p: *mut c_void, cb: u32, err: *mut i32, _pv: *mut c_void) -> u32 {
    let r = fdi_read(h, p, cb);
    if r == u32::MAX {
        *err = GetLastError() as i32;
    }
    r
}

unsafe extern "C" fn fci_write(h: isize, p: *mut c_void, cb: u32, err: *mut i32, _pv: *mut c_void) -> u32 {
    let r = fdi_write(h, p, cb);
    if r == u32::MAX {
        *err = GetLastError() as i32;
    }
    r
}

unsafe extern "C" fn fci_close(h: isize, err: *mut i32, _pv: *mut c_void) -> i32 {
    let r = fdi_close(h);
    if r != 0 {
        *err = GetLastError() as i32;
    }
    r
}

unsafe extern "C" fn fci_seek(h: isize, dist: i32, typ: i32, err: *mut i32, _pv: *mut c_void) -> i32 {
    let r = fdi_seek(h, dist, typ);
    if r == -1 {
        *err = GetLastError() as i32;
    }
    r
}

unsafe extern "C" fn fci_delete(path: *const u8, err: *mut i32, _pv: *mut c_void) -> i32 {
    if DeleteFileA(path) == 0 {
        *err = GetLastError() as i32;
        return -1;
    }
    0
}

static TEMP_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "C" fn fci_temp(out: *mut u8, cb: i32, _pv: *mut c_void) -> i32 {
    let n = TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("symsrv-fci-{}-{n}.tmp", std::process::id()));
    let a = ansi(&w(&p.to_string_lossy()));
    if a.len() > cb as usize {
        return 0;
    }
    core::ptr::copy_nonoverlapping(a.as_ptr(), out, a.len());
    1
}

unsafe extern "C" fn fci_next(_c: *mut Ccab, _prev: u32, _pv: *mut c_void) -> i32 {
    0
}

unsafe extern "C" fn fci_status(_t: u32, _a: u32, _b: u32, _pv: *mut c_void) -> i32 {
    0
}

unsafe extern "C" fn fci_info(path: *const u8, date: *mut u16, time: *mut u16, attribs: *mut u16, err: *mut i32, _pv: *mut c_void) -> isize {
    let h = open_a(path, 0);
    if h == INVALID {
        *err = GetLastError() as i32;
        return h;
    }
    let mut ft = 0u64;
    let mut lt = 0u64;
    GetFileTime(h, core::ptr::null_mut(), core::ptr::null_mut(), &mut ft);
    FileTimeToLocalFileTime(&ft, &mut lt);
    FileTimeToDosDateTime(&lt, date, time);
    let a = GetFileAttributesA(path);
    *attribs = if a == u32::MAX { 0 } else { (a & 0x27) as u16 };
    h
}

pub fn compress(src: &[u16], dst: &[u16]) -> u32 {
    unsafe {
        let mut ccab: Box<Ccab> = Box::new(core::mem::zeroed());
        ccab.cb = 0x0fff_ffff;
        ccab.folder_thresh = 0x7fff_ffff;
        ccab.i_cab = 1;
        let cab = w2a(&fname_ext(dst));
        let mut dir = dir_of(dst);
        dir.push(b'\\' as u16);
        let dir = w2a(&dir);
        if cab.len() >= 256 || dir.len() >= 256 {
            return 206;
        }
        ccab.cab[..cab.len()].copy_from_slice(&cab);
        ccab.cab_path[..dir.len()].copy_from_slice(&dir);
        let mut erf = Erf::default();
        let h = FCICreate(&mut erf, fci_placed as *const (), mem_alloc as *const (), mem_free as *const (), fci_open as *const (), fci_read as *const (), fci_write as *const (), fci_close as *const (), fci_seek as *const (), fci_delete as *const (), fci_temp as *const (), &mut *ccab, core::ptr::null_mut());
        if h.is_null() {
            return 8;
        }
        let s = ansi(src);
        let n = ansi(&fname_ext(src));
        let added = FCIAddFile(h, s.as_ptr(), n.as_ptr(), 0, fci_next as *const (), fci_status as *const (), fci_info as *const (), LZX21) != 0;
        let flushed = FCIFlushCabinet(h, 0, fci_next as *const (), fci_status as *const ()) != 0;
        let destroyed = FCIDestroy(h) != 0;
        if added && flushed && destroyed {
            0
        } else if erf.typ != 0 {
            erf.typ as u32
        } else {
            1
        }
    }
}
