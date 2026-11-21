#![allow(dead_code)]

use core::ffi::c_void;
use std::path::Path;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    pub fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, prot: u32, old: *mut u32) -> i32;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn load(path: &Path) -> usize {
    let w = wide(&path.to_string_lossy());
    unsafe { LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), 8) as usize }
}

pub fn module(name: &str) -> usize {
    unsafe { GetModuleHandleW(wide(name).as_ptr()) as usize }
}

unsafe fn rd32(p: usize) -> u32 {
    (p as *const u32).read_unaligned()
}

unsafe fn cstr(p: usize) -> String {
    std::ffi::CStr::from_ptr(p as *const core::ffi::c_char).to_string_lossy().into_owned()
}

pub unsafe fn imports(base: usize, dll: &str) -> Vec<(String, usize)> {
    let nt = base + rd32(base + 0x3c) as usize;
    let dir = rd32(nt + 0x18 + 0x70 + 8) as usize;
    let mut out = Vec::new();
    let mut d = base + dir;
    while rd32(d + 12) != 0 {
        let name = cstr(base + rd32(d + 12) as usize);
        if name.eq_ignore_ascii_case(dll) {
            let int = base + rd32(d) as usize;
            let iat = base + rd32(d + 16) as usize;
            let mut i = 0;
            loop {
                let t = (int as *const u64).add(i).read_unaligned();
                if t == 0 {
                    break;
                }
                if t >> 63 == 0 {
                    out.push((cstr(base + t as usize + 2), iat + i * 8));
                }
                i += 1;
            }
        }
        d += 20;
    }
    out
}

pub unsafe fn patch(slot: usize, value: usize) -> usize {
    let mut old = 0u32;
    assert!(VirtualProtect(slot as *mut c_void, 8, 4, &mut old) != 0);
    let prev = (slot as *const usize).read();
    (slot as *mut usize).write(value);
    VirtualProtect(slot as *mut c_void, 8, old, &mut old);
    prev
}

pub unsafe fn vt(obj: usize, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

pub unsafe fn blob(b: usize) -> Vec<u8> {
    let ptr: unsafe extern "system" fn(usize) -> *const u8 = core::mem::transmute(vt(b, 3));
    let len: unsafe extern "system" fn(usize) -> usize = core::mem::transmute(vt(b, 4));
    let (p, n) = (ptr(b), len(b));
    let v = if p.is_null() || n == 0 { Vec::new() } else { core::slice::from_raw_parts(p, n).to_vec() };
    let rel: unsafe extern "system" fn(usize) -> u32 = core::mem::transmute(vt(b, 2));
    rel(b);
    v
}

type D3DCompile = unsafe extern "system" fn(*const u8, usize, *const u8, usize, usize, *const u8, *const u8, u32, u32, *mut usize, *mut usize) -> i32;

pub fn d3dcompile(h: usize, src: &str, target: &str) -> Result<Vec<u8>, String> {
    unsafe {
        let f: D3DCompile = core::mem::transmute(GetProcAddress(h as *mut c_void, b"D3DCompile\0".as_ptr()));
        let (t, e) = (format!("{target}\0"), "main\0");
        let (mut code, mut err) = (0usize, 0usize);
        let hr = f(src.as_ptr(), src.len(), b"kit.hlsl\0".as_ptr(), 0, 0, e.as_ptr(), t.as_ptr(), 0, 0, &mut code, &mut err);
        let msg = if err != 0 { String::from_utf8_lossy(&blob(err)).into_owned() } else { String::new() };
        if hr < 0 || code == 0 {
            return Err(format!("{hr:#x} {msg}"));
        }
        Ok(blob(code))
    }
}

#[repr(C)]
struct DxcBuffer {
    ptr: *const u8,
    size: usize,
    encoding: u32,
}

fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let mut g = [0u8; 16];
    g[..4].copy_from_slice(&a.to_le_bytes());
    g[4..6].copy_from_slice(&b.to_le_bytes());
    g[6..8].copy_from_slice(&c.to_le_bytes());
    g[8..].copy_from_slice(&d);
    g
}

