use core::ffi::c_void;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[link(name = "kernel32")]
extern "system" {
    fn GetLogicalProcessorInformationEx(rel: u32, buf: *mut u8, len: *mut u32) -> i32;
    fn GetThreadId(h: *mut c_void) -> u32;
    fn GetProcessId(h: *mut c_void) -> u32;
    fn GetCurrentProcessId() -> u32;
    fn SetLastError(code: u32);
}

pub const DEFAULT_CONSOLE_CORES: usize = 7;
const ERROR_INVALID_PARAMETER: u32 = 87;

#[derive(Clone, Debug)]
pub struct Topology {
    pub cores: usize,
    pub full: u64,
    pub host: u64,
    pub map: Vec<u64>,
}

fn full_mask(n: usize) -> u64 {
    if n >= 64 {
        u64::MAX
    } else {
        (1u64 << n) - 1
    }
}

pub fn build_map(physical: &[u64], n: usize) -> Vec<u64> {
    let n = n.clamp(1, 64);
    let physical: Vec<u64> = physical.iter().copied().filter(|m| *m != 0).collect();
    if physical.is_empty() {
        return vec![0; n];
    }
    let units: Vec<u64> = if physical.len() >= n {
        physical.clone()
    } else {
        let mut v = Vec::new();
        let mut layer = 0;
        loop {
            let mut any = false;
            for m in &physical {
                let bits: Vec<u32> = (0..64).filter(|b| m & (1u64 << b) != 0).collect();
                if let Some(b) = bits.get(layer) {
                    v.push(1u64 << b);
                    any = true;
                }
            }
            if !any {
                break;
            }
            layer += 1;
        }
        v
    };
    let mut map = vec![0u64; n];
    if physical.len() >= n {
        for (i, m) in map.iter_mut().enumerate() {
            *m = physical[i] & physical[i].wrapping_neg();
        }
    } else if units.len() >= n {
        for (j, u) in units.iter().enumerate() {
            map[j % n] |= u;
        }
    } else {
        for (i, m) in map.iter_mut().enumerate() {
            *m = units[i % units.len()];
        }
    }
    map
}

impl Topology {
    pub fn new(physical: &[u64], host: u64, cores: usize) -> Topology {
        let cores = cores.clamp(1, 64);
        let phys: Vec<u64> = physical.iter().map(|m| m & host).filter(|m| *m != 0).collect();
        let phys = if phys.is_empty() {
            (0..64).filter(|b| host & (1u64 << b) != 0).map(|b| 1u64 << b).collect()
        } else {
            phys
        };
        Topology { cores, full: full_mask(cores), host, map: build_map(&phys, cores) }
    }

    pub fn to_host(&self, mask: u64, process_console: u64) -> Option<u64> {
        if mask == 0 {
            return None;
        }
        if mask & !self.full != 0 || mask & !process_console != 0 {
            return None;
        }
        let mut h = 0u64;
        for (i, m) in self.map.iter().enumerate() {
            if mask & (1u64 << i) != 0 {
                h |= m;
            }
        }
        let h = h & self.host;
        if h == 0 {
            None
        } else {
            Some(h)
        }
    }

    pub fn to_console(&self, host_mask: u64) -> u64 {
        let mut c = 0u64;
        for (i, m) in self.map.iter().enumerate() {
            if m & host_mask != 0 {
                c |= 1u64 << i;
            }
        }
        c
    }

    pub fn console_index(&self, host_lp: u32) -> u32 {
        let bit = if host_lp < 64 { 1u64 << host_lp } else { 0 };
        match self.map.iter().position(|m| m & bit != 0) {
            Some(i) => i as u32,
            None => host_lp % self.cores as u32,
        }
    }

    pub fn describe(&self) -> String {
        let parts: Vec<String> = self.map.iter().enumerate().map(|(i, m)| format!("c{i}->{m:#x}")).collect();
        format!("console {} cores (mask {:#x}) -> host process mask {:#x}: {}", self.cores, self.full, self.host, parts.join(" "))
    }
}

