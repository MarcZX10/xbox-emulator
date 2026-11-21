use core::ffi::c_void;

pub const WAVE_FORMAT_PCM: u16 = 1;
pub const WAVE_FORMAT_ADPCM: u16 = 2;
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
pub const WAVE_FORMAT_XMA: u16 = 0x165;
pub const WAVE_FORMAT_XMA2: u16 = 0x166;
pub const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const XAPO_E_FORMAT_UNSUPPORTED: i32 = 0x8897_0001u32 as i32;
pub const XAPO_FLAG_CHANNELS_MUST_MATCH: u32 = 1;
pub const XAPO_FLAG_FRAMERATE_MUST_MATCH: u32 = 2;
pub const XAPO_FLAG_BITSPERSAMPLE_MUST_MATCH: u32 = 4;

const KSDATAFORMAT_TAIL: [u8; 12] = [0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71];
const ADPCM_COEF1: [i16; 7] = [256, 512, 0, 192, 240, 460, 392];
const ADPCM_COEF2: [i16; 7] = [0, -256, 0, 64, 0, -208, -232];
const DEFAULT_MASKS: [u32; 8] = [4, 3, 11, 51, 59, 1551, 1807, 1599];

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemAlloc(size: usize) -> *mut c_void;
}

unsafe fn u8_at(f: *const u8, o: usize) -> u8 {
    *f.add(o)
}

unsafe fn u16_at(f: *const u8, o: usize) -> u16 {
    core::ptr::read_unaligned(f.add(o) as *const u16)
}

unsafe fn i16_at(f: *const u8, o: usize) -> i16 {
    core::ptr::read_unaligned(f.add(o) as *const i16)
}

unsafe fn u32_at(f: *const u8, o: usize) -> u32 {
    core::ptr::read_unaligned(f.add(o) as *const u32)
}

unsafe fn put16(f: *mut u8, o: usize, v: u16) {
    core::ptr::write_unaligned(f.add(o) as *mut u16, v)
}

unsafe fn put32(f: *mut u8, o: usize, v: u32) {
    core::ptr::write_unaligned(f.add(o) as *mut u32, v)
}

unsafe fn put_subformat(f: *mut u8, tag: u32) {
    put32(f, 0x18, tag);
    core::ptr::copy_nonoverlapping(KSDATAFORMAT_TAIL.as_ptr(), f.add(0x1c), 12);
}

pub unsafe fn format_tag(f: *const u8) -> u16 {
    let tag = u16_at(f, 0);
    if tag != WAVE_FORMAT_EXTENSIBLE {
        return tag;
    }
    if core::slice::from_raw_parts(f.add(0x1c), 12) == KSDATAFORMAT_TAIL {
        u16_at(f, 0x18)
    } else {
        0
    }
}

unsafe fn adpcm_valid(f: *const u8) -> bool {
    let ch = u16_at(f, 2);
    let mut ok = (1..=2).contains(&ch) && u16_at(f, 0xe) == 4 && u16_at(f, 0x10) == 0x20;
    if !ok {
        return false;
    }
    if u16_at(f, 0x14) != 7 {
        ok = false;
    } else {
        for i in 0..7 {
            if i16_at(f, 0x16 + 4 * i) != ADPCM_COEF1[i] || i16_at(f, 0x18 + 4 * i) != ADPCM_COEF2[i] {
                ok = false;
            }
        }
    }
    let ch = ch as i32;
    let spb = u16_at(f, 0x12);
    let want = ((u16_at(f, 0xc) as i32 - 7 * ch) * 8) / (4 * ch) + 2;
    ok && spb as i32 == want && spb != 0
}

unsafe fn xma2_valid(f: *const u8) -> bool {
    let ch = u16_at(f, 2);
    let base = u16_at(f, 0xc) as u32 == 2 * ch as u32 && u16_at(f, 0xe) == 16;
    if u16_at(f, 0x10) != 0x22 {
        return false;
    }
    let enc = u8_at(f, 0x31) >= 3 && base;
    let bpb = u32_at(f, 0x1c);
    let mut ok = bpb != 0 && u16_at(f, 0x32) != 0 && enc && bpb <= 0x7f_f800;
    let mask = u32_at(f, 0x14);
    if mask != 0 && mask.count_ones() != ch as u32 {
        ok = false;
    }
    let encoded = u32_at(f, 0x18);
    ok = ok && u16_at(f, 0x12) as u32 == (ch as u32 + 1) >> 1;
    ok = ok && u32_at(f, 0x20).wrapping_add(u32_at(f, 0x24)) <= encoded;
    ok && u32_at(f, 0x28).wrapping_add(u32_at(f, 0x2c)) <= encoded
}

