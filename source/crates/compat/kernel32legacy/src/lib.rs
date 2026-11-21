#![allow(non_snake_case)]

use compat_common::os_host::{affinity, k32legacy};
use core::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct GroupAffinity {
    pub mask: usize,
    pub group: u16,
    pub reserved: [u16; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ProcessorNumber {
    pub group: u16,
    pub number: u8,
    pub reserved: u8,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetNumaNodeProcessorMaskEx(node: u16, affinity: *mut GroupAffinity) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn SetLastError(code: u32);
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetCurrentProcessorNumberEx(number: *mut ProcessorNumber);
    fn RtlCreateQueryDebugBuffer(max_commit: u32, use_event_pair: u8) -> *mut c_void;
    fn RtlQueryProcessDebugInformation(pid: *mut c_void, flags: u32, buffer: *mut c_void) -> i32;
    fn RtlDestroyQueryDebugBuffer(buffer: *mut c_void) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

pub fn numa_mask_for_group(node: GroupAffinity, current: ProcessorNumber) -> u64 {
    if node.group == current.group {
        node.mask as u64
    } else {
        0
    }
}

pub const TH32CS_SNAPHEAPLIST: u32 = 0x1;
pub const TH32CS_SNAPMODULE: u32 = 0x8;
pub const TH32CS_SNAPMODULE32: u32 = 0x10;
pub const RTL_QUERY_PROCESS_HEAP_SUMMARY: u32 = 0x4;
const STATUS_UNSUCCESSFUL: i32 = 0xc000_0001u32 as i32;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

pub fn module_query_flags(flags: u32) -> u32 {
    ((flags & TH32CS_SNAPMODULE32) | 0xe000_0000).wrapping_shl(2) | ((flags >> 3) & 1)
}

unsafe fn console_debug_query(pid: u32, query: u32) -> i32 {
    let buf = RtlCreateQueryDebugBuffer(0, 0);
    if buf.is_null() {
        return STATUS_UNSUCCESSFUL;
    }
    let st = RtlQueryProcessDebugInformation(pid as i32 as isize as *mut c_void, query, buf);
    RtlDestroyQueryDebugBuffer(buf);
    st
}

pub unsafe fn console_snapshot_status(flags: u32, pid: u32) -> i32 {
    let pid = if pid == 0 { GetCurrentProcessId() } else { pid };
    if flags & (TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32) != 0 {
        let st = console_debug_query(pid, module_query_flags(flags));
        if st < 0 {
            return st;
        }
    }
    if flags & TH32CS_SNAPHEAPLIST != 0 {
        let st = console_debug_query(pid, RTL_QUERY_PROCESS_HEAP_SUMMARY);
        if st < 0 {
            return st;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn k32l_CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void {
    let st = console_snapshot_status(flags, pid);
    if st < 0 {
        SetLastError(RtlNtStatusToDosError(st));
        return INVALID_HANDLE_VALUE;
    }
    CreateToolhelp32Snapshot(flags, pid)
}

#[no_mangle]
pub unsafe extern "system" fn k32l_GetNumaNodeProcessorMask(node: u8, mask: *mut u64) -> i32 {
    let mut aff = GroupAffinity::default();
    if GetNumaNodeProcessorMaskEx(node as u16, &mut aff) == 0 {
        return 0;
    }
    let mut cur = ProcessorNumber::default();
    RtlGetCurrentProcessorNumberEx(&mut cur);
    *mask = affinity::topology().to_console(numa_mask_for_group(aff, cur));
    1
}

#[no_mangle]
pub unsafe extern "system" fn k32l_SetThreadAffinityMask(thread: *mut c_void, mask: usize) -> usize {
    k32legacy::set_thread_affinity_mask(thread, mask)
}

#[no_mangle]
pub unsafe extern "system" fn k32l_GetProcessAffinityMask(process: *mut c_void, process_mask: *mut usize, system_mask: *mut usize) -> i32 {
    k32legacy::get_process_affinity_mask(process, process_mask, system_mask)
}

#[no_mangle]
pub unsafe extern "system" fn k32l_SetProcessAffinityMask(process: *mut c_void, mask: usize) -> i32 {
    k32legacy::set_process_affinity_mask(process, mask)
}

#[no_mangle]
pub unsafe extern "system" fn k32l_SetDllDirectoryW(path: *const u16) -> i32 {
    k32legacy::set_dll_directory_w(path)
}

pub const EXPORTS_TXT: &str = include_str!(env!("K32L_EXPORTS"));
pub const FORWARDS_TXT: &str = include_str!(env!("K32L_FORWARDS"));
pub const DLL_FILE: &str = concat!(env!("CARGO_CRATE_NAME"), ".dll");
pub const CONSOLE_REFS: &str = env!("K32L_REFS");

pub fn forwards() -> Vec<(String, String, String)> {
    let fwd: Vec<String> = EXPORTS_TXT
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let _ord = w.next()?;
            let name = w.next()?;
            (w.next()? == "forwarded").then(|| name.to_string())
        })
        .collect();
    FORWARDS_TXT
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| l.split_once(' ').map_or(false, |(n, _)| fwd.iter().any(|f| f == n)))
        .map(|l| {
            let (n, t) = l.split_once(' ').unwrap();
            let (dll, f) = t.trim().split_once('.').unwrap();
            (n.to_string(), format!("{dll}.dll"), f.to_string())
        })
        .collect()
}
