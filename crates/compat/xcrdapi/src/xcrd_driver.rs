use crate::xcrd_a::{Wnf, WnfCallback, WNF_CORRUPTION};
use crate::xcrd_core::*;
use core::ffi::c_void;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub mod info;

pub const ST_OK: i32 = 0;
pub const ST_NOT_FOUND: i32 = 0xc000_0034u32 as i32;
pub const ST_COLLISION: i32 = 0xc000_0035u32 as i32;
pub const ST_ACCESS_DENIED: i32 = 0xc000_0022u32 as i32;
pub const ST_SHARING: i32 = 0xc000_0043u32 as i32;
pub const ST_BAD_REQUEST: i32 = 0xc000_0010u32 as i32;
pub const ST_NOT_SUPPORTED: i32 = 0xc000_00bbu32 as i32;
pub const ST_TOO_SMALL: i32 = 0xc000_0023u32 as i32;
pub const ST_NO_MORE_FILES: i32 = 0x8000_0006u32 as i32;
pub const ST_NO_MORE_ENTRIES: i32 = 0x8000_001au32 as i32;
pub const ST_CORRUPT: i32 = 0xc000_0102u32 as i32;
pub const ST_TYPE_MISMATCH: i32 = 0xc000_0024u32 as i32;
pub const ST_SYNTAX: i32 = 0xc000_003bu32 as i32;
pub const ST_DISK_FULL: i32 = 0xc000_007fu32 as i32;

pub const CONTROL_PAGE_IOCTL: u32 = 0x7800_013c;
pub const DISK_BASE: u32 = 0x40;
pub const SCSI_BUS: u8 = 7;
pub const SCSI_TARGET: u8 = 0x58;
pub const HEADER_SIZE: usize = 0x1000;
pub const BLOCK_SIZE: u32 = 0xa_a000;
const HANDLE_TAG: isize = 0x5843 << 32;

#[derive(Clone, Debug, PartialEq)]
pub enum Backing {
    Store(PathBuf),
    Package(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Volume {
    pub xcrd_path: String,
    pub backing: Backing,
    pub root: PathBuf,
    pub read_only: bool,
    pub size: u64,
    pub part_size: u64,
    pub dismounted: bool,
}

pub type BlobEntry = (String, u64, u64, u32);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Obj {
    Adapter,
    Disk(u8),
    Volume(u8),
    VolumeRoot(u8),
    MountManager,
}

pub struct Driver {
    pub package: PathBuf,
    pub store: PathBuf,
    pub luns: Vec<Option<Volume>>,
    handles: HashMap<isize, Obj>,
    next: isize,
    pub control_page: usize,
    pub control_flags: usize,
    subs: Vec<(usize, u64, usize, usize)>,
    states: HashMap<u64, (u32, Vec<u8>)>,
    pending: Vec<u64>,
    next_sub: usize,
    enums: HashMap<u64, (Vec<BlobEntry>, usize)>,
    next_enum: u64,
    pub counters: HashMap<String, info::Counters>,
    #[cfg(test)]
    pub log: Vec<(u32, Vec<u8>)>,
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn wide_str(b: &[u8], off: usize, cb: usize) -> Option<String> {
    let end = off.checked_add(cb)?;
    if end > b.len() {
        return None;
    }
    let v: Vec<u16> = b[off..end].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    Some(String::from_utf16_lossy(&v))
}

fn put_wide(b: &mut [u8], off: usize, cap: usize, s: &str) -> bool {
    let w: Vec<u16> = s.encode_utf16().chain([0]).collect();
    if w.len() * 2 > cap || off + w.len() * 2 > b.len() {
        return false;
    }
    for (i, c) in w.iter().enumerate() {
        b[off + i * 2..off + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
    true
}

fn g32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).unwrap_or(0)
}

fn g64(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).unwrap_or(0)
}

fn s32(b: &mut [u8], o: usize, v: u32) {
    if let Some(s) = b.get_mut(o..o + 4) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

fn s64(b: &mut [u8], o: usize, v: u64) {
    if let Some(s) = b.get_mut(o..o + 8) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

pub fn split_xcrd_path(p: &str) -> Option<(usize, String)> {
    let close = p.find(":]")?;
    let tag = &p[..close + 2];
    let idx = PATH_PREFIXES.iter().position(|x| !x.is_empty() && x.eq_ignore_ascii_case(tag))?;
    let rest = p[close + 2..].trim_start_matches('\\').to_string();
    if rest.split('\\').any(|c| c == "..") {
        return None;
    }
    Some((idx, rest))
}

fn prefix_dir(idx: usize) -> String {
    PATH_PREFIXES[idx].trim_start_matches('[').trim_end_matches(":]").to_string()
}

pub fn filetime_now() -> u64 {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (d.as_nanos() / 100) as u64 + 116_444_736_000_000_000
}

pub fn build_header(drive_size: u64, xvd_type: u32, content_type: u32, volume_flags: u32, vduid: [u8; 16], uduid: [u8; 16]) -> Vec<u8> {
    let mut f = vec![0u8; HEADER_SIZE];
    let h = &mut f[0x200..];
    h[0..8].copy_from_slice(b"msft-xvd");
    s32(h, 8, volume_flags);
    s32(h, 0xc, 3);
    s64(h, 0x10, filetime_now());
    s64(h, 0x18, drive_size);
    h[0x20..0x30].copy_from_slice(&vduid);
    h[0x30..0x40].copy_from_slice(&uduid);
    s32(h, 0x80, xvd_type);
    s32(h, 0x84, content_type);
    s32(h, 0x98, BLOCK_SIZE);
    f
}

pub fn basic_info(file: &[u8], out: &mut [u8]) {
    let h = &file[0x200..];
    out[0..2].copy_from_slice(&h[0xc..0xe]);
    out[2] = h[0x271];
    out[4..8].copy_from_slice(&h[0x80..0x84]);
    out[8..0xc].copy_from_slice(&h[0x84..0x88]);
    out[0xc..0x10].copy_from_slice(&h[8..0xc]);
    out[0x10..0x18].copy_from_slice(&h[0x10..0x18]);
    out[0x18..0x20].copy_from_slice(&h[0x18..0x20]);
    out[0x20..0x40].copy_from_slice(&h[0x20..0x40]);
    out[0x40..0x50].copy_from_slice(&h[0x1ac..0x1bc]);
    out[0x50..0x58].copy_from_slice(&h[0x1bc..0x1c4]);
    out[0x58..0x68].copy_from_slice(&h[0x18c..0x19c]);
    out[0x68..0x78].copy_from_slice(&h[0x19c..0x1ac]);
    out[0x78..0x80].copy_from_slice(&h[0x78..0x80]);
}

fn sidecar(p: &Path, ext: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

fn random_guid() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let mut g = [0u8; 16];
    for half in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(filetime_now() ^ half);
        g[half as usize * 8..half as usize * 8 + 8].copy_from_slice(&h.finish().to_le_bytes());
    }
    g[7] = (g[7] & 0x0f) | 0x40;
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

fn meta_get(p: &Path, key: &str) -> Option<String> {
    let t = std::fs::read_to_string(sidecar(p, ".meta")).ok()?;
    t.lines().find_map(|l| l.strip_prefix(&format!("{key}=")).map(|v| v.to_string()))
}

fn meta_set(p: &Path, key: &str, val: &str) -> bool {
    let m = sidecar(p, ".meta");
    let t = std::fs::read_to_string(&m).unwrap_or_default();
    let mut lines: Vec<String> = t.lines().filter(|l| !l.starts_with(&format!("{key}="))).map(|l| l.to_string()).collect();
    lines.push(format!("{key}={val}"));
    std::fs::write(m, lines.join("\n")).is_ok()
}

fn dir_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => t += dir_size(&e.path()),
                Ok(m) => t += m.len(),
                Err(_) => {}
            }
        }
    }
    t
}

