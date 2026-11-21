use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicUsize, Ordering};

pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;

pub const IOCTL_OPEN: u32 = 0x7900_c190;
pub const IOCTL_MAP: u32 = 0x7900_c1b0;
pub const IOCTL_ARM: u32 = 0x7900_c1a3;
pub const FILE_MAGIC: u32 = 0x7469_6858;
pub const FILE_VERSION: u32 = 0x0016_1200;
pub const CB_FENCE: u64 = 0x0bad_f00d_dead_beef;
pub const DEADLOCK_BIT: u64 = 1 << 13;
pub const KMD: &str = "\\\\.\\XgsKmd";
pub const HANG_DIR: &str = "t:\\";

const RULE: &str = "+-----------------------------------------------------------------------------+\n";
const PAD: &str = "                                                                            |";

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(n: *const u16, a: u32, s: u32, sa: *mut c_void, d: u32, f: u32, t: *mut c_void) -> isize;
    fn CloseHandle(h: isize) -> i32;
    fn DeviceIoControl(h: isize, code: u32, i: *const c_void, il: u32, o: *mut c_void, ol: u32, ret: *mut u32, ov: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn WriteFile(h: isize, b: *const c_void, n: u32, w: *mut u32, ov: *mut c_void) -> i32;
    fn FlushFileBuffers(h: isize) -> i32;
    fn GetSystemTime(t: *mut [u16; 8]);
    fn OutputDebugStringA(s: *const u8);
    fn GetModuleHandleW(n: *const u16) -> isize;
    fn GetProcAddress(m: isize, n: *const u8) -> usize;
}

type LogFn = unsafe extern "system" fn(*const u8);
type DgcFn = unsafe extern "system" fn(u32, *const c_void, u32, *mut c_void, *mut u32) -> i32;

static LOG_CB: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_OBJ: u8 = 0;
static REGION0: AtomicUsize = AtomicUsize::new(0);
static REGION1: AtomicUsize = AtomicUsize::new(0);
static DEVICE: AtomicIsize = AtomicIsize::new(-1);
static DEVICE_LOCK: AtomicI32 = AtomicI32::new(0);
static DGC: AtomicBool = AtomicBool::new(false);
static ARM_STATE: AtomicU32 = AtomicU32::new(0);
static VERBOSITY: AtomicU32 = AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn hr_last() -> i32 {
    let e = unsafe { GetLastError() };
    if e as i32 <= 0 {
        e as i32
    } else {
        ((e & 0xffff) | 0x8007_0000) as i32
    }
}

fn dgc() -> Option<DgcFn> {
    let n = wide("ntdll.dll");
    let m = unsafe { GetModuleHandleW(n.as_ptr()) };
    if m == 0 {
        return None;
    }
    let f = unsafe { GetProcAddress(m, b"NtDirectGraphicsCall\0".as_ptr()) };
    if f == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<usize, DgcFn>(f) })
    }
}

