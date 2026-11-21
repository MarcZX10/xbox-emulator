use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub const XFER_MSG: usize = 0x210;
pub const MSG_START: u32 = 1;
pub const MSG_START_ACK: u32 = 2;
pub const MSG_READ: u32 = 3;
pub const MSG_READ_ACK: u32 = 4;
pub const MSG_STOP: u32 = 5;
pub const MSG_STOP_ACK: u32 = 6;
pub const XFER_SESSION_MSG: u32 = 0x7800_00c0;

pub const STATUS_SUCCESS: i32 = 0;
pub const STATUS_INVALID_PARAMETER: i32 = 0xc000_000du32 as i32;
pub const STATUS_END_OF_FILE: i32 = 0xc000_0011u32 as i32;
pub const STATUS_OBJECT_NAME_INVALID: i32 = 0xc000_0033u32 as i32;
pub const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xc000_0034u32 as i32;
pub const STATUS_OBJECT_PATH_NOT_FOUND: i32 = 0xc000_003au32 as i32;
pub const STATUS_DEVICE_NOT_READY: i32 = 0xc000_00a3u32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: i32 = 0xc000_0010u32 as i32;
pub const STATUS_IO_DEVICE_ERROR: i32 = 0xc000_0185u32 as i32;
pub const STATUS_NO_MEMORY: i32 = 0xc000_0017u32 as i32;

pub trait XferBlob: Send {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, i32>;
}

pub trait XferStore: Send + Sync {
    fn open(&self, crd_path: &str) -> Result<Box<dyn XferBlob>, i32>;
}

static STORE: RwLock<Option<Arc<dyn XferStore>>> = RwLock::new(None);

pub fn set_store(store: Option<Arc<dyn XferStore>>) {
    *STORE.write().unwrap_or_else(|e| e.into_inner()) = store;
}

pub fn store() -> Option<Arc<dyn XferStore>> {
    if let Some(s) = STORE.read().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(s);
    }
    let root = std::env::var_os("TOMBOX_XVD_STORE")?;
    Some(Arc::new(DirStore::new(PathBuf::from(root))))
}

pub struct DirStore {
    pub root: PathBuf,
}

impl DirStore {
    pub fn new(root: PathBuf) -> DirStore {
        DirStore { root }
    }
    pub fn resolve(&self, crd_path: &str) -> Result<PathBuf, i32> {
        let rest = crd_path.strip_prefix('[').ok_or(STATUS_OBJECT_NAME_INVALID)?;
        let close = rest.find(":]").ok_or(STATUS_OBJECT_NAME_INVALID)?;
        let tag = &rest[..close];
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(STATUS_OBJECT_NAME_INVALID);
        }
        let tail = rest[close + 2..].trim_start_matches(['\\', '/']);
        if tail.is_empty() {
            return Err(STATUS_OBJECT_NAME_INVALID);
        }
        let mut out = self.root.join(tag.to_ascii_uppercase());
        for c in Path::new(&tail.replace('/', "\\")).components() {
            match c {
                Component::Normal(p) => out.push(p),
                _ => return Err(STATUS_OBJECT_NAME_INVALID),
            }
        }
        Ok(out)
    }
}

struct FileBlob(std::fs::File);

impl XferBlob for FileBlob {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, i32> {
        use std::os::windows::fs::FileExt;
        let mut done = 0usize;
        while done < buf.len() {
            match self.0.seek_read(&mut buf[done..], offset + done as u64) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(e) if e.raw_os_error() == Some(38) => break,
                Err(_) => return Err(STATUS_IO_DEVICE_ERROR),
            }
        }
        Ok(done)
    }
}

impl XferStore for DirStore {
    fn open(&self, crd_path: &str) -> Result<Box<dyn XferBlob>, i32> {
        let p = self.resolve(crd_path)?;
        match std::fs::File::open(&p) {
            Ok(f) => {
                if f.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                    return Err(STATUS_INVALID_DEVICE_REQUEST);
                }
                Ok(Box::new(FileBlob(f)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if p.parent().map(|d| d.is_dir()).unwrap_or(false) {
                    Err(STATUS_OBJECT_NAME_NOT_FOUND)
                } else {
                    Err(STATUS_OBJECT_PATH_NOT_FOUND)
                }
            }
            Err(_) => Err(STATUS_IO_DEVICE_ERROR),
        }
    }
}

static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);
static NEXT_TRANSFER: AtomicU32 = AtomicU32::new(1);

fn sessions() -> &'static Mutex<HashMap<u32, String>> {
    static S: std::sync::OnceLock<Mutex<HashMap<u32, String>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn session_open(id: u32) -> bool {
    sessions().lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id)
}

fn new_id(c: &AtomicU32) -> u32 {
    loop {
        let v = c.fetch_add(1, Ordering::Relaxed);
        if v != 0 && v != u32::MAX {
            return v;
        }
    }
}

