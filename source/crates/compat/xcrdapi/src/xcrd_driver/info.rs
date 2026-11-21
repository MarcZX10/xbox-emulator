use super::*;
use crate::xcrd_xvdfile::{page_align, XvdFile};

pub const ST_NO_SUCH_DEVICE: i32 = 0xc000_000eu32 as i32;
pub const PRESENT_CRDS: [u32; 5] = [0, 1, 2, 3, 5];
pub const DEVICE_NAME: &str = "TomBox emulated internal storage";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counters {
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

extern "system" {
    fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
}

fn disk_space(p: &Path) -> Option<(u64, u64)> {
    let mut d = p.to_path_buf();
    while !d.exists() {
        d = d.parent()?.to_path_buf();
    }
    let w: Vec<u16> = d.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } == 0 {
        return None;
    }
    Some((total, free))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

impl Driver {
    fn counters_key(path: &str) -> String {
        lower(path)
    }

    pub fn counters_of(&self, path: &str) -> Counters {
        self.counters.get(&Self::counters_key(path)).copied().unwrap_or_default()
    }

    pub(super) fn xvd_user_data(&mut self, sp: &Path, p: &mut [u8], write: bool, data_off: usize, want: usize, offset: usize, flags: u32) -> i32 {
        let path = Self::path_at(p, 4, 8).unwrap_or_default();
        let mut f = match XvdFile::load(sp) {
            Ok(f) => f,
            Err(e) => return e,
        };
        let key = Self::counters_key(&path);
        if write {
            if data_off + want > p.len() {
                return STATUS_INVALID_PARAMETER;
            }
            let mut cur = f.user.clone();
            if flags & 1 != 0 {
                cur.truncate(offset);
            }
            if cur.len() < offset + want {
                cur.resize(offset + want, 0);
            }
            cur[offset..offset + want].copy_from_slice(&p[data_off..data_off + want]);
            if cur.len() > u32::MAX as usize {
                return STATUS_INVALID_PARAMETER;
            }
            f.set_user(cur);
            if let Err(e) = f.save(sp) {
                return e;
            }
            if flags & 2 != 0 {
                meta_set(sp, "hidden", "1");
            }
            let c = self.counters.entry(key).or_default();
            c.write_ops += 1;
            c.write_bytes += want as u64;
            return ST_OK;
        }
        let avail = f.user.len().saturating_sub(offset);
        if want == 0 {
            s32(p, 0xc, avail as u32);
            return if avail == 0 { ST_OK } else { ST_TOO_SMALL };
        }
        let n = avail.min(want);
        if data_off + n > p.len() {
            return STATUS_INVALID_PARAMETER;
        }
        p[data_off..data_off + n].copy_from_slice(&f.user[offset..offset + n]);
        s32(p, 0xc, n as u32);
        let c = self.counters.entry(key).or_default();
        c.read_ops += 1;
        c.read_bytes += n as u64;
        ST_OK
    }

    pub(super) fn resize(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 4, 8) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        let grow = g64(p, 0x10);
        let sp = match self.resolve(&path) {
            Some(Backing::Store(sp)) => sp,
            Some(Backing::Package(_)) => return ST_ACCESS_DENIED,
            None => return ST_NOT_FOUND,
        };
        let mut f = match XvdFile::load(&sp) {
            Ok(f) => f,
            Err(e) => return e,
        };
        let new = match f.drive_size().checked_add(page_align(grow)) {
            Some(v) if v <= 0x100_0000_0000 => v,
            _ => return STATUS_INVALID_PARAMETER,
        };
        f.set_drive_size(new);
        match f.save(&sp) {
            Ok(()) => ST_OK,
            Err(e) => e,
        }
    }

    pub(super) fn bat_info(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 0x18, 0x1c) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        let data_off = g32(p, 0x18) as usize + g32(p, 0x1c) as usize + 2;
        let cap = g32(p, 0x20) as usize;
        let sp = match self.resolve(&path) {
            Some(Backing::Store(sp)) => sp,
            Some(Backing::Package(_)) => return ST_NOT_SUPPORTED,
            None => return ST_NOT_FOUND,
        };
        let mut f = match XvdFile::load(&sp) {
            Ok(f) => f,
            Err(e) => return e,
        };
        if f.xvd_type() == crate::xcrd_xvdfile::XVD_DYNAMIC && f.block_size() != 0 {
            let used = dir_size(&sidecar(&sp, ".content"));
            let need = (used.div_ceil(f.block_size() as u64) as usize).min(f.bat.len());
            let mut next = f.bat.iter().filter(|&&b| b != crate::xcrd_xvdfile::INVALID_BLOCK).map(|&b| b + 1).max().unwrap_or(0);
            let mut changed = false;
            for i in 0..need {
                if f.bat[i] == crate::xcrd_xvdfile::INVALID_BLOCK {
                    f.bat[i] = next;
                    next += 1;
                    changed = true;
                }
            }
            if changed {
                if let Err(e) = f.save(&sp) {
                    return e;
                }
            }
        }
        let bat = f.bat_bytes();
        s64(p, 0, f.bat.len() as u64);
        s64(p, 8, f.bat_offset());
        s32(p, 0x10, f.block_size());
        s32(p, 0x14, 4);
        s64(p, 0x28, f.drive_size());
        s32(p, 0x30, f.allocated());
        s32(p, 0x34, f.xvd_type());
        s64(p, 0x38, f.file_len());
        s32(p, 0x40, f.dynamic_header_len());
        s32(p, 0x20, bat.len() as u32);
        if bat.len() > cap {
            return ST_TOO_SMALL;
        }
        if data_off + bat.len() > p.len() {
            return STATUS_INVALID_PARAMETER;
        }
        p[data_off..data_off + bat.len()].copy_from_slice(&bat);
        ST_OK
    }

    pub(super) fn query(&mut self, p: &mut [u8]) -> i32 {
        if g32(p, 0) == 0x418 {
            return self.query_xvd(p);
        }
        self.query_crd(p)
    }

    fn query_xvd(&mut self, p: &mut [u8]) -> i32 {
        let code = g32(p, 0xc);
        let path = match Self::path_at(p, 0, 4) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        if code == 0x17 && self.is_any_xvc(&path) {
            s64(p, 0x10, self.global_features());
            return ST_OK;
        }
        let sp = match self.resolve(&path) {
            Some(Backing::Store(sp)) => sp,
            Some(Backing::Package(_)) => return ST_NOT_SUPPORTED,
            None => return ST_NOT_FOUND,
        };
        let f = match XvdFile::load(&sp) {
            Ok(f) => f,
            Err(e) => return e,
        };
        if p.len() < 0x418 {
            return STATUS_INVALID_PARAMETER;
        }
        let out = &mut p[0x10..0x418];
        match code {
            0 | 5 => {
                out[..0x80].fill(0);
                basic_info(&f.head[..HEADER_SIZE], &mut out[..0x80]);
                out[0x78..0x80].copy_from_slice(&f.file_len().to_le_bytes());
            }
            1 => out[..0x402].fill(0),
            3 => out[..8].fill(0),
            6 => {
                let min = meta_get(&sp, "pls_min").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                let max = meta_get(&sp, "pls_max").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                s32(out, 0, min);
                s32(out, 4, max);
            }
            7 => {
                out[..0x18].fill(0);
                s64(out, 0, f.h64(0x1bc));
                let id = unhex(&meta_get(&sp, "content_id").unwrap_or_default());
                if id.len() == 16 {
                    out[8..0x18].copy_from_slice(&id);
                }
            }
            9 | 0xc | 0xf => out[0] = 0,
            0xa => out[..0x168].fill(0),
            0xb => out[..0x158].fill(0),
            0xd => out[..0x38].fill(0),
            0xe => {
                out[..0x150].fill(0);
                let c = self.counters_of(&path);
                s64(out, 0, c.read_ops);
                s64(out, 8, c.read_ops);
                s64(out, 0x10, c.write_ops);
                s64(out, 0x18, c.write_ops);
                s64(out, 0x20, c.read_bytes);
                s64(out, 0x28, c.write_bytes);
                s64(out, 0x40, f.file_len());
            }
            0x12..=0x15 => out[..0x1f8].fill(0),
            0x16 => out[0] = f.head[0x200 + 0x271],
            0x17 => {
                let v = meta_get(&sp, "features").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                s64(out, 0, v);
            }
            0x1a | 0x1b => return ST_NOT_SUPPORTED,
            _ => return ST_BAD_REQUEST,
        }
        ST_OK
    }

    fn query_crd(&mut self, p: &mut [u8]) -> i32 {
        let crd = g32(p, 8);
        let class = g32(p, 0xc);
        if !matches!(class, 2 | 4 | 8 | 0x10 | 0x11) {
            return ST_BAD_REQUEST;
        }
        if !PRESENT_CRDS.contains(&crd) {
            return ST_NO_SUCH_DEVICE;
        }
        if p.len() < 0x418 {
            return STATUS_INVALID_PARAMETER;
        }
        let root = if (crd as usize) < PATH_PREFIXES.len() { self.store.join(prefix_dir(crd as usize)) } else { self.store.clone() };
        let (total, free) = match disk_space(&root) {
            Some(v) => v,
            None => return STATUS_IO_DEVICE_ERROR,
        };
        match class {
            2 => {
                s64(p, 0x10, total);
                s64(p, 0x18, free);
            }
            4 => {
                p[0x10..0x418].fill(0);
                s64(p, 0x10, total);
                s64(p, 0x18, free);
                s64(p, 0x20, total);
                s32(p, 0x28, 0);
                s32(p, 0x2c, 1);
                p[0x44] = 1;
                p[0x4a] = 1;
                let name_off = 0xf0usize;
                let cb = (DEVICE_NAME.encode_utf16().count() + 1) * 2;
                put_wide(p, 0x10 + name_off, cb, DEVICE_NAME);
                s32(p, 0x4c, name_off as u32);
                s32(p, 0x50, cb as u32);
            }
            0x10 => p[0x10..0x20].fill(0),
            _ => return ST_NOT_SUPPORTED,
        }
        ST_OK
    }

    pub(super) fn set_info(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 0, 4) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        let class = g32(p, 8);
        if class == 0xa && self.is_any_xvc(&path) {
            if std::fs::create_dir_all(&self.store).is_err() {
                return STATUS_IO_DEVICE_ERROR;
            }
            return match std::fs::write(self.store.join("features"), g64(p, 0x10).to_string()) {
                Ok(()) => ST_OK,
                Err(e) => io_status(&e),
            };
        }
        let sp = match self.resolve(&path) {
            Some(Backing::Store(sp)) => sp,
            Some(Backing::Package(_)) => return ST_ACCESS_DENIED,
            None => return ST_NOT_FOUND,
        };
        if self.lun_of(&path).is_some() {
            return ST_SHARING;
        }
        let mut f = match XvdFile::load(&sp) {
            Ok(f) => f,
            Err(e) => return e,
        };
        let meta = |k: &str, v: String| if meta_set(&sp, k, &v) { ST_OK } else { STATUS_IO_DEVICE_ERROR };
        match class {
            0 => {
                f.head[0x200 + 0x1bc..0x200 + 0x1c4].copy_from_slice(&p[0x10..0x18]);
                match f.save(&sp) {
                    Ok(()) => ST_OK,
                    Err(e) => e,
                }
            }
            1 => meta("content_id", hex(&p[0x10..0x20])),
            2 => {
                f.head[0x200 + 0x19c..0x200 + 0x1ac].copy_from_slice(&p[0x10..0x20]);
                match f.save(&sp) {
                    Ok(()) => ST_OK,
                    Err(e) => e,
                }
            }
            3 => meta("unsealed", "1".into()),
            4 => meta("stream_name", wide_str(p, 0x10, 0x100).unwrap_or_default()),
            5 | 6 => {
                let id = g32(p, 0x10);
                let mut list: Vec<u32> = meta_get(&sp, "rsm").unwrap_or_default().split(',').filter_map(|s| s.parse().ok()).collect();
                let has = list.contains(&id);
                if class == 5 {
                    if has {
                        return ST_COLLISION;
                    }
                    list.push(id);
                } else {
                    if !has {
                        return ST_NOT_FOUND;
                    }
                    list.retain(|&x| x != id);
                }
                meta("rsm", list.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))
            }
            8 => {
                let n = p[0x13] as usize;
                let name = wide_str(p, 0x14, n + 2).unwrap_or_default();
                let r = meta("xvcs_port", u16::from_le_bytes([p[0x10], p[0x11]]).to_string());
                if r != ST_OK {
                    return r;
                }
                let r = meta("xvcs_flags", p[0x12].to_string());
                if r != ST_OK {
                    return r;
                }
                meta("xvcs_server", name)
            }
            7 => meta("blob_tier", g32(p, 0x10).to_string()),
            9 => meta("recreate_if_corrupt", "1".into()),
            0xa => meta("features", g64(p, 0x10).to_string()),
            0x11 => {
                let (min, max) = (g32(p, 0x10), g32(p, 0x14));
                if min > max {
                    return STATUS_INVALID_PARAMETER;
                }
                let r = meta("pls_min", min.to_string());
                if r != ST_OK {
                    return r;
                }
                meta("pls_max", max.to_string())
            }
            _ => ST_BAD_REQUEST,
        }
    }
}