type SetThreadFn = unsafe extern "system" fn(*mut c_void, usize) -> usize;
type GetProcFn = unsafe extern "system" fn(*mut c_void, *mut usize, *mut usize) -> i32;
type SetProcFn = unsafe extern "system" fn(*mut c_void, usize) -> i32;
type CreateThreadFn = unsafe extern "system" fn(*mut c_void, usize, *mut c_void, *mut c_void, u32, *mut u32) -> *mut c_void;
type ProcNumFn = unsafe extern "system" fn() -> u32;
type ResumeFn = unsafe extern "system" fn(*mut c_void) -> u32;

struct Real {
    set_thread: SetThreadFn,
    get_process: GetProcFn,
    set_process: SetProcFn,
    create_thread: CreateThreadFn,
    proc_num: ProcNumFn,
    resume: ResumeFn,
}

fn real() -> &'static Real {
    static R: OnceLock<Real> = OnceLock::new();
    R.get_or_init(|| unsafe {
        Real {
            set_thread: core::mem::transmute(super::resolve("kernelbase.dll", "SetThreadAffinityMask")),
            get_process: core::mem::transmute(super::resolve("kernelbase.dll", "GetProcessAffinityMask")),
            set_process: core::mem::transmute(super::resolve("kernelbase.dll", "SetProcessAffinityMask")),
            create_thread: core::mem::transmute(super::resolve("kernelbase.dll", "CreateThread")),
            proc_num: core::mem::transmute(super::resolve("kernelbase.dll", "GetCurrentProcessorNumber")),
            resume: core::mem::transmute(super::resolve("kernelbase.dll", "ResumeThread")),
        }
    })
}

const CREATE_SUSPENDED: u32 = 4;

pub fn confine_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("TOMBOX_CONFINE_THREADS").ok().or_else(|| crate::cfg("confine_threads")).map(|v| v.trim() != "0").unwrap_or(true)
    })
}

pub unsafe extern "system" fn create_thread(sa: *mut c_void, stack: usize, start: *mut c_void, param: *mut c_void, flags: u32, tid: *mut u32) -> *mut c_void {
    let r = real();
    if !confine_enabled() {
        return (r.create_thread)(sa, stack, start, param, flags, tid);
    }
    let t = topology();
    let Some(host) = t.to_host(process_console_mask(), t.full) else {
        return (r.create_thread)(sa, stack, start, param, flags, tid);
    };
    let h = (r.create_thread)(sa, stack, start, param, flags | CREATE_SUSPENDED, tid);
    if h.is_null() {
        return h;
    }
    (r.set_thread)(h, host as usize);
    if flags & CREATE_SUSPENDED == 0 {
        (r.resume)(h);
    }
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        crate::record_raw("os_host", "CreateThread", &format!("title threads start on console cores {:#x} -> host {host:#x}", process_console_mask()));
    }
    h
}

pub unsafe extern "system" fn get_current_processor_number() -> u32 {
    topology().console_index((real().proc_num)())
}

pub fn console_cores() -> usize {
    std::env::var("TOMBOX_CONSOLE_CORES")
        .ok()
        .or_else(|| crate::cfg("console_cores"))
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| (1..=64).contains(n))
        .unwrap_or(DEFAULT_CONSOLE_CORES)
}

pub unsafe fn host_physical_cores() -> Vec<(u8, u64)> {
    let mut len = 0u32;
    GetLogicalProcessorInformationEx(0, core::ptr::null_mut(), &mut len);
    if len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; len as usize];
    if GetLogicalProcessorInformationEx(0, buf.as_mut_ptr(), &mut len) == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 48 <= len as usize {
        let rel = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let size = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap()) as usize;
        if size == 0 {
            break;
        }
        if rel == 0 {
            let eff = buf[off + 9];
            let groups = u16::from_le_bytes(buf[off + 30..off + 32].try_into().unwrap()) as usize;
            for g in 0..groups {
                let p = off + 32 + g * 16;
                if p + 16 > off + size {
                    break;
                }
                let mask = u64::from_le_bytes(buf[p..p + 8].try_into().unwrap());
                let group = u16::from_le_bytes(buf[p + 8..p + 10].try_into().unwrap());
                if group == 0 && mask != 0 {
                    out.push((eff, mask));
                }
            }
        }
        off += size;
    }
    out
}

pub fn order_cores(mut cores: Vec<(u8, u64)>) -> Vec<u64> {
    cores.sort_by(|a, b| b.0.cmp(&a.0));
    cores.into_iter().map(|c| c.1).collect()
}

