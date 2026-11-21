use std::cell::Cell;
use std::sync::Mutex;

pub mod cab;
pub mod eula;
pub mod exclude;
pub mod http;
pub mod store;
pub mod trace;
pub mod win;

use store::Store;
use win::*;

pub const SSRVOPT_CALLBACK: u64 = 0x1;
pub const SSRVOPT_DWORD: u64 = 0x2;
pub const SSRVOPT_DWORDPTR: u64 = 0x4;
pub const SSRVOPT_GUIDPTR: u64 = 0x8;
pub const SSRVOPT_OLDGUIDPTR: u64 = 0x10;
pub const SSRVOPT_UNATTENDED: u64 = 0x20;
pub const SSRVOPT_NOCOPY: u64 = 0x40;
pub const SSRVOPT_PARENTWIN: u64 = 0x80;
pub const SSRVOPT_PARAMTYPE: u64 = 0x100;
pub const SSRVOPT_SECURE: u64 = 0x200;
pub const SSRVOPT_TRACE: u64 = 0x400;
pub const SSRVOPT_SETCONTEXT: u64 = 0x800;
pub const SSRVOPT_PROXY: u64 = 0x1000;
pub const SSRVOPT_DOWNSTREAM_STORE: u64 = 0x2000;
pub const SSRVOPT_OVERWRITE: u64 = 0x4000;
pub const SSRVOPT_RESETTOU: u64 = 0x8000;
pub const SSRVOPT_CALLBACKW: u64 = 0x10000;
pub const SSRVOPT_FLAT_DEFAULT_STORE: u64 = 0x20000;
pub const SSRVOPT_PROXYW: u64 = 0x40000;
pub const SSRVOPT_SERVICE: u64 = 0x100000;
pub const SSRVOPT_FAVOR_COMPRESSED: u64 = 0x200000;
pub const SSRVOPT_STRING: u64 = 0x400000;
pub const SSRVOPT_WINHTTP: u64 = 0x800000;
pub const SSRVOPT_WININET: u64 = 0x1000000;
pub const SSRVOPT_DONT_UNCOMPRESS: u64 = 0x2000000;
pub const SSRVOPT_DISABLE_PING_HOST: u64 = 0x4000000;
pub const SSRVOPT_DISABLE_TIMEOUT: u64 = 0x8000000;
pub const SSRVOPT_ENABLE_COMM_MSG: u64 = 0x10000000;

pub const TYPE_BITS: u64 = 0x40001e;
pub const MAX_PATH_W: usize = 0x105;
pub const API_MAJOR: u32 = 0xa;
pub const API_REVISION: u32 = 0x2908;

pub struct State {
    pub options: u64,
    pub param_type: u64,
    pub callback: usize,
    pub parent: isize,
    pub proxy: Vec<u16>,
    pub downstream: Vec<u16>,
    pub ping_disabled: bool,
    pub timeout_disabled: bool,
    pub comm_msg: bool,
}

pub static STATE: Mutex<State> = Mutex::new(State {
    options: 0,
    param_type: 0,
    callback: 0,
    parent: 0,
    proxy: Vec::new(),
    downstream: Vec::new(),
    ping_disabled: false,
    timeout_disabled: false,
    comm_msg: false,
});

thread_local! {
    static CONTEXT: Cell<u64> = const { Cell::new(0) };
}

pub fn options() -> u64 {
    STATE.lock().unwrap().options
}

pub fn has(o: u64) -> bool {
    options() & o != 0
}

