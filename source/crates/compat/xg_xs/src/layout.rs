use crate::equations::{Eq, EQUATIONS};
use crate::equations_ms::{MsEq, MS_EQUATIONS, MS_EQUATIONS_V4};
use crate::equations_v4::EQUATIONS_V4;
use crate::formats;

pub const LAYOUT_BYTES: usize = 0x1b40;
pub const PLANE_BASE: usize = 0x18;
pub const MIP_BASE: usize = 0x38;
pub const MIP_BYTES: usize = 0x70;
pub const MAX_MIPS: usize = 15;

pub static HARDWARE_V4: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static LEGACY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

thread_local! {
    pub static ALLOW_VAR_19041: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

pub fn legacy_19041() -> bool {
    use std::sync::atomic::Ordering;
    match LEGACY.load(Ordering::Acquire) {
        0 => {
            let v = std::env::var("TOMBOX_XG_VERSION").map(|v| v.trim() == "19041").unwrap_or(false);
            LEGACY.store(if v { 2 } else { 1 }, Ordering::Release);
            v
        }
        x => x == 2,
    }
}

#[cfg(test)]
pub fn set_legacy_19041(on: bool) {
    LEGACY.store(if on { 2 } else { 1 }, std::sync::atomic::Ordering::Release);
}

pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;

const MIP_TAIL_256B: [u64; 16] = [2048, 1024, 512, 256, 128, 64, 32, 16, 8, 6, 5, 4, 3, 2, 1, 0];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tex2DDesc {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub array_size: u32,
    pub format: u32,
    pub sample_count: u32,
    pub sample_quality: u32,
    pub usage: u32,
    pub bind_flags: u32,
    pub cpu_access: u32,
    pub misc_flags: u32,
    pub swizzle_mode: u32,
    pub reserved: u32,
}

impl Tex2DDesc {
    pub unsafe fn read(p: *const u8) -> Tex2DDesc {
        let u = |o: usize| core::ptr::read_unaligned(p.add(o) as *const u32);
        Tex2DDesc {
            width: u(0),
            height: u(4),
            mip_levels: u(8),
            array_size: u(0xc),
            format: u(0x10),
            sample_count: u(0x14),
            sample_quality: u(0x18),
            usage: u(0x1c),
            bind_flags: u(0x20),
            cpu_access: u(0x24),
            misc_flags: u(0x28),
            swizzle_mode: u(0x2c),
            reserved: u(0x30),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mip {
    pub size: u64,
    pub offset: u64,
    pub pad_w: u32,
    pub pad_h: u32,
    pub w_e: u32,
    pub h_e: u32,
    pub w_px: u32,
    pub h_px: u32,
    pub align: u32,
    pub mode: u32,
    pub in_tail: bool,
    pub tail_x: u32,
    pub tail_y: u32,
    pub base_w: u32,
    pub base_h: u32,
    pub synthetic: bool,
}

#[derive(Clone, Debug)]
pub struct Layout2D {
    pub desc: Tex2DDesc,
    pub bpe: u32,
    pub block: u32,
    pub block_bits: u32,
    pub bw: u32,
    pub bh: u32,
    pub mips: u32,
    pub first_tail: u32,
    pub tail_base: u64,
    pub slice_size: u64,
    pub total: u64,
    pub align: u64,
    pub mip: [Mip; MAX_MIPS],
    pub eq: Option<&'static Eq>,
    pub hw4: bool,
    pub samples: u32,
    pub ms: Option<&'static MsEq>,
    pub cmask: u64,
    pub main_blank: bool,
    pub reported_mips: u32,
    pub total_bits: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    Invalid(&'static str),
    Unsupported(&'static str),
}

impl Fail {
    pub fn hr(&self) -> i32 {
        match self {
            Fail::Invalid(_) => E_INVALIDARG,
            Fail::Unsupported(_) => compat_common::E_NOTIMPL,
        }
    }
    pub fn why(&self) -> &'static str {
        match self {
            Fail::Invalid(s) | Fail::Unsupported(s) => s,
        }
    }
}

pub fn block_bits(mode: u32) -> u32 {
    match mode {
        0 => 0,
        1..=3 => 8,
        4..=7 | 20..=23 => 12,
        8..=19 | 24..=27 => 16,
        _ => 19,
    }
}

pub fn equation(mode: u32, log2_bpe: u32) -> Option<&'static Eq> {
    let table: &'static [Eq] = if HARDWARE_V4.load(std::sync::atomic::Ordering::Acquire) { &EQUATIONS_V4 } else { &EQUATIONS };
    table.iter().find(|e| e.mode as u32 == mode && e.log2_bpe as u32 == log2_bpe)
}

fn dims(bits: u32, lb: u32) -> (u32, u32) {
    let n = bits - lb;
    (1 << ((n + 1) / 2), 1 << (n / 2))
}

fn align_up(v: u64, a: u64) -> u64 {
    (v + a - 1) / a * a
}

fn ceil_shift(v: u32, s: u32) -> u32 {
    ((v as u64 + (1u64 << s) - 1) >> s).max(1) as u32
}

pub fn full_chain(w: u32, h: u32) -> u32 {
    32 - w.max(h).leading_zeros()
}

pub fn wrap14(v: u32) -> u32 {
    (v.wrapping_sub(1) & 0x3fff) + 1
}

pub fn log2_samples(n: u32) -> Option<u32> {
    match n {
        0 | 1 => Some(0),
        2..=15 => Some(31 - n.leading_zeros()),
        _ => None,
    }
}

pub fn ms_equation(mode: u32, log2_bpe: u32, log2_samples: u32) -> Option<&'static MsEq> {
    let table: &'static [MsEq] = if HARDWARE_V4.load(std::sync::atomic::Ordering::Acquire) { &MS_EQUATIONS_V4 } else { &MS_EQUATIONS };
    table.iter().find(|e| e.mode as u32 == mode && e.log2_bpe as u32 == log2_bpe && e.log2_samples as u32 == log2_samples)
}

pub fn compute(d: &Tex2DDesc) -> Result<Layout2D, Fail> {
    compute_with_pitch(d, 0, None)
}

pub const CMASK_ALIGN: u64 = 0x2000;

pub fn block_index_offset(yb: u64, pb: u64, xb: u64, bits: u32) -> u64 {
    let idx = (yb as u32).wrapping_mul(pb as u32).wrapping_add(xb as u32);
    if bits == 8 {
        return idx.wrapping_shl(8) as u64;
    }
    (idx as u64) << bits
}

pub fn size_field(size: u64) -> u64 {
    if size < 0x100 {
        return size;
    }
    ((((size >> 8) - 1) & 0xff_ffff) + 1) << 8
}

pub fn cmask_bytes(width: u32, height: u32, array: u32) -> u64 {
    align_up(width as u64, 1024) * align_up(height as u64, 1024) / 128 * array as u64
}

pub fn compute_with_pitch(d: &Tex2DDesc, pitch: u32, fmt: Option<(u32, u32)>) -> Result<Layout2D, Fail> {
    if d.misc_flags & 0x0840_0000 == 0x0840_0000 {
        return Err(Fail::Invalid("misc 0x400000 with bit 27"));
    }
    let mut l = compute_main(d, pitch, fmt, false)?;
    if d.bind_flags & 0x20 != 0 && l.samples == 1 && l.mips == 1 {
        let bit27 = d.misc_flags & 0x0800_0000 != 0;
        let bc_z = l.block != 1 && matches!(d.swizzle_mode & 31, 24 | 28);
        if !bit27 && !bc_z {
            l.cmask = cmask_bytes(l.mip[0].w_px, l.mip[0].h_px, l.desc.array_size);
        }
        l.main_blank = l.block != 1;
    }
    Ok(l)
}

pub fn compute_descriptor(d: &Tex2DDesc, bpe: u32, block: u32) -> Result<Layout2D, Fail> {
    compute_main(d, 0, Some((bpe, block)), true)
}

fn compute_main(d: &Tex2DDesc, pitch: u32, fmt: Option<(u32, u32)>, srd: bool) -> Result<Layout2D, Fail> {
    if d.width == 0 || d.height == 0 || d.array_size == 0 {
        return Err(Fail::Invalid("zero dimension"));
    }
    let width = ((d.width - 1) & 0xffff) + 1;
    let height = ((d.height - 1) & 0xffff) + 1;
    let ls = log2_samples(d.sample_count).ok_or(Fail::Invalid("sample count"))?;
    let chain = full_chain(width, height);
    let mut mips = if d.mip_levels == 0 { chain } else { d.mip_levels };
    if mips as usize > MAX_MIPS {
        return Err(Fail::Invalid("mip count"));
    }
    if ls > 0 {
        mips = 1;
    }
    let (fbpe, fblock) = match fmt {
        Some(v) => v,
        None => match formats::lookup(d.format) {
            Some(f) => (f.bpe, f.block),
            None if formats::accepted_elsewhere(d.format) => return Err(Fail::Unsupported("format")),
            None => return Err(Fail::Invalid("format")),
        },
    };
    struct F {
        bpe: u32,
        block: u32,
    }
    let f = F { bpe: fbpe, block: fblock };
    let mode = d.swizzle_mode & 31;
    if d.usage == 3 && mode != 0 {
        return Err(Fail::Invalid("staging texture must be linear"));
    }
    if mode == 0 && d.usage != 3 && f.block != 1 {
        return Err(Fail::Invalid("linear block-compressed texture needs staging usage"));
    }
    let bpe = f.bpe;
    let lb = bpe.trailing_zeros();
    if ls > 0 {
        let ok = match lb {
            0..=2 => matches!(mode, 24 | 27 | 28 | 31),
            _ => matches!(mode, 27 | 31),
        };
        if !ok || (f.block != 1 && !srd) {
            return Err(Fail::Invalid("swizzle mode for multisampled texture"));
        }
    }
    let pitch = if pitch != 0 { wrap14(pitch) } else { 0 };
    let mut out = Layout2D {
        desc: *d,
        bpe,
        block: f.block,
        block_bits: 0,
        bw: 1,
        bh: 1,
        mips,
        first_tail: mips,
        tail_base: 0,
        slice_size: 0,
        total: 0,
        align: 0x100,
        mip: [Mip::default(); MAX_MIPS],
        eq: None,
        hw4: HARDWARE_V4.load(std::sync::atomic::Ordering::Acquire),
        samples: 1 << ls,
        ms: None,
        cmask: 0,
        main_blank: false,
        reported_mips: mips,
        total_bits: 0,
    };
    let w0 = ceil_shift(width, f.block.trailing_zeros());
    let h0 = ceil_shift(height, f.block.trailing_zeros());
    for i in 0..mips as usize {
        let m = &mut out.mip[i];
        m.w_px = (width >> i).max(1);
        m.h_px = (height >> i).max(1);
        m.w_e = ceil_shift(m.w_px, f.block.trailing_zeros());
        m.h_e = ceil_shift(m.h_px, f.block.trailing_zeros());
        m.base_w = ceil_shift(w0, i as u32);
        m.base_h = ceil_shift(h0, i as u32);
    }
    if mode == 0 {
        let mut off = 0u64;
        for i in (0..mips as usize).rev() {
            let m = &mut out.mip[i];
            let mut pitch_b = align_up(m.base_w as u64 * bpe as u64, 256);
            if i == 0 && pitch != 0 {
                pitch_b = pitch_b.max(align_up(pitch as u64 * bpe as u64, 256));
            }
            m.pad_w = (pitch_b / bpe as u64) as u32;
            m.pad_h = m.base_h;
            m.size = pitch_b * m.pad_h as u64;
            m.offset = off;
            m.align = 0x100;
            m.mode = 0;
            off += m.size;
        }
        out.slice_size = off;
        out.total = off * d.array_size as u64;
        return Ok(out);
    }
    let eq = match equation(mode, lb) {
        Some(e) if e.valid => e,
        _ => return Err(Fail::Invalid("swizzle mode for element size")),
    };
    if mode >= 28 && legacy_19041() && !ALLOW_VAR_19041.with(|c| c.get()) {
        return Err(Fail::Invalid("VAR swizzle modes are not in the 19041 library"));
    }
    out.eq = Some(eq);
    if ls > 0 {
        out.ms = Some(ms_equation(mode, lb, ls).ok_or(Fail::Invalid("swizzle mode for multisampled texture"))?);
    }
    let bits = if out.hw4 && mode >= 28 { 17 } else { block_bits(mode) };
    let bs = 1u64 << bits;
    let (mut bw, mut bh) = dims(bits, lb);
    let (q, r) = (ls >> 1, ls & 1);
    if bits & 1 != 0 && ls > 0 {
        let n = bits - lb - ls;
        bw = 1 << n.div_ceil(2);
        bh = 1 << (n / 2);
    } else {
        bw >>= q + r;
        bh >>= q;
    }
    let (mw, mh) = dims(8, lb);
    out.block_bits = bits;
    out.bw = bw;
    out.bh = bh;
    out.align = bs;
    let (tw, th) = (bw / 2, bh);
    let z_mode = mode == 24 || mode == 28;
    if z_mode && f.block != 1 && !(srd && ls > 0) && !(d.bind_flags & 0x20 != 0 && ls == 0 && mips == 1) {
        return Err(Fail::Invalid("depth swizzle with block-compressed format"));
    }
    let (fit_w, fit_h) = match (z_mode, lb) {
        (true, 0) => (tw / 2, th / 2),
        (true, 1) => (tw / 2, th),
        _ => (tw, th),
    };
    if bits > 8 && mips > 1 {
        for i in 0..mips as usize {
            if out.mip[i].base_w <= fit_w && out.mip[i].base_h <= fit_h {
                out.first_tail = i as u32;
                break;
            }
        }
        let max_in_tail = bits - 4;
        if out.first_tail < mips && mips - out.first_tail > max_in_tail {
            out.first_tail = mips - max_in_tail;
        }
    }
    let ft = out.first_tail as usize;
    let start = (20 - bits) as usize;
    let mut off = 0u64;
    if ft < mips as usize {
        for i in ft..mips as usize {
            let j = i - ft;
            let m = &mut out.mip[i];
            m.pad_w = (tw >> j).max(mw);
            m.pad_h = (th >> j).max(mh);
            m.size = m.pad_w as u64 * m.pad_h as u64 * bpe as u64;
            let t = MIP_TAIL_256B[(start + j).min(15)];
            m.offset = t * 256;
            m.align = 0x100;
            m.mode = eq.tail_mode as u32;
            m.in_tail = true;
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
            m.tail_x = mx * mw;
            m.tail_y = my * mh;
        }
        off = bs;
    }
    for i in (0..ft).rev() {
        let m = &mut out.mip[i];
        m.pad_w = align_up(m.base_w as u64, bw as u64) as u32;
        if i == 0 && pitch != 0 {
            m.pad_w = m.pad_w.max(align_up(pitch as u64, bw as u64) as u32);
        }
        m.pad_h = align_up(m.base_h as u64, bh as u64) as u32;
        m.size = m.pad_w as u64 * m.pad_h as u64 * bpe as u64 * out.samples as u64;
        off = align_up(off, bs);
        m.offset = off;
        m.align = bs as u32;
        m.mode = mode;
        off += m.size;
    }
    out.slice_size = align_up(off, bs);
    out.total = out.slice_size * d.array_size as u64;
    Ok(out)
}

impl Layout2D {
    pub fn set_reported_mips(&mut self, n: u32, xg_bw: u32, xg_bh: u32) {
        self.reported_mips = n;
        let old = self.mips;
        let slots = n.min(MAX_MIPS as u32);
        if slots <= old {
            self.first_tail = self.first_tail.min(slots);
        } else {
            let (mut w, mut h) = if old == 0 { (self.desc.width, self.desc.height) } else { ((self.mip[old as usize - 1].w_px >> 1).max(1), (self.mip[old as usize - 1].h_px >> 1).max(1)) };
            for i in old as usize..slots as usize {
                self.mip[i] = Mip { w_px: w, h_px: h, w_e: w.div_ceil(xg_bw * self.block), h_e: h.div_ceil(xg_bh * self.block), synthetic: true, ..Mip::default() };
                w = (w >> 1).max(1);
                h = (h >> 1).max(1);
            }
            self.first_tail += slots - old;
        }
        self.mips = slots;
    }

    pub fn cmask_offset(&self) -> u64 {
        if self.main_blank { 0 } else { align_up(self.total, CMASK_ALIGN) }
    }

    pub fn resource_total(&self) -> u64 {
        let t = if self.cmask == 0 && !self.main_blank { self.total } else { self.cmask_offset() + self.cmask };
        if self.total_bits == 0 { t } else { t & ((1u64 << self.total_bits) - 1) }
    }

    pub fn resource_align(&self) -> u64 {
        if self.main_blank {
            if self.cmask == 0 { 0 } else { CMASK_ALIGN }
        } else if self.cmask == 0 {
            self.align
        } else {
            self.align.max(CMASK_ALIGN)
        }
    }

    pub fn write(&self, b: &mut [u8]) {
        b[..LAYOUT_BYTES].fill(0);
        let p64 = |b: &mut [u8], o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
        let p32 = |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        p64(b, 0, self.resource_total());
        p64(b, 8, self.resource_align());
        p32(b, 0x10, self.reported_mips);
        p32(b, 0x14, if self.cmask == 0 { 1 } else { 2 });
        p32(b, 0x1b38, 3);
        if self.cmask != 0 {
            let q = PLANE_BASE + MIP_BASE + MAX_MIPS * MIP_BYTES;
            p32(b, q, 2);
            p64(b, q + 8, self.cmask);
            p64(b, q + 0x18, self.cmask_offset());
            p64(b, q + 0x20, CMASK_ALIGN);
            p64(b, q + MIP_BASE, self.cmask);
            p32(b, q + MIP_BASE + 0x20, CMASK_ALIGN as u32);
        }
        let p = PLANE_BASE;
        p32(b, p, 1);
        if self.main_blank {
            return;
        }
        p64(b, p + 8, if self.total_bits == 0 { self.total } else { self.total & ((1u64 << self.total_bits) - 1) });
        p64(b, p + 0x10, self.slice_size);
        p64(b, p + 0x20, self.align);
        p32(b, p + 0x28, self.bpe);
        if self.eq.is_none() && legacy_19041() {
            p32(b, p + 0x2c, 0);
            p32(b, p + 0x30, self.mips);
        } else {
            p32(b, p + 0x2c, self.first_tail);
            p32(b, p + 0x30, self.reported_mips.saturating_sub(self.first_tail));
        }
        let arr = self.desc.array_size;
        for i in 0..self.mips as usize {
            let m = &self.mip[i];
            let r = p + MIP_BASE + i * MIP_BYTES;
            if m.synthetic {
                let (a, c) = if self.hw4 { (0xe000, 0x2000) } else { (0x8000, 0x8000) };
                p64(b, r, 0x100);
                p64(b, r + 0x10, 0x100);
                p32(b, r + 0x18, self.block);
                p32(b, r + 0x1c, self.bpe);
                p32(b, r + 0x20, 0x100);
                p32(b, r + 0x24, 1);
                p32(b, r + 0x28, 1);
                p32(b, r + 0x2c, arr);
                p32(b, r + 0x30, m.w_e);
                p32(b, r + 0x34, m.h_e);
                p32(b, r + 0x38, m.w_px);
                p32(b, r + 0x3c, m.h_px);
                p32(b, r + 0x40, arr);
                p32(b, r + 0x44, self.samples);
                p32(b, r + 0x48, self.desc.swizzle_mode & 31);
                p32(b, r + 0x50, a);
                p32(b, r + 0x58, c);
                p32(b, r + 0x60, 1);
                continue;
            }
            p64(b, r, size_field(m.size));
            p64(b, r + 8, if m.in_tail { self.tail_base + m.offset } else { m.offset });
            p64(b, r + 0x10, size_field(m.size));
            p32(b, r + 0x18, wrap14(m.pad_w) * self.block);
            p32(b, r + 0x1c, wrap14(m.pad_w) * self.bpe);
            p32(b, r + 0x20, m.align);
            p32(b, r + 0x24, wrap14(m.pad_w));
            p32(b, r + 0x28, wrap14(m.pad_h));
            p32(b, r + 0x2c, arr);
            p32(b, r + 0x30, m.w_e);
            p32(b, r + 0x34, m.h_e);
            p32(b, r + 0x38, m.w_px);
            p32(b, r + 0x3c, m.h_px);
            p32(b, r + 0x40, arr);
            p32(b, r + 0x44, self.samples);
            p32(b, r + 0x48, m.mode);
            if !m.in_tail && (24..=27).contains(&m.mode) {
                let (a, c) = if self.hw4 { (0xe000, 0x2000) } else { (0x8000, 0x8000) };
                p32(b, r + 0x50, a);
                p32(b, r + 0x58, c);
            }
            p32(b, r + 0x60, 1);
            p32(b, r + 0x64, (m.in_tail || (self.eq.is_none() && legacy_19041())) as u32);
        }
    }

    pub fn mip_offset(&self, mip: u32) -> u64 {
        if mip >= self.mips {
            return 0;
        }
        let m = &self.mip[mip as usize];
        if m.in_tail {
            self.tail_base + m.offset
        } else {
            m.offset
        }
    }

    fn eval(eq: &Eq, x: u64, y: u64, z: u64) -> u64 {
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

    fn eval_ms(eq: &MsEq, x: u64, y: u64, z: u64, s: u64) -> u64 {
        let mut v = 0u64;
        for i in 0..14 {
            if x >> i & 1 != 0 {
                v ^= eq.x[i] as u64;
            }
            if y >> i & 1 != 0 {
                v ^= eq.y[i] as u64;
            }
        }
        for i in 0..3 {
            if s >> i & 1 != 0 {
                v ^= eq.s[i] as u64;
            }
        }
        for i in 0..11 {
            if z >> i & 1 != 0 {
                v ^= eq.slice[i] as u64;
            }
        }
        v
    }

    pub fn element_offset(&self, mip: u32, x: u64, y: u64, z: u64) -> Option<u64> {
        self.sample_offset(mip, x, y, z, 0)
    }

    pub fn sample_offset(&self, mip: u32, x: u64, y: u64, z: u64, s: u64) -> Option<u64> {
        if let Some(ms) = self.ms {
            if mip != 0 || z >= self.desc.array_size as u64 || s >= self.samples as u64 {
                return None;
            }
            let m = &self.mip[0];
            if x >= m.pad_w as u64 || y >= m.pad_h as u64 {
                return None;
            }
            let bs = 1u64 << self.block_bits;
            let blocks_x = (m.pad_w / self.bw) as u64;
            let blk = block_index_offset(y / self.bh as u64, blocks_x, x / self.bw as u64, self.block_bits);
            return Some(z * self.slice_size + m.offset + blk + (Self::eval_ms(ms, x, y, z, s) & (bs - 1)));
        }
        if mip >= self.mips || z >= self.desc.array_size as u64 {
            return None;
        }
        let m = &self.mip[mip as usize];
        if x >= m.pad_w as u64 || y >= m.pad_h as u64 {
            return None;
        }
        let base = z * self.slice_size;
        let eq = match self.eq {
            None => return Some(base + m.offset + (y as u32).wrapping_mul(m.pad_w).wrapping_add(x as u32).wrapping_mul(self.bpe) as u64),
            Some(e) => e,
        };
        let bs = 1u64 << self.block_bits;
        if m.in_tail {
            let v = Self::eval(eq, x + m.tail_x as u64, y + m.tail_y as u64, z) & (bs - 1);
            return Some(base + self.tail_base + v);
        }
        let blocks_x = (m.pad_w / self.bw) as u64;
        let blk = block_index_offset(y / self.bh as u64, blocks_x, x / self.bw as u64, self.block_bits);
        Some(base + m.offset + blk + (Self::eval(eq, x, y, z) & (bs - 1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(w: u32, h: u32, arr: u32, mips: u32, fmt: u32, mode: u32) -> Tex2DDesc {
        Tex2DDesc { width: w, height: h, mip_levels: mips, array_size: arr, format: fmt, sample_count: 1, swizzle_mode: mode, ..Default::default() }
    }

    #[test]
    fn linear_pitch_alignment() {
        let l = compute(&d(1000, 33, 1, 1, 2, 0)).unwrap();
        assert_eq!(l.mip[0].pad_w, 1008);
        assert_eq!(l.total, 0x81f00);
        let l = compute(&d(300, 33, 2, 0, 2, 0)).unwrap();
        assert_eq!(l.mip[0].pad_w, 304);
        assert_eq!(l.slice_size, 0x36400);
        assert_eq!(l.total, 0x6c800);
        let l = compute(&d(300, 33, 1, 0, 28, 0)).unwrap();
        assert_eq!(l.total, 0xf700);
        assert_eq!(l.mip[0].offset, 0x5200);
        assert_eq!(l.mip[1].pad_w, 192);
        assert_eq!(l.mip[1].pad_h, 17);
        assert_eq!(l.mip[8].offset, 0);
        assert_eq!(l.align, 0x100);
    }

    #[test]
    fn swizzled_4kb_chain_with_tail() {
        let l = compute(&d(256, 256, 6, 0, 28, 5)).unwrap();
        assert_eq!(l.mips, 9);
        assert_eq!(l.first_tail, 4);
        assert_eq!(l.slice_size, 0x56000);
        assert_eq!(l.total, 0x204000);
        assert_eq!(l.align, 0x1000);
        assert_eq!(l.mip[0].offset, 0x16000);
        assert_eq!(l.mip[3].offset, 0x1000);
        assert_eq!(l.mip[4].offset, 0x800);
        assert_eq!(l.mip[5].offset, 0x600);
        assert_eq!(l.mip[8].offset, 0x300);
        assert_eq!((l.mip[4].pad_w, l.mip[4].pad_h), (16, 32));
        assert_eq!(l.mip[4].mode, 1);
    }

    #[test]
    fn single_mip_never_in_tail() {
        for mode in [5, 9, 25, 28] {
            let l = compute(&d(1, 1, 1, 1, 28, mode)).unwrap();
            assert_eq!(l.first_tail, 1);
            assert!(!l.mip[0].in_tail);
        }
        let l = compute(&d(64, 64, 1, 0, 28, 9)).unwrap();
        assert_eq!(l.first_tail, 0);
        assert_eq!(l.total, 0x10000);
    }

    #[test]
    fn block_dims() {
        assert_eq!(dims(12, 2), (32, 32));
        assert_eq!(dims(16, 2), (128, 128));
        assert_eq!(dims(8, 0), (16, 16));
        assert_eq!(dims(8, 1), (16, 8));
        assert_eq!(dims(8, 3), (8, 4));
        assert_eq!(dims(8, 4), (4, 4));
    }

    #[test]
    fn invalid_inputs() {
        assert_eq!(compute(&d(256, 256, 1, 1, 28, 3)).unwrap_err().hr(), E_INVALIDARG);
        assert_eq!(compute(&d(256, 256, 1, 16, 28, 5)).unwrap_err().hr(), E_INVALIDARG);
        assert_eq!(compute(&d(0, 256, 1, 1, 28, 5)).unwrap_err().hr(), E_INVALIDARG);
        assert_eq!(compute(&d(256, 256, 1, 1, 5, 5)).unwrap_err().hr(), E_INVALIDARG);
        let mut x = d(256, 256, 1, 1, 28, 5);
        x.sample_count = 4;
        assert_eq!(compute(&x).unwrap_err().hr(), E_INVALIDARG);
    }

    #[test]
    fn bc_elements() {
        let l = compute(&d(256, 256, 1, 1, 71, 5)).unwrap();
        assert_eq!(l.bpe, 8);
        assert_eq!(l.mip[0].w_e, 64);
        let mut b = vec![0u8; LAYOUT_BYTES];
        l.write(&mut b);
        let pitch_px = u32::from_le_bytes(b[0x18 + 0x38 + 0x18..0x18 + 0x38 + 0x1c].try_into().unwrap());
        assert_eq!(pitch_px, l.mip[0].pad_w * 4);
    }
}
