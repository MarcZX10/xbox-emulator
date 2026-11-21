use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

pub const MAGIC: [u8; 4] = *b"TBPS";
pub const VERSION: u16 = 1;
pub const HEADER: usize = 16;
pub const GFX_PARTS: usize = 14;
pub const COMPUTE_PARTS: usize = 4;
pub const GFX_DESC_SIZE: usize = 0x290;
pub const COMPUTE_DESC_SIZE: usize = 0x38;
pub const GFX_STAGES: [usize; 5] = [0x08, 0x18, 0x28, 0x38, 0x48];
const GFX_PTR_FIELDS: [usize; 10] = [0x00, 0x08, 0x18, 0x28, 0x38, 0x48, 0x58, 0x68, 0x228, 0x278];
const COMPUTE_PTR_FIELDS: [usize; 3] = [0x00, 0x08, 0x20];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    GfxDesc = 1,
    GfxInput = 2,
    ComputeDesc = 3,
    Code = 4,
    Pad = 5,
}

impl Kind {
    fn from_u16(v: u16) -> Option<Kind> {
        Some(match v {
            1 => Kind::GfxDesc,
            2 => Kind::GfxInput,
            3 => Kind::ComputeDesc,
            4 => Kind::Code,
            5 => Kind::Pad,
            _ => return None,
        })
    }
}

pub fn part(kind: Kind, index: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + payload.len());
    v.extend_from_slice(&MAGIC);
    v.extend_from_slice(&VERSION.to_le_bytes());
    v.extend_from_slice(&(kind as u16).to_le_bytes());
    v.extend_from_slice(&index.to_le_bytes());
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

