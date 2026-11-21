use crate::alloc::Allocator;
use crate::result::{Buf, Fail, Handle, TAG_COMPRESSED, TAG_DECOMPRESSED};
use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

pub const MAGIC: u32 = 0x4843_4258;
pub const VERSION_STORED: u8 = 2;
pub const VERSION_ZLIB: u8 = 3;
pub const HEADER: usize = 0x18;
pub const MIN_INPUT: usize = 0x20;
pub const ZLIB_LEVEL: u8 = 6;

pub const E_ALLOC: &str = "Could not allocate memory for compressed buffer";
pub const E_ZCOMP: &str = "Failed to zlib compress buffer";
pub const E_INVALID: &str = "Decompress Failed: Input buffer is invalid.";
pub const E_SMALL: &str = "Decompress Failed: Input buffer is too small.";
pub const E_MAGIC: &str = "Decompress Failed: Input is not an xbsc compressed buffer.";
pub const E_OLD: &str = "Decompress Failed: Input is not a supported version.";
pub const E_TRUNC: &str = "Decompress Failed: Input is not a complete xbsc compressed buffer.";
pub const E_UNSUPPORTED: &str = "Unsupported compressed buffer version";
pub const E_ZDECOMP: &str = "Failed to zlib decompress buffer";
pub const E_SIZE: &str = "Decompressed size does not match original input";

pub fn deflate_bound(n: usize) -> usize {
    n + (n >> 12) + (n >> 14) + (n >> 25) + 13
}

pub fn zlib_inflate(src: &[u8], out: &mut [u8]) -> Result<usize, Fail> {
    let mut d = DecompressorOxide::new();
    let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
        | inflate_flags::TINFL_FLAG_COMPUTE_ADLER32
        | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let (st, _, n) = decompress(&mut d, src, out, 0, flags);
    match st {
        TINFLStatus::Done => Ok(n),
        _ => Err(Fail::msg(E_ZDECOMP)),
    }
}

pub unsafe fn compress(a: *mut Allocator, src: &[u8]) -> Result<*mut Handle, Fail> {
    let mut tmp = Buf::new(a, deflate_bound(src.len()), E_ALLOC)?;
    let z = miniz_oxide::deflate::compress_to_vec_zlib(src, ZLIB_LEVEL);
    if z.len() > tmp.len {
        return Err(Fail::msg(E_ZCOMP));
    }
    tmp.as_mut()[..z.len()].copy_from_slice(&z);
    let cap = if z.is_empty() { 0x20 } else { z.len() + 0x1f };
    let mut out = Buf::new(a, cap, E_ALLOC)?;
    let o = out.as_mut();
    o[..HEADER].fill(0);
    o[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    o[4] = VERSION_ZLIB;
    o[8..16].copy_from_slice(&(z.len() as u64).to_le_bytes());
    o[16..24].copy_from_slice(&(src.len() as u64).to_le_bytes());
    o[HEADER..HEADER + z.len()].copy_from_slice(&tmp.as_mut()[..z.len()]);
    drop(tmp);
    let h = Handle::with_buffer(a, TAG_COMPRESSED, out, z.len() + HEADER);
    if h.is_null() {
        return Err(Fail::BadAlloc);
    }
    Ok(h)
}

pub unsafe fn decompress_buffer(a: *mut Allocator, src: *const u8, len: usize) -> Result<*mut Handle, Fail> {
    if src.is_null() {
        return Err(Fail::msg(E_INVALID));
    }
    if len < MIN_INPUT {
        return Err(Fail::msg(E_SMALL));
    }
    let s = core::slice::from_raw_parts(src, len);
    if u32::from_le_bytes(s[0..4].try_into().unwrap()) != MAGIC {
        return Err(Fail::msg(E_MAGIC));
    }
    let ver = s[4];
    if ver < VERSION_STORED {
        return Err(Fail::msg(E_OLD));
    }
    let clen = u64::from_le_bytes(s[8..16].try_into().unwrap()) as usize;
    if clen.checked_add(HEADER).map_or(true, |t| t > len) {
        return Err(Fail::msg(E_TRUNC));
    }
    let body = &s[HEADER..HEADER + clen];
    let (buf, n) = match ver {
        VERSION_STORED => {
            let mut b = Buf::new(a, clen, E_ALLOC)?;
            b.as_mut().copy_from_slice(body);
            (b, clen)
        }
        VERSION_ZLIB => {
            let orig = u64::from_le_bytes(s[16..24].try_into().unwrap()) as usize;
            let mut b = Buf::new(a, orig, E_ALLOC)?;
            let got = zlib_inflate(body, b.as_mut())?;
            if got != orig {
                return Err(Fail::msg(E_SIZE));
            }
            (b, orig)
        }
        _ => return Err(Fail::msg(E_UNSUPPORTED)),
    };
    let h = Handle::with_buffer(a, TAG_DECOMPRESSED, buf, n);
    if h.is_null() {
        return Err(Fail::BadAlloc);
    }
    Ok(h)
}
