use crate::crt::{__acrt_iob_func, fflush};
use crate::parse_args::*;
use crate::remote_run::*;
use crate::rest::*;
use crate::stl::{wide, AString, RawVec, WString};
use crate::strings::*;
use core::ffi::c_void;
use std::sync::Mutex;

static STDOUT: Mutex<()> = Mutex::new(());

extern "C" {
    fn _dup(fd: i32) -> i32;
    fn _get_osfhandle(fd: i32) -> *mut c_void;
    fn _dup2(src: i32, dst: i32) -> i32;
    fn _close(fd: i32) -> i32;
    fn _wopen(path: *const u16, flags: i32, mode: i32) -> i32;
    fn _set_thread_local_invalid_parameter_handler(h: Option<InvalidParameterHandler>) -> Option<InvalidParameterHandler>;
}

type InvalidParameterHandler = unsafe extern "C" fn(*const u16, *const u16, *const u16, u32, usize);

unsafe extern "C" fn ignore_invalid_parameter(_: *const u16, _: *const u16, _: *const u16, _: u32, _: usize) {}

#[link(name = "runtimeobject")]
extern "system" {
    fn RoInitialize(kind: i32) -> i32;
    fn WindowsGetStringRawBuffer(s: *mut c_void, len: *mut u32) -> *const u16;
    fn WindowsDeleteString(s: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetLastError() -> u32;
    fn SetStdHandle(n: u32, h: *mut c_void) -> i32;
}

fn capture_wide(f: impl FnOnce()) -> String {
    let b = capture_bytes(f);
    let w: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16(&w).unwrap()
}

fn capture_bytes(f: impl FnOnce()) -> Vec<u8> {
    let _g = STDOUT.lock().unwrap_or_else(|e| e.into_inner());
    let path = std::env::temp_dir().join(format!("wdbase_capture_{}_{:?}.txt", std::process::id(), std::thread::current().id()));
    let wpath = wide(path.to_str().unwrap());
    unsafe {
        let out = __acrt_iob_func(1);
        fflush(out);
        let saved = _dup(1);
        let fd = _wopen(wpath.as_ptr(), 0x0001 | 0x0100 | 0x0200 | 0x8000, 0x0180);
        assert!(fd >= 0);
        _dup2(fd, 1);
        SetStdHandle(0xffff_fff5, _get_osfhandle(saved));
        f();
        fflush(out);
        _dup2(saved, 1);
        _close(fd);
        _close(saved);
    }
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    bytes
}

fn ws(s: &str) -> WString {
    let mut w = WString::new();
    let v: Vec<u16> = s.encode_utf16().collect();
    unsafe { WString::assign(&mut w, v.as_ptr(), v.len()) };
    w
}

fn text(w: *const WString) -> String {
    unsafe { String::from_utf16(WString::as_slice(w)).unwrap() }
}

fn def(id: u32, name: &str, kind: u32, alias: &str, short: &str, detailed: &str, subs: &[u32]) -> ArgDefinition {
    let mut d: ArgDefinition = unsafe { core::mem::zeroed() };
    d.id = id;
    d.name = ws(name);
    d.kind = kind;
    d.alias = ws(alias);
    d.short_help = ws(short);
    d.detailed_help = ws(detailed);
    d.sub_options[..subs.len()].copy_from_slice(subs);
    d
}

fn sample_defs() -> Vec<ArgDefinition> {
    vec![
        def(0, "", 0, "", "", "", &[]),
        def(1, "list", KIND_COMMAND, "", "Lists things.", "Lists every installed thing.", &[3]),
        def(2, "remove", KIND_COMMAND, "rm", "Removes a thing.", "", &[]),
        def(3, "verbose", KIND_TOPIC, "/v", "Prints more.", "Verbose output topic.", &[]),
    ]
}

fn build(defs: &[ArgDefinition], count: u32) -> Box<ParseArgs> {
    let mut p: Box<ParseArgs> = Box::new(unsafe { core::mem::zeroed() });
    let (mut a, mut b, mut c) = (ws("wdtool"), ws("Developer tool."), ws("Built by tests."));
    unsafe { parse_args_ctor(&mut *p, &mut a, &mut b, &mut c, if defs.is_empty() { core::ptr::null() } else { defs.as_ptr() }, count) };
    assert_eq!((a.size, a.res, b.res, c.res), (0, 7, 7, 7));
    p
}

fn argv(args: &[&str]) -> (Vec<Vec<u16>>, Vec<*const u16>) {
    let store: Vec<Vec<u16>> = args.iter().map(|a| wide(a)).collect();
    let ptrs = store.iter().map(|v| v.as_ptr()).collect();
    (store, ptrs)
}

fn sym(p: &ParseArgs, i: usize) -> (u32, String, String, u8) {
    unsafe {
        let s = symbol(p, i);
        ((*s).id, text(&(*s).arg), text(&(*s).value), (*s).error)
    }
}

#[test]
fn parse_args_constructor() {
    let defs = sample_defs();
    let p = build(&defs, 4);
    unsafe {
        assert_eq!(entry_count(&*p), 4);
        assert_eq!((p.duplicate_id, p.id_out_of_range, p.no_definitions, p.help_requested), (0, 0, 0, 0));
        assert_eq!(text(&p.tool_name), "wdtool");
        assert_eq!(text(&p.build_info), "Built by tests.");
        let v = String::from_utf16(&p.version[..p.version.iter().position(|&c| c == 0).unwrap()]).unwrap();
        assert_eq!(v, "10.0.22621.1852");
        assert_eq!((*entry(&*p, 0)).def.id, 0);
        assert_eq!(text(&(*entry(&*p, 2)).def.alias), "rm");
        assert_eq!((*entry(&*p, 1)).def.sub_options[0], 3);
        let e = build(&[], 0);
        assert_eq!(e.no_definitions, 1);
        assert_eq!(entry_count(&*e), 0);
        let range = build(&defs[1..], 3);
        assert_eq!(range.id_out_of_range, 1);
        let dup_defs = vec![def(1, "a", 1, "", "", "", &[]), def(1, "b", 1, "", "", "", &[])];
        let dup = build(&dup_defs, 2);
        assert_eq!((dup.duplicate_id, dup.id_out_of_range), (1, 0));
    }
}

#[test]
fn parse_args_index_operator() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        let e = parse_args_index(&mut *p, 3);
        assert_eq!(e as usize, p.entries.first as usize + 3 * 0x238);
        assert_eq!(text(&(*e).def.name), "verbose");
    }
}

