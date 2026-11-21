use compat_common::{record, E_FAIL, S_OK};
use crate::layout::{Fail, Tex2DDesc, E_INVALIDARG};

const W: &str = "XGValidateDescriptor";

fn bad(why: &str) -> bool {
    record(crate::M, W, why);
    false
}

fn swizzle_ok(sel: u32) -> bool {
    !(2..=3).contains(&sel)
}

fn image_ok(d: &[u32; 8], kind: u32) -> bool {
    let w1 = d[1];
    if w1 & 0xff != 0 {
        return bad("descriptor base address is not 256-byte aligned");
    }
    let f = w1 & 0x1ff0_0000;
    if f.wrapping_sub(0x07f0_0001) > 0x037f_fffe && f.wrapping_sub(1) > 0x04df_fffe && f.wrapping_sub(0x1080_0001) > 0x017f_fffe {
        return bad("image format is not valid");
    }
    let w3 = d[3];
    for sel in [w3 & 7, w3 >> 3 & 7, w3 >> 6 & 7, w3 >> 9 & 7] {
        if !swizzle_ok(sel) {
            return bad("destination swizzle is not valid");
        }
    }
    if (w3 >> 4 & 0xf000) < (w3 & 0xf000) {
        return bad("last level is less than base level");
    }
    let ty = (w3 >> 28).wrapping_sub(8);
    if kind < 8 {
        if ty > 1 {
            return bad("descriptor type is not valid for a 4-dword descriptor");
        }
        return true;
    }
    if d[7] & 0xff00_0000 != 0 {
        return bad("reserved descriptor bits are set");
    }
    let depth = d[4] & 0xffff;
    if depth < d[4] >> 16 {
        return false;
    }
    let width = (w1 >> 30).wrapping_add((d[2] & 0x3fff).wrapping_mul(4)).wrapping_add(1);
    let height = (d[2] >> 14 & 0xffff) + 1;
    if ty > 7 {
        return bad("descriptor type is not valid");
    }
    if depth + 1 > 0x2000 {
        return bad("depth is too large");
    }
    if width > 0x4000 {
        return bad("width is too large");
    }
    if height > 0x4000 {
        return bad("height is too large");
    }
    true
}

fn buffer_ok(d: &[u32; 8]) -> bool {
    let w3 = d[3];
    if !swizzle_ok(w3 & 7) || (w3 & 0x38).wrapping_sub(0x10) & !8 == 0 || (w3 & 0x1c0).wrapping_sub(0x80) & !0x40 == 0 || (w3 & 0xe00).wrapping_sub(0x400) & !0x200 == 0 {
        return bad("destination swizzle is not valid");
    }
    if (w3 & 0x7_f000).wrapping_sub(1) > 0x4_dffe {
        return bad("buffer format is not valid");
    }
    if d[1] & 0xffff > 0xff {
        return bad("buffer stride is too large");
    }
    true
}

