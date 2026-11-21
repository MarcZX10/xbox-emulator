use std::sync::Mutex;
use xtfcommon::printf::sprintf_s;
use xtfcommon::sys::*;

extern "C" {
    fn __p__wpgmptr() -> *mut *mut u16;
}

pub const DEFAULT_PATTERN: &str = "#PROC#-#NOW#_#PROCID#.txt";

pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn find(s: &[u16], pat: &[u16], from: usize) -> Option<usize> {
    if pat.is_empty() {
        return if from <= s.len() { Some(from) } else { None };
    }
    if s.len() < pat.len() {
        return None;
    }
    (from..=s.len() - pat.len()).find(|&i| &s[i..i + pat.len()] == pat)
}

pub fn replace_all(s: &mut Vec<u16>, pat: &[u16], rep: &[u16]) {
    let mut from = 0;
    while let Some(p) = find(s, pat, from) {
        s.splice(p..p + pat.len(), rep.iter().copied());
        from = p + rep.len();
    }
}

pub fn rfind_char(s: &[u16], c: u16) -> Option<usize> {
    s.iter().rposition(|&x| x == c)
}

pub fn basename(s: &[u16]) -> Vec<u16> {
    match rfind_char(s, b'\\' as u16) {
        Some(p) if p + 1 < s.len() => s[p + 1..].to_vec(),
        _ => s.to_vec(),
    }
}

pub unsafe fn wpgmptr() -> Vec<u16> {
    let pp = __p__wpgmptr();
    if pp.is_null() || (*pp).is_null() {
        Vec::new()
    } else {
        wslice(*pp).to_vec()
    }
}

pub fn process_name() -> Vec<u16> {
    static NAME: Mutex<Vec<u16>> = Mutex::new(Vec::new());
    let mut cache = NAME.lock().unwrap_or_else(|e| e.into_inner());
    if cache.is_empty() {
        *cache = unsafe { wpgmptr() };
    }
    let mut s = cache.clone();
    drop(cache);
    if let Some(p) = rfind_char(&s, b'\\' as u16) {
        s = s[p + 1..].to_vec();
    }
    if let Some(p) = rfind_char(&s, b'.' as u16) {
        if p == 0 {
            s.clear();
        } else {
            s.truncate(p);
        }
    }
    s
}

fn narrow_print(fmt: &[u8], args: &[u64]) -> Option<Vec<u16>> {
    let mut buf = [0u8; 0x200];
    let n = unsafe { sprintf_s(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), args) };
    if n <= 0 {
        return None;
    }
    Some(buf[..n as usize].iter().map(|&b| b as u16).collect())
}

pub unsafe fn local_systemtime(ft: u64) -> SYSTEMTIME {
    let mut local = 0u64;
    let src = if FileTimeToLocalFileTime(&ft, &mut local) != 0 { local } else { ft };
    let mut st = SYSTEMTIME::default();
    FileTimeToSystemTime(&src, &mut st);
    st
}

pub fn expand(pattern: &[u16]) -> Vec<u16> {
    let mut s = pattern.to_vec();
    let mut ft = 0u64;
    unsafe { GetSystemTimePreciseAsFileTime(&mut ft) };
    let st = unsafe { local_systemtime(ft) };
    let now = narrow_print(
        b"%d-%02d-%02d-%02d-%02d-%02d-%03d\0",
        &[st.wYear as u64, st.wMonth as u64, st.wDay as u64, st.wHour as u64, st.wMinute as u64, st.wSecond as u64, st.wMilliseconds as u64],
    )
    .unwrap_or_default();
    replace_all(&mut s, &w("#NOW#"), &now);
    if find(&s, &w("#PROC#"), 0).is_some() {
        replace_all(&mut s, &w("#PROC#"), &process_name());
    }
    if find(&s, &w("#PROCID#"), 0).is_some() {
        if let Some(pid) = narrow_print(b"%d\0", &[unsafe { GetCurrentProcessId() } as u64]) {
            replace_all(&mut s, &w("#PROCID#"), &pid);
        }
    }
    s
}

fn z(s: &[u16]) -> Vec<u16> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn is_dir(p: &[u16]) -> bool {
    let a = unsafe { GetFileAttributesW(z(p).as_ptr()) };
    a != INVALID_FILE_ATTRIBUTES && a & FILE_ATTRIBUTE_DIRECTORY != 0
}

pub fn split_backslashes(p: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < p.len() {
        if p[i] == b'\\' as u16 {
            out.push(core::mem::take(&mut cur));
            while i < p.len() && p[i] == b'\\' as u16 {
                i += 1;
            }
            continue;
        }
        cur.push(p[i]);
        i += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn create_directory(p: &[u16], recursive: bool) -> bool {
    if is_dir(p) {
        return true;
    }
    if !recursive {
        return unsafe { CreateDirectoryW(z(p).as_ptr(), core::ptr::null_mut()) } != 0;
    }
    let mut acc: Vec<u16> = Vec::new();
    let mut prefixes = Vec::new();
    for t in split_backslashes(p) {
        acc.extend_from_slice(&t);
        acc.push(b'\\' as u16);
        prefixes.push(acc.clone());
    }
    for q in &prefixes {
        if !create_directory(q, false) {
            return false;
        }
    }
    true
}

pub fn ensure_directory_for(file: &[u16]) -> bool {
    let dot = rfind_char(file, b'.' as u16);
    let slash = rfind_char(file, b'\\' as u16);
    let target: Vec<u16> = match (dot, slash) {
        (None, _) => file.to_vec(),
        (Some(_), None) => return true,
        (Some(d), Some(s)) if d <= s => return true,
        (Some(_), Some(s)) => file[..s].to_vec(),
    };
    if target.is_empty() {
        return true;
    }
    create_directory(&target, true)
}
