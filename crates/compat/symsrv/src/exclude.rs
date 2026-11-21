use crate::win::*;
use crate::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub const SECTION_CHARS: u32 = 0x7d00;
pub const REG_EXCLUSIONS: &str = "Software\\Microsoft\\Symbol Server\\Exclusions";
pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
pub const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;

pub static PATTERNS: Mutex<Vec<Vec<u16>>> = Mutex::new(Vec::new());
pub static LOADED: AtomicBool = AtomicBool::new(false);

#[link(name = "kernel32")]
extern "system" {
    fn GetPrivateProfileSectionW(app: *const u16, out: *mut u16, n: u32, file: *const u16) -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(key: isize, sub: *const u16, opts: u32, sam: u32, out: *mut isize) -> i32;
    fn RegEnumValueW(key: isize, i: u32, name: *mut u16, n: *mut u32, r: *mut u32, t: *mut u32, d: *mut u8, dn: *mut u32) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}

pub fn reset() {
    LOADED.store(false, Ordering::SeqCst);
    PATTERNS.lock().unwrap().clear();
}

pub fn ini_path() -> Vec<u16> {
    let mut d = eula::module_dir();
    add_backslash(&mut d);
    d.extend(w("symsrv.ini"));
    d
}

fn parse(multi: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < multi.len() && multi[i] != 0 {
        let n = multi[i..].iter().position(|c| *c == 0).unwrap_or(multi.len() - i);
        out.push(multi[i..i + n].to_vec());
        i += n + 1;
    }
    out
}

pub fn load_ini(ini: &[u16]) -> bool {
    if !exists(ini) {
        unsafe { SetLastError(map_missing(GetLastError())) };
        return false;
    }
    let mut buf = vec![0u16; SECTION_CHARS as usize];
    let n = unsafe { GetPrivateProfileSectionW(z(&w("exclusions")).as_ptr(), buf.as_mut_ptr(), SECTION_CHARS, z(ini).as_ptr()) };
    if n == 0 {
        return true;
    }
    if n == SECTION_CHARS - 2 {
        trace::print(&format!("Exclusion section in {} is too large.  Use the registry\n", trace::ws(ini)));
        return true;
    }
    *PATTERNS.lock().unwrap() = parse(&buf[..n as usize + 1]);
    true
}

pub fn load_reg(root: isize, sub: &str) -> bool {
    unsafe {
        let mut key = 0isize;
        if RegOpenKeyExW(root, z(&w(sub)).as_ptr(), 0, 0x20019, &mut key) != 0 {
            return false;
        }
        let mut names = Vec::new();
        let mut i = 0u32;
        loop {
            let mut name = [0u16; MAX_PATH_W];
            let mut n = MAX_PATH_W as u32;
            if RegEnumValueW(key, i, name.as_mut_ptr(), &mut n, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) != 0 {
                break;
            }
            names.push(name[..n as usize].to_vec());
            i += 1;
        }
        RegCloseKey(key);
        if names.is_empty() {
            return false;
        }
        *PATTERNS.lock().unwrap() = names;
        true
    }
}