pub fn verbosity() -> u32 {
    VERBOSITY.load(Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn test_reset(verbosity: u32) {
    REGION0.store(0, Ordering::SeqCst);
    REGION1.store(0, Ordering::SeqCst);
    VERBOSITY.store(verbosity, Ordering::SeqCst);
}

pub fn print(level: u32, text: &str) {
    if level > VERBOSITY.load(Ordering::SeqCst) {
        return;
    }
    let mut b: Vec<u8> = text.bytes().take(0x3ff).collect();
    b.push(0);
    unsafe { OutputDebugStringA(b.as_ptr()) };
    let cb = LOG_CB.load(Ordering::SeqCst);
    if cb != 0 {
        let f: LogFn = unsafe { core::mem::transmute(cb) };
        unsafe { f(b.as_ptr()) };
    }
}

pub fn banner(text: &str) {
    print(0, RULE);
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let n = line.len().min(0x4d);
        print(0, &format!("| {}{}\n", line, &PAD[n.min(PAD.len())..]));
    }
    print(0, RULE);
}

pub fn map_region(id: u32) -> Result<(usize, usize), i32> {
    let k = wide(KMD);
    let h = unsafe { CreateFileW(k.as_ptr(), 0xC000_0000, 0, core::ptr::null_mut(), 2, 0x80, core::ptr::null_mut()) };
    if h == -1 {
        return Err(hr_last());
    }
    let mut out = [0usize; 2];
    let mut ret = 0u32;
    let ok = unsafe { DeviceIoControl(h, IOCTL_MAP, &id as *const u32 as *const c_void, 4, out.as_mut_ptr() as *mut c_void, 16, &mut ret, core::ptr::null_mut()) };
    let r = if ok == 0 {
        Err(hr_last())
    } else if ret != 16 {
        Err(E_FAIL)
    } else {
        Ok((out[0], out[1]))
    };
    unsafe { CloseHandle(h) };
    r
}

pub fn open_device() -> Result<isize, i32> {
    let h = DEVICE.load(Ordering::SeqCst);
    if h != -1 {
        return Ok(h);
    }
    while DEVICE_LOCK.fetch_add(1, Ordering::SeqCst) + 1 != 1 {
        DEVICE_LOCK.fetch_sub(1, Ordering::SeqCst);
    }
    let r = open_device_locked();
    DEVICE_LOCK.fetch_sub(1, Ordering::SeqCst);
    r
}

fn open_device_locked() -> Result<isize, i32> {
    let h = DEVICE.load(Ordering::SeqCst);
    if h != -1 {
        return Ok(h);
    }
    let st = match dgc() {
        Some(f) => unsafe { f(0, core::ptr::null(), 0, core::ptr::null_mut(), core::ptr::null_mut()) },
        None => -1,
    };
    DGC.store(st == 0, Ordering::SeqCst);
    let k = wide(KMD);
    let h = unsafe { CreateFileW(k.as_ptr(), 0xC000_0000, 0, core::ptr::null_mut(), 2, 0x80, core::ptr::null_mut()) };
    if h == -1 {
        return Err(hr_last());
    }
    let mut ret = 0u32;
    let ok = unsafe { DeviceIoControl(h, IOCTL_OPEN, core::ptr::null(), 0, core::ptr::null_mut(), 0, &mut ret, core::ptr::null_mut()) };
    let hr = if ok == 0 { hr_last() } else if ret != 0 { E_FAIL } else { 0 };
    if hr < 0 {
        unsafe { CloseHandle(h) };
        return Err(hr);
    }
    DEVICE.store(h, Ordering::SeqCst);
    Ok(h)
}

pub fn regions() -> Option<(usize, usize)> {
    let a = REGION0.load(Ordering::SeqCst);
    let b = REGION1.load(Ordering::SeqCst);
    if a == 0 || b == 0 {
        None
    } else {
        Some((a, b))
    }
}

pub fn device_handle() -> isize {
    DEVICE.load(Ordering::SeqCst)
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst) != 0
}

#[no_mangle]
pub unsafe extern "system" fn XhitInitialize(log: usize) -> i32 {
    ACTIVE.store(&ACTIVE_OBJ as *const u8 as usize, Ordering::SeqCst);
    LOG_CB.store(log, Ordering::SeqCst);
    if let Some((d, m)) = kmd::installed() {
        REGION0.store(d, Ordering::SeqCst);
        REGION1.store(m, Ordering::SeqCst);
    }
    if REGION0.load(Ordering::SeqCst) == 0 {
        if let Ok((p, _)) = map_region(0) {
            REGION0.store(p, Ordering::SeqCst);
        }
    }
    if REGION1.load(Ordering::SeqCst) == 0 {
        if let Ok((p, _)) = map_region(1) {
            REGION1.store(p, Ordering::SeqCst);
        }
    }
    let h = open_device().unwrap_or(-1);
    let mut st = ARM_STATE.load(Ordering::SeqCst);
    if DGC.load(Ordering::SeqCst) {
        if let Some(f) = dgc() {
            let mut n = 4u32;
            f(IOCTL_ARM, core::ptr::null(), 0, &mut st as *mut u32 as *mut c_void, &mut n);
        }
    } else {
        let mut ret = 0u32;
        DeviceIoControl(h, IOCTL_ARM, core::ptr::null(), 0, &mut st as *mut u32 as *mut c_void, 4, &mut ret, core::ptr::null_mut());
    }
    ARM_STATE.store(st, Ordering::SeqCst);
    0
}

#[no_mangle]
pub unsafe extern "system" fn XhitTerminate() -> i32 {
    ACTIVE.store(0, Ordering::SeqCst);
    0
}

