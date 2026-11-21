use crate::sys::*;
use crate::url::{escape, unescape};
use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

#[repr(C)]
pub struct Slist {
    pub data: *mut u8,
    pub next: *mut Slist,
}

#[no_mangle]
pub unsafe extern "C" fn curl_slist_append(list: *mut Slist, s: *const u8) -> *mut Slist {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    let node = heap_alloc(core::mem::size_of::<Slist>()) as *mut Slist;
    if node.is_null() {
        return core::ptr::null_mut();
    }
    let data = heap_bytes(cstr_bytes(s));
    if data.is_null() {
        heap_free(node as *mut c_void);
        return core::ptr::null_mut();
    }
    node.write(Slist { data, next: core::ptr::null_mut() });
    if list.is_null() {
        return node;
    }
    let mut t = list;
    while !(*t).next.is_null() {
        t = (*t).next;
    }
    (*t).next = node;
    list
}

#[no_mangle]
pub unsafe extern "C" fn curl_slist_free_all(list: *mut Slist) {
    let mut p = list;
    while !p.is_null() {
        let next = (*p).next;
        heap_free((*p).data as *mut c_void);
        heap_free(p as *mut c_void);
        p = next;
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_free(p: *mut c_void) {
    heap_free(p);
}

unsafe fn input<'a>(s: *const u8, len: i32) -> &'a [u8] {
    if len > 0 {
        core::slice::from_raw_parts(s, len as usize)
    } else {
        cstr_bytes(s)
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_escape(_h: *mut c_void, s: *const u8, len: i32) -> *mut u8 {
    if s.is_null() || len < 0 {
        return core::ptr::null_mut();
    }
    heap_bytes(escape(input(s, len), b"").as_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn curl_escape(s: *const u8, len: i32) -> *mut u8 {
    curl_easy_escape(core::ptr::null_mut(), s, len)
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_unescape(_h: *mut c_void, s: *const u8, len: i32, outlen: *mut i32) -> *mut u8 {
    if s.is_null() || len < 0 {
        return core::ptr::null_mut();
    }
    let d = unescape(input(s, len));
    if !outlen.is_null() {
        *outlen = d.len() as i32;
    }
    heap_bytes(&d)
}

#[no_mangle]
pub unsafe extern "C" fn curl_unescape(s: *const u8, len: i32) -> *mut u8 {
    curl_easy_unescape(core::ptr::null_mut(), s, len, core::ptr::null_mut())
}

pub fn easy_strerror(code: i32) -> &'static str {
    match code {
        0 => "No error",
        1 => "Unsupported protocol",
        2 => "Failed initialization",
        3 => "URL using bad/illegal format or missing URL",
        4 => "A requested feature, protocol or option was not found built-in in this libcurl due to a build-time decision.",
        5 => "Couldn't resolve proxy name",
        6 => "Couldn't resolve host name",
        7 => "Couldn't connect to server",
        8 => "Weird server reply",
        9 => "Access denied to remote resource",
        10 => "FTP: The server failed to connect to data port",
        11 => "FTP: unknown PASS reply",
        12 => "FTP: Accepting server connect has timed out",
        13 => "FTP: unknown PASV reply",
        14 => "FTP: unknown 227 response format",
        15 => "FTP: can't figure out the host in the PASV response",
        16 => "Error in the HTTP2 framing layer",
        17 => "FTP: couldn't set file type",
        18 => "Transferred a partial file",
        19 => "FTP: couldn't retrieve (RETR failed) the specified file",
        21 => "Quote command returned error",
        22 => "HTTP response code said error",
        23 => "Failed writing received data to disk/application",
        25 => "Upload failed (at start/before it took off)",
        26 => "Failed to open/read local data from file/application",
        27 => "Out of memory",
        28 => "Timeout was reached",
        30 => "FTP: command PORT failed",
        31 => "FTP: command REST failed",
        33 => "Requested range was not delivered by the server",
        34 => "Internal problem setting up the POST",
        35 => "SSL connect error",
        36 => "Couldn't resume download",
        37 => "Couldn't read a file:// file",
        38 => "LDAP: cannot bind",
        39 => "LDAP: search failed",
        41 => "A required function in the library was not found",
        42 => "Operation was aborted by an application callback",
        43 => "A libcurl function was given a bad argument",
        45 => "Failed binding local connection end",
        47 => "Number of redirects hit maximum amount",
        48 => "An unknown option was passed in to libcurl",
        49 => "Malformed option provided in a setopt",
        52 => "Server returned nothing (no headers, no data)",
        53 => "SSL crypto engine not found",
        54 => "Can not set SSL crypto engine as default",
        55 => "Failed sending data to the peer",
        56 => "Failure when receiving data from the peer",
        58 => "Problem with the local SSL certificate",
        59 => "Couldn't use specified SSL cipher",
        60 => "SSL peer certificate or SSH remote key was not OK",
        61 => "Unrecognized or bad HTTP Content or Transfer-Encoding",
        62 => "Invalid LDAP URL",
        63 => "Maximum file size exceeded",
        64 => "Requested SSL level failed",
        65 => "Send failed since rewinding of the data stream failed",
        66 => "Failed to initialise SSL crypto engine",
        67 => "Login denied",
        68 => "TFTP: File Not Found",
        69 => "TFTP: Access Violation",
        70 => "Disk full or allocation exceeded",
        71 => "TFTP: Illegal operation",
        72 => "TFTP: Unknown transfer ID",
        73 => "Remote file already exists",
        74 => "TFTP: No such user",
        75 => "Conversion failed",
        76 => "Caller must register CURLOPT_CONV_ callback options",
        77 => "Problem with the SSL CA cert (path? access rights?)",
        78 => "Remote file not found",
        79 => "Error in the SSH layer",
        80 => "Failed to shut down the SSL connection",
        81 => "Socket not ready for send/recv",
        82 => "Failed to load CRL file (path? access rights?, format?)",
        83 => "Issuer check against peer certificate failed",
        84 => "FTP: The server did not accept the PRET command.",
        85 => "RTSP CSeq mismatch or invalid CSeq",
        86 => "RTSP session error",
        87 => "Unable to parse FTP file list",
        88 => "Chunk callback failed",
        89 => "The max connection limit is reached",
        90 => "SSL public key does not match pinned public key",
        91 => "SSL server certificate status verification FAILED",
        92 => "Stream error in the HTTP/2 framing layer",
        93 => "API function called from within callback",
        94 => "An authentication function returned an error",
        95 => "HTTP/3 error",
        96 => "QUIC connection error",
        97 => "proxy handshake error",
        98 => "SSL Client Certificate required",
        99 => "Unrecoverable error in select/poll",
        _ => "Unknown error",
    }
}

fn leak_c(s: &'static str) -> *const u8 {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<&'static str, usize>>> = Mutex::new(None);
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    *m.entry(s).or_insert_with(|| {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        Box::leak(v.into_boxed_slice()).as_ptr() as usize
    }) as *const u8
}

#[no_mangle]
pub extern "C" fn curl_easy_strerror(code: i32) -> *const u8 {
    leak_c(easy_strerror(code))
}

#[no_mangle]
pub extern "C" fn curl_multi_strerror(code: i32) -> *const u8 {
    leak_c(match code {
        -1 => "Please call curl_multi_perform() soon",
        0 => "No error",
        1 => "Invalid multi handle",
        2 => "Invalid easy handle",
        3 => "Out of memory",
        4 => "Internal error",
        5 => "Invalid socket argument",
        6 => "Unknown option",
        7 => "The easy handle is already added to a multi handle",
        8 => "API function called from within callback",
        9 => "Wakeup is unavailable or failed",
        10 => "A libcurl function was given a bad argument",
        _ => "Unknown error",
    })
}

#[no_mangle]
pub extern "C" fn curl_share_strerror(code: i32) -> *const u8 {
    leak_c(match code {
        0 => "No error",
        1 => "Unknown share option",
        2 => "Share currently in use",
        3 => "Invalid share handle",
        4 => "Out of memory",
        5 => "Feature not enabled in this library",
        _ => "CURLSHcode unknown",
    })
}

#[repr(C)]
pub struct VersionInfo {
    age: i32,
    version: *const u8,
    version_num: u32,
    host: *const u8,
    features: i32,
    ssl_version: *const u8,
    ssl_version_num: i32,
    libz_version: *const u8,
    protocols: *const *const u8,
    ares: *const u8,
    ares_num: i32,
    libidn: *const u8,
    iconv_ver_num: i32,
    libssh_version: *const u8,
    brotli_ver_num: u32,
    brotli_version: *const u8,
    nghttp2_ver_num: u32,
    nghttp2_version: *const u8,
    quic_version: *const u8,
}

struct SyncPtr<T>(T);
unsafe impl<T> Sync for SyncPtr<T> {}

static PROTOCOLS: SyncPtr<[*const u8; 3]> = SyncPtr([b"http\0".as_ptr(), b"https\0".as_ptr(), core::ptr::null()]);

static VERSION_INFO: SyncPtr<VersionInfo> = SyncPtr(VersionInfo {
    age: 5,
    version: b"2206.4.0.0\0".as_ptr(),
    version_num: 0x55f0_114d,
    host: b"Microsoft GDK\0".as_ptr(),
    features: 0x1124d,
    ssl_version: b"Schannel\0".as_ptr(),
    ssl_version_num: 0,
    libz_version: b"Custom\0".as_ptr(),
    protocols: &PROTOCOLS.0 as *const [*const u8; 3] as *const *const u8,
    ares: core::ptr::null(),
    ares_num: 0,
    libidn: core::ptr::null(),
    iconv_ver_num: 0,
    libssh_version: core::ptr::null(),
    brotli_ver_num: 0,
    brotli_version: core::ptr::null(),
    nghttp2_ver_num: 0,
    nghttp2_version: core::ptr::null(),
    quic_version: core::ptr::null(),
});

#[no_mangle]
pub extern "C" fn curl_version() -> *const u8 {
    b"2206.4.0.0.amd64fre\0".as_ptr()
}

#[no_mangle]
pub extern "C" fn curl_version_info(_age: i32) -> *const VersionInfo {
    &VERSION_INFO.0
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn parse_date(s: &str) -> i64 {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let (mut year, mut month, mut day) = (None::<i64>, None::<i64>, None::<i64>);
    let (mut hh, mut mm, mut ss) = (0i64, 0i64, 0i64);
    let mut tz: i64 = 0;
    let mut have_time = false;
    let norm: String = s.chars().map(|c| if c == ',' { ' ' } else { c }).collect();
    for raw in norm.split_whitespace() {
        let parts: Vec<&str> = if raw.contains(':') { vec![raw] } else if raw.starts_with('+') || (raw.starts_with('-') && raw.len() == 5) { vec![raw] } else { raw.split('-').filter(|x| !x.is_empty()).collect() };
        for tok in parts {
            let lower = tok.to_ascii_lowercase();
            if tok.contains(':') && !have_time {
                let n: Vec<i64> = tok.split(':').map(|x| x.parse::<i64>().unwrap_or(-1)).collect();
                if n.len() >= 2 && n.iter().all(|v| *v >= 0) {
                    hh = n[0];
                    mm = n[1];
                    ss = *n.get(2).unwrap_or(&0);
                    have_time = true;
                    if hh > 23 || mm > 59 || ss > 60 {
                        return -1;
                    }
                    continue;
                }
                return -1;
            }
            if (tok.starts_with('+') || tok.starts_with('-')) && tok.len() == 5 && tok[1..].bytes().all(|c| c.is_ascii_digit()) {
                let v: i64 = tok[1..].parse().unwrap_or(0);
                let off = (v / 100) * 3600 + (v % 100) * 60;
                tz = if tok.starts_with('-') { -off } else { off };
                continue;
            }
            if lower.bytes().all(|c| c.is_ascii_alphabetic()) {
                if let Some(i) = MONTHS.iter().position(|m| lower.starts_with(m)) {
                    month = Some(i as i64 + 1);
                } else if DAYS.iter().any(|d| lower.starts_with(d)) {
                } else {
                    tz = match lower.as_str() {
                        "gmt" | "utc" | "ut" | "z" => 0,
                        "est" => -5 * 3600,
                        "edt" => -4 * 3600,
                        "cst" => -6 * 3600,
                        "cdt" => -5 * 3600,
                        "mst" => -7 * 3600,
                        "mdt" => -6 * 3600,
                        "pst" => -8 * 3600,
                        "pdt" => -7 * 3600,
                        "cet" => 3600,
                        "cest" => 2 * 3600,
                        _ => return -1,
                    };
                }
                continue;
            }
            if tok.bytes().all(|c| c.is_ascii_digit()) {
                let v: i64 = tok.parse().unwrap_or(-1);
                if tok.len() >= 3 || v > 31 || day.is_some() {
                    if year.is_some() {
                        return -1;
                    }
                    year = Some(if tok.len() <= 2 { if v < 70 { 2000 + v } else { 1900 + v } } else { v });
                } else {
                    day = Some(v);
                }
                continue;
            }
            return -1;
        }
    }
    match (year, month, day) {
        (Some(y), Some(m), Some(d)) if (1..=31).contains(&d) => days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss - tz,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_getdate(s: *const u8, _unused: *const i64) -> i64 {
    match cstring(s) {
        Some(v) => parse_date(&v),
        None => -1,
    }
}

static REQUEST_LIMIT: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
pub extern "C" fn curl_global_init(flags: i32) -> i32 {
    crate::global::init(flags)
}

#[no_mangle]
pub extern "C" fn curl_global_init_mem(_flags: i32, m: usize, f: usize, r: usize, s: usize, c: usize) -> i32 {
    crate::global::init_mem([m, f, r, s, c])
}

#[no_mangle]
pub extern "C" fn xcurl_global_init_mem(_flags: i32, a: usize, b: usize, c: usize, d: usize, e: usize) -> i32 {
    crate::global::init_mem([a, b, c, d, e])
}

#[no_mangle]
pub extern "C" fn curl_global_cleanup() {
    crate::global::cleanup()
}

#[no_mangle]
pub extern "C" fn curl_global_sslset(_id: i32, _name: *const u8, _avail: *mut c_void) -> i32 {
    3
}

#[no_mangle]
pub extern "C" fn xcurl_global_suspend() -> i32 {
    let changed = crate::gate::suspend();
    compat_common::record(
        "xcurl",
        "xcurl_global_suspend",
        &format!("{}; {} transfers in flight keep their connections, callbacks and new transfers are held until resume", if changed { "suspended" } else { "already suspended" }, crate::gate::active()),
    );
    0
}

#[no_mangle]
pub extern "C" fn xcurl_global_resume() -> i32 {
    let changed = crate::gate::resume();
    compat_common::record("xcurl", "xcurl_global_resume", if changed { "resumed; held transfers and callbacks continue" } else { "was not suspended" });
    0
}

#[no_mangle]
pub extern "C" fn xcurl_global_set_request_limit(limit: u32) {
    REQUEST_LIMIT.store(limit, Ordering::Relaxed);
    crate::gate::set_limit(limit);
    compat_common::record("xcurl", "xcurl_global_set_request_limit", &format!("at most {limit} concurrent transfers{}", if limit == 0 { " (0 = unlimited)" } else { "" }));
}

#[no_mangle]
pub extern "C" fn curl_share_init() -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn curl_share_setopt(_sh: *mut c_void, _opt: i32, _arg: usize) -> i32 {
    5
}

#[no_mangle]
pub extern "C" fn curl_share_cleanup(_sh: *mut c_void) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn curl_formadd(_first: *mut c_void, _last: *mut c_void, _a: usize, _b: usize) -> i32 {
    7
}

#[no_mangle]
pub extern "C" fn curl_formget(_form: *mut c_void, _arg: *mut c_void, _append: usize) -> i32 {
    7
}

#[no_mangle]
pub extern "C" fn curl_formfree(_form: *mut c_void) {}

#[no_mangle]
pub extern "C" fn curl_mime_init(_easy: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn curl_mime_free(_mime: *mut c_void) {}

#[no_mangle]
pub extern "C" fn curl_mime_addpart(_mime: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn curl_mime_name(_part: *mut c_void, _s: *const u8) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_filename(_part: *mut c_void, _s: *const u8) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_type(_part: *mut c_void, _s: *const u8) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_encoder(_part: *mut c_void, _s: *const u8) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_filedata(_part: *mut c_void, _s: *const u8) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_data(_part: *mut c_void, _d: *const u8, _n: usize) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_data_cb(_part: *mut c_void, _n: i64, _r: usize, _s: usize, _f: usize, _arg: *mut c_void) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_subparts(_part: *mut c_void, _sub: *mut c_void) -> i32 {
    4
}

#[no_mangle]
pub extern "C" fn curl_mime_headers(_part: *mut c_void, _h: *mut c_void, _take: i32) -> i32 {
    4
}
