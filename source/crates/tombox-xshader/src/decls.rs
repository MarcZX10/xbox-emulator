//! Resource declarations of a shader, in a form comparable across compilers.
//!
//! Two sources: the `SHEX`/`SHDR` chunk of a PC shader-model-5.0 DXBC container (what the
//! PC compiler recorded) and the HLSL text this crate generates for an Xbox shader (what
//! the translator inferred). Comparing the two for the same engine shader shows where the
//! translator's inference of a resource's kind, element type or size is wrong.

use std::collections::BTreeMap;

/// What is bound at one register.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Constant buffer; size in 16-byte vectors.
    ConstantBuffer(u32),
    Sampler { comparison: bool },
    /// Typed texture or buffer: dimension name (`buffer`, `2d`, `3d`, `cube`, `2darray`, …)
    /// and component type (`float`, `uint`, `sint`; unorm/snorm read as float).
    Typed { dim: &'static str, ty: &'static str },
    /// Structured buffer; stride in bytes.
    Structured(u32),
    Raw,
}

/// Register class (`b`, `t`, `s`, `u`) and slot → declaration.
pub type Decls = BTreeMap<(char, u32), Kind>;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShaderDecls {
    pub res: Decls,
    /// Compute thread-group size, if declared.
    pub threads: Option<[u32; 3]>,
}

fn dim_name(d: u32) -> &'static str {
    match d {
        1 => "buffer",
        2 => "1d",
        3 => "2d",
        4 => "2dms",
        5 => "3d",
        6 => "cube",
        7 => "1darray",
        8 => "2darray",
        9 => "2dmsarray",
        10 => "cubearray",
        _ => "?",
    }
}

fn ret_name(t: u32) -> &'static str {
    match t & 0xf {
        1 | 2 | 5 => "float",
        3 => "sint",
        4 => "uint",
        _ => "?",
    }
}

/// Declarations from a shader-model-5.0 DXBC container. `None` if it has no SHEX/SHDR
/// chunk or the chunk is malformed. Shader model 5.1 operands (register spaces) are not
/// handled and yield `None`.
pub fn from_dxbc(c: &[u8]) -> Option<ShaderDecls> {
    let rd = |o: usize| c.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let n = rd(28)? as usize;
    let mut body: Option<&[u8]> = None;
    for i in 0..n.min(64) {
        let off = rd(32 + 4 * i)? as usize;
        let tag = c.get(off..off + 4)?;
        if tag == b"SHEX" || tag == b"SHDR" {
            let size = rd(off + 4)? as usize;
            body = c.get(off + 8..off + 8 + size);
        }
    }
    let body = body?;
    let w: Vec<u32> = body.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
    if w.len() < 2 {
        return None;
    }
    let version = w[0];
    if (version >> 4) & 0xf != 5 || version & 0xf != 0 {
        return None;
    }
    let end = (w[1] as usize).min(w.len());
    let mut out = ShaderDecls::default();
    let mut i = 2;
    while i < end {
        let tok = w[i];
        let op = tok & 0x7ff;
        let len = if op == 53 { *w.get(i + 1)? as usize } else { ((tok >> 24) & 0x7f) as usize };
        if len == 0 || i + len > end {
            return None;
        }
        let ins = &w[i..i + len];
        // Operand start (after extended opcode tokens).
        let mut o = 1;
        let mut t = tok;
        while t & 0x8000_0000 != 0 {
            t = *ins.get(o)?;
            o += 1;
        }
        // Reads an operand with immediate-32 indices; returns (indices, tokens consumed).
        let operand = |o: usize| -> Option<(Vec<u32>, usize)> {
            let ot = *ins.get(o)?;
            let mut k = o + 1;
            if ot & 0x8000_0000 != 0 {
                k += 1;
            }
            let dims = (ot >> 20) & 3;
            let mut idx = Vec::new();
            for d in 0..dims {
                if (ot >> (22 + 3 * d)) & 7 != 0 {
                    return None;
                }
                idx.push(*ins.get(k)?);
                k += 1;
            }
            Some((idx, k - o))
        };
        match op {
            // dcl_resource: dimension in the opcode token, return-type token after the operand
            88 => {
                let (idx, used) = operand(o)?;
                let rt = *ins.get(o + used)?;
                out.res.insert(('t', *idx.first()?), Kind::Typed { dim: dim_name((tok >> 11) & 0x1f), ty: ret_name(rt) });
            }
            89 => {
                let (idx, _) = operand(o)?;
                out.res.insert(('b', *idx.first()?), Kind::ConstantBuffer(*idx.get(1)?));
            }
            90 => {
                let (idx, _) = operand(o)?;
                out.res.insert(('s', *idx.first()?), Kind::Sampler { comparison: (tok >> 11) & 0xf == 1 });
            }
            155 => out.threads = Some([*ins.get(1)?, *ins.get(2)?, *ins.get(3)?]),
            156 => {
                let (idx, used) = operand(o)?;
                let rt = *ins.get(o + used)?;
                out.res.insert(('u', *idx.first()?), Kind::Typed { dim: dim_name((tok >> 11) & 0x1f), ty: ret_name(rt) });
            }
            157 | 161 => {
                let (idx, _) = operand(o)?;
                out.res.insert((if op == 157 { 'u' } else { 't' }, *idx.first()?), Kind::Raw);
            }
            158 | 162 => {
                let (idx, used) = operand(o)?;
                let stride = *ins.get(o + used)?;
                out.res.insert((if op == 158 { 'u' } else { 't' }, *idx.first()?), Kind::Structured(stride));
            }
            _ => {}
        }
        i += len;
    }
    Some(out)
}