#[test]
fn parse_args_get_dictionary_size() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe { assert_eq!(get_dictionary_size(&mut *p), 4) };
    let mut e = build(&[], 0);
    unsafe { assert_eq!(get_dictionary_size(&mut *e), 0) };
}

#[test]
fn parse_args_parse() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        let (_s, a) = argv(&["tool.exe"]);
        assert_eq!(parse(&mut *p, 1, a.as_ptr(), true), 2);
        assert_eq!(parse(&mut *p, 3, core::ptr::null(), true), 3);
        let (_s, a) = argv(&["tool.exe", "LIST", "/?", "rem:thing", "extra=5"]);
        assert_eq!(parse(&mut *p, 5, a.as_ptr(), true), 0);
        assert_eq!(p.help_requested, 1);
        assert_eq!(symbol_count(&*p), 4);
        assert_eq!(sym(&p, 0), (0, "tool.exe".into(), "".into(), 0));
        assert_eq!(sym(&p, 1), (1, "LIST".into(), "".into(), 0));
        assert_eq!(sym(&p, 2), (2, "rem:thing".into(), "".into(), 0));
        assert_eq!(sym(&p, 3), (0, "extra=5".into(), "5".into(), 0));
        assert_eq!(((*entry(&*p, 0)).count, (*entry(&*p, 1)).count, (*entry(&*p, 2)).count), (1, 1, 1));
        let mut q = build(&defs, 4);
        let (_s, a) = argv(&["tool.exe", "help"]);
        assert_eq!(parse(&mut *q, 2, a.as_ptr(), false), 0);
        assert_eq!(q.help_requested, 0);
        assert_eq!(sym(&q, 1).0, 0);
        let mut e = build(&[], 0);
        assert_eq!(parse(&mut *e, 2, a.as_ptr(), true), 6);
        let dup_defs = vec![def(1, "a", 1, "", "", "", &[]), def(1, "b", 1, "", "", "", &[])];
        let mut d = build(&dup_defs, 2);
        assert_eq!(parse(&mut *d, 2, a.as_ptr(), true), 4);
        let mut r = build(&defs[1..], 3);
        assert_eq!(parse(&mut *r, 2, a.as_ptr(), true), 5);
        let named = vec![def(0, "only", 1, "", "", "", &[])];
        let mut n = build(&named, 1);
        let (_s, a) = argv(&["tool.exe", "only", "other"]);
        let previous = _set_thread_local_invalid_parameter_handler(Some(ignore_invalid_parameter));
        assert_eq!(parse(&mut *n, 3, a.as_ptr(), true), 1);
        assert_eq!(sym(&n, 2), (0, "other".into(), "".into(), 1));
        _set_thread_local_invalid_parameter_handler(previous);
    }
}