pub fn validate(kind: u32, d: Option<&[u32; 8]>, flags: u32) -> i32 {
    if kind.wrapping_sub(4) & !4 != 0 {
        record(crate::M, W, "descriptor size must be 4 or 8 dwords");
        return E_INVALIDARG;
    }
    let d = match d {
        Some(d) => d,
        None => {
            record(crate::M, W, "descriptor pointer is null");
            return E_INVALIDARG;
        }
    };
    if flags != 0 {
        record(crate::M, W, "flags must be zero");
        return E_INVALIDARG;
    }
    let ok = if d[0] == 0 && d[1] == 0 && d[2] == 0 && d[3] == 0 {
        true
    } else if d[3] < 0x4000_0000 {
        buffer_ok(d)
    } else if d[3] >> 28 < 8 {
        bad("descriptor type is not valid")
    } else {
        image_ok(d, kind)
    };
    if ok { S_OK } else { E_FAIL }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrdDim {
    Tex1D,
    Tex2D,
    Tex3D,
}

#[derive(Clone, Copy, Debug)]
pub struct Srd {
    pub dim: SrdDim,
    pub hw_format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mips: u32,
    pub addr_mips: u32,
    pub samples: u32,
    pub swizzle_mode: u32,
}

pub fn parse_srd(d: &[u32; 8]) -> Result<Srd, Fail> {
    if d[3] >> 31 == 0 || d[1] & 0x1ff0_0000 == 0 {
        return Err(Fail::Invalid("Descriptor is not valid"));
    }
    let ty = d[3] >> 28;
    let dim = match ty {
        9 | 11 | 13 | 14 | 15 => SrdDim::Tex2D,
        8 | 12 => SrdDim::Tex1D,
        _ => SrdDim::Tex3D,
    };
    let msaa = ty == 14 || ty == 15;
    let last_level = d[3] >> 16 & 0xf;
    let addr_mips = if msaa && last_level != 0 { 1 } else { (d[5] >> 4 & 0xf) + 1 };
    let mips = if msaa { 1 } else { last_level + 1 };
    let samples = if msaa { 1u32 << last_level } else { 1 };
    Ok(Srd {
        dim,
        hw_format: d[1] >> 20 & 0x1ff,
        width: (d[1] >> 30).wrapping_add((d[2] & 0x3fff) * 4) + 1,
        height: (d[2] >> 14 & 0xffff) + 1,
        depth: (d[4] & 0xffff) + 1,
        mips,
        addr_mips,
        samples,
        swizzle_mode: d[3] >> 20 & 0x1f,
    })
}

pub(crate) fn representative_format(hw: u32) -> Option<u32> {
    let idx = *HW_FORMAT_INDEX.get(hw as usize)? as usize;
    let i = FORMAT_INFO[idx];
    let (log2, elems, bw, bh) = (i[1], i[2], i[3], i[4]);
    if elems != 1 {
        return None;
    }
    match (log2, bw, bh) {
        (0, 1, 1) => Some(61),
        (1, 1, 1) => Some(56),
        (2, 1, 1) => Some(28),
        (3, 1, 1) => Some(10),
        (4, 1, 1) => Some(2),
        (3, 4, 4) => Some(71),
        (4, 4, 4) => Some(77),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Element {
    pub bpe: u32,
    pub block: u32,
    pub xg_bw: u32,
    pub xg_bh: u32,
}

pub(crate) fn element(hw: u32) -> Option<Element> {
    let idx = HW_FORMAT_INDEX.get(hw as usize).copied().unwrap_or(0) as usize;
    let i = FORMAT_INFO[idx];
    let (bits, log2, elems, bw, bh) = (i[0] as u32, i[1] as u32, i[2] as u32, i[3] as u32, i[4] as u32);
    let bpe = 1u32 << log2;
    if elems != 1 {
        return None;
    }
    if bits == 0 || bits == 1 {
        return Some(Element { bpe, block: 1, xg_bw: bw, xg_bh: bh });
    }
    match (bw, bh) {
        (1, 1) => Some(Element { bpe, block: 1, xg_bw: 1, xg_bh: 1 }),
        (4, 4) => Some(Element { bpe, block: 4, xg_bw: 1, xg_bh: 1 }),
        _ => None,
    }
}

pub fn srd_to_tex2d(s: &Srd) -> Result<Tex2DDesc, Fail> {
    if s.addr_mips > 15 {
        return Err(Fail::Invalid("Failed to initialize color target interface"));
    }
    if s.dim != SrdDim::Tex2D {
        return Err(Fail::Unsupported("1D/3D descriptor"));
    }
    element(s.hw_format).ok_or(Fail::Unsupported("descriptor format"))?;
    let format = representative_format(s.hw_format).unwrap_or(0);
    let mode = s.swizzle_mode;
    Ok(Tex2DDesc {
        width: s.width,
        height: s.height,
        mip_levels: s.addr_mips,
        array_size: s.depth,
        format,
        sample_count: s.samples,
        sample_quality: 0,
        usage: if mode == 0 { 3 } else { 0 },
        bind_flags: 8,
        cpu_access: 0,
        misc_flags: if mode == 10 || (16..=19).contains(&mode) { 0x0800_0000 } else { 0 },
        swizzle_mode: mode,
        reserved: 0,
    })
}

pub static HW_FORMAT_INDEX: [u8; 288] = [0, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 8, 8, 8, 8, 8, 8, 9, 9, 9, 9, 9, 9, 10, 10, 10, 10, 10, 10, 11, 11, 11, 12, 12, 12, 12, 12, 12, 12, 13, 13, 13, 14, 14, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, 10, 31, 34, 16, 17, 18, 19, 57, 59, 60, 4, 20, 20, 21, 21, 22, 22, 32, 32, 32, 32, 33, 33, 33, 33, 0, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 35, 35, 36, 36, 37, 37, 38, 38, 39, 39, 40, 40, 41, 41, 64, 64, 65, 65, 66, 66, 67, 67, 68, 68, 69, 69, 69, 69, 69, 69, 69, 69, 69, 69, 69, 69, 69, 69, 70, 70, 70, 70, 70, 70, 70, 70, 70, 70, 70, 70, 70, 70, 71, 71, 71, 71, 71, 71, 71, 71, 71, 71, 71, 71, 71, 71, 72, 72, 72, 72, 72, 72, 72, 72, 72, 72, 73, 73, 73, 73, 73, 73, 73, 73, 73, 73, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 76, 76, 77, 77, 78, 78, 79, 79, 80, 80, 81, 81, 82, 82, 83, 83, 84, 84, 85, 85, 30, 29, 29];

pub static FORMAT_INFO: [[u8; 6]; 86] = [[0, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0], [96, 2, 3, 1, 1, 0], [128, 4, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [4, 3, 1, 4, 4, 0], [4, 3, 1, 4, 4, 0], [4, 3, 1, 4, 4, 0], [8, 4, 1, 4, 4, 0], [4, 3, 1, 4, 4, 0], [0, 0, 1, 4, 4, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [16, 2, 1, 2, 1, 0], [16, 2, 1, 2, 1, 0], [32, 2, 1, 1, 1, 0], [4, 3, 1, 4, 4, 8], [8, 4, 1, 4, 4, 16], [8, 4, 1, 4, 4, 16], [4, 3, 1, 4, 4, 8], [8, 4, 1, 4, 4, 16], [8, 4, 1, 4, 4, 16], [8, 4, 1, 4, 4, 16], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [1, 0, 1, 8, 1, 0], [1, 0, 1, 8, 1, 0], [8, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [128, 2, 4, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [0, 0, 1, 1, 1, 0], [8, 0, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0], [16, 1, 1, 1, 1, 0], [32, 2, 1, 1, 1, 0], [64, 3, 1, 1, 1, 0]];

pub fn layout_from_srd(d: &[u32; 8]) -> Result<crate::layout::Layout2D, Fail> {
    let s = parse_srd(d)?;
    let t = srd_to_tex2d(&s)?;
    let e = element(s.hw_format).ok_or(Fail::Unsupported("descriptor format"))?;
    let mut l = crate::layout::compute_descriptor(&t, e.bpe, e.block)?;
    if e.xg_bw != 1 || e.xg_bh != 1 {
        for m in l.mip.iter_mut().take(l.mips as usize) {
            m.w_e = m.w_px.div_ceil(e.xg_bw);
            m.h_e = m.h_px.div_ceil(e.xg_bh);
        }
    }
    if s.mips != l.mips {
        l.set_reported_mips(s.mips, e.xg_bw, e.xg_bh);
    }
    l.total_bits = 40;
    Ok(l)
}
