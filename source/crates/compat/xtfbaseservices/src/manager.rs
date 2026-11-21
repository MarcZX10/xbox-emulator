use crate::ctx::Ctx;
use crate::paths::{basename, expand, local_systemtime, w, DEFAULT_PATTERN};
use crate::targets::{self, ConsoleTarget, FileTarget, NcObject, TargetBase, CONSOLE_TYPE};
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;
use xtfcommon::bena::CheckedBenaphore;
use xtfcommon::msvc::{operator_new, AString, SharedPtr, WString};
use xtfcommon::printf::sprintf_s;
use xtfcommon::sys::*;

pub const HR_SHUTTING_DOWN: HRESULT = 0x8C12_01FDu32 as i32;
pub const HR_NOT_FOUND: HRESULT = 0x8C12_01FEu32 as i32;
pub const HR_ALREADY_ADDED: HRESULT = 0x8C12_01FFu32 as i32;

const FLAG_SHUTDOWN: u8 = 1;
const FLAG_PREFIX: u8 = 2;

pub struct Impl {
    targets: UnsafeCell<Vec<SharedPtr>>,
    level: AtomicU32,
    filename: UnsafeCell<WString>,
    master: UnsafeCell<SharedPtr>,
    prefix: UnsafeCell<[u8; 0x200]>,
    lock: CheckedBenaphore,
    flags: AtomicU8,
}

unsafe impl Send for Impl {}
unsafe impl Sync for Impl {}

#[cfg(not(test))]
pub fn log_root() -> Vec<u16> {
    w("D:\\xtflogs\\")
}

#[cfg(test)]
pub static TEST_ROOT: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub fn log_root() -> Vec<u16> {
    let g = TEST_ROOT.lock().unwrap_or_else(|e| e.into_inner());
    if !g.is_empty() {
        return g.clone();
    }
    let mut p = std::env::temp_dir().to_string_lossy().into_owned();
    if !p.ends_with('\\') {
        p.push('\\');
    }
    p.push_str(&format!("xtfbaseservices-test-{}\\", std::process::id()));
    w(&p)
}

fn assert_hook(file: &'static [u8], func: &'static [u8], line: u32, expr: &'static [u8]) {
    crate::errsys::internal_assert(file, func, line, expr);
}

static IMPL: OnceLock<Impl> = OnceLock::new();

pub fn imp() -> &'static Impl {
    IMPL.get_or_init(|| {
        let i = Impl {
            targets: UnsafeCell::new(Vec::new()),
            level: AtomicU32::new(0),
            filename: UnsafeCell::new(WString::empty()),
            master: UnsafeCell::new(SharedPtr::null()),
            prefix: UnsafeCell::new([0; 0x200]),
            lock: CheckedBenaphore::new(assert_hook),
            flags: AtomicU8::new(FLAG_PREFIX),
        };
        unsafe {
            i.init();
            atexit(shutdown_hook);
        }
        i
    })
}

unsafe extern "C" fn shutdown_hook() {
    if let Some(i) = IMPL.get() {
        i.shutdown();
    }
}

pub unsafe fn env_string(name: &str) -> Option<Vec<u16>> {
    let n = wide(name);
    let count = GetEnvironmentVariableW(n.as_ptr(), core::ptr::null_mut(), 0);
    if count == 0 {
        return None;
    }
    if count > 0x7fff && crate::errsys::XtfErrAreAssertsEnabled() {
        crate::errsys::internal_assert(b"xbox\\devex\\xtf\\lib\\xtfbase\\XtfBaseHelpers_STL.h\0", b"xtfbase::GetEnvironmentVariableAsString\0", 0x1c2, b"theCharCount <= 32767\0");
    }
    let mut buf = vec![0u16; count as usize + 1];
    let got = GetEnvironmentVariableW(n.as_ptr(), buf.as_mut_ptr(), count + 1);
    if got == 0 || got > count {
        return None;
    }
    buf.truncate(got as usize);
    Some(buf)
}

