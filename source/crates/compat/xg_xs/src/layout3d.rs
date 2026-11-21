use crate::equations::Eq;
use crate::equations3d::{Eq3, EQUATIONS_3D, EQUATIONS_3D_V4};
use crate::formats;
use crate::layout::{self, Fail, Layout2D, Tex2DDesc, LAYOUT_BYTES, MAX_MIPS, MIP_BASE, MIP_BYTES, PLANE_BASE};
use std::sync::atomic::Ordering;

const MIP_TAIL_256B: [u64; 16] = [2048, 1024, 512, 256, 128, 64, 32, 16, 8, 6, 5, 4, 3, 2, 1, 0];
const TAIL_3D: [(u32, u32); 10] = [(4, 0), (0, 4), (2, 0), (1, 2), (0, 3), (0, 2), (1, 1), (1, 0), (0, 1), (0, 0)];
const MICRO_3D: [(u32, u32, u32); 5] = [(8, 4, 8), (4, 4, 8), (4, 4, 4), (4, 2, 4), (2, 2, 4)];
const BLOCK_1K_3D: [(u32, u32, u32); 5] = [(16, 8, 8), (8, 8, 8), (8, 8, 4), (8, 4, 4), (4, 4, 4)];
const SIZE_LIMIT_BITS: u32 = 40;

pub fn is_thick(mode: u32) -> bool {
    matches!(mode, 5 | 9 | 17 | 21 | 25 | 26)
}

fn valid_3d_mode(mode: u32) -> bool {
    matches!(mode, 0 | 5 | 9 | 17 | 21 | 24 | 25 | 26 | 27 | 28 | 31)
}