unsafe fn xma_valid(f: *const u8) -> bool {
    if u16_at(f, 0x10) < 0x20 {
        return false;
    }
    let streams = u16_at(f, 8) as usize;
    let rate = u32_at(f, 0x10);
    let mut ok = u16_at(f, 2) == 16 && u8_at(f, 0xb) == 2 && streams != 0 && rate != 0;
    for k in 0..streams {
        let s = 0x1c + 0x14 * k;
        let sub = u8_at(f, s);
        ok = ok && u32_at(f, s - 0xc) == rate && sub & 0xf <= 4 && sub & 0xf0 <= 0x30;
        let chans = u8_at(f, s + 1);
        if chans != 1 && chans != 2 {
            ok = false;
        }
        let mask = u16_at(f, s + 2);
        if mask != 0 && mask.count_ones() != chans as u32 {
            ok = false;
        }
    }
    ok
}

pub unsafe fn wave_format_valid(f: *const u8) -> bool {
    let raw = u16_at(f, 0);
    let ext = raw == WAVE_FORMAT_EXTENSIBLE;
    let ch = u16_at(f, 2);
    let rate = u32_at(f, 4);
    let avg = u32_at(f, 8);
    let align = u16_at(f, 0xc);
    let bits = u16_at(f, 0xe);
    let basic = raw == WAVE_FORMAT_XMA || (ch != 0 && avg != 0);
    let mut ok = basic;
    let sized = |container: u16, sample_ok: bool, ok: &mut bool| {
        let expect = (ch as i32 * container as i32) / 8;
        if !(align as i32 == expect && avg == (align as u32).wrapping_mul(rate) && sample_ok) {
            *ok = false;
        }
    };
    match format_tag(f) {
        WAVE_FORMAT_PCM => {
            let mut sample_ok = (bits.wrapping_sub(8)) & 0xffe7 == 0;
            let mut fail = false;
            if ext {
                let valid = u16_at(f, 0x12);
                if valid > 32 || (0x1_0111_0101u64 >> valid) & 1 == 0 {
                    sample_ok = false;
                }
                fail = valid > bits;
            }
            if fail {
                ok = false;
            } else {
                sized(bits, sample_ok, &mut ok);
            }
        }
        WAVE_FORMAT_ADPCM => ok = adpcm_valid(f) && basic,
        WAVE_FORMAT_IEEE_FLOAT => {
            let sample_ok = bits == 32;
            if ext && u16_at(f, 0x12) != 32 && u16_at(f, 0x12) != 0 {
                ok = false;
            } else {
                sized(bits, sample_ok, &mut ok);
            }
        }
        0x161 | 0x162 => {
            let mut sample_ok = bits == 16;
            if ext && u16_at(f, 0x12) != 16 {
                sample_ok = false;
            }
            ok = ok && align != 0 && sample_ok && avg != 0;
        }
        WAVE_FORMAT_XMA => ok = ok && xma_valid(f),
        WAVE_FORMAT_XMA2 => ok = ok && xma2_valid(f),
        _ => {}
    }
    if ext {
        if u16_at(f, 0x10) < 22 {
            ok = false;
        } else {
            let mask = u32_at(f, 0x14);
            if mask != 0 && mask.count_ones() != ch as u32 {
                ok = false;
            }
        }
    }
    ok
}

pub unsafe fn channel_mask(f: *const u8, use_default: bool) -> u32 {
    let tag = u16_at(f, 0);
    if tag == WAVE_FORMAT_XMA {
        let mut m = 0u32;
        for k in 0..u16_at(f, 8) as usize {
            let v = u16_at(f, 0x1e + 0x14 * k) as u32;
            m |= (v & 0xff) | (v >> 8);
        }
        return m;
    }
    if tag == WAVE_FORMAT_XMA2 || tag == WAVE_FORMAT_EXTENSIBLE {
        return u32_at(f, 0x14);
    }
    match u16_at(f, 2) {
        1 => 4,
        2 => 3,
        n if use_default && (1..=8).contains(&n) => DEFAULT_MASKS[n as usize - 1],
        _ => 0,
    }
}