#[test]
fn parse_args_parse_fortnite() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        let (_s, a) = argv(&["tool.exe", "list", "help", "x=1:2"]);
        assert_eq!(parse_v1(&mut *p, 4, a.as_ptr()), 0);
        assert_eq!(p.help_requested, 0);
        assert_eq!(symbol_count(&*p), 4);
        assert_eq!(sym(&p, 1).0, 1);
        assert_eq!(sym(&p, 2), (0, "help".into(), "".into(), 0));
        assert_eq!(sym(&p, 3), (0, "x=1:2".into(), "2".into(), 0));
        let mut q = build(&defs, 4);
        let (_s, a) = argv(&["tool.exe", "x=1:2"]);
        assert_eq!(parse(&mut *q, 2, a.as_ptr(), true), 0);
        assert_eq!(sym(&q, 1).2, "1:2");
        assert_eq!(parse_v1(&mut *q, 1, a.as_ptr()), 2);
    }
}

#[test]
fn parse_args_is_help() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        assert!(!is_help(&mut *p));
        let (_s, a) = argv(&["tool.exe", "/?"]);
        parse(&mut *p, 2, a.as_ptr(), true);
        assert!(is_help(&mut *p));
    }
}

#[test]
fn parse_args_get_symbols() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        let (_s, a) = argv(&["tool.exe", "list", "a-much-longer-free-argument=value"]);
        parse(&mut *p, 3, a.as_ptr(), true);
        let mut v = RawVec { first: core::ptr::null_mut(), last: core::ptr::null_mut(), end: core::ptr::null_mut() };
        assert_eq!(get_symbols(&mut *p, &mut v), &mut v as *mut RawVec);
        assert_eq!(RawVec::len(&v, SYMBOL), 3);
        assert_eq!(RawVec::cap(&v, SYMBOL), 3);
        let s2 = RawVec::at(&v, SYMBOL, 2) as *mut ArgSymbol;
        assert_eq!(text(&(*s2).arg), "a-much-longer-free-argument=value");
        assert_ne!(WString::data(&(*s2).arg), WString::data(&(*symbol(&*p, 2)).arg));
        assert_eq!(text(&(*s2).value), "value");
        for i in 0..3 {
            let s = RawVec::at(&v, SYMBOL, i) as *mut ArgSymbol;
            WString::tidy(&mut (*s).arg);
            WString::tidy(&mut (*s).value);
        }
        crate::stl::deallocate(v.first, 3 * SYMBOL);
        let mut e = build(&[], 0);
        get_symbols(&mut *e, &mut v);
        assert!(v.first.is_null() && v.last.is_null() && v.end.is_null());
    }
}