pub fn load() -> bool {
    if load_ini(&ini_path()) {
        return true;
    }
    let root = if http::is_service() { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    load_reg(root, REG_EXCLUSIONS)
}

pub const TIMER_NAME: &str = "SymSrvIniTimer";
pub const RELOAD_DUE: i64 = -600_000_000;
pub const RELOAD_TICKS: u32 = 0x1e;
pub static RELOAD_THREAD: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

#[link(name = "kernel32")]
extern "system" {
    fn CreateWaitableTimerExW(sa: *const u8, name: *const u16, flags: u32, access: u32) -> isize;
    fn SetWaitableTimer(h: isize, due: *const i64, period: i32, routine: *const (), arg: *const (), resume: i32) -> i32;
    fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    fn CreateThread(sa: *const u8, stack: usize, start: unsafe extern "system" fn(*mut u8) -> u32, arg: *mut u8, flags: u32, id: *mut u32) -> isize;
}

pub fn reload_timer() -> isize {
    unsafe { CreateWaitableTimerExW(core::ptr::null(), z(&w(TIMER_NAME)).as_ptr(), 1, 0x1f0003) }
}

pub fn reload_cycle(timer: isize, due: i64, ticks: u32, reload: &mut dyn FnMut()) {
    let mut n = 0u32;
    loop {
        if unsafe { SetWaitableTimer(timer, &due, 0, core::ptr::null(), core::ptr::null(), 0) } == 0 {
            break;
        }
        if unsafe { WaitForSingleObject(timer, 0xffff_ffff) } != 0 {
            break;
        }
        n += 1;
        if n >= ticks {
            break;
        }
    }
    reload();
}

unsafe extern "system" fn reload_thread(_: *mut u8) -> u32 {
    let timer = reload_timer();
    if timer == 0 {
        return 0;
    }
    loop {
        reload_cycle(timer, RELOAD_DUE, RELOAD_TICKS, &mut || {
            load();
        });
    }
}

pub fn start_reload_thread() {
    if RELOAD_THREAD.load(Ordering::SeqCst) != -1 {
        return;
    }
    let h = unsafe { CreateThread(core::ptr::null(), 0, reload_thread, core::ptr::null_mut(), 0, core::ptr::null_mut()) };
    RELOAD_THREAD.store(h, Ordering::SeqCst);
}

pub fn first_call() -> bool {
    if LOADED.swap(true, Ordering::SeqCst) {
        return true;
    }
    let r = load();
    if r && http::is_service() {
        start_reload_thread();
    }
    r
}

extern "C" {
    fn towlower(c: u16) -> u16;
    fn tolower(c: i32) -> i32;
}

fn token(p: &mut Option<usize>, pat: &[u16], case: bool) -> u16 {
    let Some(mut i) = *p else { return 0 };
    let at = |i: usize| pat.get(i).copied().unwrap_or(0);
    let c = at(i);
    let code = match c {
        0 => return 0,
        0x23 => 6,
        0x2a => 2,
        0x2b => 5,
        0x3f => 1,
        0x5b => 3,
        0x5d => 4,
        _ => {
            if c == 0x5c {
                i += 1;
                *p = Some(i);
                if at(i) == 0 {
                    return 0;
                }
            }
            let ch = at(i);
            if case {
                ch
            } else {
                unsafe { towlower(ch) as u8 as i8 as i16 as u16 }
            }
        }
    };
    *p = Some(i + 1);
    code
}

fn low(c: u16) -> u32 {
    unsafe { tolower(c as i32) as u32 }
}

pub fn matches(s: &[u16], si: usize, pat: &[u16], pi: Option<usize>, case: bool) -> u32 {
    let sat = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut s_i = si;
    let mut p = pi;
    let mut cur = pi;
    let mut prev: Option<usize> = None;
    let mut t = token(&mut p, pat, case);
    loop {
        match t {
            0 => return (sat(s_i) != 0) as u32,
            1 => {
                if sat(s_i) == 0 {
                    return 1;
                }
                s_i += 1;
            }
            2 => loop {
                if matches(s, s_i, pat, p, case) == 0 {
                    return 0;
                }
                let c = sat(s_i);
                s_i += 1;
                if c == 0 {
                    return 1;
                }
            },
            3 => {
                let mut c = sat(s_i);
                if c == 0 {
                    return 1;
                }
                if !case {
                    c = unsafe { towlower(c) };
                }
                let mut lo = 0u16;
                let mut hit = false;
                let mut k = token(&mut p, pat, case);
                loop {
                    if k == 0 {
                        break;
                    }
                    if k == 4 {
                        if token(&mut p, pat, case) != 6 {
                            return 1;
                        }
                        break;
                    }
                    if k == 0x2d {
                        let hi = token(&mut p, pat, case);
                        if hi & 0xfffb == 0 {
                            return 1;
                        }
                        if c >= lo && c <= hi {
                            hit = true;
                            k = hi;
                            break;
                        }
                        lo = hi;
                        if c == hi {
                            hit = true;
                            k = hi;
                            break;
                        }
                    } else {
                        lo = k;
                        if c == k {
                            hit = true;
                            break;
                        }
                    }
                    k = token(&mut p, pat, case);
                }
                if hit {
                    s_i += 1;
                    while k != 0 && k != 4 {
                        k = token(&mut p, pat, case);
                    }
                }
            }
            5 | 6 => {
                if matches(s, s_i, pat, p, case) == 0 {
                    return 0;
                }
                p = prev;
            }
            lit => {
                let c = sat(s_i);
                let ne = if case { lit != c } else { lit as u32 != low(c) };
                if ne {
                    if token(&mut p, pat, case) != 6 {
                        return 1;
                    }
                } else {
                    s_i += 1;
                }
            }
        }
        prev = cur;
        cur = p;
        t = token(&mut p, pat, case);
    }
}

pub fn wild(s: &[u16], pat: &[u16]) -> bool {
    matches(s, 0, pat, Some(0), false) == 0
}

pub fn excluded(file: &[u16]) -> bool {
    let list = PATTERNS.lock().unwrap().clone();
    for pat in list.iter() {
        if wild(file, pat) {
            trace::print(&format!("{} is in the file exclusion list\n", trace::ws(file)));
            return true;
        }
    }
    false
}