pub fn parse(b: &[u8]) -> Option<(Kind, u32, &[u8])> {
    if b.len() < HEADER || b[0..4] != MAGIC {
        return None;
    }
    if u16::from_le_bytes([b[4], b[5]]) != VERSION {
        return None;
    }
    let kind = Kind::from_u16(u16::from_le_bytes([b[6], b[7]]))?;
    let index = u32::from_le_bytes(b[8..12].try_into().ok()?);
    let len = u32::from_le_bytes(b[12..16].try_into().ok()?) as usize;
    let payload = b.get(HEADER..HEADER + len)?;
    Some((kind, index, payload))
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct InputElem {
    pub name: Vec<u8>,
    pub index: u32,
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub class: u32,
    pub step: u32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SoEntry {
    pub stream: u32,
    pub name: Vec<u8>,
    pub index: u32,
    pub start: u8,
    pub count: u8,
    pub slot: u8,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct GfxInput {
    pub elems: Vec<InputElem>,
    pub so: Vec<SoEntry>,
    pub strides: Vec<u32>,
}

struct Rd<'a> {
    b: &'a [u8],
    o: usize,
}

impl<'a> Rd<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.o..self.o.checked_add(n)?)?;
        self.o += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    fn name(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        if n > 256 {
            return None;
        }
        Some(self.bytes(n)?.to_vec())
    }
}

fn put_name(v: &mut Vec<u8>, n: &[u8]) {
    v.extend_from_slice(&(n.len() as u32).to_le_bytes());
    v.extend_from_slice(n);
}

pub fn encode_input(g: &GfxInput) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(g.elems.len() as u32).to_le_bytes());
    for e in &g.elems {
        put_name(&mut v, &e.name);
        for x in [e.index, e.format, e.slot, e.offset, e.class, e.step] {
            v.extend_from_slice(&x.to_le_bytes());
        }
    }
    v.extend_from_slice(&(g.so.len() as u32).to_le_bytes());
    for s in &g.so {
        put_name(&mut v, &s.name);
        v.extend_from_slice(&s.stream.to_le_bytes());
        v.extend_from_slice(&s.index.to_le_bytes());
        v.extend_from_slice(&[s.start, s.count, s.slot]);
    }
    v.extend_from_slice(&(g.strides.len() as u32).to_le_bytes());
    for s in &g.strides {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

pub fn decode_input(b: &[u8]) -> Option<GfxInput> {
    let mut r = Rd { b, o: 0 };
    let mut g = GfxInput::default();
    let n = r.u32()? as usize;
    if n > 64 {
        return None;
    }
    for _ in 0..n {
        let name = r.name()?;
        let mut f = [0u32; 6];
        for x in f.iter_mut() {
            *x = r.u32()?;
        }
        g.elems.push(InputElem { name, index: f[0], format: f[1], slot: f[2], offset: f[3], class: f[4], step: f[5] });
    }
    let n = r.u32()? as usize;
    if n > 64 {
        return None;
    }
    for _ in 0..n {
        let name = r.name()?;
        let stream = r.u32()?;
        let index = r.u32()?;
        let (start, count, slot) = (r.u8()?, r.u8()?, r.u8()?);
        g.so.push(SoEntry { stream, name, index, start, count, slot });
    }
    let n = r.u32()? as usize;
    if n > 4 {
        return None;
    }
    for _ in 0..n {
        g.strides.push(r.u32()?);
    }
    if r.o != b.len() {
        return None;
    }
    Some(g)
}

fn scrub(desc: &[u8], fields: &[usize]) -> Vec<u8> {
    let mut d = desc.to_vec();
    for &o in fields {
        d[o..o + 8].fill(0);
    }
    d
}

pub fn encode_gfx(desc: &[u8], input: &GfxInput, code: &[Option<Vec<u8>>; 5]) -> Vec<Vec<u8>> {
    let mut d = scrub(&desc[..GFX_DESC_SIZE], &GFX_PTR_FIELDS);
    d[0x280..0x288].fill(0);
    let mut parts = vec![part(Kind::GfxDesc, 0, &d), part(Kind::GfxInput, 1, &encode_input(input))];
    for (k, c) in code.iter().enumerate() {
        let i = 2 + 2 * k as u32;
        parts.push(part(Kind::Code, i, c.as_deref().unwrap_or(&[])));
        parts.push(part(Kind::Pad, i + 1, &[]));
    }
    while parts.len() < GFX_PARTS {
        let i = parts.len() as u32;
        parts.push(part(Kind::Pad, i, &[]));
    }
    parts
}

#[derive(Debug, PartialEq)]
pub struct GfxPlan {
    pub desc: Vec<u8>,
    pub input: GfxInput,
    pub code: [Option<Vec<u8>>; 5],
}

pub fn decode_gfx(parts: &[&[u8]]) -> Result<GfxPlan, String> {
    let mut desc = None;
    let mut input = None;
    let mut code: [Option<Vec<u8>>; 5] = Default::default();
    for (n, p) in parts.iter().enumerate() {
        if p.is_empty() {
            continue;
        }
        let (kind, index, payload) = parse(p).ok_or_else(|| format!("part {n}: not a TomBox PSO part"))?;
        match kind {
            Kind::GfxDesc if payload.len() == GFX_DESC_SIZE => desc = Some(payload.to_vec()),
            Kind::GfxInput => input = Some(decode_input(payload).ok_or_else(|| format!("part {n}: bad input layout"))?),
            Kind::Code if (2..12).contains(&index) && index % 2 == 0 => {
                if !payload.is_empty() {
                    code[(index as usize - 2) / 2] = Some(payload.to_vec());
                }
            }
            Kind::Pad => {}
            _ => return Err(format!("part {n}: unexpected kind {kind:?} index {index}")),
        }
    }
    let desc = desc.ok_or("no graphics desc part")?;
    let input = input.ok_or("no input-layout part")?;
    if code[0].is_none() {
        return Err("no vertex shader part".into());
    }
    Ok(GfxPlan { desc, input, code })
}

pub fn encode_compute(desc: &[u8], code: &[u8]) -> Vec<Vec<u8>> {
    let d = scrub(&desc[..COMPUTE_DESC_SIZE], &COMPUTE_PTR_FIELDS);
    let mut d = d;
    d[0x28..0x30].fill(0);
    vec![
        part(Kind::ComputeDesc, 0, &d),
        part(Kind::Pad, 1, &[]),
        part(Kind::Code, 2, code),
        part(Kind::Pad, 3, &[]),
    ]
}

pub fn decode_compute(parts: &[&[u8]]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut desc = None;
    let mut code = None;
    for (n, p) in parts.iter().enumerate() {
        if p.is_empty() {
            continue;
        }
        let (kind, index, payload) = parse(p).ok_or_else(|| format!("part {n}: not a TomBox PSO part"))?;
        match kind {
            Kind::ComputeDesc if payload.len() == COMPUTE_DESC_SIZE => desc = Some(payload.to_vec()),
            Kind::Code if index == 2 && !payload.is_empty() => code = Some(payload.to_vec()),
            Kind::Pad => {}
            _ => return Err(format!("part {n}: unexpected kind {kind:?} index {index}")),
        }
    }
    Ok((desc.ok_or("no compute desc part")?, code.ok_or("no compute shader part")?))
}

#[repr(C)]
struct BlobVt {
    qi: unsafe extern "system" fn(*mut Blob, *const [u8; 16], *mut *mut c_void) -> i32,
    addref: unsafe extern "system" fn(*mut Blob) -> u32,
    release: unsafe extern "system" fn(*mut Blob) -> u32,
    ptr: unsafe extern "system" fn(*mut Blob) -> *mut c_void,
    size: unsafe extern "system" fn(*mut Blob) -> usize,
}

#[repr(C)]
pub struct Blob {
    vt: *const BlobVt,
    refs: AtomicU32,
    data: Vec<u8>,
}

const IID_IUNKNOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
const IID_ID3D10BLOB: [u8; 16] = [0x08, 0xfb, 0xa5, 0x8b, 0x95, 0x51, 0xe2, 0x40, 0xac, 0x58, 0x0d, 0x98, 0x9c, 0x3a, 0x01, 0x02];

unsafe extern "system" fn blob_qi(this: *mut Blob, riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return 0x8000_4003u32 as i32;
    }
    let g = unsafe { *riid };
    if g == IID_IUNKNOWN || g == IID_ID3D10BLOB {
        unsafe {
            (*this).refs.fetch_add(1, Ordering::SeqCst);
            *ppv = this as *mut c_void;
        }
        return 0;
    }
    unsafe { *ppv = core::ptr::null_mut() };
    0x8000_4002u32 as i32
}