impl Impl {
    pub fn is_shutting_down(&self) -> bool {
        self.flags.load(Ordering::Acquire) & FLAG_SHUTDOWN != 0
    }

    pub fn prefix_enabled(&self) -> bool {
        self.flags.load(Ordering::Acquire) & FLAG_PREFIX != 0
    }

    fn set_prefix_enabled(&self, on: bool) {
        if on {
            self.flags.fetch_or(FLAG_PREFIX, Ordering::AcqRel);
        } else {
            self.flags.fetch_and(!FLAG_PREFIX, Ordering::AcqRel);
        }
    }

    pub fn level(&self) -> u32 {
        self.level.load(Ordering::Acquire)
    }

    pub unsafe fn targets(&self) -> &mut Vec<SharedPtr> {
        &mut *self.targets.get()
    }

    pub unsafe fn filename(&self) -> &mut WString {
        &mut *self.filename.get()
    }

    pub unsafe fn master(&self) -> &mut SharedPtr {
        &mut *self.master.get()
    }

    unsafe fn init(&self) {
        if let Some(v) = env_string("XTF_LOG_LEVEL") {
            if !v.is_empty() {
                let mut z = v.clone();
                z.push(0);
                let l = _wtoi(z.as_ptr()) as u32;
                if l <= 5 {
                    self.level.store(l, Ordering::Release);
                }
            }
        }
        if let Some(v) = env_string("XTF_LOG_FILE") {
            if !v.is_empty() {
                let e = expand(&v);
                self.set_filename(&e);
            }
        }
        self.lock.lock();
        let found = self.find_by_type(&CONSOLE_TYPE[..CONSOLE_TYPE.len() - 1]);
        let have = !found.ptr.is_null();
        let mut found = found;
        found.release();
        if !have {
            let obj = operator_new(core::mem::size_of::<ConsoleTarget>()) as *mut ConsoleTarget;
            targets::console_ctor(obj);
            if let Some(sp) = SharedPtr::adopt(obj as *mut c_void) {
                self.targets().push(sp);
            }
        }
        let name = self.filename().as_slice().to_vec();
        self.open_master(Some(&name));
        self.lock.unlock();
    }

    pub unsafe fn shutdown(&self) {
        self.flags.fetch_or(FLAG_SHUTDOWN, Ordering::AcqRel);
        for t in self.targets().iter_mut() {
            t.release();
        }
        self.targets().clear();
        self.master().release();
        self.filename().destroy();
    }

    pub unsafe fn find_by_type(&self, name: &[u8]) -> SharedPtr {
        for t in self.targets().iter() {
            let tb = t.ptr as *mut TargetBase;
            let tn = cslice(((*(*tb).vtbl).type_name)(tb));
            if tn.len() == name.len() && tn == name {
                return SharedPtr::copy_from(t);
            }
        }
        SharedPtr::null()
    }

    pub unsafe fn set_filename(&self, name: &[u16]) -> bool {
        let base = basename(name);
        let f = self.filename();
        if base.is_empty() {
            if f.size == 0 {
                return false;
            }
            f.clear();
            return true;
        }
        let mut full = log_root();
        full.extend_from_slice(&base);
        if f.as_slice() == full.as_slice() {
            return false;
        }
        f.assign(&full);
        true
    }

    unsafe fn remove_internal(&self, ptr: *mut c_void) -> HRESULT {
        let ts = self.targets();
        let Some(i) = ts.iter().position(|t| t.ptr == ptr) else { return HR_NOT_FOUND };
        if self.master().ptr == ptr {
            self.master().release();
        }
        let mut gone = ts.remove(i);
        gone.release();
        0
    }