pub fn engine_name(ctx: *const u8, idx: u32) -> String {
    let d = unsafe { *ctx.add(0x10 + idx as usize) } as u32;
    match d & 7 {
        1 => "Gpu".to_string(),
        2 => format!("SDMA {}", (d >> 3) & 3),
        3 => format!("Compute pipe {} queue {}", (d >> 3) & 3, d >> 5),
        _ => format!("Unknown {}", idx),
    }
}

pub fn fence_deadlocks(ctx: *const u8) -> bool {
    if ctx.is_null() {
        return false;
    }
    let packed = unsafe { (ctx.add(8) as *const u64).read_unaligned() };
    let arr = (packed & 0xff_ffff_ffff) as *const u64;
    if arr.is_null() {
        return false;
    }
    let count = ((packed >> 40) & 0x3f) as u32;
    let mut found = false;
    for i in 0..count {
        let v = unsafe { arr.add(i as usize).read_unaligned() };
        if v == 0 {
            continue;
        }
        if !found {
            print(0, "One or more fence deadlocks detected:\n");
        }
        found = true;
        let me = engine_name(ctx, i);
        if v == CB_FENCE {
            print(0, &format!("Engine {} waiting for command buffer execution\n", me));
        } else {
            let other = engine_name(ctx, ((v >> 48) & 0xff) as u32);
            print(0, &format!("Engine {} waiting for fence 0x{:016x} to be signaled from engine {}\n", me, v, other));
        }
    }
    found
}

#[no_mangle]
pub unsafe extern "system" fn XhitPerformHangSummary(ctx: *const u8, out: *mut u64) -> i32 {
    banner("XHIT Runtime GPU Hang Analysis\nCollecting summary of the GPU hang...");
    let r = regions();
    kmd::post_hang(r, ctx);
    let found = fence_deadlocks(ctx);
    *out = (*out & !DEADLOCK_BIT) | if found { DEADLOCK_BIT } else { 0 };
    *(out as *mut u8).add(0x318).cast::<u64>() = if ctx.is_null() { 0 } else { (ctx.add(0x50) as *const u64).read_unaligned() };
    let r = match r {
        Some(r) => r,
        None => return E_UNEXPECTED,
    };
    kmd::sleep_ack(r);
    let (p, n, _) = kmd::response(r);
    let n = n.min(kmd::RESP_MAX) as usize;
    let mut snap = vec![0u8; kmd::RESP_MAX as usize];
    core::ptr::copy_nonoverlapping(p as *const u8, snap.as_mut_ptr(), n);
    sq::collect_sq(r, None);
    let sq_len = ((r.0 + kmd::D_SIZE) as *const u32).read_volatile();
    regs::decode_registers(snap.as_ptr(), out, (r.0 + kmd::D_RESP) as *const u8, sq_len);
    if kmd::post_hang(Some(r), ctx) == 0 {
        kmd::sleep_ack(r);
    }
    0
}

pub fn hang_file_name(dir: &str, t: &[u16; 8]) -> String {
    format!("{}gpuhang-{}-{:02}-{:02}-{:02}-{:02}-{:02}.xhit", dir, t[0], t[1], t[3], t[4], t[5], t[6])
}

pub unsafe fn analysis_in(dir: &str, _reason: u32, _out: *mut u16, _out_len: u32, _ctx: *const u8) -> i32 {
    banner("Collecting detailed GPU hang data...");
    let mut t = [0u16; 8];
    GetSystemTime(&mut t);
    let path = hang_file_name(dir, &t);
    let w = wide(&path);
    let h = CreateFileW(w.as_ptr(), 0x4000_0000, 1, core::ptr::null_mut(), 2, 0, core::ptr::null_mut());
    if h == -1 {
        return E_FAIL;
    }
    let hdr = [FILE_MAGIC, FILE_VERSION];
    let mut n = 0u32;
    WriteFile(h, hdr.as_ptr() as *const c_void, 8, &mut n, core::ptr::null_mut());
    FlushFileBuffers(h);
    CloseHandle(h);
    E_FAIL
}

#[no_mangle]
pub unsafe extern "system" fn XhitPerformHangAnalysis(reason: u32, out: *mut u16, out_len: u32, ctx: *const u8) -> i32 {
    analysis_in(HANG_DIR, reason, out, out_len, ctx)
}

pub mod bucket;
pub mod chunk;
pub mod kmd;
pub mod regs;
pub mod report;
pub mod sq;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_dec;
