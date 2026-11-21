use crate::computer::E_INVALIDARG;
use crate::tileshape::DXGI_TO_GCN;
use compat_common::record;

const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
const CODEC: &str = "arguments pass the console checks, but the console's software DCC block codec (42 routines, ~25 KB at 0x168c0-0x1f1a4) is not ported -> E_NOTIMPL";

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DccParams {
    pub block_shift: u32,
    pub mode: u32,
}

pub fn setup_ok(format: i32, tile_mode: i32) -> bool {
    if format <= 0 || format > 0x78 || tile_mode >= 0x1f {
        return false;
    }
    if !(0..=0x1a).contains(&tile_mode) {
        return false;
    }
    let idx = DXGI_TO_GCN[format as usize] as u32;
    (1..=63).contains(&idx) && !matches!(idx, 15 | 22 | 23 | 42 | 43)
}

#[no_mangle]
pub unsafe extern "system" fn XGEncodeDCC(count: u32, src: *const u8, dst: *mut u8, meta: *mut u8, sizes: *mut u32, format: i32, tile_mode: i32, params: *const DccParams) -> i32 {
    let _ = (count, sizes);
    if params.is_null() || src.is_null() || dst.is_null() || meta.is_null() || !setup_ok(format, tile_mode) {
        return E_INVALIDARG;
    }
    record(crate::M, "XGEncodeDCC", CODEC);
    E_NOTIMPL
}

#[no_mangle]
pub unsafe extern "system" fn XGDecodeDCC(count: u32, src: *const u8, meta: *const u8, dst: *mut u8, format: i32, tile_mode: i32, params: *const DccParams) -> i32 {
    let _ = count;
    if params.is_null() || src.is_null() || dst.is_null() || meta.is_null() || !setup_ok(format, tile_mode) {
        return E_INVALIDARG;
    }
    record(crate::M, "XGDecodeDCC", CODEC);
    E_NOTIMPL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_rejects_like_console_then_reports_missing_codec() {
        let (src, mut dst, mut meta, p) = ([0u8; 256], [0u8; 256], [0u8; 1], DccParams::default());
        let mut e = |f: i32, m: i32, pp: *const DccParams| unsafe { XGEncodeDCC(1, src.as_ptr(), dst.as_mut_ptr(), meta.as_mut_ptr(), core::ptr::null_mut(), f, m, pp) };
        assert_eq!(e(28, 14, core::ptr::null()), E_INVALIDARG);
        assert_eq!(e(0, 14, &p), E_INVALIDARG);
        assert_eq!(e(0x79, 14, &p), E_INVALIDARG);
        assert_eq!(e(28, 0x1b, &p), E_INVALIDARG);
        assert_eq!(e(28, -1, &p), E_INVALIDARG);
        assert_eq!(e(19, 14, &p), E_INVALIDARG);
        assert_eq!(e(28, 14, &p), E_NOTIMPL);
        assert_eq!(unsafe { XGEncodeDCC(1, core::ptr::null(), dst.as_mut_ptr(), meta.as_mut_ptr(), core::ptr::null_mut(), 28, 14, &p) }, E_INVALIDARG);
    }

    #[test]
    fn decode_rejects_like_console_then_reports_missing_codec() {
        let (src, mut dst, meta, p) = ([0u8; 256], [0u8; 256], [0u8; 1], DccParams::default());
        let mut d = |f: i32, m: i32| unsafe { XGDecodeDCC(1, src.as_ptr(), meta.as_ptr(), dst.as_mut_ptr(), f, m, &p) };
        assert_eq!(d(105, 14), E_INVALIDARG);
        assert_eq!(d(2, 0x1e), E_INVALIDARG);
        assert_eq!(d(71, 14), E_NOTIMPL);
        assert_eq!(unsafe { XGDecodeDCC(1, src.as_ptr(), core::ptr::null(), dst.as_mut_ptr(), 71, 14, &p) }, E_INVALIDARG);
    }
}
