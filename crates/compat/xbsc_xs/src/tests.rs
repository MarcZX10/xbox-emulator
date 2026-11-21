use super::*;
use crate::alloc::XbscAllocator;
use crate::compress::*;
use core::ffi::{c_void, CStr};
use std::sync::atomic::{AtomicIsize, Ordering};

struct Counter {
    live: AtomicIsize,
    total: AtomicIsize,
    fail_after: AtomicIsize,
}

unsafe extern "system" fn c_alloc(ctx: *mut c_void, n: usize) -> *mut c_void {
    let c = &*(ctx as *const Counter);
    let t = c.total.fetch_add(1, Ordering::SeqCst);
    let lim = c.fail_after.load(Ordering::SeqCst);
    if lim >= 0 && t >= lim {
        return core::ptr::null_mut();
    }
    c.live.fetch_add(1, Ordering::SeqCst);
    let b = vec![0u8; n.max(1) + 16].into_boxed_slice();
    let p = Box::into_raw(b) as *mut u8;
    (p as *mut usize).write(n.max(1) + 16);
    p.add(16) as *mut c_void
}

unsafe extern "system" fn c_free(ctx: *mut c_void, p: *mut c_void) {
    let c = &*(ctx as *const Counter);
    c.live.fetch_sub(1, Ordering::SeqCst);
    let base = (p as *mut u8).sub(16);
    let n = (base as *const usize).read();
    drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(base, n)));
}

fn counter(fail_after: isize) -> Box<Counter> {
    Box::new(Counter { live: AtomicIsize::new(0), total: AtomicIsize::new(0), fail_after: AtomicIsize::new(fail_after) })
}

fn desc(c: &Counter) -> XbscAllocator {
    XbscAllocator { alloc: Some(c_alloc), free: Some(c_free), ctx: c as *const Counter as *mut c_void }
}

unsafe fn msg(h: *const Handle) -> String {
    CStr::from_ptr(XBSC_GetErrorMessage(h)).to_string_lossy().into_owned()
}

unsafe fn decompress_err(bytes: &[u8]) -> String {
    let c = counter(-1);
    let d = desc(&c);
    let h = XBSC_DecompressBuffer(bytes.as_ptr(), bytes.len(), &d);
    assert!(!h.is_null());
    assert_eq!(XBSC_GetResultCode(h), 1);
    let m = msg(h);
    XBSC_DestroyResult(h);
    assert_eq!(c.live.load(Ordering::SeqCst), 0);
    m
}

