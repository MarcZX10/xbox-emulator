use super::*;
use std::sync::Mutex;

static LOG: Mutex<Vec<(u32, u32, [u64; 5])>> = Mutex::new(Vec::new());
static VETO: AtomicU32 = AtomicU32::new(0);

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn MapViewOfFile(h: Handle, access: u32, hi: u32, lo: u32, n: usize) -> *mut u8;
    fn UnmapViewOfFile(p: *const u8) -> Bool;
    fn GetCurrentProcess() -> Handle;
    fn GetExitCodeThread(h: Handle, code: *mut u32) -> Bool;
    fn GetTempPathW(n: u32, buf: *mut u16) -> u32;
    fn DeleteFileW(name: *const u16) -> Bool;
}

unsafe extern "system" fn record(op: u32, r: *mut [u64; 5]) {
    LOG.lock().unwrap().push((GetCurrentThreadId(), op, *r));
}

unsafe extern "system" fn veto(op: u32, args: *mut *mut u32) {
    assert_eq!(op, 1);
    if VETO.load(Ordering::SeqCst) != 0 {
        **args = 0;
    }
}

fn guard() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn take() -> Vec<(u32, [u64; 5])> {
    let me = unsafe { GetCurrentThreadId() };
    std::mem::take(&mut *LOG.lock().unwrap()).into_iter().filter(|e| e.0 == me).map(|e| (e.1, e.2)).collect()
}

fn cstr(p: *const u8) -> String {
    unsafe { std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().to_string() }
}

#[test]
fn redirection_information_lists_74_console_entries() {
    let info = &__RedirectionInformation__;
    assert_eq!((info.version, info.count), (1, 74));
    let t = unsafe { core::slice::from_raw_parts(info.entries, info.count as usize) };
    for (i, e) in t.iter().enumerate() {
        assert_eq!(cstr(e.function), crate::REDIRECT_FUNCTIONS[i], "entry {i}");
        assert!(!e.thunk.is_null());
    }
    assert_eq!(cstr(t[26].module), "ntdll.dll");
    assert_eq!(cstr(t[61].module), "WINHTTP.dll");
    assert_eq!(t[2].thunk, t[32].thunk);
    assert_eq!(t[1].thunk, t[73].thunk);
    assert_eq!(t[13].thunk, redirected_virtual_alloc as *const c_void);
}

#[test]
fn heap_thunks_report_to_xmem_callback() {
    let _g = guard();
    take();
    XbRegisterXMemCallback(record as *const () as usize);
    unsafe {
        let heap = redirected_get_process_heap();
        assert_eq!(heap, XbGetPartitionHeap(1));
        let p = redirected_heap_alloc(heap, 0, 40);
        assert!(!p.is_null());
        let q = redirected_heap_realloc(heap, 0, p, 80);
        XbSetCallHeapFreeCallbackForRealloc(1);
        let r = redirected_heap_realloc(heap, 0, q, 160);
        XbSetCallHeapFreeCallbackForRealloc(0);
        assert_eq!(redirected_heap_size(heap, 0, r), 160);
        assert_eq!(redirected_heap_free(heap, 0, r), 1);
        let h = heap as u64;
        let log = take();
        assert_eq!(log[0], (1, [h, 0, 40, p as u64, 0]));
        assert_eq!(log[1], (2, [h, 0, 80, q as u64, p as u64]));
        assert_eq!(log[2], (3, [h, 0, q as u64, 0, 0]));
        assert_eq!(log[3], (2, [h, 0, 160, r as u64, q as u64]));
        assert_eq!(log[4], (3, [h, 0, r as u64, 0, 0]));
        assert_eq!(log.len(), 5);
        let a = redirected_rtl_allocate_heap(heap, 8, 24);
        assert_eq!(*(a as *const u64), 0);
        let b = redirected_rtl_reallocate_heap(heap, 0, a, 48);
        assert_eq!(redirected_rtl_size_heap(heap, 0, b), 48);
        assert_eq!(redirected_rtl_free_heap(heap, 0, b), 1);
        let log = take();
        assert_eq!(log[0], (1, [h, 8, 24, a as u64, 0]));
        assert_eq!(log[1], (2, [h, 0, 48, b as u64, a as u64]));
        assert_eq!(log[2], (3, [h, 0, b as u64, 0, 0]));
    }
    XbRegisterXMemCallback(0);
    unsafe {
        let heap = redirected_get_process_heap();
        let p = redirected_heap_alloc(heap, 0, 8);
        redirected_heap_free(heap, 0, p);
    }
    assert!(take().is_empty());
}