fn valid_1d_mode(mode: u32) -> bool {
    matches!(mode, 0 | 24 | 27 | 28 | 31)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tex1DDesc {
    pub width: u32,
    pub mip_levels: u32,
    pub array_size: u32,
    pub format: u32,
    pub usage: u32,
    pub bind_flags: u32,
    pub cpu_access: u32,
    pub misc_flags: u32,
    pub swizzle_mode: u32,
    pub pitch: u32,
}

impl Tex1DDesc {
    pub unsafe fn read(p: *const u8) -> Tex1DDesc {
        let u = |o: usize| core::ptr::read_unaligned(p.add(o) as *const u32);
        Tex1DDesc {
            width: u(0),
            mip_levels: u(4),
            array_size: u(8),
            format: u(0xc),
            usage: u(0x10),
            bind_flags: u(0x14),
            cpu_access: u(0x18),
            misc_flags: u(0x1c),
            swizzle_mode: u(0x20),
            pitch: u(0x24),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tex3DDesc {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u32,
    pub format: u32,
    pub usage: u32,
    pub bind_flags: u32,
    pub cpu_access: u32,
    pub misc_flags: u32,
    pub swizzle_mode: u32,
    pub pitch: u32,
}

impl Tex3DDesc {
    pub unsafe fn read(p: *const u8) -> Tex3DDesc {
        let u = |o: usize| core::ptr::read_unaligned(p.add(o) as *const u32);
        Tex3DDesc {
            width: u(0),
            height: u(4),
            depth: u(8),
            mip_levels: u(0xc),
            format: u(0x10),
            usage: u(0x14),
            bind_flags: u(0x18),
            cpu_access: u(0x1c),
            misc_flags: u(0x20),
            swizzle_mode: u(0x24),
            pitch: u(0x28),
        }
    }
}

fn block_compressed(format: u32) -> bool {
    formats::lookup(format).map(|f| f.block != 1).unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
fn validate_common(mode: u32, usage: u32, bind: u32, misc: u32, pitch: u32, format: u32, array: u32, width: u32, height: u32, depth: u32) -> Result<(), Fail> {
    if width == 0 {
        return Err(Fail::Invalid("width is zero"));
    }
    if height == 0 {
        return Err(Fail::Invalid("height is zero"));
    }
    if depth == 0 {
        return Err(Fail::Invalid("depth or array size is zero"));
    }
    if usage == 3 && mode != 0 {
        return Err(Fail::Invalid("staging texture must be linear"));
    }
    if pitch != 0 && pitch < width {
        return Err(Fail::Invalid("pitch below width"));
    }
    if bind & 0xffff_f900 != 0 {
        return Err(Fail::Invalid("bind flags"));
    }
    if misc & 0xf385_6d0a != 0 {
        return Err(Fail::Invalid("misc flags"));
    }
    if block_compressed(format) && mode == 0 && usage != 3 {
        return Err(Fail::Invalid("linear block-compressed texture needs staging usage"));
    }
    if array == 0 {
        return Err(Fail::Invalid("array size is zero"));
    }
    if (5..=8).contains(&format) {
        return Err(Fail::Invalid("96-bit format"));
    }
    Ok(())
}

fn validate_misc(misc: u32) -> Result<(), Fail> {
    if misc & 0x0800_0000 != 0 {
        return Err(Fail::Invalid("misc bit 27 is 2D only"));
    }
    if misc & 0x0400_0000 != 0 {
        return Err(Fail::Invalid("misc bit 26 is 2D only"));
    }
    if misc & 0x0040_0000 != 0 {
        return Err(Fail::Invalid("misc bit 22 is 2D only"));
    }
    Ok(())
}

fn lookup_format(format: u32) -> Result<&'static formats::Format, Fail> {
    match formats::lookup(format) {
        Some(f) => Ok(f),
        None if formats::accepted_elsewhere(format) => Err(Fail::Unsupported("format")),
        None => Err(Fail::Invalid("format")),
    }
}

pub fn compute_1d_srd(d: &Tex1DDesc, height: u32, bpe: u32, block: u32) -> Result<Layout2D, Fail> {
    let mode = d.swizzle_mode & 31;
    if d.mip_levels as usize > MAX_MIPS {
        return Err(Fail::Invalid("mip count"));
    }
    if mode == 0 && height > block.max(1) {
        return Err(Fail::Invalid("linear 1D descriptor taller than one element row"));
    }
    if !valid_1d_mode(mode) {
        return Err(Fail::Invalid("swizzle mode not valid for 1D"));
    }
    let t = Tex2DDesc {
        width: d.width,
        height: height.max(1),
        mip_levels: d.mip_levels,
        array_size: d.array_size,
        format: d.format,
        sample_count: 1,
        sample_quality: 0,
        usage: if mode == 0 { 3 } else { 0 },
        bind_flags: 8,
        cpu_access: 0,
        misc_flags: 0,
        swizzle_mode: mode,
        reserved: 0,
    };
    layout::ALLOW_VAR_19041.with(|c| c.set(true));
    let r = layout::compute_descriptor(&t, bpe, block);
    layout::ALLOW_VAR_19041.with(|c| c.set(false));
    r
}

pub fn compute_1d(d: &Tex1DDesc) -> Result<Layout2D, Fail> {
    let mode = d.swizzle_mode & 31;
    validate_common(d.swizzle_mode, d.usage, d.bind_flags, d.misc_flags, d.pitch, d.format, d.array_size, d.width, 1, d.array_size)?;
    validate_misc(d.misc_flags)?;
    if block_compressed(d.format) {
        return Err(Fail::Invalid("1D block-compressed format"));
    }
    if d.mip_levels as usize > MAX_MIPS {
        return Err(Fail::Invalid("mip count"));
    }
    lookup_format(d.format)?;
    if !valid_1d_mode(mode) {
        return Err(Fail::Invalid("swizzle mode not valid for 1D"));
    }
    let t = Tex2DDesc {
        width: d.width,
        height: 1,
        mip_levels: d.mip_levels,
        array_size: d.array_size,
        format: d.format,
        sample_count: 1,
        sample_quality: 0,
        usage: if mode == 0 { 3 } else { 0 },
        bind_flags: 8,
        cpu_access: 0,
        misc_flags: 0,
        swizzle_mode: mode,
        reserved: 0,
    };
    let l = layout::compute(&t)?;
    if l.total >> SIZE_LIMIT_BITS != 0 {
        return Err(Fail::Unsupported("resource size above 40 bits"));
    }
    Ok(l)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mip3 {
    pub size: u64,
    pub offset: u64,
    pub slice: u64,
    pub pad_w: u32,
    pub pad_h: u32,
    pub pad_d: u32,
    pub w_e: u32,
    pub h_e: u32,
    pub w_px: u32,
    pub h_px: u32,
    pub d_px: u32,
    pub align: u32,
    pub mode: u32,
    pub in_tail: bool,
    pub tail_x: u32,
    pub tail_y: u32,
    pub base_w: u32,
    pub base_h: u32,
    pub base_d: u32,
    pub synthetic: bool,
}

#[derive(Clone, Debug)]
pub struct Layout3D {
    pub desc: Tex3DDesc,
    pub bpe: u32,
    pub block: u32,
    pub mips: u32,
    pub first_tail: u32,
    pub block_bits: u32,
    pub bw: u32,
    pub bh: u32,
    pub bd: u32,
    pub thick: bool,
    pub slab: u64,
    pub total: u64,
    pub align: u64,
    pub mip: [Mip3; MAX_MIPS],
    pub eq2: Option<&'static Eq>,
    pub eq3: Option<&'static Eq3>,
    pub hw4: bool,
    pub legacy: bool,
    pub reported_mips: u32,
    pub total_bits: u32,
}

fn align_up(v: u64, a: u64) -> u64 {
    (v + a - 1) / a * a
}

fn ceil_shift(v: u32, s: u32) -> u32 {
    ((v as u64 + (1u64 << s) - 1) >> s).max(1) as u32
}

fn dims_2d(bits: u32, lb: u32) -> (u32, u32) {
    let n = bits - lb;
    (1 << ((n + 1) / 2), 1 << (n / 2))
}

fn dims_thick(bits: u32, lb: u32) -> (u32, u32, u32) {
    let (w, h, d) = BLOCK_1K_3D[lb as usize];
    let e = bits - 10;
    let (a, r) = (e / 3, e % 3);
    (w << a, h << (a + r / 2), d << (a + (r != 0) as u32))
}

pub fn full_chain_3d(w: u32, h: u32, d: u32) -> u32 {
    32 - w.max(h).max(d).leading_zeros()
}

pub fn equation_3d(mode: u32, log2_bpe: u32, hw4: bool) -> Option<&'static Eq3> {
    let table: &'static [Eq3] = if hw4 { &EQUATIONS_3D_V4 } else { &EQUATIONS_3D };
    table.iter().find(|e| e.mode as u32 == mode && e.log2_bpe as u32 == log2_bpe)
}

fn tail_coords_2d(t: u64, bw: u32, bh: u32, mw: u32, mh: u32) -> (u32, u32) {
    let (mut mx, mut my) = (0u32, 0u32);
    let mut xi = (bw / mw).trailing_zeros();
    let mut yi = (bh / mh).trailing_zeros();
    let nmb = xi + yi;
    let mut turn_x = true;
    for b in (0..nmb.min(16)).rev() {
        let take_x = (turn_x && xi > 0) || yi == 0;
        let bit = t >> b & 1 != 0;
        if take_x {
            xi -= 1;
            if bit {
                mx |= 1 << xi;
            }
        } else {
            yi -= 1;
            if bit {
                my |= 1 << yi;
            }
        }
        turn_x = !take_x;
    }
    (mx * mw, my * mh)
}

pub fn compute_3d(d: &Tex3DDesc) -> Result<Layout3D, Fail> {
    compute_3d_inner(d, false, None)
}

struct Elem {
    bpe: u32,
    block: u32,
}

fn compute_3d_inner(d: &Tex3DDesc, from_srd: bool, fmt: Option<(u32, u32)>) -> Result<Layout3D, Fail> {
    let mode = d.swizzle_mode & 31;
    validate_common(d.swizzle_mode, d.usage, d.bind_flags, d.misc_flags, d.pitch, d.format, 1, d.width, d.height, d.depth)?;
    validate_misc(d.misc_flags)?;
    if d.mip_levels as usize > MAX_MIPS {
        return Err(Fail::Invalid("mip count"));
    }
    let f = match fmt {
        Some((bpe, block)) => Elem { bpe, block },
        None => {
            let f = lookup_format(d.format)?;
            Elem { bpe: f.bpe, block: f.block }
        }
    };
    if d.width > 16384 || d.height > 16384 || d.depth > 16384 {
        return Err(Fail::Unsupported("dimension above 16384"));
    }
    if !valid_3d_mode(mode) {
        return Err(Fail::Invalid("swizzle mode not valid for 3D"));
    }
    let legacy = layout::legacy_19041();
    if mode >= 28 && legacy && !from_srd {
        return Err(Fail::Invalid("VAR swizzle modes are not in the 19041 library"));
    }
    let hw4 = layout::HARDWARE_V4.load(Ordering::Acquire);
    let chain = full_chain_3d(d.width, d.height, d.depth);
    let mips = if d.mip_levels == 0 { chain } else { d.mip_levels };
    let bpe = f.bpe;
    let lb = bpe.trailing_zeros();
    let bl = f.block.trailing_zeros();
    let mut out = Layout3D {
        desc: *d,
        bpe,
        block: f.block,
        mips,
        first_tail: mips,
        block_bits: 0,
        bw: 1,
        bh: 1,
        bd: 1,
        thick: false,
        slab: 0,
        total: 0,
        align: 0x100,
        mip: [Mip3::default(); MAX_MIPS],
        eq2: None,
        eq3: None,
        hw4,
        legacy,
        reported_mips: mips,
        total_bits: 0,
    };
    let w0 = ceil_shift(d.width, bl);
    let h0 = ceil_shift(d.height, bl);
    for i in 0..mips as usize {
        let m = &mut out.mip[i];
        m.w_px = (d.width >> i).max(1);
        m.h_px = (d.height >> i).max(1);
        m.d_px = (d.depth >> i).max(1);
        m.w_e = ceil_shift(m.w_px, bl);
        m.h_e = ceil_shift(m.h_px, bl);
        m.base_w = ceil_shift(w0, i as u32);
        m.base_h = ceil_shift(h0, i as u32);
        m.base_d = ceil_shift(d.depth, i as u32);
    }
    if mode == 0 {
        let mut off = 0u64;
        for i in (0..mips as usize).rev() {
            let m = &mut out.mip[i];
            let pitch_b = align_up(m.base_w as u64 * bpe as u64, 256);
            m.pad_w = (pitch_b / bpe as u64) as u32;
            m.pad_h = m.base_h;
            m.pad_d = d.depth;
            m.slice = pitch_b * m.pad_h as u64;
            m.size = m.slice * d.depth as u64;
            m.offset = off;
            m.align = 0x100;
            off += m.slice;
        }
        out.slab = off;
        out.total = off * d.depth as u64;
        if out.total >> SIZE_LIMIT_BITS != 0 && !from_srd {
            return Err(Fail::Unsupported("resource size above 40 bits"));
        }
        return Ok(out);
    }
    let thick = is_thick(mode);
    let bits = if hw4 && mode >= 28 { 17 } else { layout::block_bits(mode) };
    let bs = 1u64 << bits;
    let e2 = layout::equation(mode, lb);
    let (bw, bh, bd, mw, mh, mz, tw, th, fit_w, fit_h, cap, slot0, tail_mode);
    if thick {
        let b = dims_thick(bits, lb);
        bw = b.0;
        bh = b.1;
        bd = b.2;
        let m = MICRO_3D[lb as usize];
        mw = m.0;
        mh = m.1;
        mz = m.2;
        if bits % 3 == 0 {
            tw = bw;
            th = bh / 2;
        } else {
            tw = bw / 2;
            th = bh;
        }
        fit_w = tw;
        fit_h = th;
        cap = if bits == 16 { 10 } else { 5 };
        slot0 = if bits == 16 { 0 } else { 5 };
        tail_mode = e2.map(|e| e.tail_mode as u32).unwrap_or(1);
        out.eq3 = equation_3d(mode, lb, hw4);
    } else {
        let eq = match e2 {
            Some(e) if e.valid => e,
            _ => return Err(Fail::Invalid("swizzle mode for element size")),
        };
        let z_mode = mode == 24 || mode == 28;
        if z_mode && f.block != 1 {
            return Err(Fail::Invalid("depth swizzle with block-compressed format"));
        }
        let b = dims_2d(bits, lb);
        bw = b.0;
        bh = b.1;
        bd = 1;
        let m = dims_2d(8, lb);
        mw = m.0;
        mh = m.1;
        mz = 1;
        tw = bw / 2;
        th = bh;
        let fit = match (z_mode, lb) {
            (true, 0) => (tw / 2, th / 2),
            (true, 1) => (tw / 2, th),
            _ => (tw, th),
        };
        fit_w = fit.0;
        fit_h = fit.1;
        cap = 16 - (20 - bits);
        slot0 = 0;
        tail_mode = eq.tail_mode as u32;
        out.eq2 = Some(eq);
    }
    out.thick = thick;
    out.block_bits = bits;
    out.bw = bw;
    out.bh = bh;
    out.bd = bd;
    out.align = bs;
    if bits > 8 && mips > 1 {
        for i in 0..mips as usize {
            if out.mip[i].base_w <= fit_w && out.mip[i].base_h <= fit_h {
                out.first_tail = (i as u32).max(mips.saturating_sub(cap));
                break;
            }
        }
    }
    let ft = out.first_tail as usize;
    let start = (20 - bits) as usize;
    let mut off = 0u64;
    if ft < mips as usize {
        for i in ft..mips as usize {
            let j = i - ft;
            let m = &mut out.mip[i];
            m.in_tail = true;
            m.align = 0x100;
            m.mode = tail_mode;
            m.offset = 0;
            m.pad_w = (tw >> j).max(mw);
            m.pad_h = (th >> j).max(mh);
            m.slice = bw as u64 * bh as u64 * bpe as u64;
            if thick {
                m.pad_d = align_up(m.base_d as u64, mz as u64) as u32;
                let (ux, uy) = TAIL_3D[(slot0 + j).min(TAIL_3D.len() - 1)];
                m.tail_x = ux * mw;
                m.tail_y = uy * mh;
                m.size = m.slice * m.pad_d as u64;
            } else {
                m.pad_d = 1;
                let t = MIP_TAIL_256B[(start + j).min(15)];
                let (x, y) = tail_coords_2d(t, bw, bh, mw, mh);
                m.tail_x = x;
                m.tail_y = y;
                m.size = m.slice;
            }
        }
        off = bs;
    }
    for i in (0..ft).rev() {
        let m = &mut out.mip[i];
        m.pad_w = align_up(m.base_w as u64, bw as u64) as u32;
        m.pad_h = align_up(m.base_h as u64, bh as u64) as u32;
        m.pad_d = if thick {
            align_up(m.base_d as u64, bd as u64) as u32
        } else if mips == 1 {
            d.depth
        } else {
            1
        };
        m.slice = m.pad_w as u64 * m.pad_h as u64 * bpe as u64;
        m.size = m.slice * m.pad_d as u64;
        off = align_up(off, bs);
        m.offset = off;
        m.align = bs as u32;
        m.mode = mode;
        off += m.slice * bd as u64;
    }
    out.slab = align_up(off, bs);
    let slabs = if thick { (out.mip[0].pad_d as u64 + bd as u64 - 1) / bd as u64 } else { d.depth as u64 };
    out.total = out.slab * slabs;
    if out.total >> SIZE_LIMIT_BITS != 0 && !from_srd {
        return Err(Fail::Unsupported("resource size above 40 bits"));
    }
    Ok(out)
}

fn eval_2d(eq: &Eq, x: u64, y: u64, z: u64) -> u64 {
    let mut v = 0u64;
    for i in 0..14 {
        if x >> i & 1 != 0 {
            v ^= eq.x[i] as u64;
        }
        if y >> i & 1 != 0 {
            v ^= eq.y[i] as u64;
        }
    }
    for i in 0..11 {
        if z >> i & 1 != 0 {
            v ^= eq.slice[i] as u64;
        }
    }
    v
}

fn eval_3d(eq: &Eq3, x: u64, y: u64, z: u64) -> u64 {
    let mut v = 0u64;
    for i in 0..14 {
        if x >> i & 1 != 0 {
            v ^= eq.x[i] as u64;
        }
        if y >> i & 1 != 0 {
            v ^= eq.y[i] as u64;
        }
        if z >> i & 1 != 0 {
            v ^= eq.z[i] as u64;
        }
    }
    v
}

fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

impl Layout3D {
    fn header_total(&self) -> u64 {
        let t = if self.total_bits == 0 { self.total } else { self.total & ((1u64 << self.total_bits) - 1) };
        if self.legacy {
            t & 0xffff_ffff
        } else {
            t
        }
    }

    pub fn write(&self, b: &mut [u8]) {
        b[..LAYOUT_BYTES].fill(0);
        let linear_legacy = self.legacy && self.eq2.is_none() && !self.thick;
        let mask = if self.legacy { 0xffff_ffffu64 } else { u64::MAX };
        let mask = if self.total_bits == 0 { mask } else { mask & ((1u64 << self.total_bits) - 1) };
        put64(b, 0, self.total & mask);
        put64(b, 8, self.align);
        put32(b, 0x10, self.reported_mips);
        put32(b, 0x14, 1);
        let p = PLANE_BASE;
        put32(b, p, 1);
        put64(b, p + 8, self.total & mask);
        put64(b, p + 0x10, self.slab & mask);
        put64(b, p + 0x20, self.align);
        put32(b, p + 0x28, self.bpe);
        if linear_legacy {
            put32(b, p + 0x2c, 0);
            put32(b, p + 0x30, self.mips);
        } else {
            put32(b, p + 0x2c, self.first_tail);
            put32(b, p + 0x30, self.reported_mips.saturating_sub(self.first_tail));
        }
        for i in 0..self.mips as usize {
            let m = &self.mip[i];
            let r = p + MIP_BASE + i * MIP_BYTES;
            if m.synthetic {
                let (a, c) = if self.hw4 { (0xe000, 0x2000) } else { (0x8000, 0x8000) };
                put64(b, r, 0x100);
                put64(b, r + 0x10, 0x100);
                put32(b, r + 0x18, self.block);
                put32(b, r + 0x1c, self.bpe);
                put32(b, r + 0x20, 0x100);
                put32(b, r + 0x24, 1);
                put32(b, r + 0x28, 1);
                put32(b, r + 0x2c, 1);
                put32(b, r + 0x30, m.w_e);
                put32(b, r + 0x34, m.h_e);
                put32(b, r + 0x38, m.w_px);
                put32(b, r + 0x3c, m.h_px);
                put32(b, r + 0x40, m.d_px);
                put32(b, r + 0x44, 1);
                put32(b, r + 0x48, self.desc.swizzle_mode & 31);
                put32(b, r + 0x50, a);
                put32(b, r + 0x58, c);
                put32(b, r + 0x60, self.bd);
                continue;
            }
            let sl = layout::size_field(m.slice);
            put64(b, r, if m.slice == 0 { m.size } else { sl * (m.size / m.slice) });
            put64(b, r + 8, m.offset);
            put64(b, r + 0x10, sl);
            put32(b, r + 0x18, m.pad_w * self.block);
            put32(b, r + 0x1c, m.pad_w * self.bpe);
            put32(b, r + 0x20, m.align);
            put32(b, r + 0x24, m.pad_w);
            put32(b, r + 0x28, m.pad_h);
            put32(b, r + 0x2c, m.pad_d);
            put32(b, r + 0x30, m.w_e);
            put32(b, r + 0x34, m.h_e);
            put32(b, r + 0x38, m.w_px);
            put32(b, r + 0x3c, m.h_px);
            put32(b, r + 0x40, m.d_px);
            put32(b, r + 0x44, 1);
            put32(b, r + 0x48, m.mode);
            if !m.in_tail && (24..=27).contains(&m.mode) {
                let (a, c) = if self.hw4 { (0xe000, 0x2000) } else { (0x8000, 0x8000) };
                put32(b, r + 0x50, a);
                put32(b, r + 0x58, c);
            }
            put32(b, r + 0x60, self.bd);
            put32(b, r + 0x64, (m.in_tail || linear_legacy) as u32);
            put32(b, r + 0x68, m.tail_x | m.tail_y << 11);
        }
        put32(b, 0x1b38, 4);
    }

    pub fn set_reported_mips(&mut self, n: u32, xg_bw: u32, xg_bh: u32) {
        self.reported_mips = n;
        let old = self.mips;
        let slots = n.min(MAX_MIPS as u32);
        if slots <= old {
            self.first_tail = self.first_tail.min(slots);
        } else {
            let prev = if old == 0 { None } else { Some(self.mip[old as usize - 1]) };
            let (mut w, mut h, mut dp) = match prev {
                None => (self.desc.width, self.desc.height, self.desc.depth),
                Some(m) => ((m.w_px >> 1).max(1), (m.h_px >> 1).max(1), (m.d_px >> 1).max(1)),
            };
            for i in old as usize..slots as usize {
                self.mip[i] = Mip3 { w_px: w, h_px: h, d_px: dp, w_e: w.div_ceil(xg_bw * self.block), h_e: h.div_ceil(xg_bh * self.block), synthetic: true, ..Mip3::default() };
                w = (w >> 1).max(1);
                h = (h >> 1).max(1);
                dp = (dp >> 1).max(1);
            }
            self.first_tail += slots - old;
        }
        self.mips = slots;
    }

    pub fn mip_offset(&self, mip: u32) -> u64 {
        if mip >= self.mips {
            return 0;
        }
        self.mip[mip as usize].offset
    }

    pub fn element_offset(&self, mip: u32, x: u64, y: u64, z: u64) -> Option<u64> {
        if mip >= self.mips || self.mip[mip as usize].synthetic {
            return None;
        }
        let m = &self.mip[mip as usize];
        if z >= m.d_px as u64 {
            return None;
        }
        if self.eq2.is_none() && !self.thick {
            let row = (y as u32).wrapping_mul(m.pad_w * self.bpe).wrapping_add((x as u32).wrapping_mul(self.bpe));
            return Some(z * self.slab + m.offset + row as u64);
        }
        let bs = 1u64 << self.block_bits;
        let v = match (self.eq2, self.eq3) {
            (Some(e), _) if !self.thick => eval_2d(e, x + m.tail_x as u64, y + m.tail_y as u64, z),
            (_, Some(e)) if self.thick => eval_3d(e, x + m.tail_x as u64, y + m.tail_y as u64, z),
            _ => return None,
        } & (bs - 1);
        let base = if self.thick { z / self.bd as u64 * self.slab } else { z * self.slab };
        let blocks_x = (m.pad_w / self.bw) as u64;
        let blk = layout::block_index_offset(y / self.bh as u64, blocks_x, x / self.bw as u64, self.block_bits);
        Some(base + m.offset + blk + v)
    }
}

pub fn element_offset_1d(l: &Layout2D, mip: u32, x: u64, y: u64, z: u64) -> Option<u64> {
    if mip >= l.mips || z >= l.desc.array_size as u64 || (l.eq.is_none() && y >= 1) || l.mip[mip as usize].synthetic {
        return None;
    }
    let m = &l.mip[mip as usize];
    let base = z * l.slice_size;
    let eq = match l.eq {
        None => {
            let row = (y as u32).wrapping_mul(m.pad_w * l.bpe).wrapping_add((x as u32).wrapping_mul(l.bpe));
            return Some(base + m.offset + row as u64);
        }
        Some(e) => e,
    };
    let bs = 1u64 << l.block_bits;
    let blocks_x = (m.pad_w / l.bw) as u64;
    let blk = layout::block_index_offset(y / l.bh as u64, blocks_x, x / l.bw as u64, l.block_bits);
    let v = eval_2d(eq, x + m.tail_x as u64, y + m.tail_y as u64, z) & (bs - 1);
    let start = if m.in_tail { l.tail_base } else { m.offset };
    Some(base + start + blk + v)
}

#[derive(Clone, Debug)]
pub enum Surface {
    One { l: Layout2D, srd_height: u32 },
    Three(Layout3D),
}

pub struct SubRect {
    pub mip: u32,
    pub slice: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
}

impl Surface {
    pub fn write(&self, b: &mut [u8]) {
        match self {
            Surface::One { l, srd_height } => {
                l.write(b);
                put32(b, 0x1b38, 2);
                if layout::legacy_19041() {
                    let t = l.total & 0xffff_ffff;
                    put64(b, 0, t);
                    put64(b, PLANE_BASE + 8, t);
                    put64(b, PLANE_BASE + 0x10, l.slice_size & 0xffff_ffff);
                }
                if *srd_height > 1 {
                    for i in 0..l.mips as usize {
                        let r = PLANE_BASE + MIP_BASE + i * MIP_BYTES;
                        let h = (*srd_height >> i).max(1);
                        put32(b, r + 0x34, h);
                        put32(b, r + 0x3c, h);
                    }
                }
            }
            Surface::Three(l) => l.write(b),
        }
    }

    pub fn total(&self) -> u64 {
        match self {
            Surface::One { l, .. } => {
                if layout::legacy_19041() {
                    l.resource_total() & 0xffff_ffff
                } else {
                    l.resource_total()
                }
            }
            Surface::Three(l) => l.header_total(),
        }
    }

    pub fn align(&self) -> u64 {
        match self {
            Surface::One { l, .. } => l.align,
            Surface::Three(l) => l.align,
        }
    }

    pub fn mips(&self) -> u32 {
        match self {
            Surface::One { l, .. } => l.mips,
            Surface::Three(l) => l.mips,
        }
    }

    pub fn bpe(&self) -> u32 {
        match self {
            Surface::One { l, .. } => l.bpe,
            Surface::Three(l) => l.bpe,
        }
    }

    pub fn mip_offset(&self, mip: u32) -> u64 {
        if mip >= self.mips() {
            return u64::MAX;
        }
        match self {
            Surface::One { l, .. } => l.mip_offset(mip),
            Surface::Three(l) => l.mip_offset(mip),
        }
    }

    pub fn element_offset(&self, mip: u32, x: u64, y: u64, z: u64) -> Option<u64> {
        match self {
            Surface::One { l, .. } => element_offset_1d(l, mip, x, y, z),
            Surface::Three(l) => l.element_offset(mip, x, y, z),
        }
    }

    pub fn sub_rect(&self, sub: u32) -> Option<SubRect> {
        match self {
            Surface::One { l, .. } => {
                if l.mips == 0 {
                    return None;
                }
                let (mip, slice) = (sub % l.mips, sub / l.mips);
                if slice >= l.desc.array_size {
                    return None;
                }
                let m = &l.mip[mip as usize];
                Some(SubRect { mip, slice, w: m.w_e, h: 1, d: 1 })
            }
            Surface::Three(l) => {
                if l.mips == 0 {
                    return None;
                }
                if sub / l.mips >= l.desc.depth {
                    return None;
                }
                let mip = sub % l.mips;
                let m = &l.mip[mip as usize];
                Some(SubRect { mip, slice: 0, w: m.w_e, h: m.h_e, d: m.d_px })
            }
        }
    }
}

pub fn from_d3d12(d: &crate::D3D12Desc) -> Result<Surface, Fail> {
    let fl = d.flags;
    let mut bind = 8 | (fl & 2) << 5;
    if fl & 1 != 0 {
        bind |= 0x20;
    }
    if fl & 4 != 0 {
        bind |= 0x80;
    }
    if fl & 8 != 0 {
        bind &= !8;
    }
    let mut misc = if fl & 0x20 != 0 { 0x38_0000 } else { 0 };
    if fl & 0x30_0000 != 0 && bind & 0x20 != 0 {
        misc |= 0x8_0000;
    }
    if fl & 0x40_0000 != 0 {
        misc |= 0x40_0000;
    }
    if fl & 0x80_0000 != 0 {
        misc |= 0x8000;
    }
    let lay = d.layout;
    let code = if d.dimension == 2 { 2 } else { 4 };
    let mode = if lay & !2 == 0 {
        if lay == 2 {
            misc |= 0x0800_0000;
        }
        if bind & 0x40 != 0 {
            24
        } else {
            crate::optimal_swizzle(code, d.format, d.width as u32 as u64, d.height, d.depth_or_array as u32, d.samples, bind & 0x20 != 0, bind & 0x80 != 0, misc >> 27 & 1 != 0)
        }
    } else if lay == 1 {
        0
    } else if (0x100..=0x11f).contains(&lay) {
        lay & 0x1f
    } else {
        return Err(Fail::Invalid("layout"));
    };
    let usage = if mode == 0 { 3 } else { 0 };
    if d.dimension == 2 {
        let t = Tex1DDesc {
            width: d.width as u32,
            mip_levels: d.mips as u32,
            array_size: d.depth_or_array as u32,
            format: d.format,
            usage,
            bind_flags: bind,
            cpu_access: 0,
            misc_flags: 0,
            swizzle_mode: mode,
            pitch: 0,
        };
        return Ok(Surface::One { l: compute_1d(&t)?, srd_height: 0 });
    }
    let t = Tex3DDesc {
        width: d.width as u32,
        height: d.height,
        depth: d.depth_or_array as u32,
        mip_levels: d.mips as u32,
        format: d.format,
        usage,
        bind_flags: bind,
        cpu_access: 0,
        misc_flags: misc,
        swizzle_mode: mode,
        pitch: 0,
    };
    Ok(Surface::Three(compute_3d(&t)?))
}

pub fn from_srd(d: &[u32; 8]) -> Option<Result<Surface, Fail>> {
    use crate::descriptor::{element, parse_srd, representative_format, SrdDim};
    let s = parse_srd(d).ok()?;
    if s.dim == SrdDim::Tex2D {
        return None;
    }
    Some((|| {
        if s.addr_mips > 15 {
            return Err(Fail::Invalid("Failed to initialize color target interface"));
        }
        let e = element(s.hw_format).ok_or(Fail::Unsupported("descriptor format"))?;
        let format = representative_format(s.hw_format).unwrap_or(0);
        let mode = s.swizzle_mode;
        let usage = if mode == 0 { 3 } else { 0 };
        if s.dim == SrdDim::Tex1D {
            let t = Tex1DDesc { width: s.width, mip_levels: s.addr_mips, array_size: s.depth, format, usage, bind_flags: 8, cpu_access: 0, misc_flags: 0, swizzle_mode: mode, pitch: 0 };
            let mut l = compute_1d_srd(&t, s.height, e.bpe, e.block)?;
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
            return Ok(Surface::One { l, srd_height: 0 });
        }
        let t = Tex3DDesc { width: s.width, height: s.height, depth: s.depth, mip_levels: s.addr_mips, format, usage, bind_flags: 8, cpu_access: 0, misc_flags: 0, swizzle_mode: mode, pitch: 0 };
        let mut l = compute_3d_inner(&t, true, Some((e.bpe, e.block)))?;
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
        Ok(Surface::Three(l))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d3(w: u32, h: u32, d: u32, mips: u32, fmt: u32, mode: u32) -> Tex3DDesc {
        Tex3DDesc { width: w, height: h, depth: d, mip_levels: mips, format: fmt, usage: if mode == 0 { 3 } else { 0 }, bind_flags: 8, swizzle_mode: mode, ..Default::default() }
    }

    #[test]
    fn thick_block_dims() {
        assert_eq!(dims_thick(12, 0), (16, 16, 16));
        assert_eq!(dims_thick(12, 2), (8, 16, 8));
        assert_eq!(dims_thick(16, 0), (64, 32, 32));
        assert_eq!(dims_thick(16, 2), (32, 32, 16));
        assert_eq!(dims_thick(12, 3), (8, 8, 8));
    }

    #[test]
    fn thick_4kb_slabs() {
        layout::set_legacy_19041(false);
        let l = compute_3d(&d3(64, 32, 16, 0, 28, 5)).unwrap();
        assert_eq!((l.total, l.slab, l.first_tail), (0x2e000, 0x17000, 3));
        assert_eq!((l.mip[0].offset, l.mip[1].offset, l.mip[2].offset), (0x7000, 0x3000, 0x1000));
        assert_eq!((l.mip[3].tail_x, l.mip[3].tail_y), (0, 8));
        assert_eq!(l.mip[3].size, 0x800);
    }

    #[test]
    fn thick_64kb_deep_tail() {
        layout::set_legacy_19041(false);
        let l = compute_3d(&d3(1, 1, 65, 0, 61, 9)).unwrap();
        assert_eq!(l.first_tail, 0);
        assert_eq!(l.total, 0x30000);
        assert_eq!(l.mip[0].pad_d, 72);
        assert_eq!(l.mip[1].pad_d, 40);
    }

    #[test]
    fn thin_and_linear() {
        layout::set_legacy_19041(false);
        let l = compute_3d(&d3(100, 60, 7, 0, 28, 27)).unwrap();
        assert_eq!((l.total, l.slab, l.first_tail), (0xe0000, 0x20000, 1));
        let l = compute_3d(&d3(300, 300, 5, 1, 28, 27)).unwrap();
        assert_eq!((l.mip[0].pad_d, l.mip[0].size), (5, 0x2d0000));
        let l = compute_3d(&d3(100, 60, 7, 0, 28, 0)).unwrap();
        assert_eq!((l.total, l.slab), (0x4ec00, 0xb400));
    }

    #[test]
    fn invalid_3d() {
        layout::set_legacy_19041(false);
        assert_eq!(compute_3d(&d3(64, 64, 4, 1, 28, 1)).unwrap_err().hr(), layout::E_INVALIDARG);
        assert_eq!(compute_3d(&d3(64, 64, 4, 1, 71, 24)).unwrap_err().hr(), layout::E_INVALIDARG);
        let mut x = d3(64, 64, 4, 1, 28, 5);
        x.misc_flags = 0x0800_0000;
        assert_eq!(compute_3d(&x).unwrap_err().hr(), layout::E_INVALIDARG);
    }
}