#[test]
fn parse_args_find_symbol() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        let (_s, a) = argv(&["tool.exe", "list", "free", "list"]);
        parse(&mut *p, 4, a.as_ptr(), true);
        assert_eq!(find_symbol(&mut *p, 0, 1, 99), 1);
        assert_eq!(find_symbol(&mut *p, 1, 1, 1), 3);
        assert_eq!(find_symbol(&mut *p, 1, 1, 4), -1);
        assert_eq!(find_symbol(&mut *p, 2, 0, 1), 2);
        assert_eq!(find_symbol(&mut *p, 2, 1, 1), -1);
        assert_eq!(find_symbol(&mut *p, 0, 3, 0), -1);
    }
}

unsafe extern "C" fn test_handler(this: *mut ParseArgs, index: u32, context: *mut c_void, result: *mut i32) -> i32 {
    *result = index as i32 + *(context as *const i32);
    get_dictionary_size(this) as i32 + 40
}

#[test]
fn parse_args_handle_argument() {
    let mut defs = sample_defs();
    defs[1].handler = Some(test_handler);
    let mut p = build(&defs, 4);
    unsafe {
        let (_s, a) = argv(&["tool.exe", "free", "list"]);
        parse(&mut *p, 3, a.as_ptr(), true);
        let mut ctx = 100i32;
        let mut out = 0i32;
        assert_eq!(handle_argument(&mut *p, 2, &mut ctx as *mut i32 as *mut c_void, &mut out), 44);
        assert_eq!(out, 102);
        assert_eq!(handle_argument(&mut *p, 1, core::ptr::null_mut(), &mut out), 2);
        assert_eq!(handle_argument(&mut *p, 3, core::ptr::null_mut(), &mut out), 1);
    }
}

#[test]
fn parse_args_print_with_breaks() {
    let t = wide("alpha beta\n  gamma");
    let out = capture_wide(|| unsafe { print_with_breaks(4, false, t.as_ptr()) });
    assert_eq!(out, "    alpha beta\n    gamma\r\n");
    let capped = capture_wide(|| unsafe { print_with_breaks(90, true, t.as_ptr()) });
    let at75 = capture_wide(|| unsafe { print_with_breaks(75, true, t.as_ptr()) });
    assert_eq!(capped, at75);
    let pad = " ".repeat(75);
    assert_eq!(capped, format!("\r\n{pad}\r\n{pad}alpha\r\n{pad}beta\n{pad}\r\n{pad}\r\n{pad}gamma\r\n"));
    let long = wide(&"word ".repeat(20));
    let out = capture_wide(|| unsafe { print_with_breaks(10, true, long.as_ptr()) });
    let lines: Vec<&str> = out.split("\r\n").collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "word ".repeat(13).trim_end());
    assert_eq!(lines[1], format!("{}{}", " ".repeat(10), "word ".repeat(7)));
    assert_eq!(lines[2], "");
    let out = capture_wide(|| unsafe { print_with_breaks(4, false, core::ptr::null()) });
    assert_eq!(out, "");
}

#[test]
fn parse_args_print_help_summary() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    let out = capture_wide(|| unsafe { print_help_summary(&mut *p) });
    let want = "\r\nwdtool - Developer tool.\r\n\r\nBuild 10.0.22621.1852.\r\n\r\nBuilt by tests.\r\n\r\nThe following topics are available:\r\n  commands /?    for a list of commands this tool provides\r\n  <command> /?   for detailed help on a specific command\r\n";
    assert_eq!(out, format!("{}  {:<12} /?Prints more.\r\n\r\n", want, "verbose"));
}

#[test]
fn parse_args_print_help_menu() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    let out = capture_wide(|| unsafe { print_help_menu(&mut *p) });
    assert_eq!(out, format!("\r\nAvailable wdtool commands:\r\n\r\n   {:<9}Lists things.\r\n   {:<9}Removes a thing.\r\n\r\n", "list", "remove"));
}

