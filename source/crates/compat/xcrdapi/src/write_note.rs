use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

type NoteFn = unsafe extern "system" fn(usize, usize, u32);

const UNRESOLVED: usize = 0;
const ABSENT: usize = 1;
static NOTE_FN: AtomicUsize = AtomicUsize::new(UNRESOLVED);
static TRIES: AtomicUsize = AtomicUsize::new(0);

pub const NOTE_WRITTEN: u32 = 0;

fn note_fn() -> Option<NoteFn> {
    let f = NOTE_FN.load(Ordering::Acquire);
    if f > ABSENT {
        return Some(unsafe { core::mem::transmute::<usize, NoteFn>(f) });
    }
    if f == ABSENT {
        return None;
    }
    let p = unsafe {
        let m = GetModuleHandleA(b"d3d12_xs.dll\0".as_ptr());
        if m.is_null() { 0 } else { GetProcAddress(m, b"TomboxNoteTitleMemory\0".as_ptr()) as usize }
    };
    if p > ABSENT {
        NOTE_FN.store(p, Ordering::Release);
        return Some(unsafe { core::mem::transmute::<usize, NoteFn>(p) });
    }
    if TRIES.fetch_add(1, Ordering::Relaxed) > 4096 {
        NOTE_FN.store(ABSENT, Ordering::Release);
    }
    None
}

pub fn written(addr: u64, len: usize) {
    if let Some(f) = note_fn() {
        unsafe { f(addr as usize, len, NOTE_WRITTEN) };
    }
}