#[test]
fn heap_create_and_rtl_create_heap_make_partition_heaps() {
    unsafe {
        let h = redirected_heap_create(0x4_0000 | 1, 0x1000, 0);
        assert!(!h.is_null(), "HeapCreate error {}", GetLastError());
        let p = redirected_heap_alloc(h, 0, 4096);
        assert!(!p.is_null());
        p.cast::<u8>().write_bytes(7, 4096);
        assert_eq!(redirected_heap_lock(h), 1);
        assert_eq!(redirected_heap_unlock(h), 1);
        let mut info = 0u32;
        let mut ret = 0usize;
        assert_eq!(redirected_heap_query_information(h, 0, (&mut info as *mut u32).cast(), 4, &mut ret), 1);
        let mut entry = [0u8; 0x40];
        let mut walked = 0;
        assert_eq!(redirected_heap_lock(h), 1);
        while redirected_heap_walk(h, entry.as_mut_ptr().cast()) != 0 {
            walked += 1;
        }
        redirected_heap_unlock(h);
        assert!(walked > 0);
        assert_eq!(redirected_heap_free(h, 0, p), 1);
        assert_eq!(redirected_heap_destroy(h), 1);
        let r = redirected_rtl_create_heap(2, core::ptr::null_mut(), 0, 0, core::ptr::null_mut(), core::ptr::null_mut());
        assert!(!r.is_null());
        let q = redirected_rtl_allocate_heap(r, 0, 100);
        assert!(!q.is_null());
        assert!(redirected_rtl_destroy_heap(r).is_null());
        let mut opt = 1u32;
        assert_eq!(redirected_heap_set_information(redirected_get_process_heap(), 1, (&mut opt as *mut u32).cast(), 4), 1);
    }
    assert_eq!(heap_create_maximum(0x2000, 0), 0);
    assert_eq!(heap_create_maximum(0x100, 0x10), 0x1000);
    assert_eq!(heap_create_maximum(0x5000, 0x2000), 0x5000);
}

#[test]
fn virtual_memory_thunks_count_gate_and_report() {
    let _g = guard();
    take();
    XbRegisterXMemCallback(record as *const () as usize);
    XbRegisterXMemOperationCallback(veto as *const () as usize);
    unsafe {
        let before = VIRTUAL_ALLOC_CALLS.load(Ordering::SeqCst);
        let p = redirected_virtual_alloc(core::ptr::null_mut(), 0x10000, 0x3000, 4);
        assert!(!p.is_null());
        *(p as *mut u32) = 5;
        assert_eq!(redirected_virtual_free(p, 0, 0x8000), 1);
        let log = take();
        assert_eq!(log[0], (5, [p as u64, 0x10000, 0x3000 | 4 << 32, 0, 0]));
        assert_eq!(log[1], (4, [p as u64, 0, 0x8000, 0, 0]));
        VETO.store(1, Ordering::SeqCst);
        assert!(redirected_virtual_alloc(core::ptr::null_mut(), 0x10000, 0x3000, 4).is_null());
        assert!(redirected_virtual_alloc_ex(GetCurrentProcess(), core::ptr::null_mut(), 0x10000, 0x3000, 4).is_null());
        VETO.store(0, Ordering::SeqCst);
        assert!(take().is_empty());
        assert_eq!(VIRTUAL_ALLOC_CALLS.load(Ordering::SeqCst) - before, 2);
        let e = redirected_virtual_alloc_ex(GetCurrentProcess(), core::ptr::null_mut(), 0x10000, 0x3000, 4);
        assert!(!e.is_null());
        let before = VIRTUAL_FREE_EX_CALLS.load(Ordering::SeqCst);
        assert_eq!(redirected_virtual_free_ex(GetCurrentProcess(), e, 0, 0x8000), 1);
        assert_eq!(VIRTUAL_FREE_EX_CALLS.load(Ordering::SeqCst) - before, 1);
        assert!(take().is_empty());
    }
    XbRegisterXMemOperationCallback(0);
    XbRegisterXMemCallback(0);
}