#[test]
fn parse_args_print_help_detailed() {
    let defs = sample_defs();
    let mut p = build(&defs, 4);
    unsafe {
        assert!(!print_help_detailed(&mut *p, 4));
        let out = capture_wide(|| assert!(print_help_detailed(&mut *p, 1)));
        assert_eq!(out, format!("\r\nwdtool list\r\n\r\nLists every installed thing.\r\n\r\nUsage:\r\n\r\n   wdtool list\r\n\r\n   {:<5}Prints more.\r\n\r\n", "/v"));
        let out = capture_wide(|| assert!(print_help_detailed(&mut *p, 2)));
        assert_eq!(out, "\r\nHelp not available for command 'remove'.\r\n\r\n");
        let out = capture_wide(|| assert!(print_help_detailed(&mut *p, 3)));
        assert_eq!(out, "\r\nwdtool verbose\r\n\r\nVerbose output topic.\r\n\r\n");
    }
}

#[test]
fn sub_string_insensitive_search() {
    let hay = wide("Header: x\r\nSET-COOKIE: a=b");
    let needle = wide("set-cookie:");
    unsafe {
        let r = sub_string_insensitive(hay.as_ptr(), needle.as_ptr());
        assert_eq!(r, hay.as_ptr().add(11));
        assert!(sub_string_insensitive(hay.as_ptr(), wide("zzz").as_ptr()).is_null());
        assert!(sub_string_insensitive(core::ptr::null(), needle.as_ptr()).is_null());
        assert!(sub_string_insensitive(hay.as_ptr(), wide("").as_ptr()).is_null());
    }
}

#[test]
fn convert_utf8_to_wide_roundtrip() {
    let src = "h\u{e9}llo \u{4e16}\u{754c}, a long enough string to leave SSO";
    let mut out = ws("old");
    unsafe {
        assert_eq!(convert_utf8_to_wide(src.as_ptr(), src.len() as u32, &mut out), 0);
        assert_eq!(text(&out), src);
        assert_eq!(convert_utf8_to_wide(core::ptr::null(), 5, &mut out), 0);
        assert_eq!(out.size, 0);
        assert_eq!(convert_utf8_to_wide(b"abc".as_ptr(), 2, &mut out), 0);
        assert_eq!(text(&out), "ab");
        WString::tidy(&mut out);
    }
}

#[test]
fn convert_wide_to_utf8_roundtrip() {
    let src = "h\u{e9}llo \u{4e16}\u{754c}";
    let w: Vec<u16> = src.encode_utf16().collect();
    let mut out = AString::new();
    unsafe {
        assert_eq!(convert_wide_to_utf8(w.as_ptr(), w.len() as u32, &mut out), 0);
        assert_eq!(AString::as_slice(&out), src.as_bytes());
        assert_eq!(convert_wide_to_utf8(w.as_ptr(), 0, &mut out), 0);
        assert_eq!(out.size, 0);
        AString::tidy(&mut out);
    }
}

#[test]
fn decode_base64_string_winrt() {
    unsafe {
        let mut h: *mut c_void = 1 as *mut c_void;
        assert_eq!(decode_base64_string(core::ptr::null(), &mut h), E_INVALIDARG);
        assert_eq!(decode_base64_string(wide("AA==").as_ptr(), core::ptr::null_mut()), E_POINTER);
        RoInitialize(1);
        assert_eq!(decode_base64_string(wide("SGVsbG8sIFhib3g=").as_ptr(), &mut h), 0);
        let mut len = 0u32;
        let p = WindowsGetStringRawBuffer(h, &mut len);
        assert_eq!(String::from_utf16(core::slice::from_raw_parts(p, len as usize)).unwrap(), "Hello, Xbox");
        WindowsDeleteString(h);
        assert!(decode_base64_string(wide("%%%").as_ptr(), &mut h) < 0);
        assert!(h.is_null());
    }
}

