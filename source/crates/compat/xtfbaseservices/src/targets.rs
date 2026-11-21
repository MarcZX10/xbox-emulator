use crate::ctx::Ctx;
use core::ffi::c_void;
use xtfcommon::msvc::operator_delete;
use xtfcommon::printf::format_alloc;
use xtfcommon::sys::*;

#[repr(C)]
pub struct NcVtbl {
    pub deleting_dtor: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
}

#[repr(C)]
pub struct TargetVtbl {
    pub deleting_dtor: unsafe extern "C" fn(*mut TargetBase, u32) -> *mut c_void,
    pub type_name: unsafe extern "C" fn(*const TargetBase) -> *const u8,
    pub log: unsafe extern "C" fn(*mut TargetBase, *const Ctx) -> HRESULT,
    pub on_close: unsafe extern "C" fn(*mut TargetBase) -> HRESULT,
}

#[repr(C)]
pub struct NcObject {
    pub vtbl: *const NcVtbl,
}

#[repr(C)]
pub struct TargetBase {
    pub vtbl: *const TargetVtbl,
    pub state: u32,
    pub pad: u32,
}

#[repr(C)]
pub struct FileTarget {
    pub base: TargetBase,
    pub name: [u16; 0x104],
    pub mutex: HANDLE,
    pub file: HANDLE,
    pub is_csv: u32,
    pub pad: u32,
}

#[repr(C)]
pub struct ConsoleTarget {
    pub base: TargetBase,
    pub mutex: HANDLE,
    pub stdout: HANDLE,
}

const _: () = assert!(core::mem::size_of::<TargetBase>() == 0x10);
const _: () = assert!(core::mem::size_of::<FileTarget>() == 0x230);
const _: () = assert!(core::mem::size_of::<ConsoleTarget>() == 0x20);

pub const FILE_TYPE: &[u8] = b"xtfbaseservices_commonlogtxtfile\0";
pub const CONSOLE_TYPE: &[u8] = b"xtfbaseservices_debuggerandconsole\0";
const CSV_HEADER: &[u8] = b"Debug Level,Module,Source File,Source Line,Process,Thread,Date,Time,Error,Message\r\n";

unsafe fn vector_delete(this: *mut c_void, flags: u32, elem: usize, dtor: unsafe fn(*mut c_void)) -> *mut c_void {
    if flags & 2 != 0 {
        let head = (this as *mut u8).sub(8);
        let count = *(head as *const usize);
        for i in (0..count).rev() {
            dtor((this as *mut u8).add(i * elem) as *mut c_void);
        }
        if flags & 1 != 0 {
            operator_delete(head as *mut c_void);
        }
        return head as *mut c_void;
    }
    dtor(this);
    if flags & 1 != 0 {
        operator_delete(this);
    }
    this
}

unsafe fn nc_dtor_body(this: *mut c_void) {
    (*(this as *mut NcObject)).vtbl = &NC_VTBL_LM;
}

unsafe extern "C" fn nc_deleting_dtor(this: *mut c_void, flags: u32) -> *mut c_void {
    vector_delete(this, flags, 8, nc_dtor_body)
}

#[export_name = "??_7LoggingManager@xtfbaseservices@@6B@"]
pub static NC_VTBL_LM: NcVtbl = NcVtbl { deleting_dtor: nc_deleting_dtor };

#[export_name = "??0LoggingManager@xtfbaseservices@@QEAA@XZ"]
pub unsafe extern "C" fn nc_ctor(this: *mut NcObject) -> *mut NcObject {
    (*this).vtbl = &NC_VTBL_LM;
    this
}

#[export_name = "??1LoggingManager@xtfbaseservices@@UEAA@XZ"]
pub unsafe extern "C" fn nc_dtor(this: *mut NcObject) {
    (*this).vtbl = &NC_VTBL_LM;
}

unsafe extern "C" fn pure_type_name(_t: *const TargetBase) -> *const u8 {
    _purecall();
    core::ptr::null()
}

unsafe extern "C" fn pure_log(_t: *mut TargetBase, _c: *const Ctx) -> HRESULT {
    _purecall();
    0
}

unsafe fn base_dtor_body(this: *mut c_void) {
    (*(this as *mut TargetBase)).vtbl = &NC_VTBL_LM as *const NcVtbl as *const TargetVtbl;
}

unsafe extern "C" fn base_deleting_dtor(this: *mut TargetBase, flags: u32) -> *mut c_void {
    vector_delete(this as *mut c_void, flags, 0x10, base_dtor_body)
}

