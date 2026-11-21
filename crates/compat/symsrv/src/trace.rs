use crate::win::*;
use crate::*;
use std::sync::OnceLock;

pub const SSRVACTION_TRACE: usize = 1;
pub const SSRVACTION_QUERYCANCEL: usize = 2;
pub const SSRVACTION_EVENT: usize = 3;
pub const SSRVACTION_EVENTW: usize = 4;
pub const SSRVACTION_SIZE: usize = 5;
pub const PREFIX: &str = "SYMSRV:  ";
pub const BUF_CHARS: usize = 0x3e8;

#[link(name = "kernel32")]
extern "system" {
    fn GetEnvironmentVariableW(name: *const u16, buf: *mut u16, n: u32) -> u32;
    fn OutputDebugStringW(s: *const u16);
    fn FormatMessageW(flags: u32, src: isize, id: u32, lang: u32, buf: *mut u16, n: u32, args: *const u8) -> u32;
}

#[repr(C)]
pub struct Event<T> {
    pub severity: u32,
    pub code: u32,
    pub desc: *const T,
    pub object: u64,
}

pub type Callback = unsafe extern "system" fn(usize, u64, u64) -> i32;

pub fn dbgout() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| unsafe {
        let mut b = [0u16; 0x104];
        GetEnvironmentVariableW(z(&w("SYMSRV_DBGOUT")).as_ptr(), b.as_mut_ptr(), 0x104) != 0
    })
}

pub fn callback() -> Option<Callback> {
    let p = STATE.lock().unwrap().callback;
    if p == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<usize, Callback>(p) })
    }
}

pub fn call(action: usize, data: u64) -> i32 {
    match callback() {
        Some(f) => unsafe { f(action, data, context()) },
        None => 0,
    }
}

fn ods(v: &[u16]) {
    unsafe { OutputDebugStringW(z(v).as_ptr()) };
}

fn clip(s: &str, n: usize) -> Vec<u16> {
    let mut v = w(s);
    v.truncate(n - 1);
    v
}

pub fn event(severity: u32, code: u32, desc: &[u16], object: u64) -> i32 {
    let mut shown = false;
    if dbgout() {
        ods(desc);
        shown = true;
    }
    let opts = options();
    if severity == 0 && opts & SSRVOPT_TRACE == 0 {
        return 1;
    }
    if callback().is_none() {
        if !shown {
            ods(desc);
        }
        return 1;
    }
    if opts & SSRVOPT_CALLBACKW != 0 {
        let d = z(desc);
        let e = Event { severity, code, desc: d.as_ptr(), object };
        return call(SSRVACTION_EVENTW, &e as *const Event<u16> as u64);
    }
    let mut a = w2a(desc);
    a.truncate(BUF_CHARS - 1);
    a.push(0);
    let e = Event { severity, code, desc: a.as_ptr(), object };
    call(SSRVACTION_EVENT, &e as *const Event<u8> as u64)
}

pub fn trace(text: &str) {
    if text.is_empty() {
        return;
    }
    if !has(SSRVOPT_TRACE) && !dbgout() {
        return;
    }
    let wb = clip(text, BUF_CHARS);
    if wb.is_empty() {
        return;
    }
    let r = event(0, 0, &wb, 0);
    if r == 0 && callback().is_some() {
        let mut a = w2a(&wb);
        a.push(0);
        call(SSRVACTION_TRACE, a.as_ptr() as u64);
    }
}

pub fn print(text: &str) {
    if !has(SSRVOPT_TRACE) && !dbgout() {
        return;
    }
    let mut s = String::from(PREFIX);
    s.push_str(text);
    trace(&String::from_utf16_lossy(&clip(&s, BUF_CHARS)));
}

pub fn dbg(text: &str) {
    if !dbgout() {
        return;
    }
    let mut s = String::from(PREFIX);
    s.push_str(text);
    ods(&clip(&s, BUF_CHARS));
}