#[test]
fn object_creation_thunks_honour_the_operation_callback() {
    let _g = guard();
    XbRegisterXMemOperationCallback(veto as *const () as usize);
    unsafe {
        let ev = redirected_create_event_w(core::ptr::null_mut(), 1, 0, core::ptr::null());
        assert!(!ev.is_null());
        CloseHandle(ev);
        let ev = redirected_create_event_a(core::ptr::null_mut(), 1, 0, core::ptr::null());
        assert!(!ev.is_null());
        CloseHandle(ev);
        for s in [
            redirected_create_semaphore_a(core::ptr::null_mut(), 0, 1, core::ptr::null()),
            redirected_create_semaphore_w(core::ptr::null_mut(), 0, 1, core::ptr::null()),
            redirected_create_semaphore_ex_a(core::ptr::null_mut(), 0, 1, core::ptr::null(), 0, 0x1f_0003),
            redirected_create_semaphore_ex_w(core::ptr::null_mut(), 0, 1, core::ptr::null(), 0, 0x1f_0003),
        ] {
            assert!(!s.is_null());
            CloseHandle(s);
        }
        let nul = wide("NUL");
        let f = redirected_create_file_w(nul.as_ptr(), 0x4000_0000, 0, core::ptr::null_mut(), 3, 0, core::ptr::null_mut());
        assert_ne!(f, INVALID_HANDLE_VALUE);
        CloseHandle(f);
        let f = redirected_create_file_a(b"NUL\0".as_ptr(), 0x4000_0000, 0, core::ptr::null_mut(), 3, 0, core::ptr::null_mut());
        assert_ne!(f, INVALID_HANDLE_VALUE);
        CloseHandle(f);
        let f = redirected_create_file2(nul.as_ptr(), 0x4000_0000, 0, 3, core::ptr::null_mut());
        assert_ne!(f, INVALID_HANDLE_VALUE);
        CloseHandle(f);
        VETO.store(1, Ordering::SeqCst);
        assert!(redirected_create_event_w(core::ptr::null_mut(), 1, 0, core::ptr::null()).is_null());
        assert!(redirected_create_event_a(core::ptr::null_mut(), 1, 0, core::ptr::null()).is_null());
        assert!(redirected_create_semaphore_a(core::ptr::null_mut(), 0, 1, core::ptr::null()).is_null());
        assert!(redirected_create_semaphore_w(core::ptr::null_mut(), 0, 1, core::ptr::null()).is_null());
        assert!(redirected_create_semaphore_ex_a(core::ptr::null_mut(), 0, 1, core::ptr::null(), 0, 0).is_null());
        assert!(redirected_create_semaphore_ex_w(core::ptr::null_mut(), 0, 1, core::ptr::null(), 0, 0).is_null());
        assert_eq!(redirected_create_file_w(nul.as_ptr(), 0x4000_0000, 0, core::ptr::null_mut(), 3, 0, core::ptr::null_mut()), INVALID_HANDLE_VALUE);
        assert_eq!(redirected_create_file_a(b"NUL\0".as_ptr(), 0x4000_0000, 0, core::ptr::null_mut(), 3, 0, core::ptr::null_mut()), INVALID_HANDLE_VALUE);
        assert_eq!(redirected_create_file2(nul.as_ptr(), 0x4000_0000, 0, 3, core::ptr::null_mut()), INVALID_HANDLE_VALUE);
        VETO.store(0, Ordering::SeqCst);
    }
    XbRegisterXMemOperationCallback(0);
}