pub fn topology() -> &'static Topology {
    static T: OnceLock<Topology> = OnceLock::new();
    T.get_or_init(|| unsafe {
        let mut p = 0usize;
        let mut s = 0usize;
        let host = if (real().get_process)(-1isize as *mut c_void, &mut p, &mut s) != 0 && p != 0 { p as u64 } else { 1 };
        let cores = order_cores(host_physical_cores());
        Topology::new(&cores, host, console_cores())
    })
}

static PROCESS_CONSOLE: AtomicU64 = AtomicU64::new(0);

pub fn process_console_mask() -> u64 {
    let v = PROCESS_CONSOLE.load(Ordering::Acquire);
    if v == 0 {
        topology().full
    } else {
        v
    }
}

fn threads() -> &'static Mutex<HashMap<u32, u64>> {
    static M: OnceLock<Mutex<HashMap<u32, u64>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn note(api: &str, given: u64, host: Option<u64>) {
    static SEEN: OnceLock<Mutex<HashSet<(u8, u64)>>> = OnceLock::new();
    let tag = match api {
        "SetThreadAffinityMask" => 0u8,
        "SetProcessAffinityMask" => 1,
        _ => 2,
    };
    let mut s = SEEN.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(|e| e.into_inner());
    if s.len() >= 256 || !s.insert((tag, given)) {
        return;
    }
    drop(s);
    let t = topology();
    let shape = if given & !t.full != 0 { "outside console cores" } else { "console-shaped, remapped" };
    let detail = match host {
        Some(h) => format!("{given:#x} ({shape}) -> host {h:#x}"),
        None => format!("{given:#x} rejected as on console (ERROR_INVALID_PARAMETER)"),
    };
    crate::record_raw("os_host", api, &detail);
}

unsafe fn is_self_process(h: *mut c_void) -> bool {
    h as isize == -1 || (!h.is_null() && GetProcessId(h) == GetCurrentProcessId())
}

pub unsafe extern "system" fn set_thread_affinity_mask(h: *mut c_void, mask: usize) -> usize {
    let t = topology();
    let given = mask as u64;
    let host = t.to_host(given, process_console_mask());
    note("SetThreadAffinityMask", given, host);
    let Some(host) = host else {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    };
    let r = (real().set_thread)(h, host as usize);
    if r == 0 {
        return 0;
    }
    let tid = GetThreadId(h);
    let mut m = threads().lock().unwrap_or_else(|e| e.into_inner());
    let prev = m.insert(tid, given).unwrap_or_else(|| {
        let c = t.to_console(r as u64);
        if c == 0 || r as u64 & t.host == t.host {
            process_console_mask()
        } else {
            c
        }
    });
    if m.len() > 4096 {
        m.clear();
        m.insert(tid, given);
    }
    prev as usize
}

pub unsafe extern "system" fn get_process_affinity_mask(h: *mut c_void, process: *mut usize, system: *mut usize) -> i32 {
    if process.is_null() || system.is_null() || !is_self_process(h) {
        return (real().get_process)(h, process, system);
    }
    let mut p = 0usize;
    let mut s = 0usize;
    if (real().get_process)(h, &mut p, &mut s) == 0 {
        return 0;
    }
    *process = process_console_mask() as usize;
    *system = topology().full as usize;
    1
}

pub unsafe extern "system" fn set_process_affinity_mask(h: *mut c_void, mask: usize) -> i32 {
    if !is_self_process(h) {
        return (real().set_process)(h, mask);
    }
    let t = topology();
    let given = mask as u64;
    let host = t.to_host(given, t.full);
    note("SetProcessAffinityMask", given, host);
    let Some(host) = host else {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    };
    let r = (real().set_process)(h, host as usize);
    if r != 0 {
        let c = if given & !t.full != 0 { t.to_console(host) } else { given };
        PROCESS_CONSOLE.store(c, Ordering::Release);
    }
    r
}

pub unsafe fn real_set_thread_affinity_mask(h: *mut c_void, mask: usize) -> usize {
    (real().set_thread)(h, mask)
}

pub unsafe fn real_get_process_affinity_mask() -> (u64, u64) {
    let mut p = 0usize;
    let mut s = 0usize;
    (real().get_process)(-1isize as *mut c_void, &mut p, &mut s);
    (p as u64, s as u64)
}