pub fn ws(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

pub fn error_text(err: u32) -> String {
    unsafe {
        let mut flags = 0x1000u32;
        let mut module = 0isize;
        let mut id = err;
        if err & 0x1000_0000 != 0 {
            module = GetModuleHandleW(z(&w("ntdll")).as_ptr());
            if module != 0 {
                flags = 0x1800;
                id &= !0x1000_0000;
            }
        } else if err as i32 > 0x2ee0 {
            let name = if http::engine() == SSRVOPT_WININET { "wininet" } else { "winhttp" };
            module = GetModuleHandleW(z(&w(name)).as_ptr());
            if module != 0 {
                flags = 0x1800;
            }
        }
        let mut buf = vec![0u16; 0x800];
        let mut n = FormatMessageW(flags | 0x200, module, id, 0, buf.as_mut_ptr(), 0x800, core::ptr::null()) as usize;
        while n > 0 && matches!(buf[n - 1], 0x20 | 0x09..=0x0d) {
            n -= 1;
        }
        let mut s = String::from_utf16_lossy(&buf[..n]);
        if n == 0 {
            s = format!("error 0x{id:x}");
        }
        if s.ends_with('\n') {
            s.pop();
        }
        s
    }
}

pub fn store_error(err: u32, store: &[u16]) {
    if err.wrapping_add(1) > 1 {
        print(&format!("store {} - {}\n", ws(store), error_text(err)));
    }
}

pub fn path_error(store: &mut store::Store, err: u32, path: &[u16]) -> bool {
    let e = map_missing(err);
    unsafe { SetLastError(e) };
    match e {
        0 => return true,
        2 => print(&format!("{} - file not found\n", ws(path))),
        3 => print(&format!("{} - path not found\n", ws(path))),
        5 => {
            if store.disabled() {
                print(&format!("{} - access is denied\n         Use of this store has been disabled\n", ws(&store.path)));
            } else {
                print(&format!("{} - access is denied\n", ws(path)));
            }
        }
        21 => print(&format!("{} - drive not ready\n", ws(path))),
        x => print(&format!("{} - error 0x{x:08x}\n", ws(path))),
    }
    unsafe { SetLastError(e) };
    false
}

#[repr(C)]
#[derive(Default)]
struct SystemTime {
    v: [u16; 8],
}

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemTime(t: *mut SystemTime);
}

fn second() -> u32 {
    let mut t = SystemTime::default();
    unsafe { GetSystemTime(&mut t) };
    t.v[6] as u32
}

pub struct Progress {
    pub total: i64,
    pub done: i64,
    pub last_second: u32,
    pub name: Vec<u16>,
    pub src: Vec<u16>,
    pub store: Vec<u16>,
}

impl Progress {
    pub fn new(name: &[u16], src: &[u16], store: &[u16]) -> Progress {
        Progress { total: 0, done: 0, last_second: 0, name: name.to_vec(), src: src.to_vec(), store: store.to_vec() }
    }

    pub fn display(&mut self) {
        if self.name.is_empty() {
            return;
        }
        let file = ws(&store::fname_ext(&self.src));
        let store = ws(&self.store);
        let bs = |n: usize| "\u{8}".repeat(n);
        if self.total == 0 {
            if self.done == -1 {
                trace(&bs(15));
                trace("cancelled      \n");
                unsafe { SetLastError(0x4d3) };
            } else if self.done == 0 {
                print(&format!("{file} from {store}: {} bytes -  ", self.total));
                trace(&format!("\u{8}{:>12} ", 0));
            } else {
                let s = second();
                if s != self.last_second {
                    trace(&format!("{}{:>12}", bs(12), self.done));
                }
                self.last_second = s;
            }
            if self.done != 0 && self.done == self.total {
                trace(&bs(15));
                trace("copied         \n");
            }
        } else {
            if self.done == -1 {
                trace(&bs(13));
                trace(" cancelled      \n");
                unsafe { SetLastError(0x4d3) };
            } else if self.done == 0 {
                print(&format!("{file} from {store}: {} bytes -  ", self.total));
                trace("  0 percent");
            } else {
                let s = second();
                if s != self.last_second {
                    trace(&format!("{} {:>3} percent", bs(13), self.done * 100 / self.total));
                }
                self.last_second = s;
            }
            if self.done != 0 && self.done == self.total {
                trace(&bs(13));
                trace(" copied         \n");
            }
        }
    }
}

pub unsafe extern "system" fn copy_progress(total: i64, done: i64, _ss: i64, _sd: i64, _n: u32, _why: u32, _hs: isize, _hd: isize, data: *mut Progress) -> u32 {
    let p = &mut *data;
    let mut size = total;
    if p.total == 0 && callback().is_some() {
        call(SSRVACTION_SIZE, &mut size as *mut i64 as u64);
    }
    p.total = total;
    p.done = done;
    p.display();
    let mut cancel = 0u64;
    if callback().is_some() && call(SSRVACTION_QUERYCANCEL, &mut cancel as *mut u64 as u64) != 0 && cancel as u32 != 0 {
        p.done = -1;
        p.display();
        return 1;
    }
    0
}