unsafe extern "system" fn thread_body(p: *mut c_void) -> u32 {
    p as usize as u32 + 1
}

#[test]
fn create_thread_runs_and_keeps_suspended_threads_suspended() {
    unsafe {
        let mut tid = 0u32;
        let h = redirected_create_thread(core::ptr::null_mut(), 0, thread_body as *const () as *mut c_void, 41 as *mut c_void, 0, &mut tid);
        assert!(!h.is_null());
        assert_ne!(tid, 0);
        assert_eq!(WaitForSingleObject(h, 5000), 0);
        let mut code = 0u32;
        GetExitCodeThread(h, &mut code);
        assert_eq!(code, 42);
        CloseHandle(h);
        let h = redirected_create_thread(core::ptr::null_mut(), 0, thread_body as *const () as *mut c_void, 1 as *mut c_void, CREATE_SUSPENDED, core::ptr::null_mut());
        assert_eq!(WaitForSingleObject(h, 100), 0x102);
        assert_eq!(ResumeThread(h), 1);
        assert_eq!(WaitForSingleObject(h, 5000), 0);
        CloseHandle(h);
    }
    if let Ok(a) = job_affinity() {
        assert_ne!(a, 0);
    }
}

#[test]
fn file_mapping_thunks_follow_console_protection_rules() {
    unsafe {
        let h = redirected_create_file_mapping_w(INVALID_HANDLE_VALUE, core::ptr::null_mut(), 4, 0, 0x10000, core::ptr::null());
        assert!(!h.is_null(), "error {}", GetLastError());
        let v = MapViewOfFile(h, 6, 0, 0, 0x10000);
        assert!(!v.is_null());
        *v = 9;
        UnmapViewOfFile(v);
        CloseHandle(h);
        let name = format!("Local\\xbthunk_test_{}", std::process::id());
        let ansi = format!("{name}\0");
        let a = redirected_create_file_mapping_a(INVALID_HANDLE_VALUE, core::ptr::null_mut(), 4 | SEC_COMMIT, 0, 0x1000, ansi.as_ptr());
        assert!(!a.is_null());
        let w = wide(&name);
        let b = redirected_create_file_mapping_w(INVALID_HANDLE_VALUE, core::ptr::null_mut(), 4, 0, 0x1000, w.as_ptr());
        assert!(!b.is_null());
        assert_eq!(GetLastError(), 183);
        CloseHandle(a);
        CloseHandle(b);
        SetLastError(0);
        assert!(redirected_create_file_mapping_w(INVALID_HANDLE_VALUE, core::ptr::null_mut(), 1, 0, 0x1000, core::ptr::null()).is_null());
        assert_eq!(GetLastError(), ERROR_INVALID_PARAMETER);
        let long = format!("{}\0", "x".repeat(70000));
        assert!(redirected_create_file_mapping_a(INVALID_HANDLE_VALUE, core::ptr::null_mut(), 4, 0, 0x1000, long.as_ptr()).is_null());
        assert_eq!(GetLastError(), ERROR_FILENAME_EXCED_RANGE);
        let mut tmp = vec![0u16; 300];
        let n = GetTempPathW(300, tmp.as_mut_ptr()) as usize;
        let path = format!("{}xbthunk_map_{}.bin", String::from_utf16_lossy(&tmp[..n]), std::process::id());
        let pw = wide(&path);
        std::fs::write(&path, vec![1u8; 0x1000]).unwrap();
        let f = K32CreateFileW(pw.as_ptr(), 0xc000_0000, 0, core::ptr::null_mut(), 3, 0, core::ptr::null_mut());
        assert_ne!(f, INVALID_HANDLE_VALUE);
        let ro = redirected_create_file_mapping_w(f, core::ptr::null_mut(), 2, 0, 0x1000, core::ptr::null());
        assert!(!ro.is_null(), "read-only mapping error {}", GetLastError());
        CloseHandle(ro);
        let root = title_root();
        let mut fp = vec![0u16; 0x104];
        let k = GetFinalPathNameByHandleW(f, fp.as_mut_ptr(), 0x104, 2) as usize;
        let inside = k >= root.len() && fp[..root.len()] == root[..];
        let rw = redirected_create_file_mapping_w(f, core::ptr::null_mut(), 4, 0, 0x1000, core::ptr::null());
        assert_eq!(!rw.is_null(), inside);
        if !rw.is_null() {
            CloseHandle(rw);
        }
        assert_eq!(mapping_file_allowed(f, 2), true);
        assert_eq!(mapping_file_allowed(f, 0x40), inside);
        CloseHandle(f);
        DeleteFileW(pw.as_ptr());
    }
    assert_eq!(mapping_access(4), Some(0xf_0007));
    assert_eq!(mapping_access(0x40), Some(0xf_000f));
    assert_eq!(mapping_access(0x20), Some(0xf_000d));
    assert_eq!(mapping_access(0x80), Some(0xf_000d));
    assert_eq!(mapping_access(8), Some(0xf_0005));
    assert_eq!(mapping_access(0x10), None);
}

