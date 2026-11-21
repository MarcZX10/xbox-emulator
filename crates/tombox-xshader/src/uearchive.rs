//! Unreal Engine shader-code archives (`FSerializedShaderArchive`, as found inside a pak or
//! as a standalone `.ushaderbytecode`): locating the entry table, decompressing entries and
//! splitting an entry into the engine's resource-table header, the platform shader
//! container and the engine's trailing optional data.
//!
//! The same layout is used by the Xbox build and by PC builds, which is what makes a PC
//! archive usable as a *reference* for the Xbox shaders (see `bin/xpair.rs`).

/// Minimal LZ4 *block* decompressor (bounds-checked; returns None on malformed input).
pub fn lz4_block(src: &[u8], out_len: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(out_len);
    let mut i = 0usize;
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            loop {
                let b = *src.get(i)?;
                i += 1;
                lit += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        out.extend_from_slice(src.get(i..i + lit)?);
        i += lit;
        if i >= src.len() {
            break; // last sequence has only literals
        }
        let off = u16::from_le_bytes([*src.get(i)?, *src.get(i + 1)?]) as usize;
        i += 2;
        let mut mlen = (token & 15) as usize;
        if mlen == 15 {
            loop {
                let b = *src.get(i)?;
                i += 1;
                mlen += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        mlen += 4;
        if off == 0 || off > out.len() {
            return None;
        }
        let start = out.len() - off;
        for k in 0..mlen {
            let b = out[start + k];
            out.push(b);
        }
        if out.len() > out_len {
            return None;
        }
    }
    if out.len() == out_len { Some(out) } else { None }
}

/// A UE `FSerializedShaderArchive` shader-entry table located inside a larger file.
pub struct Archive {
    /// (absolute file offset, compressed size, uncompressed size)
    pub entries: Vec<(usize, usize, usize)>,
    /// Shader frequency byte of each entry (0 VS, 3 PS, 4 GS, 5 CS).
    pub freq: Vec<u8>,
}

/// Locate UE shader-archive entry tables structurally: a `TArray<FShaderCodeEntry>`
/// (i32 count, then 17-byte records {u64 Offset, u32 Size, u32 UncompressedSize, u8
/// Frequency}) whose offsets start at 0 and chain (Offset[i+1] == Offset[i] + Size[i]),
/// followed by `TArray<FFileCachePreloadEntry>` (16-byte) and `TArray<u32>` ShaderIndices,
/// after which the shader code begins.
pub fn find_archives(data: &[u8]) -> Vec<Archive> {
    let rd32 = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let rd64 = |o: usize| data.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()) as usize);
    let mut out = Vec::new();
    let mut p = 4usize;
    while p + 17 * 64 < data.len() {
        // Cheap prefilter: first record has Offset == 0 and a small Frequency byte.
        if data[p..p + 8] != [0u8; 8] || data[p + 16] > 8 {
            p += 1;
            continue;
        }
        let n = match rd32(p - 4) {
            Some(n) if (32..4_000_000).contains(&n) && p + n * 17 + 8 <= data.len() => n,
            _ => {
                p += 1;
                continue;
            }
        };
        let mut ok = true;
        let mut expect = 0usize;
        let mut recs = Vec::with_capacity(n);
        let mut freq = Vec::with_capacity(n);
        for k in 0..n {
            let r = p + k * 17;
            let (off, size, usize_) = (rd64(r).unwrap(), rd32(r + 8).unwrap(), rd32(r + 12).unwrap());
            if off != expect || size == 0 || size > usize_ || usize_ > 0x40_0000 || data[r + 16] > 8 {
                ok = false;
                break;
            }
            expect = off + size;
            recs.push((off, size, usize_));
            freq.push(data[r + 16]);
        }
        if !ok {
            p += 1;
            continue;
        }
        let mut q = p + n * 17;
        let npre = match rd32(q) {
            Some(v) if v < 50_000_000 => v,
            _ => {
                p += 1;
                continue;
            }
        };
        q += 4 + npre * 16;
        let nidx = match rd32(q) {
            Some(v) if v < 200_000_000 => v,
            _ => {
                p += 1;
                continue;
            }
        };
        q += 4 + nidx * 4;
        if q + expect > data.len() {
            p += 1;
            continue;
        }
        eprintln!("shader archive: {n} entries, table @ {p:#x}, code @ {q:#x} ({expect} bytes)");
        out.push(Archive { freq, entries: recs.into_iter().map(|(o, s, u)| (q + o, s, u)).collect() });
        p = q + expect;
    }
    out
}


/// One archive entry split into its parts.
pub struct EntryParts<'a> {
    /// Engine resource-table header (`FShaderResourceTable`), before the container.
    pub head: &'a [u8],
    /// The platform shader container (starts with `DXBC`).
    pub container: &'a [u8],
    /// Engine optional data after the container.
    pub tail: &'a [u8],
}

/// Split a decompressed entry. `None` when it holds no well-formed DXBC container.
pub fn split_entry(d: &[u8]) -> Option<EntryParts<'_>> {
    let p = d.windows(4).position(|w| w == b"DXBC")?;
    let size = u32::from_le_bytes(d.get(p + 24..p + 28)?.try_into().ok()?) as usize;
    if size < 32 || p + size > d.len() {
        return None;
    }
    Some(EntryParts { head: &d[..p], container: &d[p..p + size], tail: &d[p + size..] })
}

/// The header parsed as `u32 bits` followed by five `TArray<u32>` (observed on both
/// platforms: the fourth array holds uniform-buffer layout hashes, the others are
/// token maps terminated by 0xffffffff). `None` if the header does not have that shape.
pub fn parse_head(head: &[u8]) -> Option<(u32, [Vec<u32>; 5])> {
    let rd = |o: usize| head.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let bits = rd(0)?;
    let mut o = 4;
    let mut arrs: [Vec<u32>; 5] = Default::default();
    for a in arrs.iter_mut() {
        let n = rd(o)? as usize;
        o += 4;
        if n > 4096 {
            return None;
        }
        for _ in 0..n {
            a.push(rd(o)?);
            o += 4;
        }
    }
    if o == head.len() { Some((bits, arrs)) } else { None }
}