    pub unsafe fn open_master(&self, name: Option<&[u16]>) -> HRESULT {
        let Some(name) = name else { return 0 };
        self.set_filename(name);
        if self.filename().size == 0 {
            return 0;
        }
        let m = self.master();
        let mut created = false;
        if !m.ptr.is_null() {
            let mt = m.ptr as *const FileTarget;
            if wslice((*mt).name.as_ptr()) == self.filename().as_slice() {
                return 0;
            }
        } else {
            let obj = operator_new(core::mem::size_of::<FileTarget>()) as *mut FileTarget;
            if obj.is_null() {
                return E_OUTOFMEMORY;
            }
            targets::file_ctor(obj);
            match SharedPtr::adopt(obj as *mut c_void) {
                Some(sp) => {
                    let mut old = core::mem::replace(m, sp);
                    old.release();
                    created = true;
                }
                None => return E_OUTOFMEMORY,
            }
        }
        if m.ptr.is_null() {
            return E_OUTOFMEMORY;
        }
        let hr = targets::file_open(m.ptr as *mut FileTarget, self.filename().c_str(), core::ptr::null_mut());
        if hr >= 0 {
            if created {
                let c = SharedPtr::copy_from(m);
                self.targets().push(c);
            }
            return hr;
        }
        self.filename().clear();
        if !created {
            let p = m.ptr;
            self.remove_internal(p);
        }
        self.master().release();
        hr
    }

    unsafe fn build_prefix(&self, c: *mut Ctx) {
        if !self.prefix_enabled() {
            return;
        }
        if (*c).time == 0 {
            (*c).prefix = core::ptr::null();
            return;
        }
        let st = local_systemtime((*c).time);
        let buf = &mut *self.prefix.get();
        let args = [
            st.wYear as u64,
            st.wMonth as u64,
            st.wDay as u64,
            st.wHour as u64,
            st.wMinute as u64,
            st.wSecond as u64,
            st.wMilliseconds as u64,
            (*c).pid as u64,
            (*c).tid as u64,
        ];
        let n = sprintf_s(buf.as_mut_ptr(), 0x200, b"[%d-%02d-%02d %02d:%02d:%02d.%03d (%d).%06d]: \0".as_ptr(), &args);
        if (n.wrapping_sub(1) as u32) > 0x1fd {
            buf[0] = 0;
            (*c).prefix_len = 0;
            (*c).prefix = core::ptr::null();
        } else {
            (*c).prefix_len = n as u32;
            (*c).prefix = buf.as_ptr();
        }
    }

    pub unsafe fn log(&self, c: *mut Ctx) -> HRESULT {
        if (*c).text.is_null() {
            return E_INVALIDARG;
        }
        if self.prefix_enabled() {
            (*c).flags |= 8;
        }
        self.lock.lock();
        if self.prefix_enabled() && !self.targets().is_empty() {
            self.build_prefix(c);
        }
        let list: Vec<*mut TargetBase> = self.targets().iter().map(|t| t.ptr as *mut TargetBase).collect();
        for t in list {
            ((*(*t).vtbl).log)(t, c);
        }
        self.lock.unlock();
        0
    }
}

static mut INSTANCE: NcObject = NcObject { vtbl: &targets::NC_VTBL_LM };

unsafe extern "C" fn instance_dtor() {
    targets::nc_dtor(core::ptr::addr_of_mut!(INSTANCE));
}

#[export_name = "?Instance@LoggingManager@xtfbaseservices@@SAAEAV12@XZ"]
pub extern "C" fn LoggingManager_Instance() -> *mut NcObject {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        atexit(instance_dtor);
    });
    core::ptr::addr_of_mut!(INSTANCE)
}

#[export_name = "?AddLoggingTarget@LoggingManager@xtfbaseservices@@QEAAJAEBV?$shared_ptr@VLoggingTargetBase@xtfbaseservices@@@std@@@Z"]
pub unsafe extern "C" fn LoggingManager_AddLoggingTarget(_this: *mut NcObject, sp: *const SharedPtr) -> HRESULT {
    let m = imp();
    if m.is_shutting_down() {
        return HR_SHUTTING_DOWN;
    }
    if (*sp).ptr.is_null() {
        return E_INVALIDARG;
    }
    m.lock.lock();
    if m.targets().iter().any(|t| t.ptr == (*sp).ptr) {
        m.lock.unlock();
        return HR_ALREADY_ADDED;
    }
    m.targets().push(SharedPtr::copy_from(sp));
    let hr = if m.is_shutting_down() { HR_SHUTTING_DOWN } else { 0 };
    m.lock.unlock();
    hr
}