fn io_status(e: &std::io::Error) -> i32 {
    match e.kind() {
        std::io::ErrorKind::NotFound => ST_NOT_FOUND,
        std::io::ErrorKind::PermissionDenied => ST_ACCESS_DENIED,
        std::io::ErrorKind::AlreadyExists => ST_COLLISION,
        _ => STATUS_IO_DEVICE_ERROR,
    }
}

impl Driver {
    pub fn new(package: PathBuf, store: PathBuf) -> Driver {
        let page = Box::leak(vec![0u8; 0x1000].into_boxed_slice()).as_mut_ptr() as usize;
        let flags = Box::leak(Box::new(0u32)) as *mut u32 as usize;
        let mut states = HashMap::new();
        for n in [crate::xcrd_a::WNF_EXTERNAL_STORAGE, crate::xcrd_a::WNF_WRITE_BUDGET, WNF_CORRUPTION] {
            states.insert(n, (0u32, vec![0u8; 4]));
        }
        Driver { package, store, luns: Vec::new(), handles: HashMap::new(), next: 1, control_page: page, control_flags: flags, subs: Vec::new(), states, pending: Vec::new(), next_sub: 1, enums: HashMap::new(), next_enum: 1, counters: HashMap::new(), #[cfg(test)] log: Vec::new() }
    }

    pub fn resolve(&self, xcrd: &str) -> Option<Backing> {
        let (idx, rel) = split_xcrd_path(xcrd)?;
        let s = self.store.join(prefix_dir(idx)).join(&rel);
        if s.is_file() {
            return Some(Backing::Store(s));
        }
        if rel.is_empty() {
            return None;
        }
        let p = self.package.join(&rel);
        if p.exists() {
            return Some(Backing::Package(p));
        }
        None
    }

    fn store_path(&self, xcrd: &str) -> Option<PathBuf> {
        let (idx, rel) = split_xcrd_path(xcrd)?;
        if rel.is_empty() {
            return None;
        }
        Some(self.store.join(prefix_dir(idx)).join(rel))
    }

    fn header_of(&self, b: &Backing) -> Result<Vec<u8>, i32> {
        let p = match b {
            Backing::Store(p) => p.clone(),
            Backing::Package(p) if p.is_file() => p.clone(),
            Backing::Package(_) => return Err(ST_NOT_SUPPORTED),
        };
        let mut f = std::fs::read(&p).map_err(|e| io_status(&e))?;
        if f.len() < HEADER_SIZE {
            return Err(ST_CORRUPT);
        }
        f.truncate(HEADER_SIZE);
        if unsafe { crate::xcrd_a::validate_xvd_header(f.as_ptr().add(0x200)) } < 0 {
            return Err(ST_CORRUPT);
        }
        Ok(f)
    }

    fn lun_of(&self, xcrd: &str) -> Option<u8> {
        self.luns.iter().position(|v| v.as_ref().map(|v| v.xcrd_path.eq_ignore_ascii_case(xcrd)).unwrap_or(false)).map(|i| i as u8)
    }

    fn open_obj(&mut self, o: Obj) -> Handle {
        let h = HANDLE_TAG | (self.next << 2);
        self.next += 1;
        self.handles.insert(h, o);
        h
    }

    fn mounted(&self, lun: u8) -> Option<&Volume> {
        self.luns.get(lun as usize).and_then(|v| v.as_ref())
    }

    pub fn open(&mut self, name: &str) -> Option<Result<Handle, u32>> {
        let n = lower(name);
        if n == "\\\\.\\scsi0:" {
            return Some(Ok(self.open_obj(Obj::Adapter)));
        }
        if n == "\\\\.\\mountpointmanager" {
            return Some(Ok(self.open_obj(Obj::MountManager)));
        }
        if let Some(d) = n.strip_prefix("\\\\.\\physicaldrive").and_then(|d| d.parse::<u32>().ok()) {
            if d >= DISK_BASE && d < DISK_BASE + 0x40 {
                let lun = (d - DISK_BASE) as u8;
                return Some(match self.mounted(lun) {
                    Some(_) => Ok(self.open_obj(Obj::Disk(lun))),
                    None => Err(2),
                });
            }
            return None;
        }
        if let Some(rest) = n.strip_prefix("\\\\?\\globalroot\\device\\harddisk") {
            let (num, tail) = rest.split_once('\\')?;
            let d = num.parse::<u32>().ok()?;
            if d < DISK_BASE || d >= DISK_BASE + 0x40 {
                return None;
            }
            let lun = (d - DISK_BASE) as u8;
            if self.mounted(lun).map(|v| !v.dismounted) != Some(true) {
                return Some(Err(2));
            }
            return Some(match tail {
                "partition1" => Ok(self.open_obj(Obj::Volume(lun))),
                "partition1\\" => Ok(self.open_obj(Obj::VolumeRoot(lun))),
                _ => Err(2),
            });
        }
        if let Some(rest) = n.strip_prefix("\\\\?\\volume{5843524b-0000-0000-0000-0000000000") {
            let lun = u8::from_str_radix(rest.trim_end_matches(['}', '\\']), 16).ok()?;
            return Some(match self.mounted(lun) {
                Some(_) => Ok(self.open_obj(Obj::Volume(lun))),
                None => Err(2),
            });
        }
        None
    }

    pub fn owns(&self, h: Handle) -> bool {
        self.handles.contains_key(&h)
    }

    pub fn close(&mut self, h: Handle) -> bool {
        self.handles.remove(&h).is_some()
    }

    pub fn ioctl(&mut self, h: Handle, code: u32, input: &[u8], out: &mut [u8], ret: &mut u32) -> Result<(), u32> {
        let o = *self.handles.get(&h).ok_or(6u32)?;
        match (o, code) {
            (Obj::Disk(_), 0x2d_1400) => {
                if out.len() < 0x28 {
                    return Err(0x7a);
                }
                out.fill(0);
                s32(out, 0, 0x28);
                s32(out, 4, 0x28);
                s32(out, 0x1c, 0xe);
                *ret = 0x28;
                Ok(())
            }
            (Obj::Disk(lun), 0x4_1018) => {
                if out.len() < 8 {
                    return Err(0x7a);
                }
                s32(out, 0, 8);
                out[4] = 0;
                out[5] = SCSI_BUS;
                out[6] = SCSI_TARGET;
                out[7] = lun;
                *ret = 8;
                Ok(())
            }
            (Obj::Volume(lun), 0x7_0024) => {
                if self.mounted(lun).map(|v| v.read_only).unwrap_or(true) {
                    Err(0x13)
                } else {
                    Ok(())
                }
            }
            (Obj::Volume(lun), 0x56_001c) => {
                if out.len() < 0x14 {
                    return Err(0x7a);
                }
                out.fill(0);
                s32(out, 0, DISK_BASE + lun as u32);
                put_wide(out, 4, 0x10, "XCRD");
                *ret = 0x14;
                Ok(())
            }
            (Obj::Volume(lun), 0x56_0000) => {
                let size = self.mounted(lun).map(|v| v.part_size).unwrap_or(0);
                if out.len() < 0x20 {
                    if out.len() >= 4 {
                        s32(out, 0, 1);
                    }
                    return Err(0xea);
                }
                s32(out, 0, 1);
                s32(out, 8, DISK_BASE + lun as u32);
                s64(out, 0x10, 0);
                s64(out, 0x18, size);
                *ret = 0x20;
                Ok(())
            }
            (Obj::Volume(_), 0x9_0018) | (Obj::Volume(_), 0x7_0140) => Ok(()),
            (Obj::Volume(lun), 0x9_0020) => {
                if let Some(Some(v)) = self.luns.get_mut(lun as usize) {
                    v.dismounted = true;
                }
                Ok(())
            }
            (Obj::Volume(lun), 0x7_c0d0) => {
                if input.len() < 0x10 || g32(input, 0) != 1 {
                    return Err(0x57);
                }
                let grow = g64(input, 8);
                let disk = self.backing_size(lun);
                let v = self.luns.get_mut(lun as usize).and_then(|v| v.as_mut()).ok_or(0x15u32)?;
                if v.read_only {
                    return Err(0x13);
                }
                let want = v.part_size.checked_add(grow).ok_or(0x57u32)?;
                if want > disk {
                    return Err(0x70);
                }
                v.part_size = want;
                Ok(())
            }
            (Obj::Volume(lun), 0x9_00f0) => {
                if input.len() < 8 {
                    return Err(0x57);
                }
                let sectors = g64(input, 0);
                let v = self.luns.get_mut(lun as usize).and_then(|v| v.as_mut()).ok_or(0x15u32)?;
                if v.read_only {
                    return Err(0x13);
                }
                let want = sectors.checked_mul(512).ok_or(0x57u32)?;
                if want > v.part_size || want < v.size {
                    return Err(0x57);
                }
                v.size = want;
                Ok(())
            }
            (Obj::MountManager, 0x6d_0008) => self.mount_points(out, ret),
            _ => Err(1),
        }
    }

    fn backing_size(&self, lun: u8) -> u64 {
        match self.mounted(lun).map(|v| v.backing.clone()) {
            Some(b) => self.header_of(&b).map(|h| g64(&h, 0x218)).unwrap_or(0),
            None => 0,
        }
    }

    fn mount_points(&self, out: &mut [u8], ret: &mut u32) -> Result<(), u32> {
        let vols: Vec<u8> = (0..self.luns.len() as u8).filter(|&l| self.mounted(l).is_some()).collect();
        let n = vols.len();
        let mut strings: Vec<(String, String)> = Vec::new();
        for &l in &vols {
            strings.push((format!("\\??\\Volume{{5843524b-0000-0000-0000-0000000000{l:02x}}}"), format!("\\Device\\Harddisk{}\\Partition1", DISK_BASE + l as u32)));
        }
        let mut off = 8 + n * 0x18;
        let mut size = off;
        for (a, b) in &strings {
            size += (a.len() + b.len()) * 2;
        }
        if out.len() < size {
            if out.len() >= 4 {
                s32(out, 0, size as u32);
            }
            return Err(0xea);
        }
        out.fill(0);
        s32(out, 0, size as u32);
        s32(out, 4, n as u32);
        for (i, (a, b)) in strings.iter().enumerate() {
            let e = 8 + i * 0x18;
            let aw: Vec<u16> = a.encode_utf16().collect();
            let bw: Vec<u16> = b.encode_utf16().collect();
            s32(out, e, off as u32);
            out[e + 4..e + 6].copy_from_slice(&((aw.len() * 2) as u16).to_le_bytes());
            for (k, c) in aw.iter().enumerate() {
                out[off + k * 2..off + k * 2 + 2].copy_from_slice(&c.to_le_bytes());
            }
            off += aw.len() * 2;
            s32(out, e + 0x10, off as u32);
            out[e + 0x14..e + 0x16].copy_from_slice(&((bw.len() * 2) as u16).to_le_bytes());
            for (k, c) in bw.iter().enumerate() {
                out[off + k * 2..off + k * 2 + 2].copy_from_slice(&c.to_le_bytes());
            }
            off += bw.len() * 2;
        }
        *ret = size as u32;
        Ok(())
    }

    pub fn fs_size(&self, h: Handle, out: &mut [u8; 0x18]) -> Option<i32> {
        let lun = match self.handles.get(&h)? {
            Obj::Volume(l) | Obj::VolumeRoot(l) => *l,
            _ => return Some(ST_BAD_REQUEST),
        };
        let v = self.mounted(lun)?;
        let units = v.size / 512;
        let used = if v.read_only { units } else { dir_size(&v.root).div_ceil(512) };
        out[0..8].copy_from_slice(&units.to_le_bytes());
        out[8..0x10].copy_from_slice(&units.saturating_sub(used).to_le_bytes());
        out[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        out[0x14..0x18].copy_from_slice(&512u32.to_le_bytes());
        Some(ST_OK)
    }

    pub fn nt_ioctl(&mut self, h: Handle, code: u32, buf: &mut [u8]) -> Option<i32> {
        let o = *self.handles.get(&h)?;
        if o != Obj::Adapter {
            return Some(ST_BAD_REQUEST);
        }
        if code == CONTROL_PAGE_IOCTL {
            if buf.len() < 0x10 {
                return Some(ST_TOO_SMALL);
            }
            s64(buf, 0, self.control_flags as u64);
            s64(buf, 8, self.control_page as u64);
            return Some(ST_OK);
        }
        if code != IOCTL_SCSI_MINIPORT || buf.len() < SRB_HEADER {
            return Some(ST_BAD_REQUEST);
        }
        let xc = g32(buf, 0x10);
        if xc == 0x7800_000c {
            buf[4..12].copy_from_slice(&XVD_SIGNATURE);
            s32(buf, 0x14, 0);
            return Some(ST_OK);
        }
        if buf[4..12] != XVD_SIGNATURE {
            return Some(STATUS_INVALID_PARAMETER);
        }
        #[cfg(test)]
        self.log.push((xc, buf[SRB_HEADER..].to_vec()));
        let rc = self.request(xc, &mut buf[SRB_HEADER..]);
        s32(buf, 0x14, rc as u32);
        Some(ST_OK)
    }

    pub fn request(&mut self, code: u32, p: &mut [u8]) -> i32 {
        match code {
            0x7800_0004 => self.mount(p),
            0x7800_0008 => self.remove_lun(p),
            0x7800_0078 => self.address_for_path(p),
            0x7800_0020 => self.create(p),
            0x7800_0024 => self.delete(p),
            0x7800_0028 => self.user_data(p, false),
            0x7800_0094 => self.user_data(p, true),
            0x7800_0050 => self.query(p),
            0x7800_0054 => self.set_info(p),
            0x7800_0084 => self.enum_blobs(p),
            0x7800_008c => self.enum_devices(p),
            0x7800_00a0 => self.resize(p),
            0x7800_002c => self.bat_info(p),
            crate::xcrd2_hsv::XFER_SESSION_MSG => crate::xcrd2_hsv::dispatch(code, p, crate::xcrd2_xfer::pipe_text).unwrap_or(ST_BAD_REQUEST),
            _ => ST_BAD_REQUEST,
        }
    }

    fn path_at(p: &[u8], off_field: usize, len_field: usize) -> Option<String> {
        wide_str(p, g32(p, off_field) as usize, g32(p, len_field) as usize)
    }

    fn mount(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 0, 4) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        if let Some(l) = self.lun_of(&path) {
            if let Some(Some(v)) = self.luns.get_mut(l as usize) {
                v.dismounted = false;
            }
            p[0x20] = l;
            p[0x21] = SCSI_TARGET;
            p[0x22] = SCSI_BUS;
            return ST_OK;
        }
        let b = match self.resolve(&path) {
            Some(b) => b,
            None => return ST_NOT_FOUND,
        };
        let want_ct = if g32(p, 0x18) & 0x80 != 0 { Some(g32(p, 0x1c)) } else { None };
        let (root, read_only, size) = match &b {
            Backing::Store(hp) => {
                let h = match self.header_of(&b) {
                    Ok(h) => h,
                    Err(e) => {
                        if e == ST_CORRUPT {
                            self.publish(WNF_CORRUPTION, 1);
                        }
                        return e;
                    }
                };
                if let Some(ct) = want_ct {
                    if g32(&h, 0x284) != ct {
                        return ST_TYPE_MISMATCH;
                    }
                }
                let root = sidecar(hp, ".content");
                if std::fs::create_dir_all(&root).is_err() {
                    return STATUS_IO_DEVICE_ERROR;
                }
                (root, g32(&h, 0x208) & 1 != 0, g64(&h, 0x218))
            }
            Backing::Package(pp) => {
                if !pp.is_dir() {
                    return ST_NOT_SUPPORTED;
                }
                let s = dir_size(pp).div_ceil(0x1000) * 0x1000;
                (pp.clone(), true, s)
            }
        };
        let slot = match self.luns.iter().position(|v| v.is_none()) {
            Some(i) => i,
            None if self.luns.len() < 0x40 => {
                self.luns.push(None);
                self.luns.len() - 1
            }
            None => return ST_DISK_FULL,
        };
        self.luns[slot] = Some(Volume { xcrd_path: path, backing: b, root, read_only, size, part_size: size, dismounted: false });
        p[0x20] = slot as u8;
        p[0x21] = SCSI_TARGET;
        p[0x22] = SCSI_BUS;
        ST_OK
    }

    fn remove_lun(&mut self, p: &mut [u8]) -> i32 {
        let (lun, target, bus) = (p[0], p[1], p[2]);
        if target != SCSI_TARGET || bus != SCSI_BUS {
            return ST_NOT_FOUND;
        }
        match self.luns.get_mut(lun as usize) {
            Some(v @ Some(_)) => {
                *v = None;
                ST_OK
            }
            _ => ST_NOT_FOUND,
        }
    }

    fn address_for_path(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 4, 8) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        match self.lun_of(&path) {
            Some(l) => {
                p[0x18] = l;
                p[0x19] = SCSI_TARGET;
                p[0x1a] = SCSI_BUS;
                ST_OK
            }
            None => ST_NOT_FOUND,
        }
    }

