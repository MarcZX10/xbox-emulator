use crate::nt::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const IOCTL_XCONFIG_GET: u32 = 0x0022_600E;
pub const IOCTL_XCONFIG_SET: u32 = 0x0022_A011;
pub const MAX_VALUE: usize = 0x1_0000;

pub struct ConfigStore {
    file: Option<PathBuf>,
    values: Mutex<Option<BTreeMap<u32, Vec<u8>>>>,
}

fn decode(b: &[u8]) -> BTreeMap<u32, Vec<u8>> {
    let mut m = BTreeMap::new();
    let mut i = 0;
    while i + 8 <= b.len() {
        let id = u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let n = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        if i + 8 + n > b.len() {
            break;
        }
        m.insert(id, b[i + 8..i + 8 + n].to_vec());
        i += 8 + n;
    }
    m
}

fn encode(m: &BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let mut o = Vec::new();
    for (id, v) in m {
        o.extend_from_slice(&id.to_le_bytes());
        o.extend_from_slice(&(v.len() as u32).to_le_bytes());
        o.extend_from_slice(v);
    }
    o
}

impl ConfigStore {
    pub fn new(file: Option<PathBuf>) -> ConfigStore {
        ConfigStore { file, values: Mutex::new(None) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut BTreeMap<u32, Vec<u8>>) -> R) -> R {
        let mut g = self.values.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(|| self.file.as_ref().and_then(|p| std::fs::read(p).ok()).map(|b| decode(&b)).unwrap_or_default());
        f(m)
    }

    pub fn get(&self, id: u32, out: &mut [u8]) -> Result<usize, i32> {
        self.with(|m| match m.get(&id) {
            None => Err(STATUS_NOT_FOUND),
            Some(v) if v.len() > out.len() => Err(STATUS_BUFFER_TOO_SMALL),
            Some(v) => {
                out[..v.len()].copy_from_slice(v);
                Ok(v.len())
            }
        })
    }

    pub fn set(&self, id: u32, data: &[u8]) -> Result<usize, i32> {
        if data.is_empty() || data.len() > MAX_VALUE {
            return Err(STATUS_INVALID_PARAMETER);
        }
        self.with(|m| {
            m.insert(id, data.to_vec());
            if let Some(p) = &self.file {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                std::fs::write(p, encode(m)).map_err(|_| STATUS_UNSUCCESSFUL)?;
            }
            Ok(0)
        })
    }

    pub fn ioctl(&self, code: u32, input: &[u8], buf: &mut [u8]) -> Result<usize, i32> {
        if input.len() < 4 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let id = u32::from_le_bytes(input[..4].try_into().unwrap());
        match code {
            IOCTL_XCONFIG_GET => self.get(id, buf),
            IOCTL_XCONFIG_SET => self.set(id, buf),
            _ => Err(STATUS_INVALID_DEVICE_REQUEST),
        }
    }
}