#[export_name = "?RemoveLoggingTarget@LoggingManager@xtfbaseservices@@QEAAJV?$shared_ptr@VLoggingTargetBase@xtfbaseservices@@@std@@@Z"]
pub unsafe extern "C" fn LoggingManager_RemoveLoggingTarget(_this: *mut NcObject, sp: *mut SharedPtr) -> HRESULT {
    let m = imp();
    let mut copy = SharedPtr::copy_from(sp);
    let hr;
    if m.is_shutting_down() {
        copy.release();
        hr = HR_SHUTTING_DOWN;
    } else if copy.ptr.is_null() {
        copy.release();
        hr = E_INVALIDARG;
    } else {
        m.lock.lock();
        let r = m.remove_internal(copy.ptr);
        m.lock.unlock();
        hr = if r < 0 { r } else { targets::base_on_removed(copy.ptr as *mut TargetBase) };
        copy.release();
    }
    (*sp).release();
    hr
}

#[export_name = "?GetLoggingTargetByName@LoggingManager@xtfbaseservices@@QEBA?AV?$shared_ptr@VLoggingTargetBase@xtfbaseservices@@@std@@AEBV?$basic_string@DU?$char_traits@D@std@@V?$allocator@D@2@@4@@Z"]
pub unsafe extern "C" fn LoggingManager_GetLoggingTargetByName(_this: *const NcObject, ret: *mut SharedPtr, name: *const AString) -> *mut SharedPtr {
    let m = imp();
    m.lock.lock();
    let r = m.find_by_type((*name).as_bytes());
    m.lock.unlock();
    ret.write(r);
    ret
}

#[export_name = "?GetLoggingProperties@LoggingManager@xtfbaseservices@@QEAAJAEAW4eLoggingLevel@log@xtfbase@@AEAV?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@AEA_N@Z"]
pub unsafe extern "C" fn LoggingManager_GetLoggingProperties(_this: *mut NcObject, level: *mut u32, file: *mut WString, prefix: *mut u8) -> HRESULT {
    let m = imp();
    *level = m.level();
    *prefix = m.prefix_enabled() as u8;
    let name = m.filename().as_slice().to_vec();
    (*file).assign(&name);
    let cur = (*file).as_slice().to_vec();
    if let Some(p) = crate::paths::rfind_char(&cur, b'\\' as u16) {
        if p + 1 < cur.len() {
            let mut nw = WString::from_slice(&cur[p + 1..]);
            (*file).destroy();
            core::ptr::swap(file, &mut nw);
        }
    }
    0
}

#[export_name = "?SetLoggingProperties@LoggingManager@xtfbaseservices@@QEAAJPEAW4eLoggingLevel@log@xtfbase@@PEBGPEA_N@Z"]
pub unsafe extern "C" fn LoggingManager_SetLoggingProperties(_this: *mut NcObject, level: *const u32, file: *const u16, prefix: *const u8) -> HRESULT {
    let m = imp();
    let mut hr = 0;
    m.lock.lock();
    if !prefix.is_null() {
        m.set_prefix_enabled(*prefix & 1 != 0);
    }
    if !file.is_null() {
        let f = wslice(file);
        if f == w("default").as_slice() {
            m.set_filename(&[]);
        } else {
            m.set_filename(f);
        }
    }
    if !level.is_null() {
        m.level.store(*level, Ordering::Release);
    }
    let mp = m.master().ptr;
    if !mp.is_null() {
        let mname = wslice((*(mp as *const FileTarget)).name.as_ptr()).to_vec();
        if mname.as_slice() != m.filename().as_slice() {
            m.remove_internal(mp);
            m.master().release();
        }
    }
    if m.filename().size != 0 && m.master().ptr.is_null() {
        let name = m.filename().as_slice().to_vec();
        hr = m.open_master(Some(&name));
    }
    m.lock.unlock();
    hr
}