    fn create(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 0x28, 0x2c) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        let sp = match self.store_path(&path) {
            Some(s) => s,
            None => return ST_SYNTAX,
        };
        if sp.exists() || self.resolve(&path).is_some() {
            return ST_COLLISION;
        }
        let mut vduid = [0u8; 16];
        vduid.copy_from_slice(&p[8..0x18]);
        if vduid == [0u8; 16] {
            vduid = random_guid();
        }
        let mut uduid = [0u8; 16];
        uduid.copy_from_slice(&p[0x40..0x50]);
        let size = g64(p, 0x20).wrapping_mul(0x1000);
        let mut hdr = build_header(size, g32(p, 0x18), g32(p, 0x1c), g32(p, 4), vduid, uduid);
        hdr[0x200 + 0x18c..0x200 + 0x19c].copy_from_slice(&p[0x50..0x60]);
        if unsafe { crate::xcrd_a::validate_xvd_header(hdr.as_ptr().add(0x200)) } < 0 {
            return STATUS_INVALID_PARAMETER;
        }
        if let Some(parent) = sp.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return io_status(&e);
            }
        }
        if let Err(e) = crate::xcrd_xvdfile::XvdFile::new(&hdr).save(&sp) {
            return e;
        }
        if let Err(e) = std::fs::create_dir_all(sidecar(&sp, ".content")) {
            return io_status(&e);
        }
        if g32(p, 0x30) != 0 {
            if let Some(p2) = Self::path_at(p, 0x30, 0x34) {
                meta_set(&sp, "source", &p2);
            }
        }
        p[0x60..0x70].copy_from_slice(&vduid);
        ST_OK
    }

    fn delete(&mut self, p: &mut [u8]) -> i32 {
        let path = match Self::path_at(p, 4, 8) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        if self.lun_of(&path).is_some() {
            return ST_SHARING;
        }
        match self.resolve(&path) {
            Some(Backing::Store(sp)) => {
                if let Err(e) = std::fs::remove_file(&sp) {
                    return io_status(&e);
                }
                let _ = std::fs::remove_dir_all(sidecar(&sp, ".content"));
                let _ = std::fs::remove_file(sidecar(&sp, ".userdata"));
                let _ = std::fs::remove_file(sidecar(&sp, ".meta"));
                ST_OK
            }
            Some(Backing::Package(_)) => ST_ACCESS_DENIED,
            None => ST_NOT_FOUND,
        }
    }

    fn user_data(&mut self, p: &mut [u8], write: bool) -> i32 {
        let path = match Self::path_at(p, 4, 8) {
            Some(s) => s,
            None => return STATUS_INVALID_PARAMETER,
        };
        let data_off = g32(p, 4) as usize + g32(p, 8) as usize + 2;
        let want = g32(p, 0xc) as usize;
        let offset = g64(p, 0x10) as usize;
        let flags = g32(p, 0x18);
        let b = self.resolve(&path);
        if let Some(Backing::Store(sp)) = &b {
            if self.header_of(&Backing::Store(sp.clone())).is_ok() {
                return self.xvd_user_data(sp, p, write, data_off, want, offset, flags);
            }
        }
        let (target, _xvd): (PathBuf, Option<PathBuf>) = match (&b, write) {
            (Some(Backing::Store(sp)), _) => (sp.clone(), None),
            (Some(Backing::Package(pp)), false) if pp.is_file() => (pp.clone(), None),
            (Some(Backing::Package(_)), false) => return ST_NOT_SUPPORTED,
            (_, true) => match self.store_path(&path) {
                Some(sp) => (sp, None),
                None => return ST_SYNTAX,
            },
            (None, false) => return ST_NOT_FOUND,
        };
        let mut cur = std::fs::read(&target).unwrap_or_default();
        if write {
            if data_off + want > p.len() {
                return STATUS_INVALID_PARAMETER;
            }
            if let Some(parent) = target.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return io_status(&e);
                }
            }
            if flags & 1 != 0 {
                cur.truncate(offset);
            }
            if cur.len() < offset + want {
                cur.resize(offset + want, 0);
            }
            cur[offset..offset + want].copy_from_slice(&p[data_off..data_off + want]);
            if let Err(e) = std::fs::write(&target, &cur) {
                return io_status(&e);
            }
            if flags & 2 != 0 {
                meta_set(&target, "hidden", "1");
            }
            return ST_OK;
        }
        let avail = cur.len().saturating_sub(offset);
        if want == 0 {
            s32(p, 0xc, avail as u32);
            return if avail == 0 { ST_OK } else { ST_TOO_SMALL };
        }
        let n = avail.min(want);
        if data_off + n > p.len() {
            return STATUS_INVALID_PARAMETER;
        }
        p[data_off..data_off + n].copy_from_slice(&cur[offset..offset + n]);
        s32(p, 0xc, n as u32);
        ST_OK
    }

    fn list_dir(&self, device: u32, dir: &str) -> Vec<(String, u64, u64, u32)> {
        let mut out = Vec::new();
        let rel = dir.trim_matches('\\');
        let mut roots = Vec::new();
        if (device as usize) < PATH_PREFIXES.len() && !PATH_PREFIXES[device as usize].is_empty() {
            roots.push(self.store.join(prefix_dir(device as usize)).join(rel));
        }
        roots.push(self.package.join(rel));
        for r in roots {
            let mut ents: Vec<_> = match std::fs::read_dir(&r) {
                Ok(rd) => rd.flatten().collect(),
                Err(_) => continue,
            };
            ents.sort_by_key(|e| e.file_name());
            for e in ents {
                let name = e.file_name().to_string_lossy().to_string();
                let l = lower(&name);
                if l.ends_with(".content") || l.ends_with(".userdata") || l.ends_with(".meta") {
                    continue;
                }
                if out.iter().any(|(n, ..): &(String, u64, u64, u32)| n.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                let m = match e.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| (d.as_nanos() / 100) as u64 + 116_444_736_000_000_000).unwrap_or(0);
                let mut attrs = if m.is_dir() { 0x10 } else if m.permissions().readonly() { 0x21 } else { 0x20 };
                if meta_get(&e.path(), "hidden").as_deref() == Some("1") {
                    attrs |= 2;
                }
                out.push((name, if m.is_dir() { 0 } else { m.len() }, t, attrs));
            }
        }
        out
    }

    fn enum_blobs(&mut self, p: &mut [u8]) -> i32 {
        let device = g32(p, 0x10);
        let op = g32(p, 0x18);
        let cookie = match op {
            0 => {
                let dir = wide_str(p, 0x20, 0x208).unwrap_or_default();
                if dir.split('\\').any(|c| c == "..") {
                    return STATUS_INVALID_PARAMETER;
                }
                let ents = self.list_dir(device, &dir);
                let id = self.next_enum;
                self.next_enum += 1;
                self.enums.insert(id, (ents, 0));
                s64(p, 0, id);
                s64(p, 8, 0x5843_5244);
                id
            }
            1 | 2 if g64(p, 8) == 0x5843_5244 && self.enums.contains_key(&g64(p, 0)) => g64(p, 0),
            0..=2 => return STATUS_INVALID_PARAMETER,
            _ => return ST_BAD_REQUEST,
        };
        if op == 2 {
            self.enums.remove(&cookie);
            return ST_OK;
        }
        let (ents, cur) = self.enums.get_mut(&cookie).unwrap();
        let e = match ents.get(*cur) {
            Some(e) => e.clone(),
            None => return ST_NO_MORE_FILES,
        };
        *cur += 1;
        p[0x20..0x228].fill(0);
        put_wide(p, 0x20, 0x208, &e.0);
        s64(p, 0x228, e.1);
        s64(p, 0x230, e.2);
        s32(p, 0x348, e.3);
        ST_OK
    }

    fn enum_devices(&mut self, p: &mut [u8]) -> i32 {
        let op = g32(p, 0);
        if op == 2 {
            return ST_OK;
        }
        let cursor = if op == 0 { 0 } else { g32(p, 8) };
        let devices: &[u32] = &[1];
        match devices.get(cursor as usize) {
            Some(&d) => {
                s32(p, 4, d);
                s32(p, 8, cursor + 1);
                ST_OK
            }
            None => ST_NO_MORE_ENTRIES,
        }
    }

    fn is_any_xvc(&self, path: &str) -> bool {
        matches!(split_xcrd_path(path), Some((_, rel)) if rel.eq_ignore_ascii_case("ANYXVC"))
    }

    pub fn global_features(&self) -> u64 {
        std::fs::read_to_string(self.store.join("features")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }

    pub fn publish(&mut self, name: u64, value: u32) {
        let st = self.states.entry(name).or_insert((0, vec![0u8; 4]));
        st.0 += 1;
        st.1 = value.to_le_bytes().to_vec();
        self.pending.push(name);
    }

    pub fn resolve_device_path(&self, path: &str) -> Option<PathBuf> {
        let n = lower(&path.replace('/', "\\"));
        let body = ["\\\\?\\", "\\\\.\\", "\\??\\"].iter().find_map(|p| n.strip_prefix(p))?;
        let (lun, tail) = if let Some(rest) = body.strip_prefix("globalroot\\device\\harddisk") {
            let (num, tail) = rest.split_once("\\partition1")?;
            if !(tail.is_empty() || tail.starts_with('\\')) {
                return None;
            }
            let d = num.parse::<u32>().ok()?;
            (u8::try_from(d.checked_sub(DISK_BASE)?).ok()?, tail)
        } else {
            let rest = body.strip_prefix("volume{5843524b-0000-0000-0000-0000000000")?;
            let (hex, tail) = rest.split_once('}')?;
            if hex.len() != 2 || !(tail.is_empty() || tail.starts_with('\\')) {
                return None;
            }
            (u8::from_str_radix(hex, 16).ok()?, tail)
        };
        let v = self.mounted(lun)?;
        if v.dismounted {
            return None;
        }
        let orig = path.replace('/', "\\");
        let tail = orig.get(orig.len().checked_sub(tail.len())?..)?;
        let rel = tail.trim_start_matches('\\');
        if rel.split('\\').any(|c| c == "..") || rel.contains(':') {
            return None;
        }
        Some(if rel.is_empty() { v.root.clone() } else { v.root.join(rel) })
    }
}

