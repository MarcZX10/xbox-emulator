pub const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
pub const E_INSUFFICIENT_BUFFER: i32 = 0x8007_007au32 as i32;
pub const MAX_PATH: usize = 0x104;
const INVALID_HANDLE: isize = -1;

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
    fn GetSystemWindowsDirectoryW(buf: *mut u16, n: u32) -> u32;
    fn FindFirstVolumeW(buf: *mut u16, n: u32) -> isize;
    fn FindNextVolumeW(h: isize, buf: *mut u16, n: u32) -> i32;
    fn FindVolumeClose(h: isize) -> i32;
    fn GetVolumePathNamesForVolumeNameW(name: *const u16, buf: *mut u16, n: u32, ret: *mut u32) -> i32;
    fn QueryDosDeviceW(name: *const u16, buf: *mut u16, n: u32) -> u32;
    fn GetLastError() -> u32;
}

pub fn last_error_hr() -> i32 {
    let e = unsafe { GetLastError() } as i32;
    if e <= 0 {
        e
    } else {
        ((e as u32 & 0xffff) | 0x8007_0000) as i32
    }
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn until_nul(b: &[u16]) -> Vec<u16> {
    b.iter().take_while(|c| **c != 0).copied().collect()
}

fn slashed(mut v: Vec<u16>) -> Vec<u16> {
    if v.last() != Some(&(b'\\' as u16)) {
        v.push(b'\\' as u16);
    }
    v
}

pub struct Volume {
    pub device: Vec<u16>,
    pub mount: Option<Vec<u16>>,
}

pub struct NtPathMap {
    pub entries: Vec<(Vec<u16>, Vec<u16>)>,
    pub unmounted: Vec<Vec<u16>>,
    pub sysdir: Vec<u16>,
    pub windir: Vec<u16>,
}

impl NtPathMap {
    fn add(&mut self, prefix: Vec<u16>, repl: Vec<u16>, slash: bool) {
        if slash {
            self.entries.push((slashed(prefix), slashed(repl)));
        } else {
            self.entries.push((prefix, repl));
        }
    }

    pub fn from_parts(sysdir: Vec<u16>, windir: Vec<u16>, volumes: &[Volume]) -> Result<NtPathMap, i32> {
        let mut m = NtPathMap { entries: Vec::new(), unmounted: Vec::new(), sysdir, windir: Vec::new() };
        m.add(w("\\??\\"), Vec::new(), false);
        if windir.len() < 2 || windir[1] != b':' as u16 {
            return Err(E_UNEXPECTED);
        }
        let mut wd = windir;
        if wd.last() != Some(&(b'\\' as u16)) {
            if wd.len() >= MAX_PATH {
                return Err(E_INSUFFICIENT_BUFFER);
            }
            wd.push(b'\\' as u16);
        }
        m.windir = wd.clone();
        m.add(w("\\SystemRoot\\"), wd, true);
        for v in volumes {
            match &v.mount {
                Some(p) => m.add(v.device.clone(), p.clone(), true),
                None => m.unmounted.push(slashed(v.device.clone())),
            }
        }
        m.add(w("\\Device\\LanmanRedirector\\"), w("\\\\"), false);
        m.add(w("\\Device\\Mup\\"), w("\\\\"), false);
        m.add(w("\\Device\\vmsmb\\"), w("\\\\?\\VMSMB\\"), false);
        m.add(w("\\GLOBAL??\\"), w("\\\\?\\"), false);
        m.add(w("\\\\"), w("\\\\"), false);
        Ok(m)
    }

    pub fn build() -> Result<NtPathMap, i32> {
        let mut buf = [0u16; 0x105];
        if unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), MAX_PATH as u32) } == 0 {
            return Err(last_error_hr());
        }
        let sysdir = until_nul(&buf);
        let mut wbuf = [0u16; 0x105];
        if unsafe { GetSystemWindowsDirectoryW(wbuf.as_mut_ptr(), 0x105) } == 0 {
            return Err(last_error_hr());
        }
        let windir = until_nul(&wbuf);
        if windir.len() < 2 || windir[1] != b':' as u16 {
            return Err(E_UNEXPECTED);
        }
        if windir.last() != Some(&(b'\\' as u16)) && windir.len() >= MAX_PATH {
            return Err(E_INSUFFICIENT_BUFFER);
        }
        let volumes = enum_volumes()?;
        NtPathMap::from_parts(sysdir, windir, &volumes)
    }

    pub fn translate_w(&self, path: &[u16]) -> (Vec<u16>, bool) {
        let bs = b'\\' as u16;
        for (p, r) in &self.entries {
            if path.starts_with(p) {
                let mut o = r.clone();
                o.extend_from_slice(&path[p.len()..]);
                return (o, true);
            }
        }
        for p in &self.unmounted {
            if path.starts_with(p) {
                return (Vec::new(), false);
            }
        }
        if !path.contains(&bs) {
            let mut o = self.sysdir.clone();
            o.extend(w("\\drivers\\"));
            o.extend_from_slice(path);
            return (o, true);
        }
        let at = |i: usize| path.get(i).copied().unwrap_or(0);
        if at(0) == bs && at(1) == b';' as u16 && at(2) != 0 && at(3) == b':' as u16 {
            if let Some(i) = path.iter().skip(4).position(|c| *c == bs) {
                let mut o = vec![bs];
                o.extend_from_slice(&path[4 + i..]);
                return (o, true);
            }
        }
        if at(0) == 0 || at(1) == b':' as u16 {
            return (path.to_vec(), true);
        }
        if at(0) == bs {
            if path.starts_with(&w("\\Device\\")) {
                return (Vec::new(), false);
            }
            if path == w("\\FI_UNKNOWN").as_slice() {
                return (path.to_vec(), true);
            }
            let mut o = vec![self.windir[0], b':' as u16];
            o.extend_from_slice(path);
            return (o, true);
        }
        let mut o = self.windir.clone();
        o.extend_from_slice(path);
        (o, true)
    }

    pub fn translate(&self, path: &str) -> (String, bool) {
        let (o, ok) = self.translate_w(&w(path));
        (String::from_utf16_lossy(&o), ok)
    }
}