pub static GLOBAL_CONTEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn context() -> u64 {
    let t = CONTEXT.with(|c| c.get());
    if t != 0 {
        t
    } else {
        GLOBAL_CONTEXT.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub fn downstream() -> Vec<u16> {
    STATE.lock().unwrap().downstream.clone()
}

pub fn reset_state() {
    let mut s = STATE.lock().unwrap();
    s.options = 0;
    s.param_type = 0;
    s.callback = 0;
    s.parent = 0;
    s.proxy.clear();
    s.downstream.clear();
    s.ping_disabled = false;
    s.timeout_disabled = false;
    s.comm_msg = false;
    drop(s);
    CONTEXT.with(|c| c.set(0));
    GLOBAL_CONTEXT.store(0, std::sync::atomic::Ordering::SeqCst);
    exclude::reset();
    store::DISABLED.lock().unwrap().clear();
}

fn bit(s: &mut State, b: u64, on: bool) {
    if on {
        s.options |= b;
    } else {
        s.options &= !b;
    }
}

pub unsafe fn set_options_w(opts: u64, data: u64) -> i32 {
    let mut s = STATE.lock().unwrap();
    let on = data != 0;
    if opts & SSRVOPT_CALLBACK != 0 {
        bit(&mut s, SSRVOPT_CALLBACK, on);
        s.callback = data as usize;
    }
    if opts & SSRVOPT_CALLBACKW != 0 {
        bit(&mut s, SSRVOPT_CALLBACKW, on);
        s.callback = data as usize;
    }
    if opts & SSRVOPT_SETCONTEXT != 0 {
        GLOBAL_CONTEXT.store(data, std::sync::atomic::Ordering::SeqCst);
        CONTEXT.with(|c| c.set(data));
    }
    if opts & SSRVOPT_TRACE != 0 {
        bit(&mut s, SSRVOPT_TRACE, on);
    }
    if opts & SSRVOPT_PARAMTYPE != 0 {
        if ![SSRVOPT_DWORD, SSRVOPT_DWORDPTR, SSRVOPT_GUIDPTR, SSRVOPT_OLDGUIDPTR, SSRVOPT_STRING].contains(&data) {
            SetLastError(87);
            return 0;
        }
        s.param_type = data;
        s.options &= !TYPE_BITS;
        s.options |= data;
    }
    for t in [SSRVOPT_DWORD, SSRVOPT_DWORDPTR, SSRVOPT_GUIDPTR, SSRVOPT_OLDGUIDPTR, SSRVOPT_STRING] {
        if opts & t == 0 {
            continue;
        }
        if on {
            s.param_type = t;
            s.options &= !TYPE_BITS;
            s.options |= t;
        } else if s.param_type == t {
            s.param_type = SSRVOPT_DWORD;
            s.options &= !TYPE_BITS;
            s.options |= SSRVOPT_DWORD;
        }
    }
    if opts & SSRVOPT_UNATTENDED != 0 {
        bit(&mut s, SSRVOPT_UNATTENDED, on);
    }
    if opts & SSRVOPT_NOCOPY != 0 {
        bit(&mut s, SSRVOPT_NOCOPY, on);
    }
    if opts & SSRVOPT_PARENTWIN != 0 {
        bit(&mut s, SSRVOPT_PARENTWIN, on);
        s.parent = data as isize;
    }
    if opts & SSRVOPT_SECURE != 0 {
        bit(&mut s, SSRVOPT_SECURE, on);
    }
    if opts & SSRVOPT_PROXY != 0 {
        bit(&mut s, SSRVOPT_PROXY, on);
        s.proxy = if on { a2w(data as *const u8, MAX_PATH_W - 1) } else { Vec::new() };
    }
    if opts & SSRVOPT_PROXYW != 0 {
        bit(&mut s, SSRVOPT_PROXYW, on);
        s.proxy = if on { wstr(data as *const u16).to_vec() } else { Vec::new() };
    }
    if opts & SSRVOPT_DOWNSTREAM_STORE != 0 {
        bit(&mut s, SSRVOPT_DOWNSTREAM_STORE, on);
        s.downstream = if on { wstr(data as *const u16).iter().take(MAX_PATH_W - 1).copied().collect() } else { Vec::new() };
    }
    if opts & SSRVOPT_OVERWRITE != 0 {
        bit(&mut s, SSRVOPT_OVERWRITE, on);
    }
    if opts & SSRVOPT_RESETTOU != 0 {
        bit(&mut s, SSRVOPT_RESETTOU, on);
        drop(s);
        eula::reset_tou();
        s = STATE.lock().unwrap();
    }
    for b in [SSRVOPT_FLAT_DEFAULT_STORE, SSRVOPT_SERVICE, SSRVOPT_FAVOR_COMPRESSED, SSRVOPT_WININET, SSRVOPT_WINHTTP, SSRVOPT_DONT_UNCOMPRESS] {
        if opts & b != 0 {
            bit(&mut s, b, on);
        }
    }
    if opts & SSRVOPT_DISABLE_PING_HOST != 0 {
        bit(&mut s, SSRVOPT_DISABLE_PING_HOST, on);
        s.ping_disabled = true;
    }
    if opts & SSRVOPT_DISABLE_TIMEOUT != 0 {
        bit(&mut s, SSRVOPT_DISABLE_TIMEOUT, on);
        s.timeout_disabled = true;
    }
    if opts & SSRVOPT_ENABLE_COMM_MSG != 0 {
        bit(&mut s, SSRVOPT_ENABLE_COMM_MSG, on);
        s.comm_msg = true;
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerSetOptionsW(opts: usize, data: u64) -> i32 {
    set_options_w(opts as u64, data)
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerSetOptions(opts: usize, data: u64) -> i32 {
    if opts as u64 & SSRVOPT_DOWNSTREAM_STORE != 0 && data != 0 {
        let w = z(&a2w(data as *const u8, MAX_PATH_W - 1));
        return set_options_w(opts as u64, w.as_ptr() as u64);
    }
    set_options_w(opts as u64, data)
}

#[no_mangle]
pub extern "system" fn SymbolServerGetOptions() -> usize {
    options() as usize
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerGetVersion(v: *mut u32) -> i32 {
    if v.is_null() {
        SetLastError(87);
        return 0;
    }
    *v = API_MAJOR;
    *v.add(1) = API_REVISION;
    1
}

fn hex_into(out: &mut String, v: u32, width: usize, upper: bool, zero: bool) {
    let s = if upper { format!("{v:X}") } else { format!("{v:x}") };
    let pad = width.saturating_sub(s.len());
    for _ in 0..pad {
        out.push(if zero { '0' } else { ' ' });
    }
    out.push_str(&s);
}

pub unsafe fn index_string(id: *const u8, two: u32, three: u32, param_type: u64) -> Vec<u16> {
    let mut s = String::new();
    match param_type {
        SSRVOPT_DWORD => {
            if !id.is_null() {
                hex_into(&mut s, id as usize as u32, 0, false, false);
            }
        }
        SSRVOPT_DWORDPTR => {
            if !id.is_null() {
                let v = (id as *const u32).read_unaligned();
                if v != 0 {
                    hex_into(&mut s, v, 0, false, false);
                }
            }
        }
        SSRVOPT_GUIDPTR | SSRVOPT_OLDGUIDPTR => {
            if !id.is_null() {
                let old = param_type == SSRVOPT_OLDGUIDPTR;
                let g = core::slice::from_raw_parts(id, 16);
                let d1 = u32::from_le_bytes([g[0], g[1], g[2], g[3]]);
                let d2 = u16::from_le_bytes([g[4], g[5]]) as u32;
                let d3 = u16::from_le_bytes([g[6], g[7]]) as u32;
                hex_into(&mut s, d1, 8, !old, !old);
                if d2 != 0 || d3 != 0 || g[8..16].iter().any(|b| *b != 0) {
                    hex_into(&mut s, d2, 4, !old, !old);
                    hex_into(&mut s, d3, 4, !old, !old);
                    for b in &g[8..16] {
                        hex_into(&mut s, *b as u32, 2, !old, !old);
                    }
                }
            }
        }
        SSRVOPT_STRING => {
            if !id.is_null() {
                s = String::from_utf16_lossy(wstr(id as *const u16));
            }
        }
        _ => {}
    }
    if two != 0 {
        hex_into(&mut s, two, 0, false, false);
    }
    if three != 0 {
        hex_into(&mut s, three, 0, false, false);
    }
    s.encode_utf16().collect()
}

pub fn param_type() -> u64 {
    STATE.lock().unwrap().param_type
}

unsafe fn copy_out(out: *mut u16, size: usize, v: &[u16]) -> bool {
    if out.is_null() || size == 0 || v.len() >= size {
        return false;
    }
    core::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
    *out.add(v.len()) = 0;
    true
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerGetIndexStringW(id: *const u8, two: u32, three: u32, out: *mut u16, size: usize) -> i32 {
    if out.is_null() {
        SetLastError(87);
        return 0;
    }
    if size > 0 {
        *out = 0;
    }
    let s = index_string(id, two, three, param_type());
    if !copy_out(out, size, &s) {
        SetLastError(122);
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerGetIndexString(id: *const u8, two: u32, three: u32, out: *mut u8, size: usize) -> i32 {
    if out.is_null() {
        SetLastError(87);
        return 0;
    }
    if size > 0 {
        *out = 0;
    }
    let s = index_string(id, two, three, param_type());
    let a = w2a(&s);
    if a.len() >= size {
        SetLastError(122);
        return 0;
    }
    core::ptr::copy_nonoverlapping(a.as_ptr(), out, a.len());
    *out.add(a.len()) = 0;
    1
}

pub unsafe fn delta_name(name: &[u16], i1: (*const u8, u32, u32), i2: (*const u8, u32, u32)) -> Vec<u16> {
    let pt = param_type();
    let mut v = index_string(i1.0, i1.1, i1.2, pt);
    v.push(b'-' as u16);
    v.extend(index_string(i2.0, i2.1, i2.2, pt));
    v.push(b'.' as u16);
    v.extend_from_slice(name);
    v
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerDeltaNameW(name: *const u16, id1: *const u8, two1: u32, three1: u32, id2: *const u8, two2: u32, three2: u32, out: *mut u16, size: usize) -> i32 {
    if out.is_null() || name.is_null() || *name == 0 {
        SetLastError(87);
        return 0;
    }
    let v = delta_name(wstr(name), (id1, two1, three1), (id2, two2, three2));
    if !copy_out(out, size, &v) {
        SetLastError(122);
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerDeltaName(name: *const u8, id1: *const u8, two1: u32, three1: u32, id2: *const u8, two2: u32, three2: u32, out: *mut u8, size: usize) -> i32 {
    if out.is_null() || name.is_null() || *name == 0 {
        SetLastError(87);
        return 0;
    }
    let v = w2a(&delta_name(&a2w(name, MAX_PATH_W - 1), (id1, two1, three1), (id2, two2, three2)));
    if v.len() >= size {
        SetLastError(122);
        return 0;
    }
    core::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
    *out.add(v.len()) = 0;
    1
}

pub fn split_params(p: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut rest = p;
    loop {
        if out.len() == 12 {
            break;
        }
        match rest.iter().position(|c| *c == b'*' as u16) {
            Some(i) if out.len() < 11 => {
                out.push(trim(&rest[..i]));
                rest = &rest[i + 1..];
            }
            _ => {
                out.push(trim(rest));
                break;
            }
        }
    }
    out
}

fn trim(s: &[u16]) -> Vec<u16> {
    let ws = |c: &u16| matches!(*c, 0x20 | 0x09..=0x0d);
    let a = s.iter().position(|c| !ws(c)).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !ws(c)).map(|i| i + 1).unwrap_or(a);
    s[a..b.max(a)].to_vec()
}

pub fn classify(p: &[u16]) -> u32 {
    let lower: String = String::from_utf16_lossy(p).to_ascii_lowercase();
    if lower.starts_with("http://") {
        0
    } else if lower.starts_with("https://") {
        1
    } else {
        2
    }
}

pub fn parse_stores(params: &[u16]) -> Vec<Store> {
    let ds = downstream();
    let mut v = Vec::new();
    for piece in split_params(params) {
        let p = if piece.is_empty() { ds.clone() } else { piece };
        if p.is_empty() {
            continue;
        }
        v.push(Store::new(p));
    }
    v
}

#[repr(C)]
pub struct ExtendedOutput {
    pub size: u32,
    pub version: u32,
    pub file_ptr_msg: [u16; MAX_PATH_W],
}

pub unsafe fn lookup(params: &[u16], file: &[u16], index: &[u16], path: *mut u16, ext: *mut ExtendedOutput) -> i32 {
    SetLastError(0);
    if exclude::first_call() && exclude::excluded(file) {
        SetLastError(2);
        return 0;
    }
    if trim(params).is_empty() {
        SetLastError(87);
        return 0;
    }
    if has(SSRVOPT_NOCOPY) {
        let mut stores = parse_stores(params);
        if stores.is_empty() {
            SetLastError(87);
            return 0;
        }
        let p = stores[0].get_path(file, index);
        write_path(path, &p);
        trace::dbg(&trace::ws(&p));
        return 1;
    }
    let r = lookup_main(params, file, index, path, ext);
    if r == 0 && !path.is_null() {
        *path = 0;
    }
    let e = GetLastError();
    let out = if path.is_null() { Vec::new() } else { wstr(path).to_vec() };
    trace::dbg(&trace::ws(&out));
    SetLastError(e);
    r
}

pub fn report(stores: &[Store], verbose: bool) {
    for s in stores {
        if verbose {
            if s.disabled() {
                trace::print(&format!("{}: not available\n", trace::ws(&s.path)));
            } else {
                let p = if s.found.is_empty() { &s.path } else { &s.found };
                trace::print(&format!("{} not found\n", trace::ws(p)));
            }
        }
        if s.is_file() {
            unsafe { SetLastError(s.err) };
        }
    }
}

unsafe fn lookup_main(params: &[u16], file: &[u16], index: &[u16], path: *mut u16, ext: *mut ExtendedOutput) -> i32 {
    let mut stores = parse_stores(params);
    if stores.is_empty() {
        SetLastError(2);
        return 0;
    }
    let want_ext = !ext.is_null() && (*ext).size == 0x214 && (*ext).version == 1;
    let opts = options();
    let mut found: Option<usize> = None;
    let mut err = 2u32;
    for i in 0..stores.len() {
        if stores[i].find(file, index) {
            found = Some(i);
            if opts & SSRVOPT_OVERWRITE == 0 {
                break;
            }
        } else {
            err = GetLastError();
            if want_ext && !stores[i].msg.is_empty() {
                fill_ext(ext, &stores[i].msg);
            }
        }
    }
    let Some(i) = found else {
        SetLastError(if err == 0 { 2 } else { err });
        report(&stores, true);
        if GetLastError() == 0 {
            SetLastError(if err == 0 { 2 } else { err });
        }
        return 0;
    };
    let dont = opts & SSRVOPT_DONT_UNCOMPRESS != 0;
    let mut src = i;
    let mut ok = true;
    let mut j = i as isize - 1;
    while j >= 0 {
        let ju = j as usize;
        let decompress = ju == 0 && !dont;
        let (a, b) = stores.split_at_mut(src);
        ok = b[0].copy_to(&mut a[ju], file, index, decompress);
        if ok {
            src = ju;
        }
        j -= 1;
    }
    let mut fin: Option<Store> = None;
    let cur = &stores[src];
    if (cur.compressed && !dont) || !cur.is_file() {
        let ds = downstream();
        if !ds.is_empty() {
            let mut d = Store::new(ds);
            let mut s = stores.swap_remove(src);
            if s.copy_to(&mut d, file, index, true) {
                fin = Some(d);
            }
        }
        if fin.is_none() {
            if want_ext && !stores.is_empty() && !stores[0].msg.is_empty() {
                fill_ext(ext, &stores[0].msg);
            }
            let e = GetLastError();
            report(&stores, false);
            SetLastError(e);
            if GetLastError() == 0 {
                SetLastError(2);
            }
            return 0;
        }
    } else {
        let _ = ok;
        fin = Some(stores.swap_remove(src));
    }
    write_path(path, &fin.unwrap().found);
    1
}

unsafe fn fill_ext(ext: *mut ExtendedOutput, msg: &[u16]) {
    let n = msg.len().min(MAX_PATH_W - 1);
    let m = &mut (*ext).file_ptr_msg;
    m[..n].copy_from_slice(&msg[..n]);
    m[n] = 0;
}

unsafe fn write_path(out: *mut u16, p: &[u16]) {
    if out.is_null() {
        return;
    }
    let n = p.len().min(MAX_PATH_W - 2);
    core::ptr::copy_nonoverlapping(p.as_ptr(), out, n);
    *out.add(n) = 0;
}

pub unsafe fn validate(params: *const u16, file: *const u16, id: *const u8, two: u32, three: u32, out: *mut u16) -> bool {
    if !out.is_null() {
        *out = 0;
    }
    if out.is_null() || params.is_null() || *params == 0 || file.is_null() || *file == 0 || (id.is_null() && two == 0 && three == 0) {
        SetLastError(87);
        return false;
    }
    if wstr(file).len() > 0x64 {
        trace::print(&format!("{} - filename cannot exceed 100 characters\n", trace::ws(wstr(file))));
        SetLastError(87);
        return false;
    }
    true
}

pub unsafe fn server_w(params: *const u16, file: *const u16, id: *const u8, two: u32, three: u32, path: *mut u16, ext: *mut ExtendedOutput) -> i32 {
    if !validate(params, file, id, two, three, path) {
        return 0;
    }
    let idx = index_string(id, two, three, param_type());
    if idx.len() >= MAX_PATH_W {
        SetLastError(87);
        return 0;
    }
    lookup(wstr(params), wstr(file), &idx, path, ext)
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerW(params: *const u16, file: *const u16, id: *const u8, two: u32, three: u32, path: *mut u16) -> i32 {
    server_w(params, file, id, two, three, path, core::ptr::null_mut())
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerWEx(params: *const u16, file: *const u16, id: *const u8, two: u32, three: u32, path: *mut u16, ext: *mut ExtendedOutput) -> i32 {
    server_w(params, file, id, two, three, path, ext)
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServer(params: *const u8, file: *const u8, id: *const u8, two: u32, three: u32, path: *mut u8) -> i32 {
    if !path.is_null() {
        *path = 0;
    }
    if params.is_null() || file.is_null() {
        SetLastError(87);
        return 0;
    }
    let p = z(&a2w(params, 0x7fff));
    let f = z(&a2w(file, MAX_PATH_W - 1));
    let sid;
    let idp = if param_type() == SSRVOPT_STRING && !id.is_null() {
        sid = z(&a2w(id, MAX_PATH_W - 1));
        sid.as_ptr() as *const u8
    } else {
        id
    };
    let mut out = [0u16; MAX_PATH_W];
    let r = server_w(p.as_ptr(), f.as_ptr(), idp, two, three, out.as_mut_ptr(), core::ptr::null_mut());
    if r != 0 && !path.is_null() {
        let a = w2a(wstr(out.as_ptr()));
        let n = a.len().min(MAX_PATH_W - 2);
        core::ptr::copy_nonoverlapping(a.as_ptr(), path, n);
        *path.add(n) = 0;
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerByIndexW(srv: *const u16, index: *const u16, file: *const u16, dest: *mut u16) -> i32 {
    if srv.is_null() || index.is_null() || file.is_null() || *file == 0 {
        SetLastError(87);
        return 0;
    }
    lookup(wstr(srv), wstr(file), wstr(index), dest, core::ptr::null_mut())
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerByIndex(srv: *const u8, index: *const u8, file: *const u8, dest: *mut u8) -> i32 {
    if srv.is_null() || index.is_null() || file.is_null() {
        SetLastError(87);
        return 0;
    }
    let s = z(&a2w(srv, 0x7fff));
    let i = z(&a2w(index, MAX_PATH_W - 1));
    let f = z(&a2w(file, MAX_PATH_W - 1));
    let mut out = [0u16; MAX_PATH_W];
    let r = SymbolServerByIndexW(s.as_ptr(), i.as_ptr(), f.as_ptr(), out.as_mut_ptr());
    if r != 0 && !dest.is_null() {
        let a = w2a(wstr(out.as_ptr()));
        let n = a.len().min(MAX_PATH_W - 2);
        core::ptr::copy_nonoverlapping(a.as_ptr(), dest, n);
        *dest.add(n) = 0;
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerClose() -> i32 {
    let mut s = STATE.lock().unwrap();
    s.downstream.clear();
    s.options &= !SSRVOPT_DOWNSTREAM_STORE;
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerIsStoreW(path: *const u16) -> i32 {
    if path.is_null() || *path == 0 {
        SetLastError(87);
        return 0;
    }
    let p = wstr(path);
    if p.contains(&(b'*' as u16)) || classify(p) < 2 {
        return 1;
    }
    Store::new(p.to_vec()).is_store() as i32
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerIsStore(path: *const u8) -> i32 {
    if path.is_null() || *path == 0 {
        SetLastError(87);
        return 0;
    }
    let w = z(&a2w(path, 0x7fff));
    SymbolServerIsStoreW(w.as_ptr())
}

pub unsafe fn first_store(params: *const u16) -> Option<Store> {
    if params.is_null() || *params == 0 {
        SetLastError(87);
        return None;
    }
    let p = split_params(wstr(params)).into_iter().next().unwrap_or_default();
    let p = if p.is_empty() { downstream() } else { p };
    if p.is_empty() {
        SetLastError(87);
        return None;
    }
    Some(Store::new(p))
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerPingW(params: *const u16) -> i32 {
    match first_store(params) {
        Some(s) => s.is_store() as i32,
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerPingWEx(params: *const u16) -> i32 {
    if has(SSRVOPT_DISABLE_PING_HOST) {
        return 1;
    }
    SymbolServerPingW(params)
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerPing(params: *const u8) -> i32 {
    if params.is_null() {
        SetLastError(87);
        return 0;
    }
    let w = z(&a2w(params, 0x7fff));
    SymbolServerPingW(w.as_ptr())
}

pub const SYMSTOREOPT_COMPRESS: u32 = 1;
pub const SYMSTOREOPT_OVERWRITE: u32 = 2;
pub const SYMSTOREOPT_RETURNINDEX: u32 = 4;
pub const SYMSTOREOPT_POINTER: u32 = 8;
pub const SYMSTOREOPT_UNICODE: u32 = 0x20;
pub const SYMSTOREOPT_PASS_IF_EXISTS: u32 = 0x40;

#[no_mangle]
pub unsafe extern "system" fn SymbolServerStoreFileW(srv: *const u16, file: *const u16, id: *const u8, two: u32, three: u32, stored: *mut u16, cch: usize, flags: u32) -> i32 {
    SetLastError(0);
    let Some(mut s) = first_store(srv) else { return 0 };
    if !s.is_file() || file.is_null() || *file == 0 {
        SetLastError(87);
        return 0;
    }
    let idx = index_string(id, two, three, param_type());
    if idx.is_empty() || idx.len() >= MAX_PATH_W {
        SetLastError(87);
        return 0;
    }
    let mut flags = flags;
    if s.path == downstream() {
        flags &= !SYMSTOREOPT_COMPRESS;
    }
    let Some(p) = s.store_file(wstr(file), &idx, flags) else { return 0 };
    s.ensure_pingme();
    let out = if flags & SYMSTOREOPT_RETURNINDEX != 0 { idx } else { p };
    if !stored.is_null() && !copy_out(stored, cch, &out) {
        SetLastError(122);
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerStoreFile(srv: *const u8, file: *const u8, id: *const u8, two: u32, three: u32, stored: *mut u8, cch: usize, flags: u32) -> i32 {
    if srv.is_null() || file.is_null() {
        SetLastError(87);
        return 0;
    }
    let s = z(&a2w(srv, 0x7fff));
    let f = z(&a2w(file, MAX_PATH_W - 1));
    let mut out = vec![0u16; MAX_PATH_W];
    let r = SymbolServerStoreFileW(s.as_ptr(), f.as_ptr(), id, two, three, out.as_mut_ptr(), MAX_PATH_W, flags);
    if r != 0 && !stored.is_null() {
        let a = w2a(wstr(out.as_ptr()));
        if a.len() >= cch {
            SetLastError(122);
            return 0;
        }
        core::ptr::copy_nonoverlapping(a.as_ptr(), stored, a.len());
        *stored.add(a.len()) = 0;
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerGetSupplementW(sym: *const u16, node: *const u16, file: *const u16, dest: *mut u16, cch: usize) -> i32 {
    if file.is_null() || node.is_null() || dest.is_null() {
        SetLastError(87);
        return 0;
    }
    let name = store::fname_ext(wstr(file));
    if name.is_empty() {
        SetLastError(87);
        return 0;
    }
    let Some(mut s) = first_store(sym) else { return 0 };
    let mut hit = s.get_supplement(wstr(node), &name);
    if !hit {
        let ds = downstream();
        if !ds.is_empty() {
            s = Store::new(ds);
            hit = s.get_supplement(wstr(node), &name);
        }
    }
    if !hit {
        return 0;
    }
    if !copy_out(dest, cch, &s.found) {
        SetLastError(122);
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerGetSupplement(sym: *const u8, node: *const u8, file: *const u8, dest: *mut u8, cch: usize) -> i32 {
    if sym.is_null() || node.is_null() || file.is_null() || dest.is_null() {
        SetLastError(87);
        return 0;
    }
    let (a, b, c) = (z(&a2w(sym, 0x7fff)), z(&a2w(node, MAX_PATH_W - 1)), z(&a2w(file, MAX_PATH_W - 1)));
    let mut out = vec![0u16; MAX_PATH_W];
    let r = SymbolServerGetSupplementW(a.as_ptr(), b.as_ptr(), c.as_ptr(), out.as_mut_ptr(), MAX_PATH_W);
    if r != 0 {
        let s = w2a(wstr(out.as_ptr()));
        if s.len() >= cch {
            SetLastError(122);
            return 0;
        }
        core::ptr::copy_nonoverlapping(s.as_ptr(), dest, s.len());
        *dest.add(s.len()) = 0;
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerStoreSupplementW(sym: *const u16, node: *const u16, file: *const u16, dest: *mut u16, cch: usize, flags: u32) -> i32 {
    if file.is_null() || node.is_null() {
        SetLastError(87);
        return 0;
    }
    let name = store::fname_ext(wstr(file));
    if name.is_empty() {
        SetLastError(87);
        return 0;
    }
    let Some(mut s) = first_store(sym) else { return 0 };
    if !s.is_file() {
        SetLastError(87);
        return 0;
    }
    let mut flags = flags;
    if s.path == downstream() {
        flags &= !SYMSTOREOPT_COMPRESS;
    }
    let Some(p) = s.store_supplement(wstr(node), wstr(file), &name, flags) else {
        if GetLastError() == 3 {
            SetLastError(8);
        }
        return 0;
    };
    if !dest.is_null() && !copy_out(dest, cch, &p) {
        SetLastError(122);
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn SymbolServerStoreSupplement(sym: *const u8, node: *const u8, file: *const u8, dest: *mut u8, cch: usize, flags: u32) -> i32 {
    if sym.is_null() || node.is_null() || file.is_null() {
        SetLastError(87);
        return 0;
    }
    let (a, b, c) = (z(&a2w(sym, 0x7fff)), z(&a2w(node, MAX_PATH_W - 1)), z(&a2w(file, MAX_PATH_W - 1)));
    let mut out = vec![0u16; MAX_PATH_W];
    let r = SymbolServerStoreSupplementW(a.as_ptr(), b.as_ptr(), c.as_ptr(), out.as_mut_ptr(), MAX_PATH_W, flags);
    if r != 0 && !dest.is_null() {
        let s = w2a(wstr(out.as_ptr()));
        if s.len() >= cch {
            SetLastError(122);
            return 0;
        }
        core::ptr::copy_nonoverlapping(s.as_ptr(), dest, s.len());
        *dest.add(s.len()) = 0;
    }
    r
}

#[cfg(test)]
mod byindex_tests;
#[cfg(test)]
mod tests;
