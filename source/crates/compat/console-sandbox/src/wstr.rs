pub unsafe fn wide_len(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn from_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, wide_len(p)))
}

pub unsafe fn from_unicode_string(us: *const u8) -> String {
    if us.is_null() {
        return String::new();
    }
    let len = (us as *const u16).read_unaligned() as usize / 2;
    let buf = (us.add(8) as *const *const u16).read_unaligned();
    if buf.is_null() || len == 0 {
        return String::new();
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(buf, len))
}

pub unsafe fn put_wide(dst: *mut u16, w: &[u16]) {
    core::ptr::copy_nonoverlapping(w.as_ptr(), dst, w.len());
    *dst.add(w.len()) = 0;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
