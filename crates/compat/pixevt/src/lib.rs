use core::ffi::c_void;
use std::arch::naked_asm;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;

pub const PIXRT_MAGIC: u32 = 0x4a61_0f6c;
pub const PIXRT_VERSION: u32 = 0x31064;
pub const TABLE_BYTES: u32 = 0x78;
pub const TOOL_MEMORY_BYTES: u32 = 0x10;
pub const SLOTS: usize = 15;

#[no_mangle]
pub static mut PIXEVT_TABLE: [usize; SLOTS] = [0; SLOTS];
#[no_mangle]
pub static mut PIXEVT_TOOL_MEMORY: [u8; 16] = [0; 16];
static RUNTIME_NAME: AtomicUsize = AtomicUsize::new(0);

pub const DIRS: [&str; 4] = ["d:\\pix\\", "g:\\", "c:\\windows\\system32\\", ".\\"];
pub const KMD: &str = "\\\\.\\XgsKmd10";
pub const RT10: &str = "pixrt10.dll";
pub const RT: &str = "pixrt.dll";

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(n: *const u16, a: u32, s: u32, sa: *mut c_void, d: u32, f: u32, t: *mut c_void) -> isize;
    fn CloseHandle(h: isize) -> i32;
    fn GetFileAttributesW(n: *const u16) -> u32;
    fn LoadLibraryExW(n: *const u16, f: *mut c_void, fl: u32) -> isize;
    fn GetModuleHandleW(n: *const u16) -> isize;
    fn GetProcAddress(m: isize, n: *const u8) -> usize;
    fn FreeLibrary(m: isize) -> i32;
    fn OutputDebugStringW(s: *const u16);
}

type InitFn = unsafe extern "system" fn(u32, u32, usize, *mut c_void, u32) -> i32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn select_runtime_name() -> &'static str {
    let k = wide(KMD);
    let h = unsafe { CreateFileW(k.as_ptr(), 0xC000_0000, 0, core::ptr::null_mut(), 2, 0x80, core::ptr::null_mut()) };
    let name = if h != -1 {
        unsafe { CloseHandle(h) };
        RT10
    } else {
        RT
    };
    RUNTIME_NAME.store(if name == RT10 { 2 } else { 1 }, Ordering::SeqCst);
    name
}

fn runtime_name() -> &'static str {
    match RUNTIME_NAME.load(Ordering::SeqCst) {
        2 => RT10,
        1 => RT,
        _ => select_runtime_name(),
    }
}

pub fn search_and_load(name: &str) -> isize {
    for (i, d) in DIRS.iter().enumerate() {
        let mut p = String::from(*d);
        p.push_str(name);
        if p.len() >= 0x104 {
            continue;
        }
        let w = wide(&p);
        if unsafe { GetFileAttributesW(w.as_ptr()) } == u32::MAX && i != 3 {
            clear_state();
            continue;
        }
        let m = unsafe { LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), 0) };
        if m == 0 {
            clear_state();
            continue;
        }
        let a = unsafe { GetProcAddress(m, b"PixDllInitialize\0".as_ptr()) };
        let b = unsafe { GetProcAddress(m, b"PixDllInitializeFromTitle\0".as_ptr()) };
        let c = unsafe { GetProcAddress(m, b"PixDllInitializeToolMemory\0".as_ptr()) };
        if a != 0 && b != 0 && c != 0 {
            return m;
        }
        clear_state();
        unsafe { FreeLibrary(m) };
    }
    0
}

pub fn clear_state() {
    unsafe {
        *core::ptr::addr_of_mut!(PIXEVT_TABLE) = [0; SLOTS];
        *core::ptr::addr_of_mut!(PIXEVT_TOOL_MEMORY) = [0; 16];
    }
}

pub fn tool_memory() -> [u8; 16] {
    unsafe { *core::ptr::addr_of!(PIXEVT_TOOL_MEMORY) }
}

fn module_for(name: &str) -> (isize, bool) {
    let w = wide(name);
    let m = unsafe { GetModuleHandleW(w.as_ptr()) };
    if m != 0 {
        return (m, true);
    }
    (search_and_load(name), false)
}

unsafe fn table_ptr() -> *mut c_void {
    core::ptr::addr_of_mut!(PIXEVT_TABLE) as *mut c_void
}