#[export_name = "??_7LoggingTargetBase@xtfbaseservices@@6B@"]
pub static BASE_VTBL: TargetVtbl = TargetVtbl { deleting_dtor: base_deleting_dtor, type_name: pure_type_name, log: pure_log, on_close: base_on_close };

#[export_name = "??0LoggingTargetBase@xtfbaseservices@@QEAA@XZ"]
pub unsafe extern "C" fn base_ctor(this: *mut TargetBase) -> *mut TargetBase {
    (*this).state = 0;
    (*this).vtbl = &BASE_VTBL;
    this
}

#[export_name = "?IsOpen@LoggingTargetBase@xtfbaseservices@@QEBA_NXZ"]
pub unsafe extern "C" fn base_is_open(this: *const TargetBase) -> bool {
    (*this).state & 1 != 0
}

#[export_name = "?IsClosed@LoggingTargetBase@xtfbaseservices@@QEBA_NXZ"]
pub unsafe extern "C" fn base_is_closed(this: *const TargetBase) -> bool {
    (*this).state & 2 != 0
}

#[export_name = "?HasBeenOpened@LoggingTargetBase@xtfbaseservices@@QEBA_NXZ"]
pub unsafe extern "C" fn base_has_been_opened(this: *const TargetBase) -> bool {
    (*this).state & 1 != 0 || (*this).state & 2 != 0
}

#[export_name = "?HasBaseState@LoggingTargetBase@xtfbaseservices@@QEBA_NW4eLoggingTargetBaseState@2@@Z"]
pub unsafe extern "C" fn base_has_state(this: *const TargetBase, s: u32) -> bool {
    (*this).state & s == s
}

#[export_name = "?SetBaseState@LoggingTargetBase@xtfbaseservices@@QEAAXW4eLoggingTargetBaseState@2@_N@Z"]
pub unsafe extern "C" fn base_set_state(this: *mut TargetBase, s: u32, on: u8) {
    let cur = (*this).state;
    if on != 0 {
        (*this).state = cur | s;
    } else if cur & s == s {
        (*this).state = !s & cur;
    }
}

#[export_name = "?OnClose@LoggingTargetBase@xtfbaseservices@@MEAAJXZ"]
pub unsafe extern "C" fn base_on_close(this: *mut TargetBase) -> HRESULT {
    (*this).state |= 2;
    0
}

#[export_name = "?OnRemovedFromLoggingManager@LoggingTargetBase@xtfbaseservices@@AEAAJXZ"]
pub unsafe extern "C" fn base_on_removed(this: *mut TargetBase) -> HRESULT {
    if (*this).state & 2 != 0 {
        return 0;
    }
    ((*(*this).vtbl).on_close)(this)
}

unsafe fn file_dtor_body(this: *mut c_void) {
    file_dtor(this as *mut FileTarget);
}

unsafe extern "C" fn file_deleting_dtor(this: *mut TargetBase, flags: u32) -> *mut c_void {
    vector_delete(this as *mut c_void, flags, 0x230, file_dtor_body)
}

unsafe extern "C" fn file_type_name_v(this: *const TargetBase) -> *const u8 {
    file_type_name(this as *const FileTarget)
}

unsafe extern "C" fn file_log_v(this: *mut TargetBase, c: *const Ctx) -> HRESULT {
    file_log(this as *mut FileTarget, c)
}

unsafe extern "C" fn file_on_close_v(this: *mut TargetBase) -> HRESULT {
    file_on_close(this as *mut FileTarget)
}

#[export_name = "??_7LoggingTargetCommonLogTextFile@xtfbaseservices@@6B@"]
pub static FILE_VTBL: TargetVtbl = TargetVtbl { deleting_dtor: file_deleting_dtor, type_name: file_type_name_v, log: file_log_v, on_close: file_on_close_v };

#[export_name = "??0LoggingTargetCommonLogTextFile@xtfbaseservices@@QEAA@XZ"]
pub unsafe extern "C" fn file_ctor(this: *mut FileTarget) -> *mut FileTarget {
    (*this).file = INVALID_HANDLE_VALUE;
    (*this).base.vtbl = &FILE_VTBL;
    (*this).base.state = 0;
    (*this).mutex = core::ptr::null_mut();
    (*this).is_csv = 0;
    (*this).name = [0; 0x104];
    this
}

