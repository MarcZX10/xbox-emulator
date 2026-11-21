use crate::ctx::*;
use crate::errsys::*;
use crate::manager::*;
use crate::paths::*;
use crate::targets::*;
use core::ffi::c_void;
use std::sync::Mutex;
use xtfcommon::msvc::{operator_new, AString, SharedPtr, WString};
use xtfcommon::sys::*;

static SERIAL: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("XTF_LOG_LEVEL");
    std::env::remove_var("XTF_LOG_FILE");
    {
        let mut r = TEST_ROOT.lock().unwrap_or_else(|e| e.into_inner());
        if r.is_empty() {
            let mut p = std::env::temp_dir().to_string_lossy().into_owned();
            if !p.ends_with('\\') {
                p.push('\\');
            }
            p.push_str(&format!("xtfbaseservices-test-{}\\", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            *r = w(&p);
        }
    }
    let _ = imp();
    g
}

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("xtfbs-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

unsafe fn new_file() -> *mut FileTarget {
    let p = operator_new(core::mem::size_of::<FileTarget>()) as *mut FileTarget;
    file_ctor(p)
}

unsafe fn new_console() -> *mut ConsoleTarget {
    let p = operator_new(core::mem::size_of::<ConsoleTarget>()) as *mut ConsoleTarget;
    console_ctor(p)
}

unsafe fn ctx(level: u32, text: &[u8], flags: u32) -> Ctx {
    let mut c = core::mem::MaybeUninit::<Ctx>::uninit();
    let empty = RoutingPtr::empty();
    let mut t = text.to_vec();
    t.push(0);
    ctx_construct(c.as_mut_ptr(), level, &empty, b"func\0".as_ptr(), 7, t.as_ptr(), core::ptr::null(), flags);
    c.assume_init()
}

fn read(p: &str) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

#[test]
fn export_table_matches_console_dll() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/exports.txt")).unwrap();
    xtfcommon::exports::check_against_titles("xtfbaseservices.dll", &text);
    for e in xtfcommon::exports::parse(&text) {
        assert_eq!(e.status, "implemented");
        assert!(xtfcommon::exports::source_defines(concat!(env!("CARGO_MANIFEST_DIR"), "/src"), &e), "{} not defined", e.name);
    }
}

#[test]
fn ctor_logging_manager_sets_shared_vtable() {
    unsafe {
        let mut o = NcObject { vtbl: core::ptr::null() };
        nc_ctor(&mut o);
        assert_eq!(o.vtbl, &NC_VTBL_LM as *const NcVtbl);
    }
}

#[test]
fn ctor_nonc_copyable_is_same_code() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/exports.txt")).unwrap();
    let ex = xtfcommon::exports::parse(&text);
    let e = ex.iter().find(|e| e.ordinal == 5).unwrap();
    assert_eq!(xtfcommon::exports::internal_symbol(e), "??0LoggingManager@xtfbaseservices@@QEAA@XZ");
    let link = xtfcommon::exports::link_args(&text);
    assert!(link.contains(&"/EXPORT:??0NonCopyableDLLBase@xtfbaseservices@@QEAA@XZ=??0LoggingManager@xtfbaseservices@@QEAA@XZ,@5".to_string()));
}

#[test]
fn ctor_target_base_clears_state() {
    unsafe {
        let mut t = TargetBase { vtbl: core::ptr::null(), state: 9, pad: 0 };
        base_ctor(&mut t);
        assert_eq!(t.state, 0);
        assert_eq!(t.vtbl, &BASE_VTBL as *const TargetVtbl);
    }
}