#[cfg(not(test))]
pub fn default_root() -> PathBuf {
    std::env::var_os("TOMBOX_XCRD_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("D:\\TomBox\\run"))
}

#[cfg(test)]
pub fn default_root() -> PathBuf {
    std::env::temp_dir().join(format!("xcrd_driver_global_{}", std::process::id()))
}

static DRIVER: OnceLock<Mutex<Driver>> = OnceLock::new();

pub fn global() -> &'static Mutex<Driver> {
    let mut fresh = false;
    let d = DRIVER.get_or_init(|| {
        fresh = true;
        let r = default_root();
        Mutex::new(Driver::new(r.join("Mount"), r.join("XcrdStore")))
    });
    if fresh && !cfg!(test) {
        crate::xcrd2_hsv::set_store(Some(std::sync::Arc::new(DriverStore { drv: d })));
    }
    d
}

pub struct DriverStore {
    pub drv: &'static Mutex<Driver>,
}

impl crate::xcrd2_hsv::XferStore for DriverStore {
    fn open(&self, crd_path: &str) -> Result<Box<dyn crate::xcrd2_hsv::XferBlob>, i32> {
        use crate::xcrd2_hsv as h;
        if split_xcrd_path(crd_path).is_none() {
            return Err(h::STATUS_OBJECT_NAME_INVALID);
        }
        let b = self.drv.lock().unwrap_or_else(|e| e.into_inner()).resolve(crd_path);
        let p = match b {
            Some(Backing::Store(p)) => p,
            Some(Backing::Package(p)) if p.is_file() => p,
            Some(Backing::Package(_)) => return Err(h::STATUS_INVALID_DEVICE_REQUEST),
            None => return Err(h::STATUS_OBJECT_NAME_NOT_FOUND),
        };
        match std::fs::File::open(&p) {
            Ok(f) => Ok(Box::new(DriverBlob(f))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(h::STATUS_OBJECT_NAME_NOT_FOUND),
            Err(_) => Err(h::STATUS_IO_DEVICE_ERROR),
        }
    }
}

struct DriverBlob(std::fs::File);

impl crate::xcrd2_hsv::XferBlob for DriverBlob {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, i32> {
        use std::os::windows::fs::FileExt;
        let mut done = 0usize;
        while done < buf.len() {
            match self.0.seek_read(&mut buf[done..], offset + done as u64) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(e) if e.raw_os_error() == Some(38) => break,
                Err(_) => return Err(crate::xcrd2_hsv::STATUS_IO_DEVICE_ERROR),
            }
        }
        Ok(done)
    }
}