pub fn enum_volumes() -> Result<Vec<Volume>, i32> {
    let mut name = [0u16; MAX_PATH];
    let h = unsafe { FindFirstVolumeW(name.as_mut_ptr(), MAX_PATH as u32) };
    if h == INVALID_HANDLE {
        return Err(last_error_hr());
    }
    let mut out = Vec::new();
    loop {
        name[MAX_PATH - 1] = 0;
        let mut paths = vec![0u16; 0x410];
        let mut ret = 0u32;
        let ok = unsafe { GetVolumePathNamesForVolumeNameW(name.as_ptr(), paths.as_mut_ptr(), 0x410, &mut ret) } != 0;
        let mounted = ok && paths[0] != 0;
        let mut n = name.iter().position(|c| *c == 0).unwrap_or(MAX_PATH);
        if n != 0 && name[n - 1] == b'\\' as u16 {
            n -= 1;
            name[n] = 0;
        }
        let skip = if n >= 4 && name[0] == b'\\' as u16 && name[1] == b'\\' as u16 && name[2] == b'?' as u16 && name[3] == b'\\' as u16 { 4 } else { 0 };
        let mut dev = [0u16; MAX_PATH];
        if unsafe { QueryDosDeviceW(name.as_ptr().add(skip), dev.as_mut_ptr(), MAX_PATH as u32) } != 0 {
            let device = until_nul(&dev);
            out.push(Volume { device, mount: if mounted { Some(until_nul(&paths)) } else { None } });
        }
        if unsafe { FindNextVolumeW(h, name.as_mut_ptr(), MAX_PATH as u32) } == 0 {
            break;
        }
    }
    unsafe { FindVolumeClose(h) };
    Ok(out)
}