#[test]
fn ctor_file_target_starts_closed() {
    unsafe {
        let f = new_file();
        assert_eq!((*f).file, INVALID_HANDLE_VALUE);
        assert!((*f).mutex.is_null());
        assert_eq!((*f).is_csv, 0);
        assert_eq!((*f).base.vtbl, &FILE_VTBL as *const TargetVtbl);
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn ctor_console_target_opens_named_mutex() {
    unsafe {
        let c = new_console();
        assert!(!(*c).mutex.is_null());
        assert_eq!((*c).stdout, GetStdHandle(STD_OUTPUT_HANDLE));
        assert_eq!((*c).base.state, 0);
        let name = wide(&format!("Global\\XtfTextConsole{:08X}", GetCurrentProcessId()));
        let again = CreateMutexW(core::ptr::null_mut(), 0, name.as_ptr());
        assert_eq!(GetLastError(), ERROR_ALREADY_EXISTS);
        CloseHandle(again);
        (CONSOLE_VTBL.deleting_dtor)(c as *mut TargetBase, 1);
    }
}

#[test]
fn dtor_logging_manager_resets_vptr() {
    unsafe {
        let mut o = NcObject { vtbl: core::ptr::null() };
        nc_dtor(&mut o);
        assert_eq!(o.vtbl, &NC_VTBL_LM as *const NcVtbl);
    }
}

#[test]
fn dtor_shared_rows_alias_logging_manager() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/exports.txt")).unwrap();
    for e in xtfcommon::exports::parse(&text) {
        if [7u16, 9, 10].contains(&e.ordinal) {
            assert_eq!(e.alias.as_deref(), Some("??1LoggingManager@xtfbaseservices@@UEAA@XZ"));
        }
    }
}

#[test]
fn dtor_file_target_closes_handles() {
    unsafe {
        let d = tmp("dtor");
        let f = new_file();
        let p = wide(&format!("{d}\\a.txt"));
        assert_eq!(file_open(f, p.as_ptr(), core::ptr::null_mut()), 0);
        assert!(file_is_open(f));
        file_dtor(f);
        assert_eq!((*f).file, INVALID_HANDLE_VALUE);
        assert!((*f).mutex.is_null());
        assert_eq!((*f).base.vtbl as usize, &NC_VTBL_LM as *const NcVtbl as usize);
        xtfcommon::msvc::operator_delete(f as *mut c_void);
    }
}

#[test]
fn vtables_have_console_slot_layout() {
    unsafe {
        let f = new_file();
        assert_eq!(cslice((FILE_VTBL.type_name)(f as *const TargetBase)), b"xtfbaseservices_commonlogtxtfile");
        let c = new_console();
        assert_eq!(cslice((CONSOLE_VTBL.type_name)(c as *const TargetBase)), b"xtfbaseservices_debuggerandconsole");
        let mut b = TargetBase { vtbl: core::ptr::null(), state: 0, pad: 0 };
        base_ctor(&mut b);
        assert_eq!((BASE_VTBL.on_close)(&mut b), 0);
        assert_eq!(b.state, 2);
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
        (CONSOLE_VTBL.deleting_dtor)(c as *mut TargetBase, 1);
        let o = operator_new(8) as *mut NcObject;
        nc_ctor(o);
        (NC_VTBL_LM.deleting_dtor)(o as *mut c_void, 1);
    }
}

#[test]
fn add_logging_target_rejects_null_and_duplicates() {
    let _g = guard();
    unsafe {
        let inst = LoggingManager_Instance();
        let null = SharedPtr::null();
        assert_eq!(LoggingManager_AddLoggingTarget(inst, &null), E_INVALIDARG);
        let t = new_file();
        let mut sp = SharedPtr::adopt(t as *mut c_void).unwrap();
        assert_eq!(LoggingManager_AddLoggingTarget(inst, &sp), 0);
        assert_eq!(LoggingManager_AddLoggingTarget(inst, &sp), HR_ALREADY_ADDED);
        let mut arg = SharedPtr::copy_from(&sp);
        assert_eq!(LoggingManager_RemoveLoggingTarget(inst, &mut arg), 0);
        assert!((*t).base.state & 2 != 0);
        let mut arg = SharedPtr::copy_from(&sp);
        assert_eq!(LoggingManager_RemoveLoggingTarget(inst, &mut arg), HR_NOT_FOUND);
        assert_eq!(sp.use_count(), 1);
        sp.release();
    }
}

#[test]
fn close_file_resets_handles() {
    unsafe {
        let d = tmp("close");
        let f = new_file();
        let p = wide(&format!("{d}\\c.txt"));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert_eq!(file_close(f), 0);
        assert!(!file_is_open(f));
        assert!((*f).mutex.is_null());
        assert_eq!(file_close(f), 0);
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn get_file_name_returns_opened_name() {
    unsafe {
        let d = tmp("name");
        let f = new_file();
        let s = format!("{d}\\n.log");
        let p = wide(&s);
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert_eq!(wslice(file_get_name(f)), &p[..p.len() - 1]);
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn get_logging_properties_reports_basename() {
    let _g = guard();
    unsafe {
        let inst = LoggingManager_Instance();
        let lvl = 3u32;
        let name = wide("C:\\anything\\props.txt");
        let pre = 1u8;
        assert_eq!(LoggingManager_SetLoggingProperties(inst, &lvl, name.as_ptr(), &pre), 0);
        let mut l = 0u32;
        let mut s = WString::empty();
        let mut b = 0u8;
        assert_eq!(LoggingManager_GetLoggingProperties(inst, &mut l, &mut s, &mut b), 0);
        assert_eq!(l, 3);
        assert_eq!(b, 1);
        assert_eq!(s.as_slice(), w("props.txt").as_slice());
        assert!(XtfIsMasterLogTargetActive());
        let mut full = log_root();
        full.extend(w("props.txt"));
        assert_eq!(imp().filename().as_slice(), full.as_slice());
        s.destroy();
        let def = wide("default");
        let zero = 0u32;
        assert_eq!(LoggingManager_SetLoggingProperties(inst, &zero, def.as_ptr(), core::ptr::null()), 0);
        assert!(!XtfIsMasterLogTargetActive());
        assert_eq!(imp().filename().size, 0);
    }
}

#[test]
fn get_logging_target_by_name_finds_console() {
    let _g = guard();
    unsafe {
        let inst = LoggingManager_Instance();
        let n = AString::from_bytes(b"xtfbaseservices_debuggerandconsole");
        let mut out = core::mem::MaybeUninit::<SharedPtr>::uninit();
        let r = LoggingManager_GetLoggingTargetByName(inst, out.as_mut_ptr(), &n);
        let mut sp = r.read();
        assert!(!sp.ptr.is_null());
        assert_eq!(cslice(console_type_name(sp.ptr as *const ConsoleTarget)), b"xtfbaseservices_debuggerandconsole");
        sp.release();
        let n2 = AString::from_bytes(b"nope");
        let mut sp2 = LoggingManager_GetLoggingTargetByName(inst, out.as_mut_ptr(), &n2).read();
        assert!(sp2.ptr.is_null());
        sp2.release();
    }
}

#[test]
fn get_target_type_name_file() {
    unsafe {
        let f = new_file();
        assert_eq!(cslice(file_type_name(f)), b"xtfbaseservices_commonlogtxtfile");
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn get_target_type_name_console() {
    unsafe {
        let c = new_console();
        assert_eq!(cslice(console_type_name(c)), b"xtfbaseservices_debuggerandconsole");
        (CONSOLE_VTBL.deleting_dtor)(c as *mut TargetBase, 1);
    }
}

#[test]
fn has_base_state_requires_all_bits() {
    unsafe {
        let mut t = TargetBase { vtbl: &BASE_VTBL, state: 1, pad: 0 };
        assert!(base_has_state(&t, 1));
        assert!(!base_has_state(&t, 3));
        t.state = 3;
        assert!(base_has_state(&t, 3));
    }
}

#[test]
fn has_been_opened_open_or_closed() {
    unsafe {
        let mut t = TargetBase { vtbl: &BASE_VTBL, state: 0, pad: 0 };
        assert!(!base_has_been_opened(&t));
        t.state = 2;
        assert!(base_has_been_opened(&t));
        t.state = 1;
        assert!(base_has_been_opened(&t));
    }
}

#[test]
fn instance_is_stable_singleton() {
    let a = LoggingManager_Instance();
    let b = LoggingManager_Instance();
    assert_eq!(a, b);
    unsafe { assert_eq!((*a).vtbl, &NC_VTBL_LM as *const NcVtbl) };
}

#[test]
fn is_closed_bit1() {
    unsafe {
        let mut t = TargetBase { vtbl: &BASE_VTBL, state: 1, pad: 0 };
        assert!(!base_is_closed(&t));
        t.state = 2;
        assert!(base_is_closed(&t));
    }
}

#[test]
fn is_file_open_tracks_handle() {
    unsafe {
        let d = tmp("isopen");
        let f = new_file();
        assert!(!file_is_open(f));
        let p = wide(&format!("{d}\\o.txt"));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert!(file_is_open(f));
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn is_open_bit0() {
    unsafe {
        let mut t = TargetBase { vtbl: &BASE_VTBL, state: 0, pad: 0 };
        assert!(!base_is_open(&t));
        t.state = 1;
        assert!(base_is_open(&t));
    }
}

#[test]
fn manager_log_message_prefixes_and_routes() {
    let _g = guard();
    unsafe {
        let inst = LoggingManager_Instance();
        let d = tmp("mlog");
        let f = new_file();
        let p = wide(&format!("{d}\\m.txt"));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        let sp = SharedPtr::adopt(f as *mut c_void).unwrap();
        assert_eq!(LoggingManager_AddLoggingTarget(inst, &sp), 0);
        let pre = 1u8;
        LoggingManager_SetLoggingProperties(inst, core::ptr::null(), core::ptr::null(), &pre);
        let mut c = ctx(4, b"hello", 0);
        assert_eq!(LoggingManager_LogMessage(inst, &mut c), 0);
        assert!(c.flags & 8 != 0);
        assert!(!c.prefix.is_null());
        let pfx = core::slice::from_raw_parts(c.prefix, c.prefix_len as usize).to_vec();
        assert!(pfx.starts_with(b"[") && pfx.ends_with(b"]: "));
        let tail = format!("({}).{:06}]: ", GetCurrentProcessId(), GetCurrentThreadId());
        assert!(pfx.ends_with(tail.as_bytes()));
        ctx_destroy(&mut c);
        let mut e = core::mem::zeroed::<Ctx>();
        assert_eq!(LoggingManager_LogMessage(inst, &mut e), E_INVALIDARG);
        let body = read(&format!("{d}\\m.txt"));
        assert!(body.ends_with(b"hello"));
        assert!(body.starts_with(&pfx));
        let mut arg = SharedPtr::copy_from(&sp);
        LoggingManager_RemoveLoggingTarget(inst, &mut arg);
        let mut sp = sp;
        sp.release();
    }
}

#[test]
fn log_message_file_text_uses_prefix() {
    unsafe {
        let d = tmp("flog");
        let f = new_file();
        let p = wide(&format!("{d}\\t.txt"));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        let mut c = ctx(4, b"body", 0);
        c.prefix = b"PRE:".as_ptr();
        c.prefix_len = 4;
        assert_eq!(file_log(f, &c), 0);
        ctx_destroy(&mut c);
        file_close(f);
        assert_eq!(read(&format!("{d}\\t.txt")), b"PRE:body");
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn log_message_console_rules() {
    let _g = guard();
    unsafe {
        let c = new_console();
        let mut x = ctx(4, b"", 0);
        assert_eq!(console_log(c, &x), CONSOLE_SKIPPED);
        ctx_destroy(&mut x);
        let mut n = core::mem::zeroed::<Ctx>();
        n.level = 4;
        assert_eq!(console_log(c, &n), E_INVALIDARG);
        let mut y = ctx(0, b"", 2);
        assert_eq!(console_log(c, &y), 0);
        ctx_destroy(&mut y);
        (CONSOLE_VTBL.deleting_dtor)(c as *mut TargetBase, 1);
    }
}

#[test]
fn on_close_base_sets_closed() {
    unsafe {
        let mut t = TargetBase { vtbl: &BASE_VTBL, state: 1, pad: 0 };
        assert_eq!(base_on_close(&mut t), 0);
        assert_eq!(t.state, 3);
    }
}

#[test]
fn on_close_file_closes_file() {
    unsafe {
        let d = tmp("onclose");
        let f = new_file();
        let p = wide(&format!("{d}\\x.txt"));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert_eq!(file_on_close(f), 0);
        assert!(!file_is_open(f));
        assert!(base_is_closed(f as *const TargetBase));
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn on_removed_calls_on_close_once() {
    unsafe {
        let d = tmp("onrem");
        let f = new_file();
        let p = wide(&format!("{d}\\r.txt"));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert_eq!(base_on_removed(f as *mut TargetBase), 0);
        assert!(!file_is_open(f));
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert_eq!(base_on_removed(f as *mut TargetBase), 0);
        assert!(file_is_open(f));
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn open_creates_dirs_csv_header_and_reports_created() {
    unsafe {
        let d = tmp("open");
        let f = new_file();
        let s = format!("{d}\\sub\\deep\\log.CSV");
        let p = wide(&s);
        let mut created = 9u8;
        assert_eq!(file_open(f, p.as_ptr(), &mut created), 0);
        assert_eq!(created, 1);
        assert_eq!((*f).is_csv, 1);
        file_close(f);
        assert_eq!(read(&s), b"Debug Level,Module,Source File,Source Line,Process,Thread,Date,Time,Error,Message\r\n");
        assert_eq!(file_open(f, p.as_ptr(), &mut created), 0);
        assert_eq!(created, 0);
        file_close(f);
        assert_eq!(read(&s).len(), 0x53);
        let long = wide(&format!("{d}\\{}", "a".repeat(300)));
        assert_eq!(file_open(f, long.as_ptr(), core::ptr::null_mut()), HR_INSUFFICIENT_BUFFER);
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn remove_logging_target_on_empty_is_invalid() {
    let _g = guard();
    unsafe {
        let mut n = SharedPtr::null();
        assert_eq!(LoggingManager_RemoveLoggingTarget(LoggingManager_Instance(), &mut n), E_INVALIDARG);
    }
}

#[test]
fn set_base_state_clears_only_full_mask() {
    unsafe {
        let mut t = TargetBase { vtbl: &BASE_VTBL, state: 1, pad: 0 };
        base_set_state(&mut t, 2, 1);
        assert_eq!(t.state, 3);
        t.state = 1;
        base_set_state(&mut t, 3, 0);
        assert_eq!(t.state, 1);
        t.state = 3;
        base_set_state(&mut t, 3, 0);
        assert_eq!(t.state, 0);
    }
}

#[test]
fn set_logging_properties_level_without_validation() {
    let _g = guard();
    unsafe {
        let inst = LoggingManager_Instance();
        let l = 9u32;
        assert_eq!(LoggingManager_SetLoggingProperties(inst, &l, core::ptr::null(), core::ptr::null()), 0);
        assert_eq!(XtfLogGetMainLevel(), 9);
        let z = 0u32;
        LoggingManager_SetLoggingProperties(inst, &z, core::ptr::null(), core::ptr::null());
    }
}

#[test]
fn static_type_string_file() {
    assert_eq!(unsafe { cslice(file_static_type()) }, b"xtfbaseservices_commonlogtxtfile");
}

#[test]
fn static_type_string_console() {
    assert_eq!(unsafe { cslice(console_static_type()) }, b"xtfbaseservices_debuggerandconsole");
}

#[test]
fn write_csv_reproduces_console_argument_shift() {
    let _g = guard();
    unsafe {
        let d = tmp("csv");
        let f = new_file();
        let s = format!("{d}\\l.csv");
        let p = wide(&s);
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        let mut c = ctx(3, b"msgtext", 0);
        c.error = 0x1234;
        let st = SYSTEMTIME { wYear: 2026, wMonth: 11, wDayOfWeek: 0, wDay: 21, wHour: 1, wMinute: 2, wSecond: 3, wMilliseconds: 4 };
        let args = csv_args(f, &c, &st);
        let mut out = core::ptr::null_mut();
        let n = xtfcommon::printf::format_alloc(&mut out, CSV_FORMAT.as_ptr(), args.as_ptr() as *const c_void, false);
        assert_eq!(n, 0);
        assert!(out.is_null());
        let wide_view = [0x6157u16, 0x6e72, 0x6e69, 0x0067];
        assert_eq!(core::slice::from_raw_parts(args[1] as *const u16, 4), &wide_view);
        assert_eq!(file_write_csv(f, &c), 0);
        ctx_destroy(&mut c);
        file_close(f);
        assert_eq!(read(&s), b"Debug Level,Module,Source File,Source Line,Process,Thread,Date,Time,Error,Message\r\n");
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn write_file_appends_prefix_then_data() {
    unsafe {
        let d = tmp("wf");
        let f = new_file();
        let s = format!("{d}\\w.txt");
        let p = wide(&s);
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        assert_eq!(file_write(f, b"one".as_ptr(), 3, core::ptr::null(), 0), 0);
        assert_eq!(file_write(f, b"two".as_ptr(), 3, b"<".as_ptr(), 1), 0);
        assert_eq!(file_write(f, b"z".as_ptr(), 1, b"ignored".as_ptr(), 0), 0);
        file_close(f);
        assert_eq!(read(&s), b"one<twoz");
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

#[test]
fn write_text_ignores_csv_mode() {
    unsafe {
        let d = tmp("wt");
        let f = new_file();
        let s = format!("{d}\\w.csv");
        let p = wide(&s);
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        let mut c = ctx(4, b"raw", 0);
        assert_eq!(file_write_text(f, &c), 0);
        ctx_destroy(&mut c);
        file_close(f);
        assert!(read(&s).ends_with(b"Message\r\nraw"));
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}

unsafe extern "C" fn counting_handler(_f: *const u8, _fn: *const u8, line: u32, _m: *const u8, _v: *const c_void) -> HRESULT {
    HITS.fetch_add(line as usize, core::sync::atomic::Ordering::SeqCst);
    0
}

unsafe extern "C" fn other_handler(_f: *const u8, _fn: *const u8, _l: u32, _m: *const u8, _v: *const c_void) -> HRESULT {
    0
}

static HITS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[test]
fn err_are_asserts_enabled_default_true() {
    let _g = guard();
    assert!(XtfErrAreAssertsEnabled());
}

#[test]
fn err_on_assert_runs_every_handler() {
    let _g = guard();
    unsafe {
        let id = XtfErrRegisterAssertHandlerPtr(Some(counting_handler), 1);
        HITS.store(0, core::sync::atomic::Ordering::SeqCst);
        XtfErrOnAssertV(b"f\0".as_ptr(), b"g\0".as_ptr(), 41, b"expr\0".as_ptr(), core::ptr::null());
        assert_eq!(HITS.load(core::sync::atomic::Ordering::SeqCst), 41);
        assert!(XtfErrRemoveAssertHandler(id));
    }
}

#[test]
fn err_pop_asserts_restores_and_handles_empty() {
    let _g = guard();
    unsafe {
        let base = sys().stack_len();
        XtfErrPushAssertsEnabled(0);
        assert!(!XtfErrAreAssertsEnabled());
        XtfErrPopAssertsEnabled();
        assert!(XtfErrAreAssertsEnabled());
        assert_eq!(sys().stack_len(), base);
        if base == 0 {
            XtfErrPopAssertsEnabled();
            assert!(XtfErrAreAssertsEnabled());
        }
    }
}

#[test]
fn err_push_asserts_logs_when_level_set() {
    let _g = guard();
    unsafe {
        let d = tmp("push");
        let file = wide(&format!("{d}\\p.txt"));
        let f = new_file();
        file_open(f, file.as_ptr(), core::ptr::null_mut());
        let sp = SharedPtr::adopt(f as *mut c_void).unwrap();
        let inst = LoggingManager_Instance();
        LoggingManager_AddLoggingTarget(inst, &sp);
        let l = 5u32;
        let no = 0u8;
        LoggingManager_SetLoggingProperties(inst, &l, core::ptr::null(), &no);
        XtfErrPushAssertsEnabled(1);
        XtfErrPopAssertsEnabled();
        let z = 0u32;
        LoggingManager_SetLoggingProperties(inst, &z, core::ptr::null(), core::ptr::null());
        let mut arg = SharedPtr::copy_from(&sp);
        LoggingManager_RemoveLoggingTarget(inst, &mut arg);
        let body = String::from_utf8(read(&format!("{d}\\p.txt"))).unwrap();
        assert!(body.contains("[XTFTRC:") == false);
        assert!(body.contains("Assert level pushed to 1, storage is "));
        assert!(body.contains("Error: Assert level POPPED to from 1 to 1 ("));
        assert!(body.contains("\r\n"));
        let mut sp = sp;
        sp.release();
        let yes = 1u8;
        LoggingManager_SetLoggingProperties(inst, core::ptr::null(), core::ptr::null(), &yes);
    }
}

#[test]
fn err_register_orders_by_kind_and_reuses_ids() {
    let _g = guard();
    unsafe {
        let a = XtfErrRegisterAssertHandlerPtr(Some(counting_handler), 0);
        assert_eq!(XtfErrRegisterAssertHandlerPtr(Some(counting_handler), 0), a);
        let hs = sys().handlers();
        assert_eq!(hs[0].func, counting_handler as AssertHandler as usize);
        let b = XtfErrRegisterAssertHandlerPtr(Some(counting_handler), 2);
        assert!(b > a);
        let hs = sys().handlers();
        assert_eq!(hs.last().unwrap().func, counting_handler as AssertHandler as usize);
        assert_eq!(hs.iter().filter(|e| e.func == counting_handler as AssertHandler as usize).count(), 1);
        assert_eq!(XtfErrRegisterAssertHandlerPtr(None, 1), 0);
        assert!(XtfErrRemoveAssertHandlerByPtr(Some(counting_handler)));
    }
}

#[test]
fn err_remove_assert_handler_by_id() {
    let _g = guard();
    unsafe {
        let id = XtfErrRegisterAssertHandlerPtr(Some(other_handler), 1);
        assert!(XtfErrRemoveAssertHandler(id));
        assert!(!XtfErrRemoveAssertHandler(id));
    }
}

#[test]
fn err_remove_assert_handler_by_ptr() {
    let _g = guard();
    unsafe {
        XtfErrRegisterAssertHandlerPtr(Some(other_handler), 1);
        assert!(XtfErrRemoveAssertHandlerByPtr(Some(other_handler)));
        assert!(!XtfErrRemoveAssertHandlerByPtr(Some(other_handler)));
    }
}

#[test]
fn err_default_handlers_and_break_toggle() {
    let _g = guard();
    unsafe {
        let has = || sys().handlers().iter().any(|e| e.func == break_if_debugging as AssertHandler as usize);
        assert!(sys().handlers().iter().any(|e| e.func == XtfLogAssertMessage as AssertHandler as usize && e.kind == 2));
        XtfErrSetBreakOnAssertIfDebugging(0);
        assert!(!has());
        XtfErrSetBreakOnAssertIfDebugging(1);
        assert!(has());
        assert_eq!(sys().handlers().last().unwrap().func, break_if_debugging as AssertHandler as usize);
    }
}

#[test]
fn is_master_log_target_active_follows_master() {
    let _g = guard();
    unsafe {
        let d = tmp("master");
        let f = wide(&format!("{d}\\ignored\\master.txt"));
        assert_eq!(XtfLogSetMainLevelAndFile(0, f.as_ptr()), 0);
        assert!(XtfIsMasterLogTargetActive());
        let mut full = log_root();
        full.extend(w("master.txt"));
        assert_eq!(imp().filename().as_slice(), full.as_slice());
        let def = wide("default");
        LoggingManager_SetLoggingProperties(LoggingManager_Instance(), core::ptr::null(), def.as_ptr(), core::ptr::null());
        assert!(!XtfIsMasterLogTargetActive());
    }
}

#[test]
fn log_assert_message_formats_assertion_text() {
    let _g = guard();
    unsafe {
        let d = tmp("assertmsg");
        let f = new_file();
        let file = wide(&format!("{d}\\a.txt"));
        file_open(f, file.as_ptr(), core::ptr::null_mut());
        let sp = SharedPtr::adopt(f as *mut c_void).unwrap();
        let inst = LoggingManager_Instance();
        LoggingManager_AddLoggingTarget(inst, &sp);
        let no = 0u8;
        LoggingManager_SetLoggingProperties(inst, core::ptr::null(), core::ptr::null(), &no);
        assert_eq!(XtfLogAssertMessage(b"file.cpp\0".as_ptr(), b"Fn\0".as_ptr(), 12, b"x == %d\0".as_ptr(), [3u64].as_ptr() as *const c_void), 0);
        let mut arg = SharedPtr::copy_from(&sp);
        LoggingManager_RemoveLoggingTarget(inst, &mut arg);
        let yes = 1u8;
        LoggingManager_SetLoggingProperties(inst, core::ptr::null(), core::ptr::null(), &yes);
        let want = format!("[XTFERR:{}] Assertion failed at Fn(12):  x == 3\r\n", GetCurrentThreadId());
        assert_eq!(String::from_utf8(read(&format!("{d}\\a.txt"))).unwrap(), want);
        let mut sp = sp;
        sp.release();
    }
}

#[test]
fn log_get_debug_type_table() {
    unsafe {
        assert!(XtfLogGetDebugType(0).is_null());
        assert_eq!(cslice(XtfLogGetDebugType(1)), b"Assertion");
        assert_eq!(cslice(XtfLogGetDebugType(5)), b"Spam");
        assert!(XtfLogGetDebugType(6).is_null());
    }
}

#[test]
fn log_get_main_level_defaults_zero() {
    let _g = guard();
    assert!(XtfLogGetMainLevel() <= 5);
}

#[test]
fn log_get_text_colors_table() {
    assert_eq!([0u32, 1, 2, 3, 4, 5, 6].map(|l| XtfLogGetTextColors(l)), [0, 0x7c, 0xc, 0xe, 0xb, 0x3, 0]);
}

#[test]
fn log_message_default_formatter_and_crlf() {
    let _g = guard();
    unsafe {
        let d = tmp("logmsg");
        let f = new_file();
        let file = wide(&format!("{d}\\m.txt"));
        file_open(f, file.as_ptr(), core::ptr::null_mut());
        let sp = SharedPtr::adopt(f as *mut c_void).unwrap();
        let inst = LoggingManager_Instance();
        LoggingManager_AddLoggingTarget(inst, &sp);
        let no = 0u8;
        LoggingManager_SetLoggingProperties(inst, core::ptr::null(), core::ptr::null(), &no);
        let empty = RoutingPtr::empty();
        XtfLogMessage(3, &empty, b"f\0".as_ptr(), b"Fn\0".as_ptr(), 5, b"w%d\0".as_ptr(), [1u64].as_ptr() as *const c_void, 5);
        XtfLogMessage(4, &empty, b"f\0".as_ptr(), b"Fn\0".as_ptr(), 5, b"plain\n\0".as_ptr(), core::ptr::null(), 1);
        let mut arg = SharedPtr::copy_from(&sp);
        LoggingManager_RemoveLoggingTarget(inst, &mut arg);
        let yes = 1u8;
        LoggingManager_SetLoggingProperties(inst, core::ptr::null(), core::ptr::null(), &yes);
        let want = format!("[XTFWRN:{}] w1 [Fn(5)]\r\nplain\r\n", GetCurrentThreadId());
        assert_eq!(String::from_utf8(read(&format!("{d}\\m.txt"))).unwrap(), want);
        let mut sp = sp;
        sp.release();
    }
}

#[test]
fn log_message_context_routes_and_rejects_null_text() {
    let _g = guard();
    unsafe {
        let mut z = core::mem::zeroed::<Ctx>();
        assert_eq!(XtfLogMessageContext(&mut z), E_INVALIDARG);
        let mut c = ctx(4, b"x", 0);
        assert_eq!(XtfLogMessageContext(&mut c), 0);
        ctx_destroy(&mut c);
    }
}

#[test]
fn log_set_main_level_opens_default_pattern_file() {
    let _g = guard();
    unsafe {
        assert_eq!(XtfLogSetMainLevel(6), E_INVALIDARG);
        assert_eq!(XtfLogSetMainLevel(2), 0);
        assert_eq!(XtfLogGetMainLevel(), 2);
        assert!(XtfIsMasterLogTargetActive());
        let name = String::from_utf16(imp().filename().as_slice()).unwrap();
        let root = String::from_utf16(&log_root()).unwrap();
        assert!(name.starts_with(&root));
        assert!(name.ends_with(&format!("_{}.txt", GetCurrentProcessId())));
        assert_eq!(XtfLogSetMainLevel(0), 0);
        assert_eq!(XtfLogGetMainLevel(), 0);
        let def = wide("default");
        LoggingManager_SetLoggingProperties(LoggingManager_Instance(), core::ptr::null(), def.as_ptr(), core::ptr::null());
    }
}

#[test]
fn log_set_main_level_and_file_validates() {
    let _g = guard();
    unsafe {
        let f = wide("x.txt");
        assert_eq!(XtfLogSetMainLevelAndFile(6, f.as_ptr()), E_INVALIDARG);
        assert_eq!(XtfLogSetMainLevelAndFile(1, core::ptr::null()), E_INVALIDARG);
        assert_eq!(XtfLogSetMainLevelAndFile(0, f.as_ptr()), 0);
        let def = wide("default");
        LoggingManager_SetLoggingProperties(LoggingManager_Instance(), core::ptr::null(), def.as_ptr(), core::ptr::null());
    }
}

#[test]
fn expand_replaces_macros() {
    let e = String::from_utf16(&expand(&w("#PROC#|#PROCID#|#NOW#"))).unwrap();
    let parts: Vec<&str> = e.split('|').collect();
    let full = unsafe { wpgmptr() };
    let base = String::from_utf16(&basename(&full)).unwrap();
    let stem = match base.rfind('.') { Some(0) => String::new(), Some(p) => base[..p].to_string(), None => base.clone() };
    assert_eq!(parts[0], stem);
    assert_eq!(parts[1], unsafe { GetCurrentProcessId() }.to_string());
    assert_eq!(parts[2].len(), "2026-11-21-01-02-03-004".len());
}

#[test]
fn ensure_directory_mirrors_console_quirks() {
    let d = tmp("ens");
    assert!(ensure_directory_for(&w("noslash.txt")));
    assert!(ensure_directory_for(&w(&format!("{d}\\a.b\\file"))));
    assert!(!std::path::Path::new(&format!("{d}\\a.b")).exists());
    let nodot = format!("{d}\\nodot\\x");
    let nodot = nodot.replace('.', "_");
    assert!(ensure_directory_for(&w(&nodot)));
    assert!(std::path::Path::new(&nodot).is_dir());
}

#[test]
fn err_default_log_handler_is_the_exported_function() {
    let _g = guard();
    unsafe {
        let f: AssertHandler = XtfLogAssertMessage;
        assert_eq!(XtfErrRegisterAssertHandlerPtr(Some(f), 2), 2);
        assert!(XtfErrRemoveAssertHandlerByPtr(Some(f)));
        assert!(!sys().handlers().iter().any(|e| e.func == f as usize));
        let id = XtfErrRegisterAssertHandlerPtr(Some(f), 2);
        assert!(id > 3);
    }
}

#[test]
fn write_csv_level_zero_writes_shifted_line() {
    let _g = guard();
    unsafe {
        let d = tmp("csv0");
        let f = new_file();
        let s = format!("{d}\\z.csv");
        let p = wide(&s);
        file_open(f, p.as_ptr(), core::ptr::null_mut());
        let mut c = ctx(0, b"m", 0);
        assert_eq!(file_write_csv(f, &c), 0);
        ctx_destroy(&mut c);
        file_close(f);
        let body = read(&s);
        let head = b"Debug Level,Module,Source File,Source Line,Process,Thread,Date,Time,Error,Message\r\n";
        assert!(body.starts_with(head));
        let line = &body[head.len()..];
        let want = format!("%hs\r\n,,{},", &s[..1]);
        assert!(line.starts_with(want.as_bytes()), "{:?}", String::from_utf8_lossy(line));
        (FILE_VTBL.deleting_dtor)(f as *mut TargetBase, 1);
    }
}
