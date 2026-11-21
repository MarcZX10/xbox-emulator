pub struct Section {
    pub va: u32,
    pub vsize: u32,
    pub raw: u32,
    pub rsize: u32,
}

pub struct Export {
    pub name: Option<String>,
    pub ordinal: u32,
    pub forwarder: Option<String>,
}

pub struct Import {
    pub dll: String,
    pub delay: bool,
    pub names: Vec<String>,
}

pub struct Pe {
    pub data: Vec<u8>,
    pub sections: Vec<Section>,
    pub dirs: Vec<(u32, u32)>,
    pub pe64: bool,
}

fn rd16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}

fn rd32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()))
}

fn rd64(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|x| u64::from_le_bytes(x.try_into().unwrap()))
}

impl Pe {
    pub fn parse(data: Vec<u8>) -> Option<Pe> {
        if data.get(0..2)? != b"MZ" {
            return None;
        }
        let nt = rd32(&data, 0x3c)? as usize;
        if data.get(nt..nt + 4)? != b"PE\0\0" {
            return None;
        }
        let nsec = rd16(&data, nt + 6)? as usize;
        let opt_size = rd16(&data, nt + 20)? as usize;
        let opt = nt + 24;
        let pe64 = rd16(&data, opt)? == 0x20b;
        let (ndir_off, dir_off) = if pe64 { (opt + 108, opt + 112) } else { (opt + 92, opt + 96) };
        let ndir = rd32(&data, ndir_off)? as usize;
        let dirs = (0..ndir.min(16)).map(|i| (rd32(&data, dir_off + i * 8).unwrap_or(0), rd32(&data, dir_off + i * 8 + 4).unwrap_or(0))).collect();
        let sec0 = opt + opt_size;
        let sections = (0..nsec)
            .filter_map(|i| {
                let s = sec0 + i * 40;
                Some(Section { vsize: rd32(&data, s + 8)?, va: rd32(&data, s + 12)?, rsize: rd32(&data, s + 16)?, raw: rd32(&data, s + 20)? })
            })
            .collect();
        Some(Pe { data, sections, dirs, pe64 })
    }

    pub fn off(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find(|s| rva >= s.va && rva < s.va + s.vsize.max(s.rsize)).map(|s| (rva - s.va + s.raw) as usize)
    }

    pub fn cstr(&self, rva: u32) -> Option<String> {
        let o = self.off(rva)?;
        let end = self.data.get(o..)?.iter().position(|&c| c == 0)?;
        Some(String::from_utf8_lossy(&self.data[o..o + end]).into_owned())
    }

    pub fn exports(&self) -> Vec<Export> {
        let mut out = Vec::new();
        let Some(&(rva, size)) = self.dirs.first() else { return out };
        let Some(e) = self.off(rva).filter(|_| rva != 0) else { return out };
        let d = &self.data;
        let (Some(base), Some(nfun), Some(nnames), Some(fun), Some(names), Some(ords)) = (rd32(d, e + 16), rd32(d, e + 20), rd32(d, e + 24), rd32(d, e + 28), rd32(d, e + 32), rd32(d, e + 36)) else { return out };
        let mut named = vec![None; nfun as usize];
        for i in 0..nnames as usize {
            let (Some(no), Some(oo)) = (self.off(names + 4 * i as u32), self.off(ords + 2 * i as u32)) else { continue };
            let (Some(nr), Some(ix)) = (rd32(d, no), rd16(d, oo)) else { continue };
            if let Some(slot) = named.get_mut(ix as usize) {
                *slot = self.cstr(nr);
            }
        }
        for i in 0..nfun as usize {
            let Some(fo) = self.off(fun + 4 * i as u32) else { continue };
            let Some(f) = rd32(d, fo) else { continue };
            if f == 0 {
                continue;
            }
            let forwarder = (f >= rva && f < rva + size).then(|| self.cstr(f)).flatten();
            out.push(Export { name: named[i].clone(), ordinal: base + i as u32, forwarder });
        }
        out
    }

    fn thunks(&self, rva: u32) -> Vec<String> {
        let mut v = Vec::new();
        let step = if self.pe64 { 8 } else { 4 };
        let mut r = rva;
        while let Some(o) = self.off(r) {
            let t = if self.pe64 { rd64(&self.data, o).unwrap_or(0) } else { rd32(&self.data, o).unwrap_or(0) as u64 };
            if t == 0 {
                break;
            }
            let ord_flag = if self.pe64 { 1u64 << 63 } else { 1u64 << 31 };
            if t & ord_flag != 0 {
                v.push(format!("#{}", t & 0xffff));
            } else if let Some(n) = self.cstr((t as u32) + 2) {
                v.push(n);
            }
            r += step;
            if v.len() > 100_000 {
                break;
            }
        }
        v
    }

    pub fn imports(&self) -> Vec<Import> {
        let mut out = Vec::new();
        if let Some(&(rva, _)) = self.dirs.get(1) {
            if let Some(mut o) = self.off(rva).filter(|_| rva != 0) {
                while let (Some(ilt), Some(name), Some(iat)) = (rd32(&self.data, o), rd32(&self.data, o + 12), rd32(&self.data, o + 16)) {
                    if name == 0 {
                        break;
                    }
                    out.push(Import { dll: self.cstr(name).unwrap_or_default(), delay: false, names: self.thunks(if ilt != 0 { ilt } else { iat }) });
                    o += 20;
                }
            }
        }
        if let Some(&(rva, _)) = self.dirs.get(13) {
            if let Some(mut o) = self.off(rva).filter(|_| rva != 0) {
                while let (Some(name), Some(int)) = (rd32(&self.data, o + 4), rd32(&self.data, o + 16)) {
                    if name == 0 {
                        break;
                    }
                    out.push(Import { dll: self.cstr(name).unwrap_or_default(), delay: true, names: self.thunks(int) });
                    o += 32;
                }
            }
        }
        out
    }
}