/// Declarations from HLSL text generated by [`crate::hlsl::translate`]. This reads the
/// generator's own fixed output forms, not HLSL in general.
pub fn from_generated_hlsl(h: &str) -> ShaderDecls {
    let mut out = ShaderDecls::default();
    let mut strides: BTreeMap<String, u32> = BTreeMap::new();
    for line in h.lines() {
        let line = line.trim();
        if let Some(r) = line.strip_prefix("[numthreads(") {
            let v: Vec<u32> = r.trim_end_matches(")]").split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() == 3 {
                out.threads = Some([v[0], v[1], v[2]]);
            }
            continue;
        }
        // struct TbElem_sbuf3 { uint w[4]; };
        if let Some(r) = line.strip_prefix("struct TbElem_") {
            if let (Some(name), Some(n)) = (r.split(' ').next(), r.split("w[").nth(1).and_then(|x| x.split(']').next()).and_then(|x| x.parse::<u32>().ok())) {
                strides.insert(format!("TbElem_{name}"), n * 4);
            }
            continue;
        }
        let Some(p) = line.find(": register(") else { continue };
        let reg = &line[p + 11..];
        let Some(class) = reg.chars().next() else { continue };
        let Ok(slot) = reg[1..].split(')').next().unwrap_or("").parse::<u32>() else { continue };
        let ty = line.split(' ').next().unwrap_or("");
        let (base, elem) = match ty.split_once('<') {
            Some((b, e)) => (b, e.trim_end_matches('>')),
            None => (ty, ""),
        };
        let comp = if elem.starts_with("uint") {
            "uint"
        } else if elem.starts_with("int") {
            "sint"
        } else {
            "float"
        };
        let kind = match base {
            "cbuffer" => {
                let n = line.split('[').nth(1).and_then(|x| x.split(']').next()).and_then(|x| x.parse().ok()).unwrap_or(0);
                Kind::ConstantBuffer(n)
            }
            "SamplerState" => Kind::Sampler { comparison: false },
            "SamplerComparisonState" => Kind::Sampler { comparison: true },
            "StructuredBuffer" | "RWStructuredBuffer" => Kind::Structured(strides.get(elem).copied().unwrap_or(0)),
            "ByteAddressBuffer" | "RWByteAddressBuffer" => Kind::Raw,
            _ => {
                let dim = match base.trim_start_matches("RW") {
                    "Buffer" => "buffer",
                    "Texture1D" => "1d",
                    "Texture1DArray" => "1darray",
                    "Texture2D" => "2d",
                    "Texture2DArray" => "2darray",
                    "Texture2DMS" => "2dms",
                    "Texture3D" => "3d",
                    "TextureCube" => "cube",
                    "TextureCubeArray" => "cubearray",
                    _ => continue,
                };
                Kind::Typed { dim, ty: comp }
            }
        };
        out.res.insert((class, slot), kind);
    }
    out
}

/// One disagreement between a reference and a translated declaration set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Diff {
    /// Register declared by the reference only.
    MissingInTranslation(char, Kind),
    /// Register declared by the translation only.
    ExtraInTranslation(char, Kind),
    /// Both declare the register, differently: (class, reference, translation).
    Mismatch(char, Kind, Kind),
    Threads,
}

