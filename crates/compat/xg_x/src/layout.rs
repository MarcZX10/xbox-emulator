pub const MODE_LINEAR_ALIGNED: u32 = 8;
pub const MODE_1D_THIN: u32 = 13;
pub const MODE_2D_THIN: u32 = 14;
pub const MODE_LINEAR_GENERAL: u32 = 31;
pub const MAX_MIPS: usize = 15;
pub const MIP_STRIDE: usize = 0x60;
pub const PLANE_STRIDE: usize = 0x20 + MAX_MIPS * MIP_STRIDE;
pub const MAX_PLANES: usize = 4;
pub const DIMENSION_OFFSET: usize = 0x20 + MAX_PLANES * PLANE_STRIDE + 0x18;
pub const LAYOUT_BYTES: usize = DIMENSION_OFFSET + 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub bpe: u32,
    pub block: u32,
}

pub fn format(dxgi: u32) -> Option<Format> {
    let f = |bpe, block| Some(Format { bpe, block });
    match dxgi {
        1..=4 => f(16, 1),
        5..=8 => f(12, 1),
        9..=22 => f(8, 1),
        23..=47 => f(4, 1),
        48..=59 => f(2, 1),
        60..=65 => f(1, 1),
        67 => f(4, 1),
        70..=72 | 79..=81 => f(8, 4),
        73..=78 | 82..=84 | 94..=99 => f(16, 4),
        85 | 86 => f(2, 1),
        87..=93 => f(4, 1),
        115 => f(2, 1),
        116 | 117 => f(4, 1),
        119 => f(4, 1),
        _ => None,
    }
}

fn al(x: u64, a: u64) -> u64 {
    x.div_ceil(a) * a
}

fn p2(x: u64) -> u64 {
    x.max(1).next_power_of_two()
}

fn macro_tile(bpe: u32) -> (u64, u64) {
    match bpe {
        1 => (128, 64),
        2 => (64, 64),
        _ => (64, 32),
    }
}