pub fn dxc_compile(h: usize, src: &str, target: &str) -> Result<Vec<u8>, String> {
    unsafe {
        let create: unsafe extern "system" fn(*const u8, *const u8, *mut usize) -> i32 = core::mem::transmute(GetProcAddress(h as *mut c_void, b"DxcCreateInstance\0".as_ptr()));
        let clsid = guid(0x73e2_2d93, 0xe6ce, 0x47f3, [0xb5, 0xbf, 0xf0, 0x66, 0x4f, 0x39, 0xc1, 0xb0]);
        let iid = guid(0x228b_4687, 0x5a6a, 0x4730, [0x90, 0x0c, 0x97, 0x02, 0xb2, 0x20, 0x3f, 0x54]);
        let rid = guid(0x5834_6cda, 0xdde7, 0x4497, [0x94, 0x61, 0x6f, 0x87, 0xaf, 0x5e, 0x06, 0x59]);
        let mut c = 0usize;
        let hr = create(clsid.as_ptr(), iid.as_ptr(), &mut c);
        if hr < 0 {
            return Err(format!("DxcCreateInstance {hr:#x}"));
        }
        let args: Vec<Vec<u16>> = ["-T", target, "-E", "main"].iter().map(|a| wide(a)).collect();
        let argv: Vec<*const u16> = args.iter().map(|a| a.as_ptr()).collect();
        let buf = DxcBuffer { ptr: src.as_ptr(), size: src.len(), encoding: 65001 };
        let compile: unsafe extern "system" fn(usize, *const DxcBuffer, *const *const u16, u32, usize, *const u8, *mut usize) -> i32 = core::mem::transmute(vt(c, 3));
        let mut res = 0usize;
        let hr = compile(c, &buf, argv.as_ptr(), argv.len() as u32, 0, rid.as_ptr(), &mut res);
        let rel: unsafe extern "system" fn(usize) -> u32 = core::mem::transmute(vt(c, 2));
        rel(c);
        if hr < 0 {
            return Err(format!("Compile {hr:#x}"));
        }
        let status: unsafe extern "system" fn(usize, *mut i32) -> i32 = core::mem::transmute(vt(res, 3));
        let result: unsafe extern "system" fn(usize, *mut usize) -> i32 = core::mem::transmute(vt(res, 4));
        let errors: unsafe extern "system" fn(usize, *mut usize) -> i32 = core::mem::transmute(vt(res, 5));
        let mut st = 0i32;
        status(res, &mut st);
        let mut eb = 0usize;
        errors(res, &mut eb);
        let msg = if eb != 0 { String::from_utf8_lossy(&blob(eb)).into_owned() } else { String::new() };
        let mut rb = 0usize;
        result(res, &mut rb);
        let out = if rb != 0 { blob(rb) } else { Vec::new() };
        let rel: unsafe extern "system" fn(usize) -> u32 = core::mem::transmute(vt(res, 2));
        rel(res);
        if st < 0 {
            return Err(format!("status {st:#x} {msg}"));
        }
        Ok(out)
    }
}

pub fn chunk<'a>(dxbc: &'a [u8], cc: &[u8; 4]) -> Option<&'a [u8]> {
    let n = u32::from_le_bytes(dxbc[28..32].try_into().ok()?) as usize;
    for i in 0..n {
        let o = u32::from_le_bytes(dxbc[32 + 4 * i..36 + 4 * i].try_into().ok()?) as usize;
        if &dxbc[o..o + 4] == cc {
            let sz = u32::from_le_bytes(dxbc[o + 4..o + 8].try_into().ok()?) as usize;
            return Some(&dxbc[o + 8..o + 8 + sz]);
        }
    }
    None
}

pub const S_ENDPGM: u32 = 0xBF81_0000;

