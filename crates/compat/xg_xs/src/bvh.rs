use core::ffi::c_void;
use core::ptr::{read_unaligned, write_unaligned};

const S_OK: i32 = 0;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;

const FMT_R32_UINT: u32 = 0x2a;
const FMT_R16_UINT: u32 = 0x39;

const NAN: u32 = 0x7fc0_0000;
const FLT_MAX: f32 = f32::MAX;
const HALF_SCALE: u32 = 0x7780_0000;
const HALF_UNSCALE: u32 = 0x0780_0000;
const HALF_MIN_NORMAL: u32 = 0x3880_0000;
const HALF_MIN_NORMAL_NEG: u32 = 0xb880_0000;

const SLOT: [u32; 3] = [2, 0, 1];
const LAST: [u32; 3] = [1, 3, 2];

pub type DecompressFn = unsafe extern "system" fn(*mut BvhComputer, *const u8, *mut u8, usize, *const *const u8, *const u32, *const u32, *const *const u8, *const u32, u32) -> i32;
pub type CompareFn = unsafe extern "system" fn(*mut BvhComputer, *const u8, *const u8) -> i32;

#[repr(C)]
pub struct BvhComputer {
    vtbl: *const Vtbl,
}

#[repr(C)]
struct Vtbl {
    decompress: DecompressFn,
    compare: CompareFn,
}

static VTBL: Vtbl = Vtbl { decompress: decompress_bvh, compare: compare_bvh };

extern "system" {
    fn OutputDebugStringA(s: *const u8);
}

fn debug(msg: &str) {
    let mut s = String::with_capacity(msg.len() + 16);
    s.push_str("XG ERROR: ");
    s.push_str(msg);
    s.push_str("\n\0");
    unsafe { OutputDebugStringA(s.as_ptr()) };
}

pub unsafe fn create(out: *mut *mut c_void) -> i32 {
    let obj = Box::into_raw(Box::new(BvhComputer { vtbl: &VTBL }));
    *out = obj as *mut c_void;
    S_OK
}

#[inline]
unsafe fn r8(p: *const u8) -> u8 {
    *p
}

#[inline]
unsafe fn r16(p: *const u8) -> u16 {
    read_unaligned(p as *const u16)
}

#[inline]
unsafe fn r32(p: *const u8) -> u32 {
    read_unaligned(p as *const u32)
}

#[inline]
unsafe fn rf(p: *const u8) -> f32 {
    f32::from_bits(r32(p))
}

#[inline]
unsafe fn w16(p: *mut u8, v: u16) {
    write_unaligned(p as *mut u16, v)
}

#[inline]
unsafe fn w32(p: *mut u8, v: u32) {
    write_unaligned(p as *mut u32, v)
}

#[inline]
unsafe fn w64(p: *mut u8, v: u64) {
    write_unaligned(p as *mut u64, v)
}

#[inline]
unsafe fn wf(p: *mut u8, v: f32) {
    w32(p, v.to_bits())
}