pub fn base_mode(mode: u32) -> Option<u32> {
    match mode {
        8 => Some(MODE_LINEAR_ALIGNED),
        31 => Some(MODE_LINEAR_GENERAL),
        5 | 9 | 13 | 27 => Some(MODE_1D_THIN),
        0..=4 | 6 | 7 | 10..=12 | 14..=26 | 28..=30 => Some(MODE_2D_THIN),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mip {
    pub size: u64,
    pub offset: u64,
    pub slice: u64,
    pub pitch_pixels: u32,
    pub pitch_bytes: u32,
    pub align: u32,
    pub padded_w: u32,
    pub padded_h: u32,
    pub padded_d: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub samples: u32,
    pub mode: u32,
    pub storage: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
    pub fmt: Format,
    pub dxgi: u32,
    pub dimension: u32,
    pub mips: Vec<Mip>,
    pub array: u32,
    pub is_3d: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Desc {
    pub dimension: u32,
    pub width: u64,
    pub height: u32,
    pub depth_or_array: u32,
    pub mips: u32,
    pub dxgi: u32,
    pub samples: u32,
    pub mode: u32,
}

pub fn compute(d: &Desc) -> Option<Layout> {
    let fmt = format(d.dxgi)?;
    let mode = base_mode(d.mode)?;
    if fmt.block > 1 && (mode == MODE_LINEAR_ALIGNED || mode == MODE_LINEAR_GENERAL) {
        return None;
    }
    if d.width == 0 || d.height == 0 || d.depth_or_array == 0 || d.width > 16384 || d.height > 16384 {
        return None;
    }
    let is_3d = d.dimension == 4;
    let (w, h) = (d.width, d.height as u64);
    let dz = d.depth_or_array as u64;
    let mut full = 1u32;
    while (w >> full) != 0 || (h >> full) != 0 || (is_3d && (dz >> full) != 0) {
        full += 1;
    }
    let n = match (d.mips, d.mips & 0xf) {
        (0, _) => full,
        (_, 0) => return None,
        (_, m) => m,
    };
    if n as usize > MAX_MIPS {
        return None;
    }
    let bd = fmt.block as u64;
    let bpe = fmt.bpe as u64;
    let samples = d.samples.max(1) as u64;
    let mut mips = Vec::with_capacity(n as usize);
    let (mut acc, mut balign, mut prevp) = (0u64, 0u64, 0u64);
    for m in 0..n as u64 {
        let (wm, hm) = if n > 1 { (p2((w >> m).max(1)), p2((h >> m).max(1))) } else { (w, h) };
        let zm = if is_3d { (dz >> m).max(1) } else { dz };
        let pz = if n > 1 { p2(zm) } else { zm };
        let mut ew = wm.div_ceil(bd);
        let eh = hm.div_ceil(bd);
        let uw = (w >> m).max(1).div_ceil(bd);
        let uh = (h >> m).max(1).div_ceil(bd);
        if m > 0 {
            ew = ew.max(prevp / 2);
        }
        let mut md = mode;
        let (mw, mh) = macro_tile(fmt.bpe);
        if md == MODE_2D_THIN && m > 0 && (ew < mw || eh < mh) {
            md = MODE_1D_THIN;
        }
        let (pw, ph, a) = match md {
            MODE_2D_THIN => (al(ew, mw), al(eh, mh), mw * mh * bpe * samples),
            MODE_1D_THIN => {
                let (mut pw, ph) = (al(ew, 8), al(eh, 8));
                while (pw * ph * bpe * samples) % 256 != 0 {
                    pw += 8;
                }
                (pw, ph, 256)
            }
            MODE_LINEAR_ALIGNED => {
                let st = 8.max(64 / bpe);
                let (mut pw, ph) = (al(ew, st), eh);
                while (pw * ph * bpe) % 256 != 0 || (pw * ph) % 64 != 0 {
                    pw += st;
                }
                (pw, ph, 256)
            }
            _ => (ew, eh, bpe),
        };
        let slice = pw * ph * bpe * samples;
        let size = slice * pz;
        acc = al(acc, a);
        let offset = if md == MODE_LINEAR_GENERAL { acc & !0xff } else { acc };
        mips.push(Mip {
            size,
            offset,
            slice,
            pitch_pixels: (pw * bd) as u32,
            pitch_bytes: (pw * bpe) as u32,
            align: a as u32,
            padded_w: pw as u32,
            padded_h: ph as u32,
            padded_d: pz as u32,
            width: uw as u32,
            height: uh as u32,
            depth: zm as u32,
            samples: samples as u32,
            mode: md,
            storage: acc as u32,
        });
        acc += size;
        balign = balign.max(a);
        prevp = pw;
    }
    for (i, m) in mips.iter_mut().enumerate() {
        m.storage = i as u32;
    }
    Some(Layout { size: acc, align: balign, fmt, dxgi: d.dxgi, dimension: d.dimension, mips, array: d.depth_or_array, is_3d })
}

fn storage_offset(l: &Layout, mip: usize) -> u64 {
    let mut acc = 0u64;
    for (i, m) in l.mips.iter().enumerate() {
        acc = al(acc, m.align as u64);
        if i == mip {
            return acc;
        }
        acc += m.size;
    }
    acc
}

pub fn element_offset(l: &Layout, mip: usize, x: u64, y: u64, z: u64) -> Option<u64> {
    let m = l.mips.get(mip)?;
    if x >= m.padded_w as u64 || y >= m.padded_h as u64 || z >= m.padded_d as u64 {
        return None;
    }
    let bpe = l.fmt.bpe as u64;
    let base = storage_offset(l, mip) + z * m.slice;
    let pw = m.padded_w as u64;
    let within = match m.mode {
        MODE_LINEAR_GENERAL | MODE_LINEAR_ALIGNED => (y * pw + x) * bpe,
        MODE_1D_THIN => {
            let tile = (y / 8) * (pw / 8) + x / 8;
            tile * 64 * bpe + zorder(x, y) * bpe
        }
        _ => {
            let (mw, mh) = macro_tile(l.fmt.bpe);
            let macro_index = (y / mh) * (pw / mw) + x / mw;
            let (lx, ly) = (x % mw, y % mh);
            let micro = (ly / 8) * (mw / 8) + lx / 8;
            macro_index * mw * mh * bpe + micro * 64 * bpe + zorder(x, y) * bpe
        }
    };
    Some(base + within)
}

fn zorder(x: u64, y: u64) -> u64 {
    (x & 1) | ((y & 1) << 1) | ((x & 2) << 1) | ((y & 2) << 2) | ((x & 4) << 2) | ((y & 4) << 3)
}

pub fn write_layout(l: &Layout, out: &mut [u8]) {
    let n = LAYOUT_BYTES.min(out.len());
    out[..n].fill(0);
    let put32 = |o: &mut [u8], at: usize, v: u32| o[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let put64 = |o: &mut [u8], at: usize, v: u64| o[at..at + 8].copy_from_slice(&v.to_le_bytes());
    put64(out, 0, l.size);
    put64(out, 8, l.align);
    put32(out, 0x10, l.mips.len() as u32);
    put32(out, 0x14, 1);
    put32(out, 0x18, 1);
    put64(out, 0x20, l.size);
    put64(out, 0x28, 0);
    put64(out, 0x30, l.align);
    put32(out, 0x38, l.fmt.bpe);
    if out.len() >= DIMENSION_OFFSET + 4 {
        put32(out, DIMENSION_OFFSET, l.dimension);
    }
    for (i, m) in l.mips.iter().enumerate() {
        let b = 0x40 + i * MIP_STRIDE;
        put64(out, b, m.size);
        put64(out, b + 8, m.offset);
        put64(out, b + 0x10, m.slice);
        let words = [m.pitch_pixels, m.pitch_bytes, m.align, m.padded_w, m.padded_h, m.padded_d, m.width, m.height, m.depth, m.samples, m.mode];
        for (j, v) in words.iter().enumerate() {
            put32(out, b + 0x18 + j * 4, *v);
        }
        if m.mode == MODE_2D_THIN {
            put64(out, b + 0x48, 0x1c00);
            put64(out, b + 0x50, 0xc00);
        }
        put32(out, b + 0x58, 1);
    }
}

pub fn mip_offset(l: &Layout, mip: usize) -> Option<u64> {
    l.mips.get(mip).map(|_| storage_offset(l, mip))
}