pub fn extents(body: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 8;
    while i + 2 <= body.len() {
        if body[i] == 0x78 && body[i + 1] == 0x01 {
            let hdr = u32::from_le_bytes(body[i - 8..i - 4].try_into().unwrap());
            if let Ok(v) = miniz_oxide::inflate::decompress_to_vec_zlib(&body[i..]) {
                out.push((hdr, v));
            }
        }
        i += 1;
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enc {
    Sop2,
    Sopk,
    Sop1,
    Sopc,
    Sopp,
    Smrd,
    Vop1,
    Vop2,
    Vopc,
    Vop3,
    Vintrp,
    Ds,
    Mubuf,
    Mtbuf,
    Mimg,
    Flat,
    Exp,
}

fn classify(w: u32) -> Option<Enc> {
    Some(match w {
        _ if w >> 23 == 0x17f => Enc::Sopp,
        _ if w >> 23 == 0x17d => Enc::Sop1,
        _ if w >> 23 == 0x17e => Enc::Sopc,
        _ if w >> 28 == 0xb => Enc::Sopk,
        _ if w >> 30 == 0b10 => Enc::Sop2,
        _ if w >> 25 == 0x3f => Enc::Vop1,
        _ if w >> 25 == 0x3e => Enc::Vopc,
        _ if w >> 31 == 0 => Enc::Vop2,
        _ if w >> 27 == 0b11000 => Enc::Smrd,
        _ => match w >> 26 {
            0b110100 => Enc::Vop3,
            0b110010 => Enc::Vintrp,
            0b110110 => Enc::Ds,
            0b111000 => Enc::Mubuf,
            0b111010 => Enc::Mtbuf,
            0b111100 => Enc::Mimg,
            0b110111 => Enc::Flat,
            0b111110 => Enc::Exp,
            _ => return None,
        },
    })
}

fn len(enc: Enc, w: u32) -> usize {
    match enc {
        Enc::Sop2 | Enc::Sopc => 1 + ((w & 0xff) == 255 || ((w >> 8) & 0xff) == 255) as usize,
        Enc::Sop1 => 1 + ((w & 0xff) == 255) as usize,
        Enc::Sopk => 1 + ((w >> 23) & 0x1f == 0x15) as usize,
        Enc::Sopp | Enc::Vintrp => 1,
        Enc::Smrd => 1 + ((w >> 8) & 1 == 0 && (w & 0xff) == 255) as usize,
        Enc::Vop2 => 1 + ((w & 0x1ff) == 255 || matches!((w >> 25) & 0x3f, 0x20 | 0x21)) as usize,
        Enc::Vop1 | Enc::Vopc => 1 + ((w & 0x1ff) == 255) as usize,
        _ => 2,
    }
}

pub fn walk(code: &[u8]) -> Option<Vec<(Enc, u32)>> {
    let w: Vec<u32> = code.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < w.len() {
        let e = classify(w[i])?;
        out.push((e, w[i]));
        i += len(e, w[i]);
    }
    (i == w.len() && w.last() == Some(&S_ENDPGM)).then_some(out)
}

pub fn isa_before_endpgm(ext: &[u8], code_size: usize) -> Option<Vec<(Enc, u32)>> {
    let mut o = 0;
    while o + 4 <= ext.len() {
        if u32::from_le_bytes(ext[o..o + 4].try_into().unwrap()) == S_ENDPGM && o + 4 >= code_size {
            if let Some(v) = walk(&ext[o + 4 - code_size..o + 4]) {
                return Some(v);
            }
        }
        o += 4;
    }
    None
}

pub fn isa_after_padding(ext: &[u8]) -> Option<Vec<(Enc, u32)>> {
    let w: Vec<u32> = ext.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let end = w.iter().position(|v| *v == S_ENDPGM)?;
    let start = w[..end].iter().rposition(|v| *v == 0).map_or(0, |z| z + 1);
    walk(&ext[start * 4..end * 4 + 4])
}

pub struct Program {
    pub header: u32,
    pub object: Vec<u8>,
    pub code: Vec<(Enc, u32)>,
}

pub fn dxil_programs(dxbc: &[u8]) -> Result<Vec<Program>, String> {
    let dxil = chunk(dxbc, b"DXIL").ok_or("no DXIL part")?;
    let xdat = dxil.get(24..).ok_or("short DXIL")?;
    if xdat.get(..4) != Some(b"XDAT") {
        return Err("no XDAT after the DXIL program header".into());
    }
    let mut out = Vec::new();
    for (header, object) in extents(xdat) {
        if header & 0x3ffff != 0x10017 {
            continue;
        }
        let code = isa_after_padding(&object).ok_or_else(|| format!("extent {header:#x}: no GCN program ending in s_endpgm"))?;
        out.push(Program { header, object, code });
    }
    if out.is_empty() {
        return Err("no precompiled extents in XDAT".into());
    }
    Ok(out)
}