pub struct EmuHost {
    pub drv: &'static Mutex<Driver>,
}

impl EmuHost {
    pub fn global() -> EmuHost {
        EmuHost { drv: global() }
    }
    fn d(&self) -> std::sync::MutexGuard<'static, Driver> {
        self.drv.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Host for EmuHost {
    fn create_file(&mut self, name: &[u16], access: u32, share: u32, disp: u32, flags: u32) -> Handle {
        let s = String::from_utf16_lossy(name);
        let r = self.d().open(&s);
        match r {
            Some(Ok(h)) => h,
            Some(Err(e)) => {
                RealHost.set_last_error(e);
                INVALID_HANDLE
            }
            None if cfg!(test) && (s.starts_with("\\\\.\\") || s.starts_with("\\\\?\\")) => {
                RealHost.set_last_error(2);
                INVALID_HANDLE
            }
            None => RealHost.create_file(name, access, share, disp, flags),
        }
    }
    fn close_handle(&mut self, h: Handle) -> bool {
        let mut d = self.d();
        if d.owns(h) {
            return d.close(h);
        }
        drop(d);
        RealHost.close_handle(h)
    }
    fn nt_ioctl(&mut self, h: Handle, code: u32, buf: &mut [u8]) -> i32 {
        let r = self.d().nt_ioctl(h, code, buf);
        match r {
            Some(s) => s,
            None => RealHost.nt_ioctl(h, code, buf),
        }
    }
    fn ioctl(&mut self, h: Handle, code: u32, input: &[u8], output: &mut [u8], returned: &mut u32) -> bool {
        let mut d = self.d();
        if d.owns(h) {
            let r = d.ioctl(h, code, input, output, returned);
            drop(d);
            return match r {
                Ok(()) => true,
                Err(e) => {
                    RealHost.set_last_error(e);
                    false
                }
            };
        }
        drop(d);
        RealHost.ioctl(h, code, input, output, returned)
    }
    fn last_error(&mut self) -> u32 {
        RealHost.last_error()
    }
    fn set_last_error(&mut self, e: u32) {
        RealHost.set_last_error(e)
    }
    fn sleep(&mut self, ms: u32) {
        RealHost.sleep(ms)
    }
    fn tick64(&mut self) -> u64 {
        RealHost.tick64()
    }
    fn query_fs_size(&mut self, h: Handle, out: &mut [u8; 0x18]) -> i32 {
        let r = self.d().fs_size(h, out);
        match r {
            Some(s) => s,
            None => RealHost.query_fs_size(h, out),
        }
    }
    fn drive_type(&mut self, root: &[u16]) -> u32 {
        RealHost.drive_type(root)
    }
    fn find_first(&mut self, pattern: &[u16], data: &mut [u8; FIND_DATA]) -> Handle {
        RealHost.find_first(pattern, data)
    }
    fn find_next(&mut self, h: Handle, data: &mut [u8; FIND_DATA]) -> bool {
        RealHost.find_next(h, data)
    }
    fn find_close(&mut self, h: Handle) -> bool {
        RealHost.find_close(h)
    }
    fn seek(&mut self, h: Handle, pos: i64) -> bool {
        RealHost.seek(h, pos)
    }
    fn read(&mut self, h: Handle, buf: &mut [u8], read: &mut u32) -> bool {
        RealHost.read(h, buf, read)
    }
}

pub struct EmuWnf {
    pub drv: &'static Mutex<Driver>,
}

impl Wnf for EmuWnf {
    fn query(&mut self, name: u64, cb: WnfCallback, ctx: *mut c_void, stamp: &mut u32) -> i32 {
        let st = self.drv.lock().unwrap_or_else(|e| e.into_inner()).states.get(&name).cloned();
        match st {
            Some((s, data)) => {
                *stamp = s;
                unsafe { cb(name, s, core::ptr::null(), ctx, data.as_ptr(), data.len() as u32) };
                0
            }
            None => ST_NOT_FOUND,
        }
    }
    fn subscribe(&mut self, name: u64, _stamp: u32, cb: WnfCallback, ctx: *mut c_void, sub: &mut *mut c_void) -> i32 {
        let mut d = self.drv.lock().unwrap_or_else(|e| e.into_inner());
        if !d.states.contains_key(&name) {
            return ST_NOT_FOUND;
        }
        let id = d.next_sub;
        d.next_sub += 1;
        d.subs.push((id, name, cb as usize, ctx as usize));
        *sub = id as *mut c_void;
        0
    }
    fn unsubscribe(&mut self, sub: *mut c_void) -> i32 {
        let mut d = self.drv.lock().unwrap_or_else(|e| e.into_inner());
        let before = d.subs.len();
        d.subs.retain(|s| s.0 != sub as usize);
        if d.subs.len() == before {
            STATUS_INVALID_PARAMETER
        } else {
            0
        }
    }
}

#[cfg(test)]
thread_local! {
    pub static TEST_DRIVER: std::cell::Cell<Option<&'static Mutex<Driver>>> = const { std::cell::Cell::new(None) };
}

pub fn active() -> &'static Mutex<Driver> {
    #[cfg(test)]
    {
        if let Some(d) = TEST_DRIVER.with(|t| t.get()) {
            return d;
        }
    }
    global()
}