#[export_name = "??1LoggingTargetCommonLogTextFile@xtfbaseservices@@UEAA@XZ"]
pub unsafe extern "C" fn file_dtor(this: *mut FileTarget) {
    (*this).base.vtbl = &FILE_VTBL;
    file_close(this);
    (*this).base.vtbl = &NC_VTBL_LM as *const NcVtbl as *const TargetVtbl;
}

#[export_name = "?StaticGetTypeString@LoggingTargetCommonLogTextFile@xtfbaseservices@@SAPEBDXZ"]
pub extern "C" fn file_static_type() -> *const u8 {
    FILE_TYPE.as_ptr()
}

#[export_name = "?GetTargetTypeName@LoggingTargetCommonLogTextFile@xtfbaseservices@@UEBAPEBDXZ"]
pub unsafe extern "C" fn file_type_name(_this: *const FileTarget) -> *const u8 {
    FILE_TYPE.as_ptr()
}

#[export_name = "?GetFileName@LoggingTargetCommonLogTextFile@xtfbaseservices@@QEBAPEBGXZ"]
pub unsafe extern "C" fn file_get_name(this: *const FileTarget) -> *const u16 {
    (*this).name.as_ptr()
}

#[export_name = "?IsFileOpen@LoggingTargetCommonLogTextFile@xtfbaseservices@@QEAA_NXZ"]
pub unsafe extern "C" fn file_is_open(this: *mut FileTarget) -> bool {
    (*this).file != INVALID_HANDLE_VALUE
}

#[export_name = "?CloseFile@LoggingTargetCommonLogTextFile@xtfbaseservices@@IEAAJXZ"]
pub unsafe extern "C" fn file_close(this: *mut FileTarget) -> HRESULT {
    if !(*this).mutex.is_null() {
        CloseHandle((*this).mutex);
        (*this).mutex = core::ptr::null_mut();
    }
    if (*this).file != INVALID_HANDLE_VALUE {
        CloseHandle((*this).file);
        (*this).file = INVALID_HANDLE_VALUE;
    }
    0
}

#[export_name = "?OnClose@LoggingTargetCommonLogTextFile@xtfbaseservices@@MEAAJXZ"]
pub unsafe extern "C" fn file_on_close(this: *mut FileTarget) -> HRESULT {
    (*this).base.state |= 2;
    file_close(this)
}

#[export_name = "?Open@LoggingTargetCommonLogTextFile@xtfbaseservices@@QEAAJPEBGPEA_N@Z"]
pub unsafe extern "C" fn file_open(this: *mut FileTarget, name: *const u16, created: *mut u8) -> HRESULT {
    file_close(this);
    let mut full = [0u16; 0x104];
    let n = GetFullPathNameW(name, 0x104, full.as_mut_ptr(), core::ptr::null_mut()) as usize;
    if n == 0 {
        let hr = hr_from_last_error();
        if hr < 0 {
            return hr;
        }
    } else if n >= 0x104 {
        return HR_INSUFFICIENT_BUFFER;
    }
    let mut mname = [0u16; 0x104];
    for (i, c) in "Global\\".encode_utf16().enumerate() {
        mname[i] = c;
    }
    let mut k = 7usize;
    let mut i = 0usize;
    while i < n && k < 0x103 {
        let c = full[i];
        mname[k] = if iswalnum(c) != 0 { towupper(c) } else { b'_' as u16 };
        k += 1;
        i += 1;
    }
    mname[k] = 0;
    (*this).mutex = CreateMutexW(core::ptr::null_mut(), 0, mname.as_ptr());
    if (*this).mutex.is_null() {
        let hr = hr_from_last_error();
        if hr < 0 {
            return hr;
        }
    }
    let mut ext = [0u16; 0x100];
    _wsplitpath_s(name, core::ptr::null_mut(), 0, core::ptr::null_mut(), 0, core::ptr::null_mut(), 0, ext.as_mut_ptr(), 0x100);
    let csv: Vec<u16> = ".csv\0".encode_utf16().collect();
    (*this).is_csv = (_wcsicmp(ext.as_ptr(), csv.as_ptr()) == 0) as u32;
    if !crate::paths::ensure_directory_for(wslice(name)) {
        return E_FAIL;
    }
    (*this).file = CreateFileW(full.as_ptr(), 0xC000_0000, 3, core::ptr::null_mut(), 4, 0, core::ptr::null_mut());
    let err = GetLastError();
    let fresh = err != ERROR_ALREADY_EXISTS;
    if (*this).file == INVALID_HANDLE_VALUE {
        let hr = hr_from_win32(err);
        if hr < 0 {
            return hr;
        }
    }
    if fresh && (*this).is_csv == 1 {
        let hr = file_write(this, CSV_HEADER.as_ptr(), CSV_HEADER.len() as u32, core::ptr::null(), 0);
        if hr < 0 {
            return hr;
        }
    }
    wcscpy_s((*this).name.as_mut_ptr(), 0x104, name);
    if !created.is_null() {
        *created = fresh as u8;
    }
    0
}

