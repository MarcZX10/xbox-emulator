use super::*;

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
    fn CloseHandle(h: Handle) -> i32;
    fn HeapAlloc(h: Handle, flags: u32, n: usize) -> *mut u8;
    fn HeapFree(h: Handle, flags: u32, p: *mut u8) -> i32;
    fn HeapDestroy(h: Handle) -> i32;
    fn GetProcessId(h: Handle) -> u32;
    fn GetCurrentProcessId() -> u32;
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[test]
fn create_partition_heap_validates_and_allocates() {
    unsafe {
        assert!(XbCreatePartitionHeap(3, 0, 0x10000).is_null());
        assert_eq!(GetLastError(), ERROR_INVALID_PARAMETER);
        if partition(2) == Partition::Missing {
            assert!(XbCreatePartitionHeap(2, 0, 0x10000).is_null());
            assert_eq!(GetLastError(), ERROR_PARTITION_NOT_FOUND);
        }
        let h = XbCreatePartitionHeap(1, 0, 0x10000);
        assert!(!h.is_null(), "error {}", GetLastError());
        let p = HeapAlloc(h, 0, 4096);
        assert!(!p.is_null());
        p.write_bytes(0xab, 4096);
        assert_eq!(HeapFree(h, 0, p), 1);
        HeapDestroy(h);
    }
    let p = segment_parameters(0x1234, 4);
    assert_eq!((p.version_size, p.memory_type_mask, p.numa_node, p.partition), (0x50_0003, 0x18, 0xffff_ffff, 0x1234));
    assert_eq!(segment_parameters(0x1234, 1).partition, 0);
    assert_eq!(segment_parameters(0x1234, 1).memory_type_mask, 2);
    assert_eq!(core::mem::size_of::<SegmentHeapParameters>(), 0x50);
}

#[test]
fn get_partition_handle_and_id_duplicates() {
    unsafe {
        let mut h: Handle = core::ptr::null_mut();
        let mut id = 9u32;
        assert_eq!(XbGetPartitionHandleAndId(3, &mut h, &mut id), 0);
        assert_eq!(GetLastError(), ERROR_INVALID_PARAMETER);
        assert_eq!(XbGetPartitionHandleAndId(1, &mut h, &mut id), 1);
        assert!(!h.is_null());
        if partition(1) == Partition::Process {
            assert_eq!(id, 0);
            assert_eq!(GetProcessId(h), GetCurrentProcessId());
        }
        assert_eq!(CloseHandle(h), 1);
        assert_eq!(XbGetPartitionHandleAndId(1, core::ptr::null_mut(), core::ptr::null_mut()), 1);
        if partition(2) == Partition::Missing {
            assert_eq!(XbGetPartitionHandleAndId(2, &mut h, &mut id), 0);
            assert_eq!(GetLastError(), ERROR_PARTITION_NOT_FOUND);
        }
    }
}

#[test]
fn get_partition_heap_is_created_once() {
    unsafe {
        assert!(XbGetPartitionHeap(7).is_null());
        assert_eq!(GetLastError(), ERROR_INVALID_PARAMETER);
        let a = XbGetPartitionHeap(1);
        assert!(!a.is_null(), "error {}", GetLastError());
        assert_eq!(XbGetPartitionHeap(1), a);
        let p = HeapAlloc(a, 0, 64);
        assert!(!p.is_null());
        assert_eq!(HeapFree(a, 0, p), 1);
    }
}

#[test]
fn report_unknown_nt_avm_error_records_fields() {
    XbReportUnknownNtAvmError(0xc000_0005u32 as i32, 0x1000, 0x2000, 0x30, 0x44, 5);
    assert_eq!(*LAST_AVM_ERROR.lock().unwrap(), Some((0xc000_0005u32 as i32, 0x1000, 0x2000, 0x30, 0x44, 5)));
}

#[test]
fn set_win_http_hooks_copies_table() {
    let _g = crate::TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let t = [1usize, 2, 3, 4, 5, 6, 7];
    unsafe { XbSetWinHttpHooks(&t) };
    assert_eq!(*WINHTTP_HOOKS.lock().unwrap(), t);
    let z = [0usize; 7];
    unsafe { XbSetWinHttpHooks(&z) };
    assert_eq!(*WINHTTP_HOOKS.lock().unwrap(), z);
}

#[test]
fn should_apply_redirection_to_function_matches_console_tables() {
    unsafe {
        let game = w("D:\\Games\\Title\\game.exe");
        let sys = w("C:\\Windows\\System32\\foo.dll");
        assert_eq!(__ShouldApplyRedirectionToFunction__(game.as_ptr(), 0x4a), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("D:\\x\\XMEM.DLL").as_ptr(), 2), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(game.as_ptr(), 2), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(sys.as_ptr(), 2), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(sys.as_ptr(), 0), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(game.as_ptr(), 0), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(game.as_ptr(), 4), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(sys.as_ptr(), 39), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("C:\\Windows\\System32\\UCRTBASE.dll").as_ptr(), 0), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("C:\\Windows\\System32\\ucrtbase.dll").as_ptr(), 2), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("scdxil.dll").as_ptr(), 2), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("ScDxil.DLL").as_ptr(), 0), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("C:\\pgo\\PGORT140.dll").as_ptr(), 67), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(game.as_ptr(), 67), 0);
        assert_eq!(__ShouldApplyRedirectionToFunction__(w("dbghelp.dll").as_ptr(), 73), 1);
        assert_eq!(__ShouldApplyRedirectionToFunction__(sys.as_ptr(), 73), 0);
    }
    assert_eq!(REDIRECT_FUNCTIONS[61], "WinHttpOpen");
}

#[test]
fn should_apply_redirection_is_true() {
    assert_eq!(__ShouldApplyRedirection__(), 1);
}