fn header(ver: u8, clen: u64, orig: u64) -> Vec<u8> {
    let mut v = vec![0u8; HEADER];
    v[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    v[4] = ver;
    v[8..16].copy_from_slice(&clen.to_le_bytes());
    v[16..24].copy_from_slice(&orig.to_le_bytes());
    v
}

#[test]
fn compress_buffer_writes_xbch_v3_zlib_container() {
    unsafe {
        let c = counter(-1);
        let d = desc(&c);
        let src: Vec<u8> = (0..5000u32).map(|i| (i % 97) as u8).collect();
        let h = XBSC_CompressBuffer(src.as_ptr(), src.len(), &d);
        assert!(!h.is_null());
        assert_eq!(XBSC_GetResultCode(h), 0);
        let (mut p, mut n) = (core::ptr::null(), 0usize);
        assert_eq!(XBSC_GetCompressedBuffer(h, &mut p, &mut n), 0);
        let out = core::slice::from_raw_parts(p, n);
        assert_eq!(&out[0..4], b"XBCH");
        assert_eq!(out[4], 3);
        assert_eq!(&out[5..8], &[0, 0, 0]);
        let clen = u64::from_le_bytes(out[8..16].try_into().unwrap()) as usize;
        assert_eq!(clen + HEADER, n);
        assert_eq!(u64::from_le_bytes(out[16..24].try_into().unwrap()), 5000);
        assert_eq!(out[HEADER] & 0x0f, 8);
        let inflated = miniz_oxide::inflate::decompress_to_vec_zlib(&out[HEADER..]).unwrap();
        assert_eq!(inflated, src);
        XBSC_DestroyResult(h);
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn get_compressed_buffer_rejects_null_outputs_and_wrong_tag() {
    unsafe {
        let src = [1u8, 2, 3];
        let h = XBSC_CompressBuffer(src.as_ptr(), 3, core::ptr::null());
        let (mut p, mut n) = (core::ptr::null(), 0usize);
        assert_eq!(XBSC_GetCompressedBuffer(h, core::ptr::null_mut(), &mut n), 1);
        assert_eq!(XBSC_GetCompressedBuffer(h, &mut p, core::ptr::null_mut()), 1);
        assert_eq!(XBSC_GetCompressedBuffer(core::ptr::null(), &mut p, &mut n), 1);
        assert_eq!(XBSC_GetDecompressedBuffer(h, &mut p, &mut n), 1);
        assert_eq!(XBSC_GetCompressedBuffer(h, &mut p, &mut n), 0);
        XBSC_DestroyResult(h);
    }
}

#[test]
fn decompress_buffer_round_trips_zlib_and_stored() {
    unsafe {
        let c = counter(-1);
        let d = desc(&c);
        let src = b"xbsc compressed buffer round trip, xbsc compressed buffer round trip".to_vec();
        let h = XBSC_CompressBuffer(src.as_ptr(), src.len(), &d);
        let (mut p, mut n) = (core::ptr::null(), 0usize);
        XBSC_GetCompressedBuffer(h, &mut p, &mut n);
        let r = XBSC_DecompressBuffer(p, n, &d);
        assert_eq!(XBSC_GetResultCode(r), 0);
        let (mut q, mut m) = (core::ptr::null(), 0usize);
        assert_eq!(XBSC_GetDecompressedBuffer(r, &mut q, &mut m), 0);
        assert_eq!(core::slice::from_raw_parts(q, m), &src[..]);
        XBSC_DestroyResult(r);
        XBSC_DestroyResult(h);
        let mut stored = header(2, 9, 0);
        stored.extend_from_slice(b"raw bytes");
        let r = XBSC_DecompressBuffer(stored.as_ptr(), stored.len(), &d);
        assert_eq!(XBSC_GetResultCode(r), 0);
        XBSC_GetDecompressedBuffer(r, &mut q, &mut m);
        assert_eq!(core::slice::from_raw_parts(q, m), b"raw bytes");
        XBSC_DestroyResult(r);
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn get_decompressed_buffer_requires_decompressed_handle() {
    unsafe {
        let mut stored = header(2, 8, 0);
        stored.extend_from_slice(b"12345678");
        let r = XBSC_DecompressBuffer(stored.as_ptr(), stored.len(), core::ptr::null());
        let (mut q, mut m) = (core::ptr::null(), 0usize);
        assert_eq!(XBSC_GetCompressedBuffer(r, &mut q, &mut m), 1);
        assert_eq!(XBSC_GetDecompressedBuffer(r, core::ptr::null_mut(), &mut m), 1);
        assert_eq!(XBSC_GetDecompressedBuffer(r, &mut q, &mut m), 0);
        assert_eq!(m, 8);
        XBSC_DestroyResult(r);
    }
}

#[test]
fn decompress_buffer_reports_console_error_strings() {
    unsafe {
        let c = counter(-1);
        let d = desc(&c);
        let h = XBSC_DecompressBuffer(core::ptr::null(), 64, &d);
        assert_eq!(msg(h), E_INVALID);
        XBSC_DestroyResult(h);
        assert_eq!(decompress_err(&[0u8; 0x1f]), E_SMALL);
        let mut bad = header(3, 0, 0);
        bad[0] = b'Y';
        bad.resize(0x20, 0);
        assert_eq!(decompress_err(&bad), E_MAGIC);
        let mut old = header(1, 0, 0);
        old.resize(0x20, 0);
        assert_eq!(decompress_err(&old), E_OLD);
        let mut trunc = header(3, 0x100, 0x100);
        trunc.resize(0x40, 0);
        assert_eq!(decompress_err(&trunc), E_TRUNC);
        let mut newer = header(4, 8, 8);
        newer.resize(0x20, 0);
        assert_eq!(decompress_err(&newer), E_UNSUPPORTED);
        let z = miniz_oxide::deflate::compress_to_vec_zlib(b"abcdefgh", 6);
        let mut mism = header(3, z.len() as u64, 16);
        mism.extend_from_slice(&z);
        mism.resize(mism.len().max(0x20), 0);
        assert_eq!(decompress_err(&mism), E_SIZE);
        let mut short = header(3, z.len() as u64, 4);
        short.extend_from_slice(&z);
        short.resize(short.len().max(0x20), 0);
        assert_eq!(decompress_err(&short), E_ZDECOMP);
        let mut junk = header(3, 8, 8);
        junk.extend_from_slice(&[0xff; 8]);
        assert_eq!(decompress_err(&junk), E_ZDECOMP);
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn compress_buffer_allocation_failure_yields_error_handle() {
    unsafe {
        let c = counter(2);
        let d = desc(&c);
        let src = [7u8; 64];
        let h = XBSC_CompressBuffer(src.as_ptr(), src.len(), &d);
        assert!(h.is_null());
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
        let c = counter(-1);
        let d = desc(&c);
        let h = XBSC_CompressBuffer(core::ptr::null(), 10, &d);
        assert_eq!(msg(h), E_ZCOMP);
        XBSC_DestroyResult(h);
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn destroy_result_frees_every_allocation_and_accepts_null() {
    unsafe {
        XBSC_DestroyResult(core::ptr::null_mut());
        let c = counter(-1);
        let d = desc(&c);
        let h = XBSC_CompressBuffer(b"abc".as_ptr(), 3, &d);
        assert!(c.live.load(Ordering::SeqCst) >= 3);
        XBSC_DestroyResult(h);
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn get_result_code_is_one_for_null_and_errors() {
    unsafe {
        assert_eq!(XBSC_GetResultCode(core::ptr::null()), 1);
        let h = XBSC_DecompressBuffer(core::ptr::null(), 0, core::ptr::null());
        assert_eq!(XBSC_GetResultCode(h), 1);
        XBSC_DestroyResult(h);
        let h = XBSC_CompressBuffer(b"x".as_ptr(), 1, core::ptr::null());
        assert_eq!(XBSC_GetResultCode(h), 0);
        XBSC_DestroyResult(h);
    }
}

#[test]
fn get_error_message_returns_unknown_error_for_non_error_handles() {
    unsafe {
        assert_eq!(msg(core::ptr::null()), "unknown error");
        let h = XBSC_CompressBuffer(b"x".as_ptr(), 1, core::ptr::null());
        assert_eq!(msg(h), "unknown error");
        XBSC_DestroyResult(h);
        let h = XBSC_DecompressBuffer(b"short".as_ptr(), 5, core::ptr::null());
        assert_eq!(msg(h), E_SMALL);
        XBSC_DestroyResult(h);
        let c = counter(2);
        let d = desc(&c);
        let h = XBSC_DecompressBuffer(b"short".as_ptr(), 5, &d);
        assert!(!h.is_null());
        assert_eq!(XBSC_GetResultCode(h), 1);
        assert!(XBSC_GetErrorMessage(h).is_null());
        XBSC_DestroyResult(h);
        assert_eq!(c.live.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn allocator_with_one_callback_falls_back_to_process_heap() {
    unsafe {
        let c = counter(-1);
        let d = XbscAllocator { alloc: Some(c_alloc), free: None, ctx: &*c as *const Counter as *mut c_void };
        let h = XBSC_CompressBuffer(b"abc".as_ptr(), 3, &d);
        assert_eq!(XBSC_GetResultCode(h), 0);
        assert_eq!(c.total.load(Ordering::SeqCst), 0);
        XBSC_DestroyResult(h);
        let none = XbscAllocator { alloc: None, free: None, ctx: core::ptr::null_mut() };
        assert!(XBSC_CompressBuffer(b"abc".as_ptr(), 3, &none).is_null());
    }
}
