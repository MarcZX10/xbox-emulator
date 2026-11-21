use core::ffi::c_void;

extern "system" {
    fn CreateEventW(sa: *mut c_void, manual: i32, initial: i32, name: *const u16) -> isize;
    fn WaitForSingleObject(h: isize, ms: u32) -> u32;
}

thread_local! {
    static EV: isize = unsafe { CreateEventW(core::ptr::null_mut(), 0, 0, core::ptr::null()) };
}

pub unsafe fn wait_fence(fence: *mut c_void, value: u64, timeout_ms: u32) -> bool {
    let vt = *(fence as *const *const usize);
    let completed: unsafe extern "system" fn(*mut c_void) -> u64 = core::mem::transmute(*vt.add(8));
    if completed(fence) >= value {
        return true;
    }
    let set_event: unsafe extern "system" fn(*mut c_void, u64, isize) -> i32 = core::mem::transmute(*vt.add(9));
    let ev = EV.with(|e| *e);
    if ev == 0 || set_event(fence, value, ev) < 0 {
        let t0 = std::time::Instant::now();
        while completed(fence) < value {
            if t0.elapsed().as_millis() as u32 > timeout_ms {
                return false;
            }
            std::thread::yield_now();
        }
        return true;
    }
    WaitForSingleObject(ev, timeout_ms) == 0 || completed(fence) >= value
}
