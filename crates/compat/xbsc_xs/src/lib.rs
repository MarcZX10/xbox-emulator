pub mod alloc;
pub mod compress;
pub mod console;
pub mod getters;
pub mod result;

use alloc::XbscAllocator;
use core::ffi::c_char;
use result::{Fail, Handle, TAG_COMPRESSED, TAG_DECOMPRESSED};

#[no_mangle]
pub unsafe extern "system" fn XBSC_CompressBuffer(src: *const u8, size: usize, alloc: *const XbscAllocator) -> *mut Handle {
    result::run(alloc, |a| {
        if src.is_null() && size != 0 {
            return Err(Fail::msg(compress::E_ZCOMP));
        }
        let s = if size == 0 { &[][..] } else { core::slice::from_raw_parts(src, size) };
        compress::compress(a, s)
    })
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetCompressedBuffer(h: *const Handle, data: *mut *const u8, size: *mut usize) -> u32 {
    Handle::buffer(h, TAG_COMPRESSED, data, size)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_DecompressBuffer(src: *const u8, size: usize, alloc: *const XbscAllocator) -> *mut Handle {
    result::run(alloc, |a| compress::decompress_buffer(a, src, size))
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetDecompressedBuffer(h: *const Handle, data: *mut *const u8, size: *mut usize) -> u32 {
    Handle::buffer(h, TAG_DECOMPRESSED, data, size)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_DestroyResult(h: *mut Handle) {
    if console::is_console_handle(h) {
        console::call("XBSC_DestroyResult", &[h as usize]);
        return;
    }
    Handle::destroy(h)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetResultCode(h: *const Handle) -> u32 {
    Handle::code(h)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetErrorMessage(h: *const Handle) -> *const c_char {
    if console::is_console_handle(h) {
        if let Some(m) = console::call("XBSC_GetErrorMessage", &[h as usize]) {
            return m as *const c_char;
        }
    }
    Handle::message(h)
}

pub const E_ASSEMBLY: &str = "Assembly is not supported with xbox console dlls";

#[no_mangle]
pub unsafe extern "system" fn XBSC_AssembleShader(_src: *const u8, _size: usize, opts: *const usize, alloc: *const XbscAllocator) -> *mut Handle {
    let a = alloc::Allocator::from_desc(alloc);
    if a.is_null() {
        return core::ptr::null_mut();
    }
    let h = if opts.is_null() { core::ptr::null_mut() } else { Handle::error(a, E_ASSEMBLY) };
    alloc::Allocator::release(a);
    h
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_DeserializePipelineCompileInput(_data: *const u8, _size: usize, _alloc: *const XbscAllocator) -> i32 {
    -1
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_console;