unsafe extern "system" fn blob_addref(this: *mut Blob) -> u32 {
    unsafe { (*this).refs.fetch_add(1, Ordering::SeqCst) + 1 }
}

unsafe extern "system" fn blob_release(this: *mut Blob) -> u32 {
    let n = unsafe { (*this).refs.fetch_sub(1, Ordering::SeqCst) } - 1;
    if n == 0 {
        drop(unsafe { Box::from_raw(this) });
    }
    n
}

unsafe extern "system" fn blob_ptr(this: *mut Blob) -> *mut c_void {
    unsafe { (*this).data.as_mut_ptr() as *mut c_void }
}

unsafe extern "system" fn blob_size(this: *mut Blob) -> usize {
    unsafe { (*this).data.len() }
}

static BLOB_VT: BlobVt = BlobVt { qi: blob_qi, addref: blob_addref, release: blob_release, ptr: blob_ptr, size: blob_size };

pub fn new_blob(data: Vec<u8>) -> *mut c_void {
    Box::into_raw(Box::new(Blob { vt: &BLOB_VT, refs: AtomicU32::new(1), data })) as *mut c_void
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> GfxInput {
        GfxInput {
            elems: vec![
                InputElem { name: b"ATTRIBUTE".to_vec(), index: 0, format: 6, slot: 0, offset: 0, class: 0, step: 0 },
                InputElem { name: b"ATTRIBUTE".to_vec(), index: 1, format: 28, slot: 1, offset: 12, class: 1, step: 1 },
            ],
            so: vec![SoEntry { stream: 0, name: b"SV_Position".to_vec(), index: 0, start: 0, count: 4, slot: 0 }],
            strides: vec![16],
        }
    }

    #[test]
    fn input_round_trip() {
        let g = sample_input();
        assert_eq!(decode_input(&encode_input(&g)).unwrap(), g);
        let mut bad = encode_input(&g);
        bad.push(0);
        assert!(decode_input(&bad).is_none());
    }

    #[test]
    fn gfx_round_trip_scrubs_pointers() {
        let mut desc = vec![0xAAu8; GFX_DESC_SIZE];
        desc[0x240..0x244].copy_from_slice(&1u32.to_le_bytes());
        let code: [Option<Vec<u8>>; 5] = [Some(vec![1, 2, 3]), Some(vec![4, 5]), None, None, None];
        let parts = encode_gfx(&desc, &sample_input(), &code);
        assert_eq!(parts.len(), GFX_PARTS);
        assert!(parts.iter().all(|p| !p.is_empty()));
        let mut rev: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        rev.reverse();
        let plan = decode_gfx(&rev).unwrap();
        assert_eq!(plan.code, code);
        assert_eq!(plan.input, sample_input());
        for o in GFX_PTR_FIELDS {
            assert!(plan.desc[o..o + 8].iter().all(|&b| b == 0), "pointer field {o:#x} not scrubbed");
        }
        assert_eq!(&plan.desc[0x240..0x244], &1u32.to_le_bytes());
        assert_eq!(plan.desc[0x1c0], 0xAA);
    }

    #[test]
    fn gfx_rejects_foreign_blobs() {
        let foreign = [0u8; 64];
        assert!(decode_gfx(&[&foreign]).is_err());
        assert!(decode_gfx(&[]).is_err());
        let parts = encode_gfx(&vec![0u8; GFX_DESC_SIZE], &GfxInput::default(), &Default::default());
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        assert!(decode_gfx(&refs).is_err());
    }

    #[test]
    fn compute_round_trip() {
        let desc = vec![0x55u8; COMPUTE_DESC_SIZE];
        let parts = encode_compute(&desc, &[9, 8, 7]);
        assert_eq!(parts.len(), COMPUTE_PARTS);
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        let (d, c) = decode_compute(&refs).unwrap();
        assert_eq!(c, vec![9, 8, 7]);
        assert!(d[0..0x10].iter().all(|&b| b == 0));
        assert!(d[0x20..0x30].iter().all(|&b| b == 0));
        assert_eq!(d[0x18], 0x55);
        assert!(decode_compute(&refs[..2]).is_err());
    }

    #[test]
    fn blob_com_object() {
        unsafe {
            let b = new_blob(vec![1, 2, 3, 4]);
            let vt = &**(b as *const *const BlobVt);
            assert_eq!((vt.size)(b as *mut Blob), 4);
            assert_eq!(*((vt.ptr)(b as *mut Blob) as *const u8).add(3), 4);
            let mut out = core::ptr::null_mut();
            assert_eq!((vt.qi)(b as *mut Blob, &IID_ID3D10BLOB, &mut out), 0);
            assert_eq!(out, b);
            assert_eq!((vt.release)(b as *mut Blob), 1);
            assert_eq!((vt.release)(b as *mut Blob), 0);
        }
    }
}
