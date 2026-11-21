use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use compat_common::record;

pub const THRESHOLD_MS: f64 = 20.0;

pub static CL_TABLE: AtomicUsize = AtomicUsize::new(0);
static TSC_THRESHOLD: AtomicU64 = AtomicU64::new(u64::MAX);
static TSC_PER_MS: AtomicU64 = AtomicU64::new(0);
static LOGGED: AtomicUsize = AtomicUsize::new(0);
static PER_SITE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, (u64, f64)>>> = OnceLock::new();

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| compat_common::cfg("slowcall").as_deref() != Some("0"))
}

fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

pub fn calibrate() {
    if TSC_PER_MS.load(Ordering::Relaxed) != 0 {
        return;
    }
    let t = std::time::Instant::now();
    let c0 = rdtsc();
    std::thread::sleep(std::time::Duration::from_millis(15));
    let c1 = rdtsc();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let per = ((c1 - c0) as f64 / ms).max(1.0) as u64;
    TSC_PER_MS.store(per, Ordering::Relaxed);
    TSC_THRESHOLD.store((per as f64 * THRESHOLD_MS) as u64, Ordering::Relaxed);
}

pub fn should_log(count: u64) -> bool {
    count <= 20 || count % 25 == 0
}

pub fn note(site: &str, ms: f64, detail: &str) {
    let m = PER_SITE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let (count, worst) = {
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        let e = g.entry(site.to_string()).or_insert((0, 0.0));
        e.0 += 1;
        e.1 = e.1.max(ms);
        *e
    };
    if should_log(count) {
        LOGGED.fetch_add(1, Ordering::Relaxed);
        let tid = unsafe { GetCurrentThreadId() };
        record("d3d12_xs", "slowcall", &format!("{site} took {ms:.1} ms on thread {tid} (#{count}, worst {worst:.1} ms){detail}"));
    }
}

pub fn time<R>(site: &str, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    let t0 = std::time::Instant::now();
    let r = f();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if ms > THRESHOLD_MS {
        note(site, ms, "");
    }
    r
}

extern "system" {
    fn GetCurrentThreadId() -> u32;
}

unsafe extern "system" fn cl_slow(slot: u64, ticks: u64, list: usize) {
    let per = TSC_PER_MS.load(Ordering::Relaxed).max(1);
    let ms = ticks as f64 / per as f64;
    note(&format!("cmdlist slot {slot}"), ms, &format!(" list={list:#x}"));
}

#[unsafe(naked)]
pub unsafe extern "system" fn cl_timed() {
    core::arch::naked_asm!(
        "push rbx",
        "push rsi",
        "push rdi",
        "push r12",
        "push r13",
        "push r14",
        "sub rsp, 0x88",
        "mov r12, r10",
        "mov r13, rcx",
        "mov r14, rdx",
        "rdtsc",
        "shl rdx, 32",
        "or rax, rdx",
        "mov rbx, rax",
        "lea rsi, [rsp + 0xE0]",
        "lea rdi, [rsp + 0x20]",
        "mov ecx, 12",
        "rep movsq",
        "mov rcx, r13",
        "mov rdx, r14",
        "mov rax, [rip + {table}]",
        "call qword ptr [rax + r12*8]",
        "mov rsi, rax",
        "rdtsc",
        "shl rdx, 32",
        "or rax, rdx",
        "sub rax, rbx",
        "cmp rax, [rip + {thr}]",
        "jb 2f",
        "movdqu [rsp + 0x20], xmm0",
        "mov rcx, r12",
        "mov rdx, rax",
        "mov r8, r13",
        "call {slow}",
        "movdqu xmm0, [rsp + 0x20]",
        "2:",
        "mov rax, rsi",
        "add rsp, 0x88",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rdi",
        "pop rsi",
        "pop rbx",
        "ret",
        table = sym CL_TABLE, thr = sym TSC_THRESHOLD, slow = sym cl_slow,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn many_args(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64) -> u64 {
        a + b * 2 + c * 3 + d * 4 + e * 5 + f * 6 + g * 7 + h * 8
    }

    unsafe extern "system" fn sleepy(a: u64, _b: u64, _c: u64, _d: u64, e: u64) -> u64 {
        std::thread::sleep(std::time::Duration::from_millis(30));
        a + e
    }

    #[test]
    fn timed_trampoline_forwards_register_and_stack_args_and_logs_slow_calls() {
        calibrate();
        let vt: &'static mut [usize; 2] = Box::leak(Box::new([many_args as usize, sleepy as usize]));
        CL_TABLE.store(vt.as_ptr() as usize, Ordering::SeqCst);
        let call8: unsafe extern "system" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> u64 = unsafe { core::mem::transmute(slot_thunk(0)) };
        assert_eq!(unsafe { call8(1, 2, 3, 4, 5, 6, 7, 8) }, 1 + 4 + 9 + 16 + 25 + 36 + 49 + 64);
        let before = LOGGED.load(Ordering::Relaxed);
        let call5: unsafe extern "system" fn(u64, u64, u64, u64, u64) -> u64 = unsafe { core::mem::transmute(slot_thunk(1)) };
        assert_eq!(unsafe { call5(10, 0, 0, 0, 5) }, 15);
        assert!(LOGGED.load(Ordering::Relaxed) > before);
        assert!(should_log(1) && should_log(20) && !should_log(21) && should_log(50));
    }

    fn slot_thunk(slot: u32) -> usize {
        extern "system" {
            fn VirtualAlloc(addr: *mut core::ffi::c_void, size: usize, ty: u32, protect: u32) -> *mut core::ffi::c_void;
        }
        unsafe {
            let p = VirtualAlloc(core::ptr::null_mut(), 32, 0x3000, 0x40) as *mut u8;
            let mut b = Vec::new();
            b.extend_from_slice(&[0x41, 0xBA]);
            b.extend_from_slice(&slot.to_le_bytes());
            b.extend_from_slice(&[0x48, 0xB8]);
            b.extend_from_slice(&(cl_timed as usize as u64).to_le_bytes());
            b.extend_from_slice(&[0xFF, 0xE0]);
            core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
            p as usize
        }
    }
}