#[export_name = "?WriteFile@LoggingTargetCommonLogTextFile@xtfbaseservices@@IEAAJPEBDK0K@Z"]
pub unsafe extern "C" fn file_write(this: *mut FileTarget, data: *const u8, len: u32, prefix: *const u8, prefix_len: u32) -> HRESULT {
    let mut written = 0u32;
    let mut hr = 0;
    WaitForSingleObject((*this).mutex, INFINITE);
    'w: {
        if SetFilePointer((*this).file, 0, core::ptr::null_mut(), 2) == 0xFFFF_FFFF {
            hr = hr_from_last_error();
            if hr < 0 {
                break 'w;
            }
        }
        if !prefix.is_null() && prefix_len != 0 {
            if WriteFile((*this).file, prefix as *const c_void, prefix_len, &mut written, core::ptr::null_mut()) == 0 {
                hr = hr_from_last_error();
            }
            if hr < 0 {
                break 'w;
            }
        }
        if WriteFile((*this).file, data as *const c_void, len, &mut written, core::ptr::null_mut()) == 0 {
            hr = hr_from_last_error();
            if hr < 0 {
                break 'w;
            }
        }
        if written != len {
            hr = STG_E_WRITEFAULT;
        }
    }
    ReleaseMutex((*this).mutex);
    hr
}

#[export_name = "?WriteText@LoggingTargetCommonLogTextFile@xtfbaseservices@@IEAAJAEBVLogMessageContext@log@xtfbase@@@Z"]
pub unsafe extern "C" fn file_write_text(this: *mut FileTarget, c: *const Ctx) -> HRESULT {
    file_write(this, (*c).text, (*c).text_len as u32, (*c).prefix, (*c).prefix_len)
}

pub unsafe fn csv_args(this: *const FileTarget, c: *const Ctx, st: &SYSTEMTIME) -> [u64; 16] {
    static EMPTY: [u8; 2] = [0, 0];
    let empty = EMPTY.as_ptr();
    let dt = crate::manager::XtfLogGetDebugType((*c).level);
    [
        b"%hs\r\n\0".as_ptr() as u64,
        if dt.is_null() { empty } else { dt } as u64,
        (*this).name.as_ptr() as u64,
        if (*c).func.is_null() { empty } else { (*c).func } as u64,
        (*c).line as u64,
        (*c).pid as u64,
        (*c).tid as u64,
        st.wMonth as u64,
        st.wDay as u64,
        st.wYear as u64,
        st.wHour as u64,
        st.wMinute as u64,
        st.wSecond as u64,
        st.wMilliseconds as u64,
        (*c).error as u64,
        if (*c).msg.is_null() { empty as *mut u8 } else { (*c).msg } as u64,
    ]
}

pub const CSV_FORMAT: &[u8] = b"%hs,%ls,%hs,%u,%u,%u,%02u/%02u/%04u,%02u:%02u:%02u.%03u,0x%08X\0";

#[export_name = "?WriteCSV@LoggingTargetCommonLogTextFile@xtfbaseservices@@IEAAJAEBVLogMessageContext@log@xtfbase@@@Z"]
pub unsafe extern "C" fn file_write_csv(this: *mut FileTarget, c: *const Ctx) -> HRESULT {
    let mut st = SYSTEMTIME::default();
    GetLocalTime(&mut st);
    let args = csv_args(this, c, &st);
    let mut out: *mut u8 = core::ptr::null_mut();
    let n = format_alloc(&mut out, CSV_FORMAT.as_ptr(), args.as_ptr() as *const c_void, false);
    let mut hr = 0;
    if n != 0 {
        hr = file_write(this, out, n as u32, core::ptr::null(), 0);
    }
    if !out.is_null() {
        free(out as *mut c_void);
    }
    hr
}

