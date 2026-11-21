use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::{heap_partition, partition_create_heap, XbGetPartitionHeap, WINHTTP_HOOKS};

type Handle = *mut c_void;
type Bool = i32;

pub const INVALID_HANDLE_VALUE: Handle = usize::MAX as Handle;
pub const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
pub const ERROR_INVALID_PARAMETER: u32 = 0x57;
pub const ERROR_FILENAME_EXCED_RANGE: u32 = 0xce;
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
pub const CREATE_SUSPENDED: u32 = 4;
pub const SEC_COMMIT: u32 = 0x800_0000;
pub const SEC_PARTITION_OWNER_HANDLE: u32 = 0x4_0000;
pub const SEC_ATTRIBUTE_MASK: u32 = 0xdd80_0000;
pub const HEAP_CREATE_MASK: u32 = 0x4_0005;
pub const HEAP_CREATE_EXTRA: u32 = 0x1102;
pub const JOB_NAME: &str = "\\GLOBAL??\\JobObjectXboxSystem";
pub const HEAP_NAME: [u8; 8] = *b"System\0\0";
pub const DEFAULT_TITLE_ROOT: &str = "d:\\";

pub const OP_HEAP_ALLOC: u32 = 1;
pub const OP_HEAP_REALLOC: u32 = 2;
pub const OP_HEAP_FREE: u32 = 3;
pub const OP_VIRTUAL_FREE: u32 = 4;
pub const OP_VIRTUAL_ALLOC: u32 = 5;
pub const OP_CREATE_RESOURCE: u32 = 1;

pub static XMEM_CALLBACK: AtomicUsize = AtomicUsize::new(0);
pub static XMEM_OPERATION_CALLBACK: AtomicUsize = AtomicUsize::new(0);
pub static REALLOC_FREE_CALLBACK: AtomicU32 = AtomicU32::new(0);

pub static VIRTUAL_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
pub static VIRTUAL_ALLOC_EX_CALLS: AtomicU64 = AtomicU64::new(0);
pub static VIRTUAL_FREE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static VIRTUAL_FREE_EX_CALLS: AtomicU64 = AtomicU64::new(0);
pub static GLOBAL_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
pub static LOCAL_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CO_TASK_MEM_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct UnicodeString {
    len: u16,
    max: u16,
    buf: *mut u16,
}

#[repr(C)]
struct AnsiString {
    len: u16,
    max: u16,
    buf: *const u8,
}

#[repr(C)]
struct ObjectAttributes {
    len: u32,
    root: Handle,
    name: *const UnicodeString,
    attributes: u32,
    sd: *const c_void,
    qos: *const c_void,
}

