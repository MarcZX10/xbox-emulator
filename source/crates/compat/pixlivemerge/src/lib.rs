use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub mod msvc;
pub mod ntpath;
pub mod livemerge;
use msvc::{AString, Record, RecordVec, WString};

pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const HR_NOT_FOUND: i32 = 0x8007_0490u32 as i32;
pub const PDB_CHARS: usize = 0x30c;
pub const VBI_KEY: &str = "System\\Software\\Microsoft";
pub const HKEY_LOCAL_MACHINE: isize = 0xffff_ffff_8000_0002u64 as isize;
pub const KEY_READ: u32 = 0x20019;

#[link(name = "rpcrt4")]
extern "system" {
    fn UuidFromStringW(s: *const u16, out: *mut [u8; 16]) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn WideCharToMultiByte(cp: u32, fl: u32, w: *const u16, wn: i32, a: *mut u8, an: i32, d: *const u8, u: *mut i32) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(k: isize, sub: *const u16, opt: u32, sam: u32, out: *mut isize) -> i32;
    fn RegEnumValueW(k: isize, i: u32, name: *mut u16, nl: *mut u32, r: *mut u32, t: *mut u32, d: *mut u8, dl: *mut u32) -> i32;
    fn RegQueryValueExW(k: isize, name: *const u16, r: *mut u32, t: *mut u32, d: *mut u8, dl: *mut u32) -> i32;
    fn RegCloseKey(k: isize) -> i32;
}

struct Global(RecordVec);
unsafe impl Send for Global {}

static RECORDS: Mutex<Global> = Mutex::new(Global(RecordVec::new()));
static VBI_SEARCHED: AtomicBool = AtomicBool::new(false);
static IMAGE_NAME_CACHE: AtomicBool = AtomicBool::new(true);