pub fn attach() -> i32 {
    let name = runtime_name();
    let (m, preloaded) = module_for(name);
    if m == 0 {
        return E_UNEXPECTED;
    }
    let f = unsafe { GetProcAddress(m, b"PixDllInitializeFromTitle\0".as_ptr()) };
    let mut hr = if f == 0 {
        E_UNEXPECTED
    } else {
        let f: InitFn = unsafe { core::mem::transmute(f) };
        unsafe { f(PIXRT_MAGIC, PIXRT_VERSION, 0, table_ptr(), TABLE_BYTES) }
    };
    if hr >= 0 {
        let (m2, pre2) = module_for(name);
        if m2 == 0 {
            hr = E_UNEXPECTED;
        } else {
            let g = unsafe { GetProcAddress(m2, b"PixDllInitializeToolMemory\0".as_ptr()) };
            hr = if g == 0 {
                E_UNEXPECTED
            } else {
                let g: InitFn = unsafe { core::mem::transmute(g) };
                unsafe { g(PIXRT_MAGIC, PIXRT_VERSION, 0, core::ptr::addr_of_mut!(PIXEVT_TOOL_MEMORY) as *mut c_void, TOOL_MEMORY_BYTES) }
            };
            if hr < 0 && !pre2 {
                unsafe { FreeLibrary(m2) };
            }
        }
    }
    if hr < 0 {
        clear_state();
        if !preloaded {
            unsafe { FreeLibrary(m) };
        }
    }
    if hr == E_NOTIMPL {
        hr = 1;
    }
    hr
}

extern "C" fn process_attach() {
    if attach() < 0 {
        let s = wide("PIXEvt failed to load PixRT\n");
        unsafe { OutputDebugStringW(s.as_ptr()) };
    }
}

#[used]
#[link_section = ".CRT$XCU"]
static PROCESS_ATTACH: extern "C" fn() = process_attach;

pub fn set_slot(i: usize, f: usize) {
    unsafe { (*core::ptr::addr_of_mut!(PIXEVT_TABLE))[i] = f };
}

pub fn slot(i: usize) -> usize {
    unsafe { (*core::ptr::addr_of!(PIXEVT_TABLE))[i] }
}

#[no_mangle]
pub extern "system" fn PIXBeginCapture1(_a: usize, _b: usize) -> i32 {
    E_NOTIMPL
}

macro_rules! fwd_hr {
    ($name:ident, $off:literal, $fail:literal) => {
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "system" fn $name() {
            naked_asm!(
                concat!("mov rax, qword ptr [rip + {t} + ", $off, "]"),
                "test rax, rax",
                "je 2f",
                "jmp rax",
                "2:",
                concat!("mov eax, ", $fail),
                "ret",
                t = sym PIXEVT_TABLE,
            )
        }
    };
}

macro_rules! fwd_void {
    ($name:ident, $off:literal) => {
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "system" fn $name() {
            naked_asm!(
                concat!("mov rax, qword ptr [rip + {t} + ", $off, "]"),
                "test rax, rax",
                "je 2f",
                "jmp rax",
                "2:",
                "ret",
                t = sym PIXEVT_TABLE,
            )
        }
    };
}

fwd_hr!(PIXBeginCapture2, "0", "0x8000ffff");
fwd_hr!(PIXEndCapture, "8", "0x8000ffff");
fwd_hr!(PIXGetCaptureState, "16", "0");
fwd_void!(PIXReportCounter, "24");
fwd_void!(RegisterForCaptureControl, "32");
fwd_void!(UnregisterForCaptureControl, "40");
fwd_hr!(GetCaptureControlState, "48", "0");
fwd_hr!(PIXEventsReplaceBlock, "56", "0");
fwd_hr!(ConfigurePMCs, "64", "0");
fwd_hr!(ConfigureNBPMCs, "72", "0");
fwd_hr!(ConfigureL3PMCs, "80", "0");
fwd_hr!(SetCaptureCallgraphState, "88", "0");
fwd_hr!(SetCaptureFunctionDetailsState, "96", "0");
fwd_hr!(SetCaptureFunctionSummaryState, "104", "0");
fwd_hr!(SetCaptureInstructionTraceState, "112", "0");

#[cfg(test)]
mod tests;