pub unsafe fn clone_as_extensible(f: *const u8) -> *mut u8 {
    let tag = u16_at(f, 0);
    let size = if tag == WAVE_FORMAT_XMA || tag == WAVE_FORMAT_PCM { 0x28 } else { (u16_at(f, 0x10) as usize + 0x12).max(0x28) };
    let out = CoTaskMemAlloc(size) as *mut u8;
    if out.is_null() {
        return out;
    }
    let copy = match tag {
        WAVE_FORMAT_XMA => 0x20,
        WAVE_FORMAT_PCM => 0x10,
        _ => u16_at(f, 0x10) as usize + 0x12,
    };
    core::ptr::copy_nonoverlapping(f, out, copy);
    core::ptr::write_bytes(out.add(copy), 0, size - copy);
    if tag != WAVE_FORMAT_EXTENSIBLE {
        put16(out, 0x12, u16_at(f, 0xe));
        put32(out, 0x14, channel_mask(f, true));
        put16(out, 0x10, 22);
        put16(out, 0, WAVE_FORMAT_EXTENSIBLE);
        put_subformat(out, tag as u32);
    }
    out
}

pub unsafe fn validate_float_format(f: *mut u8, overwrite: bool) -> i32 {
    if !wave_format_valid(f) {
        return E_INVALIDARG;
    }
    let mut hr = 0;
    let ext = u16_at(f, 0) == WAVE_FORMAT_EXTENSIBLE;
    if format_tag(f) != WAVE_FORMAT_IEEE_FLOAT {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            if ext {
                put_subformat(f, WAVE_FORMAT_IEEE_FLOAT as u32);
            } else {
                put16(f, 0, WAVE_FORMAT_IEEE_FLOAT);
            }
        }
    }
    if !(1..=64).contains(&u16_at(f, 2)) {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            put16(f, 2, 64);
        }
    }
    if !(1000..=200_000).contains(&u32_at(f, 4)) {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            put32(f, 4, 200_000);
        }
    }
    if u16_at(f, 0xe) != 32 {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            put16(f, 0xe, 32);
        }
    }
    if ext && u16_at(f, 0x12) != 32 {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            put16(f, 0x12, 32);
        }
    }
    hr
}

pub unsafe fn match_formats(flags: u32, other: *const u8, f: *mut u8, overwrite: bool) -> i32 {
    let mut hr = 0;
    if flags & XAPO_FLAG_CHANNELS_MUST_MATCH != 0 && u16_at(f, 2) != u16_at(other, 2) {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            put16(f, 2, u16_at(other, 2));
        }
    }
    if flags & XAPO_FLAG_FRAMERATE_MUST_MATCH != 0 && u32_at(f, 4) != u32_at(other, 4) {
        hr = XAPO_E_FORMAT_UNSUPPORTED;
        if overwrite {
            put32(f, 4, u32_at(other, 4));
        }
    }
    if flags & XAPO_FLAG_BITSPERSAMPLE_MUST_MATCH != 0 {
        let mut bits = u16_at(f, 0xe);
        if bits != u16_at(other, 0xe) {
            hr = XAPO_E_FORMAT_UNSUPPORTED;
            if overwrite {
                put16(f, 0xe, u16_at(other, 0xe));
                bits = u16_at(other, 0xe);
            }
        }
        let other_ext = u16_at(other, 0) == WAVE_FORMAT_EXTENSIBLE;
        let mine_ext = u16_at(f, 0) == WAVE_FORMAT_EXTENSIBLE;
        let want = match (other_ext, mine_ext) {
            (true, true) => Some(u16_at(other, 0x12)),
            (true, false) => {
                if bits != u16_at(other, 0x12) {
                    hr = XAPO_E_FORMAT_UNSUPPORTED;
                }
                None
            }
            (false, true) => Some(u16_at(other, 0xe)),
            (false, false) => None,
        };
        if let Some(w) = want {
            if u16_at(f, 0x12) != w {
                hr = XAPO_E_FORMAT_UNSUPPORTED;
                if overwrite {
                    put16(f, 0x12, w);
                }
            }
        }
    }
    hr
}