#[test]
fn execute_remote_run_without_xtf() {
    unsafe {
        let name = wide("XtfRemoteRun.dll");
        let m = LoadLibraryExW(name.as_ptr(), core::ptr::null_mut(), 0);
        let e = GetLastError();
        let hr = ExecuteRemoteRun(wide("console").as_ptr(), wide("cmd.exe").as_ptr(), wide("/c").as_ptr(), true, false, core::ptr::null_mut());
        assert!(m.is_null());
        assert_eq!(hr, ((e & 0xffff) | 0x8007_0000) as i32);
        assert_eq!(run_flags(true, false), 0x4000_0004);
        assert_eq!(run_flags(false, false), 0x4000_0000);
        assert_eq!(run_flags(true, true), 0x4);
        let mut cb = Callback { vtbl: &CALLBACK_VTBL, refs: core::sync::atomic::AtomicI32::new(1) };
        let mut out: *mut c_void = core::ptr::null_mut();
        assert_eq!((CALLBACK_VTBL.query_interface)(&mut cb, &IID_REMOTE_RUN_CALLBACK, &mut out), 0);
        assert_eq!(out, &mut cb as *mut Callback as *mut c_void);
        assert_eq!((CALLBACK_VTBL.query_interface)(&mut cb, &IID_REMOTE_RUN_CLIENT, &mut out), E_NOINTERFACE);
        assert_eq!((CALLBACK_VTBL.release)(&mut cb), 1);
        let msg = wide("remote line\n");
        let printed = capture_bytes(|| assert_eq!((CALLBACK_VTBL.write_output)(&mut cb, msg.as_ptr()), 0));
        assert_eq!(printed, b"remote line\n".to_vec());
        let mut bstr: *mut u16 = 1 as *mut u16;
        assert_eq!((CALLBACK_VTBL.read_input)(&mut cb, &mut bstr), 0);
        assert!(bstr.is_null());
    }
}

fn rest() -> Box<RestContext> {
    let mut r: Box<RestContext> = Box::new(unsafe { core::mem::zeroed() });
    unsafe { rest_context_ctor(&mut *r, wide("ignored.host").as_ptr(), 0) };
    r
}

#[test]
fn rest_context_default_constructor() {
    let mut r: Box<RestContext> = Box::new(unsafe { core::mem::zeroed() });
    r.port = 77;
    unsafe { rest_context_default_ctor(&mut *r) };
    assert_eq!((r.ssl_enabled, r.port, r.initialized, r.host.size, r.host.res, r.csrf_token.res), (1, 77, 0, 0, 7, 7));
}

#[test]
fn rest_context_constructor() {
    let r = rest();
    assert_eq!(text(&r.host), "localhost");
    assert_eq!((r.port, r.initialized, r.ssl_enabled), (DEFAULT_PORT, 0, 1));
    assert!(r.session.is_null() && r.connection.is_null());
    let mut s: Box<RestContext> = Box::new(unsafe { core::mem::zeroed() });
    unsafe { rest_context_ctor(&mut *s, core::ptr::null(), 8080) };
    assert_eq!(s.port, 8080);
}

#[test]
fn rest_context_initialize() {
    let mut r = rest();
    unsafe {
        assert_eq!(initialize(&mut *r), 0);
        assert_eq!(r.initialized, 1);
        assert!(!r.session.is_null() && !r.connection.is_null());
        assert_eq!(initialize(&mut *r), ERROR_ALREADY_INITIALIZED);
        rest_context_dtor(&mut *r);
    }
}

#[test]
fn rest_context_close_handles() {
    let mut r = rest();
    unsafe {
        initialize(&mut *r);
        close_handles(&mut *r);
        assert!(r.session.is_null() && r.connection.is_null());
        assert_eq!(r.initialized, 1);
        close_handles(&mut *r);
    }
}

#[test]
fn rest_context_destructor() {
    let mut r = rest();
    unsafe {
        initialize(&mut *r);
        let token = wide("a-csrf-token-long-enough-for-heap");
        WString::assign_cstr(&mut r.csrf_token, token.as_ptr());
        rest_context_dtor(&mut *r);
        assert!(r.session.is_null() && r.connection.is_null());
        assert_eq!((r.host.size, r.host.res, r.csrf_token.size, r.csrf_token.res), (0, 7, 0, 7));
    }
}

