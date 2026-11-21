use std::path::{Path, PathBuf};

use crate::registry::Value;

pub const SOFTWARE_ROOT: &str = "\\Registry\\Machine\\SOFTWARE";

pub struct Hive {
    data: Vec<u8>,
}

pub struct HiveKey {
    pub name: String,
    pub values: Vec<Value>,
    pub subkeys: Vec<u32>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn name_of(raw: &[u8], compressed: bool) -> String {
    if compressed {
        raw.iter().map(|&c| c as char).collect()
    } else {
        String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<u16>>())
    }
}

impl Hive {
    pub fn open(path: &Path) -> Option<Hive> {
        let data = std::fs::read(path).ok()?;
        if data.get(0..4) != Some(b"regf") || data.len() < 0x2000 {
            return None;
        }
        Some(Hive { data })
    }

    fn cell(&self, off: u32) -> Option<&[u8]> {
        let start = 0x1000 + off as usize + 4;
        self.data.get(start..)
    }

    pub fn root(&self) -> Option<u32> {
        u32_at(&self.data, 0x24)
    }

    pub fn key(&self, off: u32) -> Option<HiveKey> {
        let c = self.cell(off)?;
        if c.get(0..2)? != b"nk" {
            return None;
        }
        let flags = u16_at(c, 2)?;
        let nsub = u32_at(c, 0x14)?;
        let subl = u32_at(c, 0x1c)?;
        let nval = u32_at(c, 0x24)?;
        let vall = u32_at(c, 0x28)?;
        let nl = u16_at(c, 0x48)? as usize;
        let name = name_of(c.get(0x4c..0x4c + nl)?, flags & 0x20 != 0);
        let mut subkeys = Vec::new();
        if nsub > 0 {
            self.list(subl, &mut subkeys, 0);
        }
        let mut values = Vec::new();
        if nval > 0 {
            let vl = self.cell(vall)?;
            for i in 0..nval as usize {
                if let Some(v) = u32_at(vl, i * 4).and_then(|o| self.value(o)) {
                    values.push(v);
                }
            }
        }
        Some(HiveKey { name, values, subkeys })
    }

    fn list(&self, off: u32, out: &mut Vec<u32>, depth: u32) {
        let Some(c) = self.cell(off) else { return };
        let Some(n) = u16_at(c, 2) else { return };
        match c.get(0..2) {
            Some(b"lf") | Some(b"lh") => out.extend((0..n as usize).filter_map(|i| u32_at(c, 4 + i * 8))),
            Some(b"li") => out.extend((0..n as usize).filter_map(|i| u32_at(c, 4 + i * 4))),
            Some(b"ri") if depth < 4 => {
                for i in 0..n as usize {
                    if let Some(o) = u32_at(c, 4 + i * 4) {
                        self.list(o, out, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    fn value(&self, off: u32) -> Option<Value> {
        let v = self.cell(off)?;
        if v.get(0..2)? != b"vk" {
            return None;
        }
        let nl = u16_at(v, 2)? as usize;
        let len = u32_at(v, 4)?;
        let doff = u32_at(v, 8)?;
        let kind = u32_at(v, 12)?;
        let fl = u16_at(v, 16)?;
        let name = name_of(v.get(0x14..0x14 + nl)?, fl & 1 != 0);
        let size = (len & 0x7fff_ffff) as usize;
        let data = if len & 0x8000_0000 != 0 {
            doff.to_le_bytes()[..size.min(4)].to_vec()
        } else if size == 0 {
            Vec::new()
        } else {
            let dc = self.cell(doff)?;
            if dc.get(0..2) == Some(b"db") && size > 16344 {
                let n = u16_at(dc, 2)? as usize;
                let lst = self.cell(u32_at(dc, 4)?)?;
                let mut d = Vec::new();
                for j in 0..n {
                    let seg = self.cell(u32_at(lst, j * 4)?)?;
                    d.extend_from_slice(seg.get(..16344.min(seg.len()))?);
                }
                d.truncate(size);
                d
            } else {
                dc.get(..size)?.to_vec()
            }
        };
        Some(Value { name, kind, data })
    }

    pub fn find(&self, path: &str) -> Option<u32> {
        let mut o = self.root()?;
        for part in path.split('\\').filter(|p| !p.is_empty()) {
            let k = self.key(o)?;
            o = *k.subkeys.iter().find(|s| self.key(**s).map(|x| x.name.eq_ignore_ascii_case(part)).unwrap_or(false))?;
        }
        Some(o)
    }

    pub fn walk(&self, off: u32, path: &str, out: &mut Vec<(String, Vec<Value>)>, depth: u32) {
        let Some(k) = self.key(off) else { return };
        out.push((path.to_string(), k.values));
        if depth == 0 {
            return;
        }
        for s in k.subkeys {
            if let Some(c) = self.key(s) {
                self.walk(s, &format!("{path}\\{}", c.name), out, depth - 1);
            }
        }
    }
}

pub fn software_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("TOMBOX_CONSOLE_SOFTWARE_HIVE").filter(|p| !p.is_empty()) {
        v.push(PathBuf::from(p));
    }
    for d in console_image::dump_dirs(Path::new(console_image::GAMES)) {
        let h = d.join("config").join("SOFTWARE");
        if h.is_file() {
            v.push(h);
        }
    }
    v
}

pub fn software() -> Option<Hive> {
    software_candidates().iter().find_map(|p| Hive::open(p))
}

pub fn seed_software(subtree: &str) -> usize {
    let Some(h) = software() else { return 0 };
    seed_from(&h, subtree)
}

pub fn seed_from(h: &Hive, subtree: &str) -> usize {
    let Some(o) = h.find(subtree) else { return 0 };
    let mut keys = Vec::new();
    h.walk(o, &format!("{SOFTWARE_ROOT}\\{subtree}"), &mut keys, 32);
    let mut n = 0;
    for (path, values) in keys {
        if crate::registry::key_exists(&path) {
            continue;
        }
        crate::registry::create_key(&path);
        for v in values {
            crate::registry::set(&path, &v.name, v.kind, &v.data);
        }
        n += 1;
    }
    n
}