pub fn hr_win32(e: i32) -> i32 {
    if e <= 0 {
        e
    } else {
        ((e as u32 & 0xffff) | 0x8007_0000) as i32
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn wcslen(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

unsafe fn wcschr(p: *mut u16, c: u16) -> *mut u16 {
    let mut q = p;
    while *q != 0 {
        if *q == c {
            return q;
        }
        q = q.add(1);
    }
    core::ptr::null_mut()
}

fn fold(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + 32
    } else {
        c
    }
}

pub fn wcsnicmp_eq(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| fold(*x) == fold(*y))
}

pub unsafe fn wcstol10(p: *const u16) -> u32 {
    let mut q = p;
    while matches!(*q, 0x20 | 0x09..=0x0d) {
        q = q.add(1);
    }
    let mut neg = false;
    if *q == b'-' as u16 || *q == b'+' as u16 {
        neg = *q == b'-' as u16;
        q = q.add(1);
    }
    let mut v: i64 = 0;
    let mut over = false;
    while (b'0' as u16..=b'9' as u16).contains(&*q) {
        v = v * 10 + (*q - b'0' as u16) as i64;
        if v > i32::MAX as i64 + 1 {
            over = true;
            v = i32::MAX as i64 + 1;
        }
        q = q.add(1);
    }
    let r = if over || (!neg && v > i32::MAX as i64) {
        if neg { i32::MIN } else { i32::MAX }
    } else if neg {
        (-v) as i32
    } else {
        v as i32
    };
    r as u32
}

pub unsafe fn parse_into(contents: *mut u16, count: usize, out: &mut RecordVec) -> i32 {
    let mut pos = 0usize;
    let mut len = wcslen(contents);
    while len != 0 && pos < count {
        let entry = contents.add(pos);
        let at = wcschr(entry, b'@' as u16);
        if at.is_null() {
            return E_FAIL;
        }
        let rb = wcschr(at, b'}' as u16);
        if rb.is_null() {
            return E_FAIL;
        }
        if rb.add(1) >= entry.add(len - 1) {
            return E_FAIL;
        }
        let age_at = rb.add(2);
        let sp = wcschr(age_at, b' ' as u16);
        if sp.is_null() {
            return E_FAIL;
        }
        *at = 0;
        let name = WString::from_slice(core::slice::from_raw_parts(entry, wcslen(entry)));
        *rb = 0;
        let mut guid = [0u8; 16];
        let mut name = name;
        if UuidFromStringW(at.add(2), &mut guid) != 0 {
            name.drop_in_place();
            return E_FAIL;
        }
        let age = wcstol10(age_at);
        let mut buf = vec![0u8; PDB_CHARS];
        if WideCharToMultiByte(0, 0, sp.add(1), -1, buf.as_mut_ptr(), PDB_CHARS as i32, core::ptr::null(), core::ptr::null_mut()) <= 0 {
            name.drop_in_place();
            return E_FAIL;
        }
        out.push(Record { name, guid, age, pad: 0, pdb: AString::copy_of(&buf) });
        pos += 2 + len;
        if pos < count {
            len = wcslen(contents.add(pos));
        }
    }
    out.as_mut_slice().sort_by_key(|r| r.name.size);
    0
}

#[no_mangle]
pub unsafe extern "system" fn ParsePdbInfo(contents: *mut u16, count: usize, out: *mut RecordVec) -> i32 {
    parse_into(contents, count, &mut *out)
}

pub unsafe fn find_in(name: *const u16, v: &RecordVec) -> i32 {
    for (i, r) in v.as_slice().iter().enumerate() {
        let rn = r.name.data();
        if name.is_null() || rn.is_null() {
            continue;
        }
        let n = wcslen(name);
        let m = wcslen(rn);
        if m > n {
            continue;
        }
        let tail = core::slice::from_raw_parts(name.add(n - m), m);
        let rs = core::slice::from_raw_parts(rn, m);
        if wcsnicmp_eq(tail, rs) {
            return i as i32;
        }
    }
    -1
}

#[no_mangle]
pub unsafe extern "system" fn FindPdbInfo(name: *const u16, v: *const RecordVec) -> i32 {
    find_in(name, &*v)
}

pub unsafe fn load_vbi_from(key: &str, out: &mut RecordVec) -> i32 {
    let k = wide(key);
    let mut h: isize = 0;
    let st = RegOpenKeyExW(HKEY_LOCAL_MACHINE, k.as_ptr(), 0, KEY_READ, &mut h);
    if st != 0 && hr_win32(st) < 0 {
        return hr_win32(st);
    }
    let mut i = 0u32;
    let r = loop {
        let mut name = [0u16; 0x100];
        let mut nl = 0x100u32;
        let st = RegEnumValueW(h, i, name.as_mut_ptr(), &mut nl, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        if st != 0 {
            let hr = hr_win32(st);
            break if hr < 0 { hr } else { HR_NOT_FOUND };
        }
        let n = wcslen(name.as_ptr());
        let tag: Vec<u16> = "_vbi".encode_utf16().collect();
        if n < 4 || !wcsnicmp_eq(&name[n - 4..n], &tag) {
            i += 1;
            continue;
        }
        let mut size = 0u32;
        let st = RegQueryValueExW(h, name.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), &mut size);
        if st != 0 && hr_win32(st) < 0 {
            break hr_win32(st);
        }
        let mut data = vec![0u16; size as usize];
        let st = RegQueryValueExW(h, name.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut(), data.as_mut_ptr() as *mut u8, &mut size);
        if st != 0 && hr_win32(st) < 0 {
            break hr_win32(st);
        }
        if data.is_empty() {
            data.push(0);
        }
        let hr = parse_into(data.as_mut_ptr(), data.len(), out);
        break if hr < 0 { hr } else { 0 };
    };
    if h != 0 {
        RegCloseKey(h);
    }
    r
}

pub unsafe fn write_info(r: &Record, out: *mut u8) {
    core::ptr::copy_nonoverlapping(r.guid.as_ptr(), out, 16);
    (out.add(0x10) as *mut u32).write_unaligned(r.age);
    let dst = &mut *(out.add(0x18) as *mut AString);
    if !core::ptr::eq(dst, &r.pdb) {
        dst.assign(r.pdb.as_slice());
    }
}

#[no_mangle]
pub unsafe extern "system" fn PdbInfoFromVBI(name: *const u16, out: *mut u8) -> i32 {
    let mut g = RECORDS.lock().unwrap();
    if !VBI_SEARCHED.load(Ordering::SeqCst) {
        let hr = load_vbi_from(VBI_KEY, &mut g.0);
        VBI_SEARCHED.store(true, Ordering::SeqCst);
        if hr < 0 {
            return hr;
        }
    }
    let i = find_in(name, &g.0);
    if i < 0 {
        return HR_NOT_FOUND;
    }
    write_info(&g.0.as_slice()[i as usize], out);
    0
}

#[no_mangle]
pub extern "system" fn GetVbiRecordsSearched() -> bool {
    VBI_SEARCHED.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "system" fn ResetVbiRecordsSearched() {
    VBI_SEARCHED.store(false, Ordering::SeqCst);
}

#[no_mangle]
pub extern "system" fn SetImageFileNameCacheUsage(on: bool) {
    IMAGE_NAME_CACHE.store(on, Ordering::SeqCst);
}

pub fn image_name_cache_enabled() -> bool {
    IMAGE_NAME_CACHE.load(Ordering::SeqCst)
}

pub unsafe fn global_records<R>(f: impl FnOnce(&mut RecordVec) -> R) -> R {
    f(&mut RECORDS.lock().unwrap().0)
}

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod livemerge_tests;
#[cfg(test)]
mod ntpath_tests;
#[cfg(test)]
mod livemerge_etw_tests;
