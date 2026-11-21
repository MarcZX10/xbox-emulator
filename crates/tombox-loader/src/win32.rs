//! Minimal hand-declared Win32 bindings used by the compatibility loader.
//! Declared directly (no external crate) so the FFI surface stays small and
//! auditable. Windows x64 only.

#![allow(non_camel_case_types)]

use core::ffi::c_void;

pub type HMODULE = *mut c_void;
pub type HANDLE = *mut c_void;
pub type FARPROC = *mut c_void;

// Memory allocation types / protections.
pub const MEM_COMMIT: u32 = 0x0000_1000;
pub const MEM_RESERVE: u32 = 0x0000_2000;
pub const MEM_RELEASE: u32 = 0x0000_8000;

pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;

extern "system" {
    pub fn VirtualAlloc(
        lpAddress: *mut c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut c_void;

    pub fn VirtualFree(lpAddress: *mut c_void, dwSize: usize, dwFreeType: u32) -> i32;

    pub fn VirtualProtect(
        lpAddress: *mut c_void,
        dwSize: usize,
        flNewProtect: u32,
        lpflOldProtect: *mut u32,
    ) -> i32;

    pub fn LoadLibraryA(lpLibFileName: *const u8) -> HMODULE;

    pub fn GetProcAddress(hModule: HMODULE, lpProcName: *const u8) -> FARPROC;

    pub fn GetCurrentProcess() -> HANDLE;

    pub fn FlushInstructionCache(
        hProcess: HANDLE,
        lpBaseAddress: *const c_void,
        dwSize: usize,
    ) -> i32;

    pub fn GetLastError() -> u32;

    // Registers x64 unwind (.pdata) info so SEH/stack unwinding works for the
    // mapped image. RUNTIME_FUNCTION entries are relative to BaseAddress.
    pub fn RtlAddFunctionTable(
        FunctionTable: *const RUNTIME_FUNCTION,
        EntryCount: u32,
        BaseAddress: u64,
    ) -> u8;
}

/// x64 RUNTIME_FUNCTION (12 bytes): three image-relative u32s.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RUNTIME_FUNCTION {
    pub begin_address: u32,
    pub end_address: u32,
    pub unwind_info: u32,
}