#[test]
fn co_get_malloc_returns_the_partition_heap_allocator() {
    unsafe {
        let mut m: *mut c_void = core::ptr::null_mut();
        assert_eq!(redirected_co_get_malloc(1, &mut m), 0);
        let mut m2: *mut c_void = core::ptr::null_mut();
        assert_eq!(redirected_co_get_malloc(0, &mut m2), 0);
        assert_eq!(m, m2);
        let v = *(m as *const *const usize);
        let slot = |i: usize| *v.add(i);
        let qi: unsafe extern "system" fn(*mut c_void, *const [u8; 16], *mut *mut c_void) -> i32 = core::mem::transmute(slot(0));
        let mut out: *mut c_void = core::ptr::null_mut();
        assert_eq!(qi(m, &IID_IMALLOC, &mut out), 0);
        assert_eq!(out, m);
        assert_eq!(qi(m, &[9u8; 16], &mut out), E_NOINTERFACE);
        assert!(out.is_null());
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(2));
        assert_eq!(rel(m), 1);
        let alloc: unsafe extern "system" fn(*mut c_void, usize) -> *mut c_void = core::mem::transmute(slot(3));
        let realloc: unsafe extern "system" fn(*mut c_void, *mut c_void, usize) -> *mut c_void = core::mem::transmute(slot(4));
        let free: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(slot(5));
        let size: unsafe extern "system" fn(*mut c_void, *const c_void) -> usize = core::mem::transmute(slot(6));
        let did: unsafe extern "system" fn(*mut c_void, *const c_void) -> i32 = core::mem::transmute(slot(7));
        let p = alloc(m, 32);
        assert_eq!(size(m, p), 32);
        assert_eq!(did(m, p), 1);
        assert_eq!(did(m, core::ptr::null()), -1);
        assert_eq!(size(m, core::ptr::null()), usize::MAX);
        let q = realloc(m, p, 64);
        assert_eq!(size(m, q), 64);
        assert!(realloc(m, q, 0).is_null());
        let r = realloc(m, core::ptr::null_mut(), 16);
        assert_eq!(RtlSizeHeap(XbGetPartitionHeap(1), 0, r), 16);
        free(m, r);
    }
}