#[inline]
fn fmin(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

#[inline]
fn fmax(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

#[inline]
fn half_to_f32(h: u16) -> f32 {
    let mag = (h & 0x7fff) as u32;
    let sign = (h & 0x8000) as u32;
    if mag < 0x7c00 {
        f32::from_bits(((sign << 3) | mag) << 13) * f32::from_bits(HALF_SCALE)
    } else if mag == 0x7c00 {
        f32::from_bits((sign | 0x7f80) << 16)
    } else {
        f32::from_bits(NAN)
    }
}

#[inline]
fn pack_half(v: f32) -> u16 {
    let b = (v * f32::from_bits(HALF_UNSCALE)).to_bits();
    (((b >> 16) as u16) & 0x8000) | ((b >> 13) as u16)
}

fn half_lower(v: f32) -> u16 {
    let neg = 0.0f32 > v;
    let f = f32::from_bits(v.to_bits().wrapping_add(if neg { 0x1fff } else { 0 }) & 0xffff_e000);
    let g = if neg {
        fmin(f32::from_bits(HALF_MIN_NORMAL_NEG), f)
    } else if f32::from_bits(HALF_MIN_NORMAL) > f {
        0.0
    } else {
        f
    };
    pack_half(g)
}

fn half_upper(v: f32) -> u16 {
    let pos = v > 0.0f32;
    let f = f32::from_bits(v.to_bits().wrapping_add(if pos { 0x1fff } else { 0 }) & 0xffff_e000);
    let g = if pos {
        fmax(f32::from_bits(HALF_MIN_NORMAL), f)
    } else if f > f32::from_bits(HALF_MIN_NORMAL_NEG) {
        0.0
    } else {
        f
    };
    pack_half(g)
}

fn vertex_format_supported(fmt: u32) -> bool {
    matches!(fmt, 6 | 10 | 11 | 13 | 16 | 24 | 28 | 31 | 34 | 35 | 37 | 49 | 51)
}

unsafe fn fetch_vertex(fmt: u32, v: *const u8, out: *mut u8) {
    match fmt {
        6 => {
            let x = r32(v);
            let y = r32(v.add(4));
            let z = r32(v.add(8));
            w32(out, x);
            w32(out.add(4), y);
            w32(out.add(8), z);
        }
        16 => {
            let x = r32(v);
            let y = r32(v.add(4));
            w32(out, x);
            w32(out.add(4), y);
            w32(out.add(8), 0);
        }
        10 | 34 => {
            let n = if fmt == 10 { 3 } else { 2 };
            for c in 0..n {
                wf(out.add(c * 4), half_to_f32(r16(v.add(c * 2))));
            }
            if n == 2 {
                w32(out.add(8), 0);
            }
        }
        11 | 35 => {
            let n = if fmt == 11 { 3 } else { 2 };
            for c in 0..n {
                wf(out.add(c * 4), r16(v.add(c * 2)) as i32 as f32 / 65535.0f32);
            }
            if n == 2 {
                w32(out.add(8), 0);
            }
        }
        13 | 37 => {
            let n = if fmt == 13 { 3 } else { 2 };
            for c in 0..n {
                wf(out.add(c * 4), fmax(-1.0f32, r16(v.add(c * 2)) as i16 as i32 as f32 / 32767.0f32));
            }
            if n == 2 {
                w32(out.add(8), 0);
            }
        }
        24 => {
            let d = r32(v);
            let x = (d & 0x3ff) as f32 / 1023.0f32;
            let y = ((d >> 10) & 0x3ff) as f32 / 1023.0f32;
            let z = ((d >> 20) & 0x3ff) as f32 / 1023.0f32;
            wf(out, x);
            wf(out.add(4), y);
            wf(out.add(8), z);
        }
        28 | 49 => {
            let n = if fmt == 28 { 3 } else { 2 };
            for c in 0..n {
                wf(out.add(c * 4), r8(v.add(c)) as i32 as f32 / 255.0f32);
            }
            if n == 2 {
                w32(out.add(8), 0);
            }
        }
        31 | 51 => {
            let n = if fmt == 31 { 3 } else { 2 };
            for c in 0..n {
                wf(out.add(c * 4), fmax(-1.0f32, r8(v.add(c)) as i8 as i32 as f32 / 127.0f32));
            }
            if n == 2 {
                w32(out.add(8), 0);
            }
        }
        _ => {}
    }
}

unsafe fn read_index(p: *const u8, mask: u32) -> u32 {
    if mask == 0xffff {
        r16(p) as u32
    } else {
        r32(p) & mask
    }
}

fn last_vertex_hit(sel: u32) -> bool {
    sel < 3 && LAST[SLOT[sel as usize] as usize] == 3
}

fn slot_of(sel: u32) -> Option<usize> {
    if sel < 3 { Some(SLOT[sel as usize] as usize) } else { None }
}

struct Geometry {
    vbs: *const *const u8,
    strides: *const u32,
    vfmts: *const u32,
    ibs: *const *const u8,
    ifmts: *const u32,
    count: u32,
}

unsafe fn build_leaves(n: u32, refs: *const u8, leaves: *mut u8, g: &Geometry) -> i32 {
    for i in 0..n {
        let r = refs.add(i as usize * 12);
        let l = leaves.add(i as usize * 64);
        w32(l.add(0x30), r32(r));
        w32(l.add(0x34), r32(r.add(4)));
        let d2 = r32(r.add(8));
        w32(l.add(0x3c), ((d2 >> 24) & 0xf) | (((d2 >> 28) & 0xf) << 8));
        let geom = r32(r) >> 8;
        if geom >= g.count {
            debug(&format!("Found a Leaf referencing a non-existent geometry : Found {}, but DecompressBVH called with only {} ", geom, g.count));
            return E_UNEXPECTED;
        }
        let gi = geom as usize;
        let (isz, mask) = if *g.ifmts.add(gi) == FMT_R32_UINT { (4u32, u32::MAX) } else { (2u32, 0xffffu32) };
        let ib = *g.ibs.add(gi);
        let second = d2 & 0x00ff_ffff;
        let mut pair: Option<(*const u8, u32)> = None;
        if second != 0x00ff_ffff {
            w32(l.add(0x38), second);
            let s28 = (d2 >> 28) & 3;
            let s30 = d2 >> 30;
            let last = if last_vertex_hit(s28) {
                0
            } else if last_vertex_hit(s30) {
                1
            } else if last_vertex_hit(3u32.wrapping_sub(s28).wrapping_sub(s30)) {
                2
            } else {
                debug(&format!("Could not find the last vertex for Leaf number {}", i));
                return E_UNEXPECTED;
            };
            let off = second.wrapping_mul(isz).wrapping_mul(3);
            pair = Some((ib.add(off as usize), last));
        } else {
            w32(l.add(0x38), u32::MAX);
        }
        let prim = r32(r.add(4));
        let first = ib.add(prim.wrapping_mul(isz).wrapping_mul(3) as usize);
        let s24 = (d2 >> 24) & 3;
        let s26 = (d2 >> 26) & 3;
        let slots = [slot_of(s24), slot_of(s26), slot_of(3u32.wrapping_sub(s26).wrapping_sub(s24))];
        if pair.is_none() {
            for k in 0..3 {
                w32(l.add(0x24 + k * 4), NAN);
            }
        }
        if prim == u32::MAX {
            for k in 0..9 {
                w32(l.add(k * 4), NAN);
            }
            continue;
        }
        let fmt = *g.vfmts.add(gi);
        if !vertex_format_supported(fmt) {
            return E_INVALIDARG;
        }
        let vb = *g.vbs.add(gi);
        let stride = *g.strides.add(gi);
        for k in 0..3 {
            let idx = read_index(first.add((k as u32 * isz) as usize), mask);
            let v = vb.add(idx.wrapping_mul(stride) as usize);
            if let Some(s) = slots[k] {
                fetch_vertex(fmt, v, l.add(s * 12));
            }
        }
        if let Some((p, last)) = pair {
            let idx = read_index(p.add(last.wrapping_mul(isz) as usize), mask);
            fetch_vertex(fmt, vb.add(idx.wrapping_mul(stride) as usize), l.add(0x24));
        }
    }
    S_OK
}

unsafe fn transform_leaves(n: u32, leaves: *mut u8, bounds: *const u8, mode: u32) {
    if mode == 0x007f_7f7f {
        return;
    }
    let mut c = [0.0f32; 3];
    if (mode >> 30) & 1 != 0 {
        for a in 0..3 {
            let lo = rf(bounds.add(a * 4));
            let hi = rf(bounds.add(0xc + a * 4));
            c[a] = if lo > 0.0f32 || 0.0f32 > hi { (hi + lo) * 0.5f32 } else { 0.0 };
        }
    }
    let s = if mode >> 31 != 0 {
        [f32::from_bits((mode & 0xff) << 23), f32::from_bits((mode & 0xff00) << 15), f32::from_bits((mode & 0xff_0000) << 7)]
    } else {
        [1.0f32; 3]
    };
    for i in 0..n as usize {
        let l = leaves.add(i * 64);
        let verts = if r32(l.add(0x38)) != 0x00ff_ffff { 4 } else { 3 };
        for v in 0..verts {
            let p = l.add(v * 12);
            for a in 0..3 {
                let q = p.add(a * 4);
                wf(q, (rf(q) - c[a]) * s[a]);
            }
        }
    }
}

struct Ctx {
    stream: *const u8,
    exc: *const u8,
    parents: *mut u8,
    hier_off: u32,
    hier_base: *mut u8,
    leaf_off: u32,
    leaf_base: *mut u8,
    leaf_parents: bool,
}

#[derive(Clone, Copy)]
struct Bad;

const EMPTY: [f32; 6] = [FLT_MAX, FLT_MAX, FLT_MAX, -FLT_MAX, -FLT_MAX, -FLT_MAX];

#[inline]
fn is_box(p: u32) -> bool {
    (p & 7).wrapping_sub(4) <= 1
}

#[inline]
fn parent_index(p: u32, leaf_off: u32) -> usize {
    (((p & !7) << 3).wrapping_sub(leaf_off) >> 6) as usize
}

unsafe fn leaf_bounds(c: &Ctx, p: u32) -> [f32; 6] {
    let li = (p >> 3).wrapping_sub(c.leaf_off >> 6);
    let l = c.leaf_base.add((li as usize) << 6);
    let verts = if r32(l.add(0x38)) != u32::MAX { 4 } else { 3 };
    let mut b = EMPTY;
    for v in 0..verts {
        for a in 0..3 {
            let x = rf(l.add(v * 12 + a * 4));
            b[a] = fmin(x, b[a]);
            b[a + 3] = fmax(x, b[a + 3]);
        }
    }
    b
}

unsafe fn rebuild_node(c: &mut Ctx, p: u32, out: &mut [f32; 6]) -> Result<(), Bad> {
    let index = p >> 3;
    let kind = p & 7;
    let node = c.hier_base.add((index.wrapping_sub(c.hier_off >> 6) << 6) as usize);
    let s = c.stream;
    c.stream = s.add(16);
    let mut child_bounds = [EMPTY; 4];
    for i in 0..4 {
        let ch = r32(s.add(i * 4));
        w32(node.add(i * 4), ch);
        let bx = is_box(ch);
        if ch != u32::MAX && (bx || c.leaf_parents) {
            w32(c.parents.add(parent_index(ch, c.leaf_off) * 4), p);
        }
        if bx {
            rebuild_node(c, ch, &mut child_bounds[i])?;
        }
    }
    for i in 0..4 {
        let ch = r32(s.add(i * 4));
        if (ch as i32) <= 0 {
            match kind {
                4 => {
                    let q = node.add(0x10 + i * 12);
                    w32(q, 0x7c00_7c00);
                    w32(q.add(4), 0xfc00_7c00);
                    w16(q.add(8), 0xfc00);
                    w16(q.add(10), 0xfc00);
                }
                5 => {
                    let q = node.add(0x10 + i * 24);
                    for k in 0..6 {
                        wf(q.add(k * 4), EMPTY[k]);
                    }
                    w64(node.add(0x70), 0);
                    w64(node.add(0x78), 0);
                }
                _ => return Err(Bad),
            }
            continue;
        }
        let ck = ch & 7;
        let mut v = if ck == 0 {
            leaf_bounds(c, ch)
        } else if ck == 4 || ck == 5 {
            child_bounds[i]
        } else {
            return Err(Bad);
        };
        let e = c.exc;
        let hit = r32(e) == index && (r8(e.add(4)) >> 6) as usize == i;
        let mask = r8(e.add(4));
        let mut q = e.add(5);
        match kind {
            4 => {
                let dst = node.add(0x10 + i * 12);
                for k in 0..6 {
                    if hit && mask & (1 << k) != 0 {
                        let h = r16(q);
                        w16(dst.add(k * 2), h);
                        v[k] = half_to_f32(h);
                        q = q.add(2);
                    } else {
                        w16(dst.add(k * 2), if k < 3 { half_lower(v[k]) } else { half_upper(v[k]) });
                    }
                }
            }
            5 => {
                let dst = node.add(0x10 + i * 24);
                for k in 0..6 {
                    if hit && mask & (1 << k) != 0 {
                        let bits = r32(q);
                        w32(dst.add(k * 4), bits);
                        v[k] = f32::from_bits(bits);
                        q = q.add(4);
                    } else {
                        wf(dst.add(k * 4), v[k]);
                    }
                }
            }
            _ => return Err(Bad),
        }
        if hit {
            c.exc = q;
        }
        for k in 0..3 {
            out[k] = fmin(v[k], out[k]);
            out[k + 3] = fmax(v[k + 3], out[k + 3]);
        }
    }
    Ok(())
}

unsafe fn flush(dst: *mut u8, size: usize) {
    use core::arch::x86_64::{_mm_clflush, _mm_mfence};
    _mm_mfence();
    let start = (dst as usize) & !0x3f;
    let end = start.wrapping_add(size).wrapping_add(0x3f) & !0x3f;
    let mut a = start;
    while a < end {
        _mm_clflush(a as *const u8);
        a += 0x40;
    }
    _mm_mfence();
}

unsafe fn count_leaves(prims: u32, base: *const u8, stride: usize, field: usize) -> u32 {
    let mut n = 0u32;
    if prims != 0 {
        let mut left = prims;
        loop {
            let e = r32(base.add(n as usize * stride + field));
            n += 1;
            if e != u32::MAX {
                left -= 1;
            }
            if left == 0 {
                break;
            }
        }
        if n > prims && n & 1 != 0 {
            n += 1;
        }
    }
    n
}

pub unsafe extern "system" fn decompress_bvh(
    _this: *mut BvhComputer,
    src: *const u8,
    dst: *mut u8,
    dst_size: usize,
    vertex_buffers: *const *const u8,
    vertex_strides: *const u32,
    vertex_formats: *const u32,
    index_buffers: *const *const u8,
    index_formats: *const u32,
    geometry_count: u32,
) -> i32 {
    let flags = r32(src);
    if flags & 0x1_0000 == 0 {
        debug("DecompressBVH was been called with a non-compressed BVH");
        return E_INVALIDARG;
    }
    for i in 0..geometry_count as usize {
        let f = *index_formats.add(i);
        if f != FMT_R32_UINT && f != FMT_R16_UINT {
            debug(&format!("Geometry {} has an unsupported Index format. Currently the code only supports DXGI_FORMAT_R32_UINT and DXGI_FORMAT_R16_UINT formats", f));
            return E_INVALIDARG;
        }
    }
    w32(dst, flags ^ 0x1_0000);
    w32(dst.add(4), r32(src.add(4)));
    w32(dst.add(8), r32(src.add(8)));
    w32(dst.add(0xc), r32(src.add(0xc)));
    let mid = read_unaligned(src.add(0x10) as *const [u8; 16]);
    write_unaligned(dst.add(0x10) as *mut [u8; 16], mid);
    let tail = read_unaligned(src.add(0x20) as *const u64);
    w64(dst.add(0x20), tail);
    w64(dst.add(0x28), 0);
    for o in [0x30usize, 0x34, 0x38, 0x3c] {
        w32(dst.add(o), r32(src.add(o)));
    }
    let prims = r32(src.add(0xc));
    let refs = src.add(r32(src.add(0x28)) as usize);
    let leaves = dst.add(r32(src.add(0x34)) as usize);
    let n = count_leaves(prims, refs, 12, 4);
    let g = Geometry {
        vbs: vertex_buffers,
        strides: vertex_strides,
        vfmts: vertex_formats,
        ibs: index_buffers,
        ifmts: index_formats,
        count: geometry_count,
    };
    let hr = build_leaves(n, refs, leaves, &g);
    transform_leaves(n, leaves, src.add(0x10), r32(src.add(0x3c)));
    if hr != S_OK {
        return hr;
    }
    let stream = ((refs as usize + n as usize * 12 + 3) & !3) as *const u8;
    let leaf_off = r32(src.add(0x34));
    let hier_off = ((n as u64) * 64 + leaf_off as u64 + 0x7f) & !0x7f;
    let end = (r32(src.add(4)).wrapping_add(1) << 6) as u64;
    let hier_base = dst.add(hier_off as usize);
    if end > hier_off {
        core::ptr::write_bytes(hier_base, 0, (end - hier_off) as usize);
    }
    let root = r32(src.add(0x30));
    let parents = dst.offset(r32(src.add(0x38)) as i32 as isize);
    let leaf_parents = r32(src.add(0xc)) < n;
    if root & 7 == 4 || root & 7 == 5 {
        let mut c = Ctx {
            stream,
            exc: src.add(0x40),
            parents,
            hier_off: hier_off as u32,
            hier_base,
            leaf_off,
            leaf_base: leaves,
            leaf_parents,
        };
        let mut bounds = EMPTY;
        if rebuild_node(&mut c, root, &mut bounds).is_err() {
            return E_INVALIDARG;
        }
    }
    let root = r32(src.add(0x30));
    w32(parents.add(parent_index(root, r32(src.add(0x34))) * 4), u32::MAX);
    flush(dst, dst_size);
    S_OK
}

unsafe fn same(a: *const u8, b: *const u8, n: usize) -> bool {
    n == 0 || core::slice::from_raw_parts(a, n) == core::slice::from_raw_parts(b, n)
}

unsafe fn parents_match(pa: *const u8, pb: *const u8, hier_off: u32, hier_b: *const u8, leaf_off: u32, p: u32, leaf_parents: bool) -> bool {
    let node = hier_b.add((((p >> 3).wrapping_sub(hier_off >> 6)) << 6) as usize);
    let mut ok = true;
    for i in 0..4 {
        if !ok {
            break;
        }
        let ch = r32(node.add(i * 4));
        let bx = is_box(ch);
        if ch == u32::MAX || !(bx || leaf_parents) {
            continue;
        }
        let pi = parent_index(ch, leaf_off) * 4;
        if r32(pa.add(pi)) != r32(pb.add(pi)) {
            return false;
        }
        if bx {
            ok &= parents_match(pa, pb, hier_off, hier_b, leaf_off, ch, leaf_parents);
        }
    }
    ok
}

pub unsafe extern "system" fn compare_bvh(_this: *mut BvhComputer, a: *const u8, b: *const u8) -> i32 {
    if !same(a, b, 0x40) {
        debug("Comparison of the BVH Headers failed. The two BVH's do not match");
        return 1;
    }
    let leaf_off = r32(a.add(0x34));
    let n = count_leaves(r32(a.add(0xc)), a.add((leaf_off & !0x3f) as usize), 64, 0x34);
    let leaf_bytes = n << 6;
    let hier_off = leaf_bytes.wrapping_add(leaf_off).wrapping_add(0x7f) & !0x7f;
    let hier_len = (r32(a.add(4)) << 6).wrapping_sub(hier_off);
    if !same(a.add(hier_off as usize), b.add(hier_off as usize), hier_len as usize) {
        debug("Comparison of the BVH Hierarchy data failed. The two BVH's do not match");
        return 1;
    }
    let po = r32(a.add(0x38)) as i32;
    let leaf_parents = po >= 0x40;
    if !parents_match(a.offset(po as isize), b.offset(po as isize), hier_off, b.add(hier_off as usize), leaf_off, r32(a.add(0x30)), leaf_parents) {
        debug("Found errors in the Parent data. The two BVH's do not match");
        return 1;
    }
    if !same(a.add(leaf_off as usize), b.add(leaf_off as usize), leaf_bytes as usize) {
        debug("Found errors in the Leaf (Element) data. The two BVH's do not match");
        return 1;
    }
    0
}
