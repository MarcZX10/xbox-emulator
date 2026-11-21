use compat_common::{record, E_FAIL, E_NOTIMPL, S_OK};
use crate::layout::E_INVALIDARG;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileShape {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

fn fail(hr: i32, why: &str) -> i32 {
    record(crate::M, "XGComputeTileShape", why);
    hr
}

pub fn compute(dim: u32, format: u32, bytes_per_element: u32, samples: u32, texels: Option<&mut TileShape>, elements: Option<&mut TileShape>) -> i32 {
    if dim != 3 && samples != 1 {
        return fail(E_INVALIDARG, "SampleCount must be 1 for resources dimensions other than Texture2D");
    }
    let info = if format == 0 {
        None
    } else {
        match TILE_INFO.get(format as usize) {
            Some(i) => Some(*i),
            None => return fail(E_INVALIDARG, &format!("format {format} outside the console format table")),
        }
    };
    let (mut w, mut h) = (0u32, 0u32);
    if dim == 1 {
        if format != 0 {
            return fail(E_INVALIDARG, "Format must be XG_FORMAT_UNKNOWN for buffers");
        }
        if bytes_per_element > 1 {
            return fail(E_INVALIDARG, "BytesPerElement must be 0 or 1 for buffers");
        }
        w = 0x10000;
        h = 1;
    } else {
        let bpe = match info {
            Some(i) => 1u32.wrapping_shl(i[0] as u32),
            None => bytes_per_element,
        };
        match dim {
            2 | 4 => return fail(E_NOTIMPL, "Texture1D and Texture3D is not implemented yet"),
            3 => {
                (w, h) = match bpe {
                    1 => (0x100, 0x100),
                    2 => (0x100, 0x80),
                    4 => (0x80, 0x80),
                    8 => (0x80, 0x40),
                    16 => (0x40, 0x40),
                    _ => {
                        if format != 0 {
                            return fail(E_FAIL, &format!("Could not compute tile shape for format {format}"));
                        }
                        return fail(E_INVALIDARG, &format!("BytesPerElement has an invalid value ({bpe})"));
                    }
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
                    _ => return fail(E_INVALIDARG, &format!("Unsupported sample count {samples}")),
                }
            }
            _ => {}
        }
    }
    if let Some(e) = elements {
        *e = TileShape { width: w, height: h, depth: 1 };
    }
    if let Some(t) = texels {
        *t = match info {
            None => TileShape::default(),
            Some(i) => TileShape { width: (i[2] as u32 / i[1] as u32).wrapping_mul(w), height: (i[3] as u32).wrapping_mul(h), depth: 1 },
        };
    }
    S_OK
}

pub static TILE_INFO: [[u8; 4]; 193] = [[0, 1, 1, 1], [4, 1, 1, 1], [4, 1, 1, 1], [4, 1, 1, 1], [4, 1, 1, 1], [2, 3, 1, 1], [2, 3, 1, 1], [2, 3, 1, 1], [2, 3, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [3, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [2, 1, 1, 1], [2, 1, 2, 1], [2, 1, 2, 1], [3, 1, 4, 4], [3, 1, 4, 4], [3, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [3, 1, 4, 4], [3, 1, 4, 4], [3, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [1, 1, 1, 1], [1, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [2, 1, 1, 1], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [4, 1, 4, 4], [2, 1, 1, 1], [2, 1, 1, 1], [3, 1, 1, 1], [0, 1, 1, 1], [1, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [2, 1, 2, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [0, 1, 1, 1], [2, 1, 1, 1], [1, 1, 1, 1], [1, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [2, 1, 1, 1], [0, 1, 1, 1], [2, 1, 1, 1], [0, 1, 1, 1]];