#[test]
fn legacy_allocators_count_calls() {
    unsafe {
        let g = GLOBAL_ALLOC_CALLS.load(Ordering::SeqCst);
        let l = LOCAL_ALLOC_CALLS.load(Ordering::SeqCst);
        let c = CO_TASK_MEM_ALLOC_CALLS.load(Ordering::SeqCst);
        assert!(!redirected_global_alloc(0x40, 16).is_null());
        assert!(!redirected_local_alloc(0x40, 16).is_null());
        assert!(!redirected_co_task_mem_alloc(16).is_null());
        assert!(GLOBAL_ALLOC_CALLS.load(Ordering::SeqCst) > g);
        assert!(LOCAL_ALLOC_CALLS.load(Ordering::SeqCst) > l);
        assert!(CO_TASK_MEM_ALLOC_CALLS.load(Ordering::SeqCst) > c);
    }
}

static HOOKED: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn hook_open(_a: *const u16, access: u32, _p: *const u16, _b: *const u16, _f: u32) -> Handle {
    HOOKED.store(access + 1, Ordering::SeqCst);
    0x1234 as Handle
}

unsafe extern "system" fn hook_close(h: Handle) -> Bool {
    HOOKED.store(h as u32, Ordering::SeqCst);
    1
}

unsafe extern "system" fn hook_connect(h: Handle, _s: *const u16, port: u16, _r: u32) -> Handle {
    HOOKED.store(port as u32, Ordering::SeqCst);
    (h as usize + 1) as Handle
}

unsafe extern "system" fn hook_open_request(h: Handle, _v: *const u16, _o: *const u16, _ver: *const u16, _r: *const u16, _a: *const *const u16, flags: u32) -> Handle {
    HOOKED.store(flags, Ordering::SeqCst);
    (h as usize + 1) as Handle
}

unsafe extern "system" fn hook_upgrade(h: Handle, ctx: usize) -> Handle {
    HOOKED.store(ctx as u32, Ordering::SeqCst);
    (h as usize + 1) as Handle
}

unsafe extern "system" fn hook_receive(h: Handle, _r: *mut c_void) -> Bool {
    HOOKED.store(h as u32, Ordering::SeqCst);
    1
}

unsafe extern "system" fn hook_set_option(_h: Handle, option: u32, _b: *mut c_void, _l: u32) -> Bool {
    HOOKED.store(option, Ordering::SeqCst);
    1
}

#[test]
fn win_http_thunks_use_hooks_then_winhttp() {
    let _g = guard();
    unsafe {
        let agent = wide("xbthunk-test");
        let real = redirected_win_http_open(agent.as_ptr(), 1, core::ptr::null(), core::ptr::null(), 0);
        assert!(!real.is_null());
        let mut timeout = 1000u32;
        assert_eq!(redirected_win_http_set_option(real, 6, (&mut timeout as *mut u32).cast(), 4), 1);
        assert_eq!(redirected_win_http_close_handle(real), 1);
        let mut t = [0usize; 7];
        t[0] = hook_open as *const () as usize;
        t[1] = hook_connect as *const () as usize;
        t[2] = hook_open_request as *const () as usize;
        t[3] = hook_upgrade as *const () as usize;
        t[4] = hook_close as *const () as usize;
        t[5] = hook_receive as *const () as usize;
        t[6] = hook_set_option as *const () as usize;
        crate::XbSetWinHttpHooks(&t);
        assert_eq!(redirected_win_http_open(agent.as_ptr(), 3, core::ptr::null(), core::ptr::null(), 0), 0x1234 as Handle);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 4);
        assert_eq!(redirected_win_http_close_handle(0x77 as Handle), 1);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 0x77);
        let host = wide("localhost");
        assert_eq!(redirected_win_http_connect(0x10 as Handle, host.as_ptr(), 443, 0), 0x11 as Handle);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 443);
        assert_eq!(redirected_win_http_open_request(0x11 as Handle, host.as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null(), core::ptr::null(), 0x80_0000), 0x12 as Handle);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 0x80_0000);
        assert_eq!(redirected_win_http_web_socket_complete_upgrade(0x12 as Handle, 9), 0x13 as Handle);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 9);
        assert_eq!(redirected_win_http_receive_response(0x12 as Handle, core::ptr::null_mut()), 1);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 0x12);
        assert_eq!(redirected_win_http_set_option(0x12 as Handle, 31, core::ptr::null_mut(), 0), 1);
        assert_eq!(HOOKED.load(Ordering::SeqCst), 31);
        crate::XbSetWinHttpHooks(&[0; 7]);
    }
}