#[export_name = "?LogMessage@LoggingTargetCommonLogTextFile@xtfbaseservices@@UEAAJAEBVLogMessageContext@log@xtfbase@@@Z"]
pub unsafe extern "C" fn file_log(this: *mut FileTarget, c: *const Ctx) -> HRESULT {
    if (*this).is_csv == 1 {
        return file_write_csv(this, c);
    }
    file_write(this, (*c).text, (*c).text_len as u32, (*c).prefix, (*c).prefix_len)
}

unsafe fn console_dtor_body(this: *mut c_void) {
    base_dtor_body(this);
}

unsafe extern "C" fn console_deleting_dtor(this: *mut TargetBase, flags: u32) -> *mut c_void {
    vector_delete(this as *mut c_void, flags, 0x20, console_dtor_body)
}

unsafe extern "C" fn console_type_name_v(this: *const TargetBase) -> *const u8 {
    console_type_name(this as *const ConsoleTarget)
}

unsafe extern "C" fn console_log_v(this: *mut TargetBase, c: *const Ctx) -> HRESULT {
    console_log(this as *mut ConsoleTarget, c)
}

#[export_name = "??_7LoggingTargetDebuggerAndConsole@xtfbaseservices@@6B@"]
pub static CONSOLE_VTBL: TargetVtbl = TargetVtbl { deleting_dtor: console_deleting_dtor, type_name: console_type_name_v, log: console_log_v, on_close: base_on_close };

#[export_name = "??0LoggingTargetDebuggerAndConsole@xtfbaseservices@@QEAA@XZ"]
pub unsafe extern "C" fn console_ctor(this: *mut ConsoleTarget) -> *mut ConsoleTarget {
    (*this).base.vtbl = &CONSOLE_VTBL;
    (*this).base.state = 0;
    (*this).stdout = GetStdHandle(STD_OUTPUT_HANDLE);
    let mut name = [0u16; 0x104];
    let fmt: Vec<u16> = "Global\\XtfTextConsole%08X\0".encode_utf16().collect();
    let args = [GetCurrentProcessId() as u64];
    xtfcommon::printf::vswprintf_s(name.as_mut_ptr(), 0x104, fmt.as_ptr(), args.as_ptr() as *const c_void);
    name[0x103] = 0;
    (*this).mutex = CreateMutexW(core::ptr::null_mut(), 0, name.as_ptr());
    this
}

#[export_name = "?StaticGetTypeString@LoggingTargetDebuggerAndConsole@xtfbaseservices@@SAPEBDXZ"]
pub extern "C" fn console_static_type() -> *const u8 {
    CONSOLE_TYPE.as_ptr()
}

#[export_name = "?GetTargetTypeName@LoggingTargetDebuggerAndConsole@xtfbaseservices@@UEBAPEBDXZ"]
pub unsafe extern "C" fn console_type_name(_this: *const ConsoleTarget) -> *const u8 {
    CONSOLE_TYPE.as_ptr()
}

pub const CONSOLE_SKIPPED: HRESULT = 0x8C12_01FCu32 as i32;

#[export_name = "?LogMessage@LoggingTargetDebuggerAndConsole@xtfbaseservices@@UEAAJAEBVLogMessageContext@log@xtfbase@@@Z"]
pub unsafe extern "C" fn console_log(this: *mut ConsoleTarget, c: *const Ctx) -> HRESULT {
    let text = (*c).text;
    if text.is_null() {
        return E_INVALIDARG;
    }
    let dbg = IsDebuggerPresent() != 0;
    let main = crate::manager::XtfLogGetMainLevel();
    if (*c).level <= main && dbg {
        OutputDebugStringA(text);
    }
    let forced = (*c).flags & 2 != 0;
    let fallback = (*c).flags & 0x10 != 0 && !crate::manager::XtfIsMasterLogTargetActive();
    if !forced && !fallback {
        return CONSOLE_SKIPPED;
    }
    if (*c).level != 0 && ((*c).level > main || dbg) {
        return 0;
    }
    let color = (*c).color;
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    WaitForSingleObject((*this).mutex, INFINITE);
    let mut colored = false;
    if color != 0 && GetConsoleScreenBufferInfo((*this).stdout, &mut info) != 0 {
        colored = SetConsoleTextAttribute((*this).stdout, color) != 0;
    }
    let mut written = 0u32;
    WriteFile((*this).stdout, text as *const c_void, (*c).text_len as u32, &mut written, core::ptr::null_mut());
    if colored {
        SetConsoleTextAttribute((*this).stdout, info.attributes);
    }
    ReleaseMutex((*this).mutex);
    0
}