#[test]
fn rest_context_cache_csrf_token() {
    let mut r = rest();
    unsafe {
        assert_eq!(cache_csrf_token(&mut *r, core::ptr::null()), ERROR_INVALID_PARAMETER);
        assert_eq!(cache_csrf_token(&mut *r, wide("").as_ptr()), ERROR_INVALID_PARAMETER);
        assert_eq!(cache_csrf_token(&mut *r, wide("HTTP/1.1 200 OK\r\n").as_ptr()), 0);
        assert_eq!(r.csrf_token.size, 0);
        let h = wide("HTTP/1.1 200 OK\r\nset-cookie: csrf-token=AbC123/xyz; Path=/\r\nX: y\r\n");
        assert_eq!(cache_csrf_token(&mut *r, h.as_ptr()), 0);
        assert_eq!(text(&r.csrf_token), "AbC123/xyz; Path=/");
        assert_eq!(cache_csrf_token(&mut *r, wide("Set-Cookie: CSRF-Token=end").as_ptr()), 0);
        assert_eq!(text(&r.csrf_token), "end");
        rest_context_dtor(&mut *r);
    }
}

#[test]
fn rest_context_query_csrf_header() {
    let mut r = rest();
    unsafe {
        let mut status = 9u32;
        assert_eq!(query_csrf_header(&mut *r, core::ptr::null_mut(), &mut status), 6);
        assert_eq!(status, 0);
        rest_context_dtor(&mut *r);
    }
}

fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

#[test]
fn rest_context_get() {
    let mut r = rest();
    unsafe {
        let (mut u, mut pw, mut out) = (ws("administrator-user-name"), ws("pw"), ws("stale"));
        assert_eq!(get(&mut *r, &mut u, &mut pw, wide("/api/os/info").as_ptr(), &mut out), ERROR_NOT_INITIALIZED);
        assert_eq!((u.size, u.res, pw.res), (0, 7, 7));
        assert_eq!(text(&out), "stale");
        r.port = closed_port();
        assert_eq!(initialize(&mut *r), 0);
        let (mut u, mut pw) = (ws(""), ws(""));
        assert!(matches!(get(&mut *r, &mut u, &mut pw, wide("/api/os/info").as_ptr(), &mut out), 12029 | 12002));
        assert_eq!(out.size, 0);
        rest_context_dtor(&mut *r);
        WString::tidy(&mut out);
    }
}

#[test]
fn rest_context_get_os_version() {
    let mut r = rest();
    unsafe {
        let (mut u, mut pw, mut out) = (ws("user"), ws("a-password-that-is-long"), ws(""));
        assert_eq!(get_os_version(&mut *r, &mut u, &mut pw, &mut out), ERROR_NOT_INITIALIZED);
        assert_eq!((u.size, pw.size, pw.res), (0, 0, 7));
        r.port = closed_port();
        initialize(&mut *r);
        let (mut u, mut pw) = (ws(""), ws(""));
        assert!(matches!(get_os_version(&mut *r, &mut u, &mut pw, &mut out), 12029 | 12002));
        rest_context_dtor(&mut *r);
    }
}

#[test]
fn rest_context_post() {
    let mut r = rest();
    unsafe {
        let body = b"{\"SSHPublicKey\":\"k\"} ";
        let (mut u, mut pw, mut out) = (ws("user"), ws("a-password-that-is-long"), ws("stale"));
        assert_eq!(post(&mut *r, &mut u, &mut pw, wide("/ext/ssh/users").as_ptr(), body.as_ptr(), &mut out), ERROR_NOT_INITIALIZED);
        assert_eq!((u.size, pw.size, pw.res), (0, 0, 7));
        assert_eq!(text(&out), "stale");
        r.port = closed_port();
        initialize(&mut *r);
        let (mut u, mut pw) = (ws(""), ws(""));
        assert!(matches!(post(&mut *r, &mut u, &mut pw, wide("/ext/ssh/users").as_ptr(), body.as_ptr(), &mut out), 12029 | 12002));
        assert_eq!(out.size, 0);
        WString::assign_cstr(&mut r.csrf_token, wide("tok").as_ptr());
        let (mut u, mut pw) = (ws(""), ws(""));
        assert!(matches!(post(&mut *r, &mut u, &mut pw, wide("/ext/ssh/users").as_ptr(), body.as_ptr(), &mut out), 12029 | 12002));
        rest_context_dtor(&mut *r);
        WString::tidy(&mut out);
    }
}