#[test]
fn co_create_guid_uses_sequential_uuids() {
    unsafe {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        let ha = redirected_co_create_guid(&mut a);
        let hb = redirected_co_create_guid(&mut b);
        assert!(ha == 0 || ha == 0x8007_0720u32 as i32, "{ha:#x}");
        assert_eq!(ha, hb);
        assert_ne!(a, b);
        assert_eq!(a[7] >> 4, 1);
    }
    assert_eq!(rpc_status_to_hresult(1824), 0x8007_0720u32 as i32);
    assert_eq!(rpc_status_to_hresult(0), 0);
}

#[test]
fn output_debug_string_thunks_forward() {
    unsafe {
        redirected_output_debug_string_a(b"xbthunk test\n\0".as_ptr());
        redirected_output_debug_string_a(core::ptr::null());
        let w = wide("xbthunk test\n");
        redirected_output_debug_string_w(w.as_ptr());
    }
}

#[test]
fn register_callbacks_store_pointers() {
    let _g = guard();
    XbRegisterXMemCallback(0x10);
    assert_eq!(XMEM_CALLBACK.load(Ordering::SeqCst), 0x10);
    XbRegisterXMemCallback(0);
    XbRegisterXMemOperationCallback(0x20);
    assert_eq!(XMEM_OPERATION_CALLBACK.load(Ordering::SeqCst), 0x20);
    XbRegisterXMemOperationCallback(0);
    XbSetCallHeapFreeCallbackForRealloc(5);
    assert_eq!(REALLOC_FREE_CALLBACK.load(Ordering::SeqCst), 5);
    XbSetCallHeapFreeCallbackForRealloc(0);
    unsafe { assert!(operation_allowed()) };
}

#[test]
fn fatal_exit_terminates_the_process_with_the_code() {
    if std::env::var("XBTHUNK_FATAL_EXIT_CHILD").is_ok() {
        unsafe { redirected_fatal_exit(77) };
    }
    let exe = std::env::current_exe().unwrap();
    let st = std::process::Command::new(exe)
        .args(["--exact", "redirect::tests::fatal_exit_terminates_the_process_with_the_code", "--nocapture"])
        .env("XBTHUNK_FATAL_EXIT_CHILD", "1")
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(77));
}

#[test]
fn page_file_mapping_takes_the_partition_owner_attribute() {
    unsafe {
        let h = CreateFileMapping2(INVALID_HANDLE_VALUE, core::ptr::null_mut(), 0xf_0007, 4, SEC_COMMIT | SEC_PARTITION_OWNER_HANDLE, 0x1000, core::ptr::null(), core::ptr::null_mut(), 0);
        assert!(!h.is_null(), "error {}", GetLastError());
        CloseHandle(h);
    }
}

#[test]
fn heap_partition_memory_types_fall_back_to_cacheable_on_host() {
    for t in 0..4u32 {
        let h = unsafe { partition_create_heap(0, t, 2, core::ptr::null_mut(), 0, 0, core::ptr::null_mut(), core::ptr::null_mut()) };
        assert!(!h.is_null(), "type {t}");
        let p = unsafe { RtlAllocateHeap(h, 0, 64) };
        assert!(!p.is_null());
        unsafe { RtlDestroyHeap(h) };
    }
}