pub fn flush_active() {
    flush_events(active());
}

pub fn flush_events(drv: &'static Mutex<Driver>) {
    loop {
        let (calls, data) = {
            let mut d = drv.lock().unwrap_or_else(|e| e.into_inner());
            let name = match d.pending.pop() {
                Some(n) => n,
                None => return,
            };
            let (stamp, data) = d.states.get(&name).cloned().unwrap_or_default();
            let calls: Vec<(u64, u32, usize, usize)> = d.subs.iter().filter(|s| s.1 == name).map(|s| (name, stamp, s.2, s.3)).collect();
            (calls, data)
        };
        for (name, stamp, cb, ctx) in calls {
            let f: WnfCallback = unsafe { core::mem::transmute::<usize, WnfCallback>(cb) };
            unsafe { f(name, stamp, core::ptr::null(), ctx as *mut c_void, data.as_ptr(), data.len() as u32) };
        }
    }
}

#[derive(Clone, Copy)]
pub struct ControlPage {
    pub flags: usize,
    pub page: usize,
}

static CONTROL: Mutex<Option<ControlPage>> = Mutex::new(None);

pub fn dstorage_init() -> i32 {
    let mut c = CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    if c.is_some() {
        return 0;
    }
    let mut a: *mut c_void = core::ptr::null_mut();
    let hr = unsafe { crate::xcrd_a::XCrdOpenAdapter(&mut a) };
    if hr < 0 {
        return hr;
    }
    let h = unsafe { adapter_handle(a) };
    let mut out = [0u8; 0x10];
    let st = host(|x| x.nt_ioctl(h, CONTROL_PAGE_IOCTL, &mut out));
    let hr = nt_to_hr(st);
    if hr < 0 {
        return hr;
    }
    *c = Some(ControlPage { flags: g64(&out, 0) as usize, page: g64(&out, 8) as usize });
    0
}

pub fn control_page() -> Option<ControlPage> {
    *CONTROL.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub fn reset_control_page() {
    *CONTROL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[no_mangle]
pub unsafe extern "system" fn TomBoxXcrdResolvePath(path: *const u16, out: *mut u16, cch: u32) -> u32 {
    if path.is_null() {
        return 0;
    }
    let s = String::from_utf16_lossy(unsafe { wslice(path) });
    let r = global().lock().unwrap_or_else(|e| e.into_inner()).resolve_device_path(&s);
    let w: Vec<u16> = match r {
        Some(p) => p.to_string_lossy().encode_utf16().collect(),
        None => return 0,
    };
    if out.is_null() || (w.len() + 1) as u32 > cch {
        return (w.len() + 1) as u32;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(w.as_ptr(), out, w.len());
        *out.add(w.len()) = 0;
    }
    w.len() as u32
}