pub fn open_session(pipe: String) -> Result<u32, i32> {
    let id = new_id(&NEXT_SESSION);
    sessions().lock().unwrap_or_else(|e| e.into_inner()).insert(id, pipe.clone());
    let spawned = std::thread::Builder::new().name(format!("xvd-xfer-host-{id}")).spawn(move || serve_session(id, &pipe));
    if spawned.is_err() {
        sessions().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        return Err(STATUS_NO_MEMORY);
    }
    Ok(id)
}

pub fn close_session(id: u32) -> i32 {
    if sessions().lock().unwrap_or_else(|e| e.into_inner()).remove(&id).is_some() {
        STATUS_SUCCESS
    } else {
        STATUS_INVALID_PARAMETER
    }
}

pub fn dispatch(code: u32, payload: &mut [u8], pipe: impl FnOnce() -> String) -> Option<i32> {
    if code != XFER_SESSION_MSG {
        return None;
    }
    if payload.len() < 0x20 {
        return Some(STATUS_INVALID_PARAMETER);
    }
    if payload[4] != 0 {
        match open_session(pipe()) {
            Ok(id) => {
                payload[0..4].copy_from_slice(&id.to_le_bytes());
                Some(STATUS_SUCCESS)
            }
            Err(st) => Some(st),
        }
    } else {
        let id = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        Some(close_session(id))
    }
}

fn rd32(m: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(m[o..o + 4].try_into().unwrap())
}

fn wr32(m: &mut [u8], o: usize, v: u32) {
    m[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn connect(pipe: &str) -> Option<std::fs::File> {
    let start = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(pipe) {
            Ok(f) => return Some(f),
            Err(_) if start.elapsed() < std::time::Duration::from_millis(5000) => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(_) => return None,
        }
    }
}

fn wide_path(m: &[u8]) -> String {
    let mut v = Vec::new();
    let mut i = 8;
    while i + 2 <= XFER_MSG && v.len() < 0x104 {
        let c = u16::from_le_bytes([m[i], m[i + 1]]);
        if c == 0 {
            break;
        }
        v.push(c);
        i += 2;
    }
    String::from_utf16_lossy(&v)
}

struct Transfer {
    id: u32,
    blob: Box<dyn XferBlob>,
}

pub fn serve_session(session: u32, pipe: &str) {
    let Some(mut f) = connect(pipe) else {
        sessions().lock().unwrap_or_else(|e| e.into_inner()).remove(&session);
        return;
    };
    let mut active: Option<Transfer> = None;
    let mut m = [0u8; XFER_MSG];
    while f.read_exact(&mut m).is_ok() {
        let mut r = [0u8; XFER_MSG];
        let mut data: Vec<u8> = Vec::new();
        match rd32(&m, 0) {
            MSG_START => {
                r[0..4].copy_from_slice(&MSG_START_ACK.to_le_bytes());
                let st = if active.is_some() {
                    STATUS_INVALID_DEVICE_REQUEST
                } else if !session_open(session) {
                    STATUS_DEVICE_NOT_READY
                } else {
                    match store() {
                        None => STATUS_DEVICE_NOT_READY,
                        Some(s) => match s.open(&wide_path(&m)) {
                            Ok(blob) => {
                                let id = new_id(&NEXT_TRANSFER);
                                wr32(&mut r, 0xc, id);
                                active = Some(Transfer { id, blob });
                                STATUS_SUCCESS
                            }
                            Err(st) => st,
                        },
                    }
                };
                wr32(&mut r, 8, st as u32);
            }
            MSG_READ => {
                let (tid, slot, len) = (rd32(&m, 8), rd32(&m, 0xc), rd32(&m, 0x10));
                let off = u64::from_le_bytes(m[0x18..0x20].try_into().unwrap());
                wr32(&mut r, 0, MSG_READ_ACK);
                wr32(&mut r, 0xc, tid);
                wr32(&mut r, 0x10, slot);
                let st = match active.as_mut() {
                    Some(t) if t.id == tid => {
                        let mut buf = Vec::new();
                        if buf.try_reserve_exact(len as usize).is_err() {
                            STATUS_NO_MEMORY
                        } else {
                            buf.resize(len as usize, 0);
                            match t.blob.read_at(off, &mut buf) {
                                Ok(0) if len != 0 => STATUS_END_OF_FILE,
                                Ok(n) => {
                                    buf.truncate(n);
                                    data = buf;
                                    STATUS_SUCCESS
                                }
                                Err(st) => st,
                            }
                        }
                    }
                    _ => STATUS_INVALID_PARAMETER,
                };
                wr32(&mut r, 8, st as u32);
                wr32(&mut r, 0x14, data.len() as u32);
            }
            MSG_STOP => {
                wr32(&mut r, 0, MSG_STOP_ACK);
                let st = match active.as_ref() {
                    Some(t) if t.id == rd32(&m, 8) => {
                        active = None;
                        STATUS_SUCCESS
                    }
                    _ => STATUS_INVALID_PARAMETER,
                };
                wr32(&mut r, 8, st as u32);
            }
            _ => break,
        }
        if f.write_all(&r).is_err() || (!data.is_empty() && f.write_all(&data).is_err()) {
            break;
        }
        m = [0u8; XFER_MSG];
    }
}
