use crate::computer::E_INVALIDARG;
use compat_common::{record, S_OK};

const E_FAIL: i32 = 0x8000_4005u32 as i32;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileShape {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

pub const GCN_SHAPE: [[u8; 4]; 64] = [[0, 1, 1, 1], [0, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [2, 1, 1, 1], [4, 1, 1, 1], [0, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [3, 1, 1, 1], [0, 1, 1, 1], [3, 1, 4, 4], [3, 1, 4, 4], [3, 1, 4, 4], [4, 1, 4, 4], [3, 1, 4, 4], [0, 1, 4, 4], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 2, 1], [2, 1, 2, 1], [2, 1, 1, 1], [3, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [3, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [0, 1, 1, 1], [1, 1, 1, 1], [0, 1, 8, 1], [0, 1, 8, 1], [2, 1, 4, 1], [2, 1, 2, 1], [2, 4, 1, 1]];

const GCN_SHAPE_256: [u8; 4] = [0, 0, 0, 82];

pub const DXGI_TO_GCN: [u16; 226] = [0, 14, 14, 14, 14, 13, 13, 13, 13, 12, 12, 12, 12, 12, 12, 11, 11, 11, 11, 22, 22, 22, 22, 9, 9, 9, 6, 10, 10, 10, 10, 10, 10, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 20, 20, 20, 20, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 59, 34, 32, 33, 35, 35, 35, 36, 36, 36, 37, 37, 37, 38, 38, 38, 39, 39, 39, 16, 17, 10, 10, 9, 10, 10, 10, 10, 40, 40, 40, 41, 41, 41, 10, 9, 12, 1, 2, 0, 0, 33, 0, 0, 0, 1, 1, 1, 3, 19, 30, 31, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 57, 20, 1, 256, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

const ZERO_PAGE_END: u32 = 268;

pub fn gcn_index(format: u32) -> Option<u32> {
    match format {
        f if (f as usize) < DXGI_TO_GCN.len() => Some(DXGI_TO_GCN[f as usize] as u32),
        f if f < ZERO_PAGE_END => Some(0),
        _ => None,
    }
}

pub fn gcn_shape(idx: u32) -> [u8; 4] {
    if idx == 256 {
        GCN_SHAPE_256
    } else {
        GCN_SHAPE[(idx & 63) as usize]
    }
}

fn fail(hr: i32, why: &str) -> i32 {
    record(crate::M, "XGComputeTileShape", why);
    hr
}

pub fn compute(dimension: u32, format: u32, bytes_per_element: u32, samples: u32, texels: Option<&mut TileShape>, elements: Option<&mut TileShape>) -> i32 {
    if dimension != 3 && samples != 1 {
        return fail(E_INVALIDARG, "SampleCount must be 1 for resources dimensions other than Texture2D.");
    }
    let mut idx = 0u32;
    let (mut w, mut h) = (0u32, 0u32);
    if dimension == 1 {
        if format != 0 {
            return fail(E_INVALIDARG, "Format must be XG_FORMAT_UNKNOWN for buffers.");
        }
        if bytes_per_element > 1 {
            return fail(E_INVALIDARG, "BytesPerElement must be 0 or 1 for buffers.");
        }
        w = 0x10000;
        h = 1;
    } else {
        let mut bpe = bytes_per_element;
        if format != 0 {
            idx = match gcn_index(format) {
                Some(i) => i,
                None => return fail(E_INVALIDARG, &format!("format {format} lies outside every console format table page")),
            };
            bpe = 1u32 << gcn_shape(idx)[0];
        }
        match dimension {
            2 | 4 => return fail(E_NOTIMPL, "Texture1D and Texture3D is not implemented yet."),
            3 => {
                (w, h) = match bpe {
                    1 => (0x100, 0x100),
                    2 => (0x100, 0x80),
                    4 => (0x80, 0x80),
                    8 => (0x80, 0x40),
                    16 => (0x40, 0x40),
                    _ if format != 0 => return fail(E_FAIL, &format!("Could not compute tile shape for format {format}.")),
                    _ => return fail(E_INVALIDARG, &format!("BytesPerElement has an invalid value ({bpe}); the value must be 1, 2, 4, 8, or 16.")),
                };
                match samples {
                    1 => {}
                    2 => h >>= 1,
                    4 => {
                        w >>= 1;
                        h >>= 1;
                    }
                    8 => {
                        w >>= 1;
                        h >>= 2;
                    }
                    _ => return fail(E_INVALIDARG, &format!("Unsupported sample count {samples}.  Sample count must be 1, 2, 4, or 8.")),
                }
            }
            _ => {}
        }
    }
    if let Some(e) = elements {
        *e = TileShape { width: w, height: h, depth: 1 };
    }
    if let Some(t) = texels {
        if format == 0 {
            *t = TileShape::default();
        } else {
            let s = gcn_shape(idx);
            if s[1] == 0 {
                return fail(E_INVALIDARG, &format!("format {format} has a zero block divisor; the console divides by zero here"));
            }
            *t = TileShape { width: (s[2] as u32 / s[1] as u32) * w, height: s[3] as u32 * h, depth: 1 };
        }
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XGComputeTileShape(dimension: u32, format: u32, bytes_per_element: u32, samples: u32, texels: *mut TileShape, elements: *mut TileShape) -> i32 {
    unsafe { compute(dimension, format, bytes_per_element, samples, texels.as_mut(), elements.as_mut()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dim: u32, fmt: u32, bpe: u32, s: u32) -> (i32, TileShape, TileShape) {
        let junk = TileShape { width: 0xaaaa, height: 0xbbbb, depth: 0xcccc };
        let (mut t, mut e) = (junk, junk);
        let hr = unsafe { XGComputeTileShape(dim, fmt, bpe, s, &mut t, &mut e) };
        (hr, t, e)
    }

    #[test]
    fn texture2d_shapes_match_console_tables() {
        assert_eq!(run(3, 28, 0, 1), (S_OK, TileShape { width: 128, height: 128, depth: 1 }, TileShape { width: 128, height: 128, depth: 1 }));
        assert_eq!(run(3, 71, 0, 1), (S_OK, TileShape { width: 512, height: 256, depth: 1 }, TileShape { width: 128, height: 64, depth: 1 }));
        assert_eq!(run(3, 2, 0, 8), (S_OK, TileShape { width: 32, height: 16, depth: 1 }, TileShape { width: 32, height: 16, depth: 1 }));
        assert_eq!(run(3, 0, 2, 4), (S_OK, TileShape::default(), TileShape { width: 128, height: 64, depth: 1 }));
    }

    #[test]
    fn buffer_shape_and_errors_match_console() {
        assert_eq!(run(1, 0, 1, 1), (S_OK, TileShape::default(), TileShape { width: 0x10000, height: 1, depth: 1 }));
        assert_eq!(run(1, 28, 0, 1).0, E_INVALIDARG);
        assert_eq!(run(1, 0, 2, 1).0, E_INVALIDARG);
        assert_eq!(run(2, 28, 0, 2).0, E_INVALIDARG);
        assert_eq!(run(2, 28, 0, 1).0, E_NOTIMPL);
        assert_eq!(run(4, 28, 0, 1).0, E_NOTIMPL);
        assert_eq!(run(3, 0, 3, 1).0, E_INVALIDARG);
        assert_eq!(run(3, 28, 0, 3).0, E_INVALIDARG);
        assert_eq!(run(3, 6, 0, 1), (S_OK, TileShape { width: 128, height: 128, depth: 1 }, TileShape { width: 128, height: 128, depth: 1 }));
        assert_eq!(run(3, 193, 0, 1).0, E_INVALIDARG);
        assert_eq!(run(3, 250, 0, 1), (S_OK, TileShape { width: 256, height: 256, depth: 1 }, TileShape { width: 256, height: 256, depth: 1 }));
        assert_eq!(run(0, 28, 0, 1), (S_OK, TileShape { width: 0, height: 0, depth: 1 }, TileShape { width: 0, height: 0, depth: 1 }));
        assert_eq!(unsafe { XGComputeTileShape(3, 28, 0, 1, core::ptr::null_mut(), core::ptr::null_mut()) }, S_OK);
    }
}