/// Differences that matter for behaviour. A constant buffer the translation declares
/// smaller is not reported (the Xbox compiler may read fewer members than the PC one
/// declares); larger is. Signed and unsigned integer elements are treated as equal: the
/// translator cannot always tell them apart and both read the same bits.
pub fn compare(reference: &ShaderDecls, translated: &ShaderDecls) -> Vec<Diff> {
    let mut out = Vec::new();
    for (k, r) in &reference.res {
        match translated.res.get(k) {
            None => out.push(Diff::MissingInTranslation(k.0, r.clone())),
            Some(t) if t == r => {}
            Some(Kind::Typed { dim: td, ty: tt }) if matches!(r, Kind::Typed { dim: rd, ty: rt } if rd == td && *rt != "float" && *tt != "float") => {}
            Some(Kind::ConstantBuffer(tn)) if matches!(r, Kind::ConstantBuffer(rn) if tn <= rn) => {}
            Some(t) => out.push(Diff::Mismatch(k.0, r.clone(), t.clone())),
        }
    }
    for (k, t) in &translated.res {
        if !reference.res.contains_key(k) {
            out.push(Diff::ExtraInTranslation(k.0, t.clone()));
        }
    }
    if reference.threads != translated.threads {
        out.push(Diff::Threads);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_generated_forms() {
        let h = "cbuffer CB0 : register(b0) { uint4 cb0[65]; };\n\
                 Texture2D<float4> tex1 : register(t1);\n\
                 SamplerComparisonState scmp2 : register(s2);\n\
                 struct TbElem_sbuf3 { uint w[4]; };\n\
                 StructuredBuffer<TbElem_sbuf3> sbuf3 : register(t3);\n\
                 RWBuffer<uint> utb2 : register(u2);\n\
                 RWTexture3D<float4> uav0 : register(u0);\n\
                 [numthreads(4, 4, 4)]\n";
        let d = from_generated_hlsl(h);
        assert_eq!(d.threads, Some([4, 4, 4]));
        assert_eq!(d.res[&('b', 0)], Kind::ConstantBuffer(65));
        assert_eq!(d.res[&('t', 1)], Kind::Typed { dim: "2d", ty: "float" });
        assert_eq!(d.res[&('s', 2)], Kind::Sampler { comparison: true });
        assert_eq!(d.res[&('t', 3)], Kind::Structured(16));
        assert_eq!(d.res[&('u', 2)], Kind::Typed { dim: "buffer", ty: "uint" });
        assert_eq!(d.res[&('u', 0)], Kind::Typed { dim: "3d", ty: "float" });
    }

    /// Hand-assembled SM5.0 compute chunk: dcl_constantbuffer cb0[2], dcl_resource_buffer
    /// (uint) t1, dcl_uav_structured u0 stride 12, dcl_thread_group 8,8,1.
    #[test]
    fn parses_shex_declarations() {
        let words: Vec<u32> = vec![
            0x0005_0050, // cs_5_0
            0,           // length, patched below
            0x0400_0059, 0x0020_8E46, 0, 2,
            0x0400_0858, 0x0010_7000, 1, 0x4444,
            0x0400_009E, 0x0011_E000, 0, 12,
            0x0400_009B, 8, 8, 1,
        ];
        let mut w = words.clone();
        w[1] = w.len() as u32;
        let chunk: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        let mut c = Vec::new();
        c.extend_from_slice(b"DXBC");
        c.extend_from_slice(&[0u8; 16]);
        c.extend_from_slice(&1u32.to_le_bytes());
        c.extend_from_slice(&((36 + 8 + chunk.len()) as u32).to_le_bytes());
        c.extend_from_slice(&1u32.to_le_bytes());
        c.extend_from_slice(&36u32.to_le_bytes());
        c.extend_from_slice(b"SHEX");
        c.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        c.extend_from_slice(&chunk);
        let d = from_dxbc(&c).expect("parse");
        assert_eq!(d.threads, Some([8, 8, 1]));
        assert_eq!(d.res[&('b', 0)], Kind::ConstantBuffer(2));
        assert_eq!(d.res[&('t', 1)], Kind::Typed { dim: "buffer", ty: "uint" });
        assert_eq!(d.res[&('u', 0)], Kind::Structured(12));
        // Truncation must not panic.
        for n in 0..c.len() {
            let _ = from_dxbc(&c[..n]);
        }
    }
}
