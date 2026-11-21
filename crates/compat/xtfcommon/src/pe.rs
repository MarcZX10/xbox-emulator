use std::path::{Path, PathBuf};

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

pub fn export_table(bytes: &[u8]) -> Option<Vec<(u16, String)>> {
    let pe = u32_at(bytes, 0x3c)? as usize;
    if bytes.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let nsec = u16_at(bytes, pe + 6)? as usize;
    let opt = pe + 24;
    let optsz = u16_at(bytes, pe + 20)? as usize;
    let magic = u16_at(bytes, opt)?;
    let dd = opt + if magic == 0x20b { 112 } else { 96 };
    let erva = u32_at(bytes, dd)?;
    if erva == 0 {
        return Some(Vec::new());
    }
    let secs = opt + optsz;
    let map = |rva: u32| -> Option<usize> {
        (0..nsec).find_map(|i| {
            let s = secs + i * 40;
            let va = u32_at(bytes, s + 12)?;
            let vs = u32_at(bytes, s + 8)?.max(u32_at(bytes, s + 16)?);
            let raw = u32_at(bytes, s + 20)?;
            (rva >= va && rva < va + vs).then(|| (rva - va + raw) as usize)
        })
    };
    let ed = map(erva)?;
    let base = u32_at(bytes, ed + 16)?;
    let nfuncs = u32_at(bytes, ed + 20)? as usize;
    let nnames = u32_at(bytes, ed + 24)? as usize;
    let funcs = map(u32_at(bytes, ed + 28)?)?;
    let names = if nnames > 0 { map(u32_at(bytes, ed + 32)?)? } else { 0 };
    let ords = if nnames > 0 { map(u32_at(bytes, ed + 36)?)? } else { 0 };
    let mut named = std::collections::BTreeMap::new();
    for i in 0..nnames {
        let np = map(u32_at(bytes, names + i * 4)?)?;
        let end = bytes[np..].iter().position(|&c| c == 0)?;
        let idx = u16_at(bytes, ords + i * 2)? as usize;
        named.insert(idx, String::from_utf8_lossy(&bytes[np..np + end]).into_owned());
    }
    let mut out = Vec::new();
    for i in 0..nfuncs {
        if u32_at(bytes, funcs + i * 4)? == 0 {
            continue;
        }
        let ord = (base as usize + i) as u16;
        let name = named.get(&i).cloned().unwrap_or_else(|| format!("#{ord}"));
        out.push((ord, name));
    }
    out.sort();
    Some(out)
}

pub fn title_dlls(file: &str) -> Vec<(String, PathBuf)> {
    let root = Path::new(r"D:\xbox-pc-port\games");
    let mut out = Vec::new();
    for title in ["Fortnite", "XDefiant", "Terraria", "Trackmania"] {
        let ex = root.join(title).join("extracted");
        let direct = ex.join("EmbeddedXvd").join("Windows").join("System32").join(file);
        if direct.is_file() {
            out.push((title.to_string(), direct));
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&ex) else { continue };
        for e in rd.flatten() {
            let p = e.path().join("EmbeddedXvd").join("Windows").join("System32").join(file);
            if p.is_file() {
                out.push((title.to_string(), p));
            }
        }
    }
    out
}
