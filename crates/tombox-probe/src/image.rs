use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tombox_pe::constants::{DIR_DELAY_IMPORT, DIR_IMPORT};
use tombox_pe::PeFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Name(String),
    Ordinal(u16),
}

impl Entry {
    pub fn label(&self) -> String {
        match self {
            Entry::Name(n) => n.clone(),
            Entry::Ordinal(o) => format!("#{o}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Import,
    Delay,
}

impl RowKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RowKind::Import => "import",
            RowKind::Delay => "delay",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportRow {
    pub module: String,
    pub entry: Entry,
    pub kind: RowKind,
    pub desc_rva: u32,
    pub iat_rva: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Exports {
    pub dll_name: String,
    pub by_name: HashMap<String, u32>,
    pub by_ordinal: HashMap<u32, Option<String>>,
    pub forwarders: HashMap<String, String>,
}

impl Exports {
    pub fn ordinal_of(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    pub fn name_of(&self, ordinal: u32) -> Option<Option<String>> {
        self.by_ordinal.get(&ordinal).cloned()
    }

    pub fn has(&self, entry: &Entry) -> bool {
        match entry {
            Entry::Name(n) => self.by_name.contains_key(n),
            Entry::Ordinal(o) => self.by_ordinal.contains_key(&(*o as u32)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub rva: u32,
    pub vsize: u32,
    pub raw_off: u32,
    pub raw_size: u32,
    pub exec: bool,
}

pub struct Image {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub image_base: u64,
    pub sections: Vec<Section>,
}

const MAX_THUNKS: usize = 1 << 16;

impl Image {
    pub fn open(path: &Path) -> Result<Image, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Image::from_bytes(path.to_path_buf(), bytes)
    }

    pub fn from_bytes(path: PathBuf, bytes: Vec<u8>) -> Result<Image, String> {
        let (image_base, sections) = {
            let pe = PeFile::parse(&bytes).map_err(|e| format!("parse {}: {e:?}", path.display()))?;
            let secs = pe
                .sections
                .iter()
                .map(|s| Section {
                    name: s.name.clone(),
                    rva: s.virtual_address,
                    vsize: s.virtual_size,
                    raw_off: s.pointer_to_raw_data,
                    raw_size: s.size_of_raw_data,
                    exec: s.characteristics & 0x2000_0000 != 0,
                })
                .collect();
            (pe.opt.image_base, secs)
        };
        Ok(Image { path, bytes, image_base, sections })
    }

    pub fn pe(&self) -> PeFile<'_> {
        PeFile::parse(&self.bytes).expect("image parsed once already")
    }

    pub fn rva_to_off(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            let span = s.vsize.max(s.raw_size);
            if rva >= s.rva && rva < s.rva.saturating_add(span) {
                let d = rva - s.rva;
                if d >= s.raw_size {
                    return None;
                }
                return Some(s.raw_off as usize + d as usize);
            }
        }
        None
    }

    pub fn off_to_rva(&self, off: usize) -> Option<u32> {
        for s in &self.sections {
            let lo = s.raw_off as usize;
            let hi = lo + s.raw_size as usize;
            if off >= lo && off < hi {
                return Some(s.rva + (off - lo) as u32);
            }
        }
        None
    }

    fn u32_at(&self, off: usize) -> Option<u32> {
        self.bytes.get(off..off + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64_at(&self, off: usize) -> Option<u64> {
        self.bytes.get(off..off + 8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
    }

    pub fn cstr_rva(&self, rva: u32) -> Option<String> {
        let off = self.rva_to_off(rva)?;
        let tail = self.bytes.get(off..)?;
        let end = tail.iter().take(1024).position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&tail[..end]).into_owned())
    }

    fn directory(&self, index: usize) -> Option<(u32, u32)> {
        let pe = self.pe();
        let d = pe.directory(index)?;
        if d.virtual_address == 0 || d.size == 0 {
            None
        } else {
            Some((d.virtual_address, d.size))
        }
    }

    fn thunks(&self, table_rva: u32, iat_rva: u32, module: &str, kind: RowKind, desc_rva: u32, out: &mut Vec<ImportRow>) {
        for i in 0..MAX_THUNKS {
            let Some(off) = self.rva_to_off(table_rva + (i as u32) * 8) else { break };
            let Some(t) = self.u64_at(off) else { break };
            if t == 0 {
                break;
            }
            let entry = if t & (1u64 << 63) != 0 {
                Entry::Ordinal((t & 0xffff) as u16)
            } else {
                Entry::Name(self.cstr_rva((t & 0x7fff_ffff) as u32 + 2).unwrap_or_default())
            };
            out.push(ImportRow { module: module.to_string(), entry, kind, desc_rva, iat_rva: iat_rva + (i as u32) * 8 });
        }
    }

    pub fn import_rows(&self) -> Vec<ImportRow> {
        let mut out = Vec::new();
        if let Some((dir, _)) = self.directory(DIR_IMPORT) {
            for i in 0..8192u32 {
                let desc = dir + i * 20;
                let Some(off) = self.rva_to_off(desc) else { break };
                let (Some(oft), Some(name), Some(ft)) = (self.u32_at(off), self.u32_at(off + 12), self.u32_at(off + 16)) else { break };
                if oft == 0 && name == 0 && ft == 0 {
                    break;
                }
                let module = self.cstr_rva(name).unwrap_or_default();
                let table = if oft != 0 { oft } else { ft };
                self.thunks(table, ft, &module, RowKind::Import, desc, &mut out);
            }
        }
        if let Some((dir, _)) = self.directory(DIR_DELAY_IMPORT) {
            for i in 0..8192u32 {
                let desc = dir + i * 32;
                let Some(off) = self.rva_to_off(desc) else { break };
                let (Some(attr), Some(name), Some(iat), Some(int)) = (self.u32_at(off), self.u32_at(off + 4), self.u32_at(off + 12), self.u32_at(off + 16)) else { break };
                if attr == 0 && name == 0 && int == 0 {
                    break;
                }
                if attr & 1 == 0 {
                    continue;
                }
                let module = self.cstr_rva(name).unwrap_or_default();
                self.thunks(int, iat, &module, RowKind::Delay, desc, &mut out);
            }
        }
        out
    }

    pub fn exports(&self) -> Exports {
        let pe = self.pe();
        let mut ex = Exports::default();
        if let Some(t) = &pe.exports {
            ex.dll_name = t.dll_name.clone();
            for (n, o) in &t.named {
                ex.by_name.insert(n.clone(), *o);
                ex.by_ordinal.insert(*o, Some(n.clone()));
            }
            for o in &t.unnamed_ordinals {
                ex.by_ordinal.entry(*o).or_insert(None);
            }
            for (n, f) in &t.forwarders {
                ex.forwarders.insert(n.clone(), f.clone());
            }
        }
        ex
    }

    pub fn iat_map(&self) -> HashMap<u32, (String, String)> {
        self.import_rows().into_iter().map(|r| (r.iat_rva, (r.module.clone(), r.entry.label()))).collect()
    }

    pub fn file_version(&self) -> Option<String> {
        file_version(&self.bytes)
    }
}

pub fn file_version(bytes: &[u8]) -> Option<String> {
    let sig = 0xFEEF_04BDu32.to_le_bytes();
    let pos = bytes.windows(4).rposition(|w| w == sig)?;
    let rd = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let ms = rd(pos + 8)?;
    let ls = rd(pos + 12)?;
    Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testpe;

    #[test]
    fn parses_synthetic_imports_and_exports() {
        let bytes = testpe::build(&["Alpha", "Beta"], &[("KERNEL32.dll", &["GetTickCount64", "#7"])], &[("CONCRT140.dll", &["Delayed"])]);
        let img = Image::from_bytes(PathBuf::from("t.dll"), bytes).unwrap();
        let ex = img.exports();
        assert_eq!(ex.ordinal_of("Alpha"), Some(1));
        assert_eq!(ex.ordinal_of("Beta"), Some(2));
        assert_eq!(ex.name_of(2), Some(Some("Beta".to_string())));
        let rows = img.import_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].module, "KERNEL32.dll");
        assert_eq!(rows[0].entry, Entry::Name("GetTickCount64".into()));
        assert_eq!(rows[1].entry, Entry::Ordinal(7));
        assert_eq!(rows[1].iat_rva, rows[0].iat_rva + 8);
        assert_eq!(rows[2].kind, RowKind::Delay);
        assert_eq!(rows[2].module, "CONCRT140.dll");
        assert_eq!(rows[2].entry, Entry::Name("Delayed".into()));
        let iat = img.iat_map();
        assert_eq!(iat.get(&rows[0].iat_rva).unwrap().1, "GetTickCount64");
    }

    #[test]
    fn reads_fixed_file_version() {
        let mut b = vec![0u8; 64];
        b[8..12].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
        b[16..20].copy_from_slice(&((14u32 << 16) | 29).to_le_bytes());
        b[20..24].copy_from_slice(&((30139u32 << 16) | 0).to_le_bytes());
        assert_eq!(file_version(&b).as_deref(), Some("14.29.30139.0"));
    }
}