#[export_name = "?LogMessage@LoggingManager@xtfbaseservices@@QEAAJAEBVLogMessageContext@log@xtfbase@@@Z"]
pub unsafe extern "C" fn LoggingManager_LogMessage(_this: *mut NcObject, c: *mut Ctx) -> HRESULT {
    imp().log(c)
}

#[no_mangle]
pub unsafe extern "C" fn XtfLogMessage(level: u32, cfg: *const crate::ctx::RoutingPtr, _file: *const u8, func: *const u8, line: u32, fmt: *const u8, va: *const c_void, flags: u32) -> HRESULT {
    let m = imp();
    let mut c = core::mem::MaybeUninit::<Ctx>::uninit();
    crate::ctx::ctx_construct(c.as_mut_ptr(), level, cfg, func, line, fmt, va, flags);
    let hr = m.log(c.as_mut_ptr());
    crate::ctx::ctx_destroy(c.as_mut_ptr());
    hr
}

#[no_mangle]
pub unsafe extern "C" fn XtfLogAssertMessage(_file: *const u8, func: *const u8, line: u32, fmt: *const u8, va: *const c_void) -> HRESULT {
    let empty = crate::ctx::RoutingPtr::empty();
    XtfLogMessage(1, &empty, _file, func, line, fmt, va, 5)
}

#[no_mangle]
pub unsafe extern "C" fn XtfLogMessageContext(c: *mut Ctx) -> HRESULT {
    imp().log(c)
}

#[no_mangle]
pub extern "C" fn XtfLogGetMainLevel() -> u32 {
    imp().level()
}

const TEXT_COLORS: [u16; 6] = [0, 0x7c, 0xc, 0xe, 0xb, 0x3];

#[no_mangle]
pub extern "C" fn XtfLogGetTextColors(level: u32) -> u16 {
    if level < 6 {
        TEXT_COLORS[level as usize]
    } else {
        0
    }
}

static DEBUG_TYPES: [&[u8]; 5] = [b"Assertion\0", b"Error\0", b"Warning\0", b"Trace\0", b"Spam\0"];

#[no_mangle]
pub extern "C" fn XtfLogGetDebugType(level: u32) -> *const u8 {
    if (1..6).contains(&level) {
        DEBUG_TYPES[level as usize - 1].as_ptr()
    } else {
        core::ptr::null()
    }
}

#[no_mangle]
pub extern "C" fn XtfIsMasterLogTargetActive() -> bool {
    let m = imp();
    unsafe {
        let p = m.master().ptr as *const FileTarget;
        !p.is_null() && (*p).file != INVALID_HANDLE_VALUE
    }
}

#[no_mangle]
pub unsafe extern "C" fn XtfLogSetMainLevel(level: u32) -> HRESULT {
    if level.wrapping_sub(1) <= 4 {
        let m = imp();
        let name = expand(&w(DEFAULT_PATTERN));
        m.lock.lock();
        m.level.store(level, Ordering::Release);
        m.open_master(Some(&name));
        m.lock.unlock();
        return 0;
    }
    let m = imp();
    if level > 5 {
        return E_INVALIDARG;
    }
    m.lock.lock();
    m.level.store(level, Ordering::Release);
    m.lock.unlock();
    0
}

#[no_mangle]
pub unsafe extern "C" fn XtfLogSetMainLevelAndFile(level: u32, file: *const u16) -> HRESULT {
    let m = imp();
    if level > 5 || file.is_null() {
        return E_INVALIDARG;
    }
    m.lock.lock();
    m.level.store(level, Ordering::Release);
    m.open_master(Some(wslice(file)));
    m.lock.unlock();
    0
}