#[link(name = "ntdll", kind = "raw-dylib")]
extern "system" {
    fn RtlAllocateHeap(heap: Handle, flags: u32, size: usize) -> *mut c_void;
    fn RtlFreeHeap(heap: Handle, flags: u32, p: *mut c_void) -> u8;
    fn RtlReAllocateHeap(heap: Handle, flags: u32, p: *mut c_void, size: usize) -> *mut c_void;
    fn RtlSizeHeap(heap: Handle, flags: u32, p: *const c_void) -> usize;
    fn RtlDestroyHeap(heap: Handle) -> Handle;
    fn RtlValidateHeap(heap: Handle, flags: u32, p: *const c_void) -> u8;
    fn RtlSetHeapInformation(heap: Handle, class: u32, info: *const c_void, len: usize) -> i32;
    fn RtlInitAnsiStringEx(dst: *mut AnsiString, src: *const u8) -> i32;
    fn RtlAnsiStringToUnicodeString(dst: *mut UnicodeString, src: *const AnsiString, alloc: u8) -> i32;
    fn RtlFreeUnicodeString(s: *mut UnicodeString);
    fn RtlSetLastWin32Error(e: u32);
    fn RtlExitUserProcess(code: u32) -> !;
    fn NtOpenJobObject(out: *mut Handle, access: u32, oa: *const ObjectAttributes) -> i32;
    fn NtQueryInformationJobObject(job: Handle, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
    fn NtSetInformationThread(thread: Handle, class: u32, info: *const c_void, len: u32) -> i32;
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
    fn NtClose(h: Handle) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    #[link_name = "CreateThread"]
    fn K32CreateThread(sa: *mut c_void, stack: usize, start: *mut c_void, param: *mut c_void, flags: u32, tid: *mut u32) -> Handle;
    fn ResumeThread(h: Handle) -> u32;
    fn SetLastError(e: u32);
    fn CloseHandle(h: Handle) -> Bool;
    #[link_name = "HeapAlloc"]
    fn K32HeapAlloc(heap: Handle, flags: u32, size: usize) -> *mut c_void;
    #[link_name = "HeapFree"]
    fn K32HeapFree(heap: Handle, flags: u32, p: *mut c_void) -> Bool;
    #[link_name = "HeapReAlloc"]
    fn K32HeapReAlloc(heap: Handle, flags: u32, p: *mut c_void, size: usize) -> *mut c_void;
    #[link_name = "HeapDestroy"]
    fn K32HeapDestroy(heap: Handle) -> Bool;
    #[link_name = "HeapLock"]
    fn K32HeapLock(heap: Handle) -> Bool;
    #[link_name = "HeapUnlock"]
    fn K32HeapUnlock(heap: Handle) -> Bool;
    #[link_name = "HeapQueryInformation"]
    fn K32HeapQueryInformation(heap: Handle, class: u32, info: *mut c_void, len: usize, ret: *mut usize) -> Bool;
    #[link_name = "HeapSetInformation"]
    fn K32HeapSetInformation(heap: Handle, class: u32, info: *mut c_void, len: usize) -> Bool;
    #[link_name = "HeapSize"]
    fn K32HeapSize(heap: Handle, flags: u32, p: *const c_void) -> usize;
    #[link_name = "HeapWalk"]
    fn K32HeapWalk(heap: Handle, entry: *mut c_void) -> Bool;
    #[link_name = "VirtualAlloc"]
    fn K32VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    #[link_name = "VirtualAllocEx"]
    fn K32VirtualAllocEx(process: Handle, addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    #[link_name = "VirtualFree"]
    fn K32VirtualFree(addr: *mut c_void, size: usize, kind: u32) -> Bool;
    #[link_name = "VirtualFreeEx"]
    fn K32VirtualFreeEx(process: Handle, addr: *mut c_void, size: usize, kind: u32) -> Bool;
    #[link_name = "CreateFileA"]
    fn K32CreateFileA(name: *const u8, access: u32, share: u32, sa: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle;
    #[link_name = "CreateFileW"]
    fn K32CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle;
    #[link_name = "CreateFile2"]
    fn K32CreateFile2(name: *const u16, access: u32, share: u32, disposition: u32, params: *mut c_void) -> Handle;
    #[link_name = "CreateEventA"]
    fn K32CreateEventA(sa: *mut c_void, manual: Bool, initial: Bool, name: *const u8) -> Handle;
    #[link_name = "CreateEventW"]
    fn K32CreateEventW(sa: *mut c_void, manual: Bool, initial: Bool, name: *const u16) -> Handle;
    #[link_name = "CreateSemaphoreA"]
    fn K32CreateSemaphoreA(sa: *mut c_void, initial: i32, max: i32, name: *const u8) -> Handle;
    #[link_name = "CreateSemaphoreW"]
    fn K32CreateSemaphoreW(sa: *mut c_void, initial: i32, max: i32, name: *const u16) -> Handle;
    #[link_name = "CreateSemaphoreExA"]
    fn K32CreateSemaphoreExA(sa: *mut c_void, initial: i32, max: i32, name: *const u8, flags: u32, access: u32) -> Handle;
    #[link_name = "CreateSemaphoreExW"]
    fn K32CreateSemaphoreExW(sa: *mut c_void, initial: i32, max: i32, name: *const u16, flags: u32, access: u32) -> Handle;
    fn GetFinalPathNameByHandleW(h: Handle, buf: *mut u16, n: u32, flags: u32) -> u32;
    #[link_name = "GlobalAlloc"]
    fn K32GlobalAlloc(flags: u32, size: usize) -> *mut c_void;
    #[link_name = "LocalAlloc"]
    fn K32LocalAlloc(flags: u32, size: usize) -> *mut c_void;
    #[link_name = "OutputDebugStringA"]
    fn K32OutputDebugStringA(s: *const u8);
    #[link_name = "OutputDebugStringW"]
    fn K32OutputDebugStringW(s: *const u16);
}

#[link(name = "kernelbase", kind = "raw-dylib")]
extern "system" {
    fn CreateFileMapping2(file: Handle, sa: *mut c_void, access: u32, protect: u32, attributes: u32, size: u64, name: *const u16, ext: *mut c_void, count: u32) -> Handle;
}

#[link(name = "ole32")]
extern "system" {
    #[link_name = "CoTaskMemAlloc"]
    fn OleCoTaskMemAlloc(size: usize) -> *mut c_void;
}

#[link(name = "rpcrt4")]
extern "system" {
    fn UuidCreateSequential(uuid: *mut [u8; 16]) -> i32;
}

#[link(name = "winhttp")]
extern "system" {
    #[link_name = "WinHttpOpen"]
    fn HttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> Handle;
    #[link_name = "WinHttpConnect"]
    fn HttpConnect(session: Handle, server: *const u16, port: u16, reserved: u32) -> Handle;
    #[link_name = "WinHttpOpenRequest"]
    fn HttpOpenRequest(connect: Handle, verb: *const u16, object: *const u16, version: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32) -> Handle;
    #[link_name = "WinHttpCloseHandle"]
    fn HttpCloseHandle(h: Handle) -> Bool;
    #[link_name = "WinHttpReceiveResponse"]
    fn HttpReceiveResponse(h: Handle, reserved: *mut c_void) -> Bool;
    #[link_name = "WinHttpSetOption"]
    fn HttpSetOption(h: Handle, option: u32, buf: *mut c_void, len: u32) -> Bool;
    #[link_name = "WinHttpWebSocketCompleteUpgrade"]
    fn HttpWebSocketCompleteUpgrade(h: Handle, context: usize) -> Handle;
}

pub type XMemCallback = unsafe extern "system" fn(op: u32, record: *mut [u64; 5]);
pub type XMemOperationCallback = unsafe extern "system" fn(op: u32, args: *mut *mut u32);

unsafe fn notify(op: u32, record: [u64; 5]) {
    let cb = XMEM_CALLBACK.load(Ordering::Acquire);
    if cb != 0 {
        let cb: XMemCallback = core::mem::transmute(cb);
        let mut r = record;
        cb(op, &mut r);
    }
}

pub unsafe fn operation_allowed() -> bool {
    let cb = XMEM_OPERATION_CALLBACK.load(Ordering::Acquire);
    let mut allowed: u32 = 1;
    if cb != 0 {
        let cb: XMemOperationCallback = core::mem::transmute(cb);
        let mut p: *mut u32 = &mut allowed;
        cb(OP_CREATE_RESOURCE, &mut p);
    }
    allowed != 0
}

fn page_size() -> u32 {
    static P: OnceLock<u32> = OnceLock::new();
    *P.get_or_init(|| {
        let mut info = [0u32; 16];
        let st = unsafe { NtQuerySystemInformation(0, info.as_mut_ptr().cast(), 0x40, core::ptr::null_mut()) };
        if st >= 0 {
            info[2]
        } else {
            0
        }
    })
}

fn system_partition() -> usize {
    heap_partition(0).unwrap_or(0)
}

unsafe fn name_heap(heap: Handle) {
    RtlSetHeapInformation(heap, 7, HEAP_NAME.as_ptr().cast(), HEAP_NAME.len());
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn job_affinity() -> Result<u64, i32> {
    let name: Vec<u16> = JOB_NAME.encode_utf16().collect();
    let us = UnicodeString { len: (name.len() * 2) as u16, max: (name.len() * 2) as u16, buf: name.as_ptr() as *mut u16 };
    let oa = ObjectAttributes { len: 0x30, root: core::ptr::null_mut(), name: &us, attributes: 0x40, sd: core::ptr::null(), qos: core::ptr::null() };
    let mut job: Handle = core::ptr::null_mut();
    let mut info = [0u64; 8];
    let mut st = unsafe { NtOpenJobObject(&mut job, 0x1f_003f, &oa) };
    if st >= 0 {
        st = unsafe { NtQueryInformationJobObject(job, 2, info.as_mut_ptr().cast(), 0x40, core::ptr::null_mut()) };
    }
    if !job.is_null() {
        unsafe { NtClose(job) };
    }
    if st >= 0 {
        Ok(info[6])
    } else {
        Err(st)
    }
}

pub unsafe extern "system" fn redirected_create_thread(sa: *mut c_void, stack: usize, start: *mut c_void, param: *mut c_void, flags: u32, tid: *mut u32) -> Handle {
    let h = K32CreateThread(sa, stack, start, param, flags | CREATE_SUSPENDED, tid);
    if !h.is_null() {
        if let Ok(aff) = job_affinity() {
            NtSetInformationThread(h, 4, (&aff as *const u64).cast(), 8);
        }
        if flags & CREATE_SUSPENDED == 0 {
            ResumeThread(h);
        }
    }
    h
}

pub unsafe extern "system" fn redirected_get_process_heap() -> Handle {
    XbGetPartitionHeap(1)
}

pub unsafe extern "system" fn redirected_heap_alloc(heap: Handle, flags: u32, size: usize) -> *mut c_void {
    let r = K32HeapAlloc(heap, flags, size);
    notify(OP_HEAP_ALLOC, [heap as u64, flags as u64, size as u64, r as u64, 0]);
    r
}

pub fn heap_create_maximum(initial: usize, maximum: usize) -> usize {
    let ps = page_size() as usize;
    if maximum == 0 && (ps << 4) as u32 != 0 {
        return 0;
    }
    let m = if maximum != 0 && maximum < ps { ps } else { maximum };
    m.max(initial)
}

pub unsafe extern "system" fn redirected_heap_create(options: u32, initial: usize, maximum: usize) -> Handle {
    let flags = (options & HEAP_CREATE_MASK) | HEAP_CREATE_EXTRA;
    let max = heap_create_maximum(initial, maximum);
    let h = partition_create_heap(system_partition(), 2, flags, core::ptr::null_mut(), max, initial, core::ptr::null_mut(), core::ptr::null_mut());
    if h.is_null() {
        SetLastError(ERROR_NOT_ENOUGH_MEMORY);
    } else {
        name_heap(h);
    }
    h
}

pub unsafe extern "system" fn redirected_heap_destroy(heap: Handle) -> Bool {
    K32HeapDestroy(heap)
}

pub unsafe extern "system" fn redirected_heap_free(heap: Handle, flags: u32, p: *mut c_void) -> Bool {
    notify(OP_HEAP_FREE, [heap as u64, flags as u64, p as u64, 0, 0]);
    K32HeapFree(heap, flags, p)
}

pub unsafe extern "system" fn redirected_heap_lock(heap: Handle) -> Bool {
    K32HeapLock(heap)
}

pub unsafe extern "system" fn redirected_heap_query_information(heap: Handle, class: u32, info: *mut c_void, len: usize, ret: *mut usize) -> Bool {
    K32HeapQueryInformation(heap, class, info, len, ret)
}

unsafe fn realloc_with_notify(heap: Handle, flags: u32, p: *mut c_void, size: usize, f: unsafe extern "system" fn(Handle, u32, *mut c_void, usize) -> *mut c_void) -> *mut c_void {
    if XMEM_CALLBACK.load(Ordering::Acquire) != 0 && REALLOC_FREE_CALLBACK.load(Ordering::Acquire) != 0 {
        notify(OP_HEAP_FREE, [heap as u64, flags as u64, p as u64, 0, 0]);
    }
    let r = f(heap, flags, p, size);
    notify(OP_HEAP_REALLOC, [heap as u64, flags as u64, size as u64, r as u64, p as u64]);
    r
}

pub unsafe extern "system" fn redirected_heap_realloc(heap: Handle, flags: u32, p: *mut c_void, size: usize) -> *mut c_void {
    realloc_with_notify(heap, flags, p, size, K32HeapReAlloc)
}

pub unsafe extern "system" fn redirected_heap_set_information(heap: Handle, class: u32, info: *mut c_void, len: usize) -> Bool {
    K32HeapSetInformation(heap, class, info, len)
}

pub unsafe extern "system" fn redirected_heap_size(heap: Handle, flags: u32, p: *const c_void) -> usize {
    K32HeapSize(heap, flags, p)
}

pub unsafe extern "system" fn redirected_heap_unlock(heap: Handle) -> Bool {
    K32HeapUnlock(heap)
}

pub unsafe extern "system" fn redirected_heap_walk(heap: Handle, entry: *mut c_void) -> Bool {
    K32HeapWalk(heap, entry)
}

pub unsafe extern "system" fn redirected_virtual_alloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void {
    VIRTUAL_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    let r = K32VirtualAlloc(addr, size, kind, protect);
    notify(OP_VIRTUAL_ALLOC, [r as u64, size as u64, kind as u64 | (protect as u64) << 32, 0, 0]);
    r
}

pub unsafe extern "system" fn redirected_virtual_alloc_ex(process: Handle, addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void {
    VIRTUAL_ALLOC_EX_CALLS.fetch_add(1, Ordering::SeqCst);
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32VirtualAllocEx(process, addr, size, kind, protect)
}

pub unsafe extern "system" fn redirected_virtual_free(addr: *mut c_void, size: usize, kind: u32) -> Bool {
    VIRTUAL_FREE_CALLS.fetch_add(1, Ordering::SeqCst);
    notify(OP_VIRTUAL_FREE, [addr as u64, size as u64, kind as u64, 0, 0]);
    K32VirtualFree(addr, size, kind)
}

pub unsafe extern "system" fn redirected_virtual_free_ex(process: Handle, addr: *mut c_void, size: usize, kind: u32) -> Bool {
    VIRTUAL_FREE_EX_CALLS.fetch_add(1, Ordering::SeqCst);
    K32VirtualFreeEx(process, addr, size, kind)
}

pub unsafe extern "system" fn redirected_create_file_a(name: *const u8, access: u32, share: u32, sa: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle {
    if !operation_allowed() {
        return INVALID_HANDLE_VALUE;
    }
    K32CreateFileA(name, access, share, sa, disposition, flags, template)
}

pub unsafe extern "system" fn redirected_create_file_w(name: *const u16, access: u32, share: u32, sa: *mut c_void, disposition: u32, flags: u32, template: Handle) -> Handle {
    if !operation_allowed() {
        return INVALID_HANDLE_VALUE;
    }
    K32CreateFileW(name, access, share, sa, disposition, flags, template)
}

pub unsafe extern "system" fn redirected_create_file2(name: *const u16, access: u32, share: u32, disposition: u32, params: *mut c_void) -> Handle {
    if !operation_allowed() {
        return INVALID_HANDLE_VALUE;
    }
    K32CreateFile2(name, access, share, disposition, params)
}

pub unsafe extern "system" fn redirected_create_event_a(sa: *mut c_void, manual: Bool, initial: Bool, name: *const u8) -> Handle {
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32CreateEventA(sa, manual, initial, name)
}

pub unsafe extern "system" fn redirected_create_event_w(sa: *mut c_void, manual: Bool, initial: Bool, name: *const u16) -> Handle {
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32CreateEventW(sa, manual, initial, name)
}

pub unsafe extern "system" fn redirected_create_semaphore_a(sa: *mut c_void, initial: i32, max: i32, name: *const u8) -> Handle {
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32CreateSemaphoreA(sa, initial, max, name)
}

pub unsafe extern "system" fn redirected_create_semaphore_w(sa: *mut c_void, initial: i32, max: i32, name: *const u16) -> Handle {
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32CreateSemaphoreW(sa, initial, max, name)
}

pub unsafe extern "system" fn redirected_create_semaphore_ex_a(sa: *mut c_void, initial: i32, max: i32, name: *const u8, flags: u32, access: u32) -> Handle {
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32CreateSemaphoreExA(sa, initial, max, name, flags, access)
}

pub unsafe extern "system" fn redirected_create_semaphore_ex_w(sa: *mut c_void, initial: i32, max: i32, name: *const u16, flags: u32, access: u32) -> Handle {
    if !operation_allowed() {
        return core::ptr::null_mut();
    }
    K32CreateSemaphoreExW(sa, initial, max, name, flags, access)
}

pub unsafe extern "system" fn redirected_rtl_allocate_heap(heap: Handle, flags: u32, size: usize) -> *mut c_void {
    let r = RtlAllocateHeap(heap, flags, size);
    notify(OP_HEAP_ALLOC, [heap as u64, flags as u64, size as u64, r as u64, 0]);
    r
}

pub unsafe extern "system" fn redirected_rtl_create_heap(flags: u32, base: *mut c_void, reserve: usize, commit: usize, lock: *mut c_void, params: *mut c_void) -> Handle {
    let h = partition_create_heap(system_partition(), 2, flags | 0x102, base, reserve, commit, lock, params);
    if !h.is_null() {
        name_heap(h);
    }
    h
}

pub unsafe extern "system" fn redirected_rtl_destroy_heap(heap: Handle) -> Handle {
    RtlDestroyHeap(heap)
}

pub unsafe extern "system" fn redirected_rtl_free_heap(heap: Handle, flags: u32, p: *mut c_void) -> u8 {
    let r = RtlFreeHeap(heap, flags, p);
    notify(OP_HEAP_FREE, [heap as u64, flags as u64, p as u64, 0, 0]);
    r
}

unsafe extern "system" fn rtl_realloc(heap: Handle, flags: u32, p: *mut c_void, size: usize) -> *mut c_void {
    RtlReAllocateHeap(heap, flags, p, size)
}

pub unsafe extern "system" fn redirected_rtl_reallocate_heap(heap: Handle, flags: u32, p: *mut c_void, size: usize) -> *mut c_void {
    realloc_with_notify(heap, flags, p, size, rtl_realloc)
}

pub unsafe extern "system" fn redirected_rtl_size_heap(heap: Handle, flags: u32, p: *const c_void) -> usize {
    RtlSizeHeap(heap, flags, p)
}

pub fn mapping_access(protect: u32) -> Option<u32> {
    match protect {
        4 => Some(0xf_0007),
        0x40 => Some(0xf_000f),
        0x20 | 0x80 => Some(0xf_000d),
        2 | 8 => Some(0xf_0005),
        _ => None,
    }
}

pub fn title_root() -> &'static [u16] {
    static ROOT: OnceLock<Vec<u16>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::var("TOMBOX_XBTHUNK_TITLE_ROOT").unwrap_or_else(|_| DEFAULT_TITLE_ROOT.to_string());
        let w = wide(&root);
        let h = unsafe { K32CreateFileW(w.as_ptr(), 0x8000_0000, 0, core::ptr::null_mut(), 3, 0x200_0000, core::ptr::null_mut()) };
        let mut buf = vec![0u16; 0x104];
        if h != INVALID_HANDLE_VALUE {
            let n = unsafe { GetFinalPathNameByHandleW(h, buf.as_mut_ptr(), 0x104, 2) } as usize;
            unsafe { CloseHandle(h) };
            if n == 0 || n >= buf.len() {
                buf.iter_mut().for_each(|c| *c = 0);
            }
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        buf.truncate(n);
        buf
    })
}

pub unsafe fn mapping_file_allowed(file: Handle, protect: u32) -> bool {
    if protect & 0x4c == 0 {
        return true;
    }
    let mut buf = vec![0u16; 0x104];
    let mut n = GetFinalPathNameByHandleW(file, buf.as_mut_ptr(), 0x104, 2);
    if n > 0x105 {
        buf = vec![0u16; n as usize];
        n = GetFinalPathNameByHandleW(file, buf.as_mut_ptr(), 0x104, 2);
        if n == 0 {
            SetLastError(0x32);
            return false;
        }
    } else if n == 0 {
        SetLastError(0x32);
        return false;
    }
    let root = title_root();
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let path = &buf[..len];
    let k = root.len();
    path.len() >= k && path[..k] == root[..]
}

pub unsafe extern "system" fn redirected_create_file_mapping_w(file: Handle, sa: *mut c_void, protect: u32, size_high: u32, size_low: u32, name: *const u16) -> Handle {
    let mut attributes = protect & SEC_ATTRIBUTE_MASK;
    let page = protect ^ attributes;
    if attributes == 0 {
        attributes = SEC_COMMIT;
    }
    let Some(access) = mapping_access(page) else {
        SetLastError(ERROR_INVALID_PARAMETER);
        return core::ptr::null_mut();
    };
    let size = if size_low == 0 && size_high == 0 { 0 } else { (size_high as u64) << 32 | size_low as u64 };
    let file = if file == INVALID_HANDLE_VALUE {
        attributes |= SEC_PARTITION_OWNER_HANDLE;
        INVALID_HANDLE_VALUE
    } else {
        if !mapping_file_allowed(file, page) {
            return core::ptr::null_mut();
        }
        file
    };
    CreateFileMapping2(file, sa, access, page, attributes, size, name, core::ptr::null_mut(), 0)
}

pub unsafe extern "system" fn redirected_create_file_mapping_a(file: Handle, sa: *mut c_void, protect: u32, size_high: u32, size_low: u32, name: *const u8) -> Handle {
    let mut us = UnicodeString { len: 0, max: 0, buf: core::ptr::null_mut() };
    if !name.is_null() {
        let mut a = AnsiString { len: 0, max: 0, buf: core::ptr::null() };
        if RtlInitAnsiStringEx(&mut a, name) < 0 {
            SetLastError(ERROR_FILENAME_EXCED_RANGE);
            return core::ptr::null_mut();
        }
        let st = RtlAnsiStringToUnicodeString(&mut us, &a, 1);
        if st < 0 {
            if st as u32 == 0x8000_0005 {
                SetLastError(ERROR_FILENAME_EXCED_RANGE);
            } else {
                RtlSetLastWin32Error(st as u32);
            }
            return core::ptr::null_mut();
        }
    }
    let h = redirected_create_file_mapping_w(file, sa, protect, size_high, size_low, us.buf);
    if !name.is_null() {
        RtlFreeUnicodeString(&mut us);
    }
    h
}

pub unsafe extern "system" fn redirected_fatal_exit(code: u32) -> ! {
    RtlExitUserProcess(code)
}

#[repr(C)]
pub struct StaticMalloc {
    vtbl: *const [usize; 9],
}

unsafe impl Sync for StaticMalloc {}

const IID_IUNKNOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
const IID_IMALLOC: [u8; 16] = [2, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

unsafe extern "system" fn malloc_query_interface(this: *mut c_void, iid: *const [u8; 16], out: *mut *mut c_void) -> i32 {
    if *iid == IID_IUNKNOWN || *iid == IID_IMALLOC {
        *out = this;
        0
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn malloc_add_ref(_this: *mut c_void) -> u32 {
    1
}

unsafe extern "system" fn malloc_alloc(_this: *mut c_void, size: usize) -> *mut c_void {
    RtlAllocateHeap(XbGetPartitionHeap(1), 0, size)
}

unsafe extern "system" fn malloc_realloc(_this: *mut c_void, p: *mut c_void, size: usize) -> *mut c_void {
    if p.is_null() {
        return RtlAllocateHeap(XbGetPartitionHeap(1), 0, size);
    }
    let heap = XbGetPartitionHeap(1);
    if size == 0 {
        RtlFreeHeap(heap, 0, p);
        return core::ptr::null_mut();
    }
    RtlReAllocateHeap(heap, 0, p, size)
}

unsafe extern "system" fn malloc_free(_this: *mut c_void, p: *mut c_void) {
    RtlFreeHeap(XbGetPartitionHeap(1), 0, p);
}

unsafe extern "system" fn malloc_get_size(_this: *mut c_void, p: *const c_void) -> usize {
    if p.is_null() {
        return usize::MAX;
    }
    RtlSizeHeap(XbGetPartitionHeap(1), 0, p)
}

unsafe extern "system" fn malloc_did_alloc(_this: *mut c_void, p: *const c_void) -> i32 {
    if p.is_null() {
        return -1;
    }
    RtlValidateHeap(XbGetPartitionHeap(1), 0, p) as i32
}

unsafe extern "system" fn malloc_heap_minimize(_this: *mut c_void) {}

struct VtblHolder([usize; 9]);

static MALLOC_VTBL: OnceLock<VtblHolder> = OnceLock::new();
static MALLOC: OnceLock<usize> = OnceLock::new();

pub fn static_malloc() -> *mut c_void {
    *MALLOC.get_or_init(|| {
        let v = MALLOC_VTBL.get_or_init(|| {
            VtblHolder([
                malloc_query_interface as *const () as usize,
                malloc_add_ref as *const () as usize,
                malloc_add_ref as *const () as usize,
                malloc_alloc as *const () as usize,
                malloc_realloc as *const () as usize,
                malloc_free as *const () as usize,
                malloc_get_size as *const () as usize,
                malloc_did_alloc as *const () as usize,
                malloc_heap_minimize as *const () as usize,
            ])
        });
        Box::into_raw(Box::new(StaticMalloc { vtbl: &v.0 })) as usize
    }) as *mut c_void
}

pub unsafe extern "system" fn redirected_co_get_malloc(_context: u32, out: *mut *mut c_void) -> i32 {
    *out = static_malloc();
    0
}

pub unsafe extern "system" fn redirected_global_alloc(flags: u32, size: usize) -> *mut c_void {
    GLOBAL_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    K32GlobalAlloc(flags, size)
}

pub unsafe extern "system" fn redirected_local_alloc(flags: u32, size: usize) -> *mut c_void {
    LOCAL_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    K32LocalAlloc(flags, size)
}

pub unsafe extern "system" fn redirected_co_task_mem_alloc(size: usize) -> *mut c_void {
    CO_TASK_MEM_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    OleCoTaskMemAlloc(size)
}

fn hook(i: usize) -> usize {
    WINHTTP_HOOKS.lock().unwrap_or_else(|e| e.into_inner())[i]
}

pub unsafe extern "system" fn redirected_win_http_open(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> Handle {
    match hook(0) {
        0 => HttpOpen(agent, access, proxy, bypass, flags),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(*const u16, u32, *const u16, *const u16, u32) -> Handle>(f)(agent, access, proxy, bypass, flags),
    }
}

pub unsafe extern "system" fn redirected_win_http_connect(session: Handle, server: *const u16, port: u16, reserved: u32) -> Handle {
    match hook(1) {
        0 => HttpConnect(session, server, port, reserved),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(Handle, *const u16, u16, u32) -> Handle>(f)(session, server, port, reserved),
    }
}

pub unsafe extern "system" fn redirected_win_http_open_request(connect: Handle, verb: *const u16, object: *const u16, version: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32) -> Handle {
    match hook(2) {
        0 => HttpOpenRequest(connect, verb, object, version, referrer, accept, flags),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(Handle, *const u16, *const u16, *const u16, *const u16, *const *const u16, u32) -> Handle>(f)(connect, verb, object, version, referrer, accept, flags),
    }
}

pub unsafe extern "system" fn redirected_win_http_web_socket_complete_upgrade(h: Handle, context: usize) -> Handle {
    match hook(3) {
        0 => HttpWebSocketCompleteUpgrade(h, context),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(Handle, usize) -> Handle>(f)(h, context),
    }
}

pub unsafe extern "system" fn redirected_win_http_close_handle(h: Handle) -> Bool {
    match hook(4) {
        0 => HttpCloseHandle(h),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(Handle) -> Bool>(f)(h),
    }
}

pub unsafe extern "system" fn redirected_win_http_receive_response(h: Handle, reserved: *mut c_void) -> Bool {
    match hook(5) {
        0 => HttpReceiveResponse(h, reserved),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(Handle, *mut c_void) -> Bool>(f)(h, reserved),
    }
}

pub unsafe extern "system" fn redirected_win_http_set_option(h: Handle, option: u32, buf: *mut c_void, len: u32) -> Bool {
    match hook(6) {
        0 => HttpSetOption(h, option, buf, len),
        f => core::mem::transmute::<usize, unsafe extern "system" fn(Handle, u32, *mut c_void, u32) -> Bool>(f)(h, option, buf, len),
    }
}

pub fn rpc_status_to_hresult(status: i32) -> i32 {
    if status > 0 {
        (0x8007_0000u32 | (status as u32 & 0xffff)) as i32
    } else {
        status
    }
}

pub unsafe extern "system" fn redirected_co_create_guid(guid: *mut [u8; 16]) -> i32 {
    rpc_status_to_hresult(UuidCreateSequential(guid))
}

pub unsafe extern "system" fn redirected_output_debug_string_a(s: *const u8) {
    K32OutputDebugStringA(s)
}

pub unsafe extern "system" fn redirected_output_debug_string_w(s: *const u16) {
    K32OutputDebugStringW(s)
}

#[repr(C)]
pub struct RedirectionEntry {
    pub module: *const u8,
    pub function: *const u8,
    pub thunk: *const c_void,
}

#[repr(C)]
pub struct RedirectionInformation {
    pub version: u32,
    pub count: u32,
    pub entries: *const RedirectionEntry,
}

unsafe impl Sync for RedirectionEntry {}
unsafe impl Sync for RedirectionInformation {}

macro_rules! entry {
    ($m:literal, $f:literal, $t:expr) => {
        RedirectionEntry { module: concat!($m, "\0").as_ptr(), function: concat!($f, "\0").as_ptr(), thunk: $t as *const c_void }
    };
}

pub static REDIRECTION_TABLE: [RedirectionEntry; 74] = [
    entry!("kernel32.dll", "CreateThread", redirected_create_thread),
    entry!("kernel32.dll", "GetProcessHeap", redirected_get_process_heap),
    entry!("kernel32.dll", "HeapAlloc", redirected_heap_alloc),
    entry!("kernel32.dll", "HeapCreate", redirected_heap_create),
    entry!("kernel32.dll", "HeapDestroy", redirected_heap_destroy),
    entry!("kernel32.dll", "HeapFree", redirected_heap_free),
    entry!("kernel32.dll", "HeapLock", redirected_heap_lock),
    entry!("kernel32.dll", "HeapQueryInformation", redirected_heap_query_information),
    entry!("kernel32.dll", "HeapReAlloc", redirected_heap_realloc),
    entry!("kernel32.dll", "HeapSetInformation", redirected_heap_set_information),
    entry!("kernel32.dll", "HeapSize", redirected_heap_size),
    entry!("kernel32.dll", "HeapUnlock", redirected_heap_unlock),
    entry!("kernel32.dll", "HeapWalk", redirected_heap_walk),
    entry!("kernel32.dll", "VirtualAlloc", redirected_virtual_alloc),
    entry!("kernel32.dll", "VirtualAllocEx", redirected_virtual_alloc_ex),
    entry!("kernel32.dll", "VirtualFree", redirected_virtual_free),
    entry!("kernel32.dll", "VirtualFreeEx", redirected_virtual_free_ex),
    entry!("kernel32.dll", "CreateFileA", redirected_create_file_a),
    entry!("kernel32.dll", "CreateFileW", redirected_create_file_w),
    entry!("kernel32.dll", "CreateFile2", redirected_create_file2),
    entry!("kernel32.dll", "CreateEventA", redirected_create_event_a),
    entry!("kernel32.dll", "CreateEventW", redirected_create_event_w),
    entry!("kernel32.dll", "CreateSemaphoreA", redirected_create_semaphore_a),
    entry!("kernel32.dll", "CreateSemaphoreW", redirected_create_semaphore_w),
    entry!("kernel32.dll", "CreateSemaphoreExA", redirected_create_semaphore_ex_a),
    entry!("kernel32.dll", "CreateSemaphoreExW", redirected_create_semaphore_ex_w),
    entry!("ntdll.dll", "RtlAllocateHeap", redirected_rtl_allocate_heap),
    entry!("ntdll.dll", "RtlCreateHeap", redirected_rtl_create_heap),
    entry!("ntdll.dll", "RtlDestroyHeap", redirected_rtl_destroy_heap),
    entry!("ntdll.dll", "RtlFreeHeap", redirected_rtl_free_heap),
    entry!("ntdll.dll", "RtlReAllocateHeap", redirected_rtl_reallocate_heap),
    entry!("ntdll.dll", "RtlSizeHeap", redirected_rtl_size_heap),
    entry!("api-ms-win-core-heap-l1-1-0.dll", "HeapAlloc", redirected_heap_alloc),
    entry!("api-ms-win-core-heap-l1-1-0.dll", "HeapCreate", redirected_heap_create),
    entry!("api-ms-win-core-heap-l1-1-0.dll", "HeapReAlloc", redirected_heap_realloc),
    entry!("api-ms-win-core-heap-l1-1-0.dll", "HeapFree", redirected_heap_free),
    entry!("api-ms-win-core-heap-l1-1-0.dll", "GetProcessHeap", redirected_get_process_heap),
    entry!("api-ms-win-core-memory-l1-1-0.dll", "VirtualAlloc", redirected_virtual_alloc),
    entry!("api-ms-win-core-memory-l1-1-0.dll", "VirtualAllocEx", redirected_virtual_alloc_ex),
    entry!("api-ms-win-core-memory-l1-1-0.dll", "VirtualFree", redirected_virtual_free),
    entry!("api-ms-win-core-memory-l1-1-0.dll", "VirtualFreeEx", redirected_virtual_free_ex),
    entry!("api-ms-win-core-processthreads-l1-1-0.dll", "CreateThread", redirected_create_thread),
    entry!("api-ms-win-core-kernel32-legacy-l1-1-0.dll", "CreateFileMappingA", redirected_create_file_mapping_a),
    entry!("api-ms-win-core-memory-l1-1-0.dll", "CreateFileMappingW", redirected_create_file_mapping_w),
    entry!("kernel32.dll", "FatalExit", redirected_fatal_exit),
    entry!("api-ms-win-core-file-l1-1-0.dll", "CreateFileA", redirected_create_file_a),
    entry!("api-ms-win-core-file-l1-1-0.dll", "CreateFileW", redirected_create_file_w),
    entry!("api-ms-win-core-file-l1-1-0.dll", "CreateFile2", redirected_create_file2),
    entry!("api-ms-win-core-synch-l1-1-0.dll", "CreateEventA", redirected_create_event_a),
    entry!("api-ms-win-core-synch-l1-1-0.dll", "CreateEventW", redirected_create_event_w),
    entry!("api-ms-win-core-kernel32-legacy-l1-1-2.dll", "CreateSemaphoreA", redirected_create_semaphore_a),
    entry!("api-ms-win-core-synch-l1-1-0.dll", "CreateSemaphoreW", redirected_create_semaphore_w),
    entry!("api-ms-win-core-synch-ansi-l1-1-0.dll", "CreateSemaphoreExA", redirected_create_semaphore_ex_a),
    entry!("api-ms-win-core-synch-l1-1-0.dll", "CreateSemaphoreExW", redirected_create_semaphore_ex_w),
    entry!("api-ms-win-core-com-l1-1-0.dll", "CoGetMalloc", redirected_co_get_malloc),
    entry!("kernel32.dll", "GlobalAlloc", redirected_global_alloc),
    entry!("api-ms-win-core-heap-l2-1-0.dll", "GlobalAlloc", redirected_global_alloc),
    entry!("kernel32.dll", "LocalAlloc", redirected_local_alloc),
    entry!("api-ms-win-core-heap-l2-1-0.dll", "LocalAlloc", redirected_local_alloc),
    entry!("combase.dll", "CoTaskMemAlloc", redirected_co_task_mem_alloc),
    entry!("api-ms-win-core-com-l1-1-0.dll", "CoTaskMemAlloc", redirected_co_task_mem_alloc),
    entry!("WINHTTP.dll", "WinHttpOpen", redirected_win_http_open),
    entry!("WINHTTP.dll", "WinHttpConnect", redirected_win_http_connect),
    entry!("WINHTTP.dll", "WinHttpOpenRequest", redirected_win_http_open_request),
    entry!("WINHTTP.dll", "WinHttpCloseHandle", redirected_win_http_close_handle),
    entry!("WINHTTP.dll", "WinHttpReceiveResponse", redirected_win_http_receive_response),
    entry!("WINHTTP.dll", "WinHttpSetOption", redirected_win_http_set_option),
    entry!("api-ms-win-core-com-l1-1-1.dll", "CoCreateGuid", redirected_co_create_guid),
    entry!("WINHTTP.dll", "WinHttpWebSocketCompleteUpgrade", redirected_win_http_web_socket_complete_upgrade),
    entry!("kernelbase.dll", "OutputDebugStringA", redirected_output_debug_string_a),
    entry!("kernelbase.dll", "OutputDebugStringW", redirected_output_debug_string_w),
    entry!("api-ms-win-core-debug-l1-1-0.dll", "OutputDebugStringA", redirected_output_debug_string_a),
    entry!("api-ms-win-core-debug-l1-1-0.dll", "OutputDebugStringW", redirected_output_debug_string_w),
    entry!("api-ms-win-core-heap-l1-1-0.dll", "GetProcessHeap", redirected_get_process_heap),
];

#[no_mangle]
#[allow(non_upper_case_globals)]
pub static __RedirectionInformation__: RedirectionInformation = RedirectionInformation { version: 1, count: 74, entries: REDIRECTION_TABLE.as_ptr() };

#[no_mangle]
pub extern "system" fn XbRegisterXMemCallback(callback: usize) {
    XMEM_CALLBACK.store(callback, Ordering::Release);
}

#[no_mangle]
pub extern "system" fn XbRegisterXMemOperationCallback(callback: usize) {
    XMEM_OPERATION_CALLBACK.store(callback, Ordering::Release);
}

#[no_mangle]
pub extern "system" fn XbSetCallHeapFreeCallbackForRealloc(enable: u32) {
    REALLOC_FREE_CALLBACK.store(enable, Ordering::Release);
}

#[cfg(test)]
#[path = "redirect_tests.rs"]
mod tests;
