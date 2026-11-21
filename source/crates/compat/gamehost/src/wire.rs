pub const MSG_LAUNCH_NEW_GAME: u32 = 0x67;
pub const MSG_LAUNCH_RESTART_ON_CRASH: u32 = 0x68;
pub const MSG_CRASH_DUMP_INFO: u32 = 0xc8;
pub const MSG_CONTROLLER_NOTIFY: u32 = 0xc9;
pub const MSG_CRASH_DUMP_PROGRESS: u32 = 0xca;
pub const MSG_PACKAGE_MOUNT: u32 = 0x12c;
pub const MSG_PACKAGE_UNMOUNT: u32 = 0x12d;

pub const CTL_DUMP_IN_PROGRESS: u32 = 0x0b;
pub const CTL_NOTIFY: u32 = 0x0d;
pub const CTL_DUMP_PROGRESS: u32 = 0x10;

pub const CRASH_DUMP_INFO_SIZE: usize = 0x218;
pub const CRASH_DUMP_PATH_CCH: usize = 0x104;
pub const CONTROLLER_NOTIFY_SIZE: usize = 0x94;

pub const HR_INVALID_DATA: i32 = 0x8007000d_u32 as i32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: u32,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(id: u32, payload: Vec<u8>) -> Self {
        Message { id, payload }
    }
    pub fn size(&self) -> u32 {
        self.payload.len() as u32
    }
}

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn align(&mut self, a: usize) {
        while self.buf.len() % a != 0 {
            self.buf.push(0);
        }
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.align(4);
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.align(8);
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }
    pub fn wstr(&mut self, s: &[u16]) -> &mut Self {
        self.u32(s.len() as u32);
        self.align(2);
        for c in s {
            self.buf.extend_from_slice(&c.to_le_bytes());
        }
        self
    }
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

pub struct Reader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    fn align(&mut self, a: usize) {
        self.pos = (self.pos + a - 1) & !(a - 1);
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], i32> {
        if self.pos > self.buf.len() || self.buf.len() - self.pos < n {
            return Err(HR_INVALID_DATA);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u32(&mut self) -> Result<u32, i32> {
        self.align(4);
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64, i32> {
        self.align(8);
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn raw(&mut self, n: usize) -> Result<&'a [u8], i32> {
        self.take(n)
    }
    pub fn wstr(&mut self) -> Result<Vec<u16>, i32> {
        let n = self.u32()? as usize;
        self.align(2);
        let b = self.take(n.checked_mul(2).ok_or(HR_INVALID_DATA)?)?;
        let mut v: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if v.last() == Some(&0) {
            v.pop();
        }
        Ok(v)
    }
    pub fn consumed(&self) -> usize {
        self.pos
    }
}

pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashDumpInfo {
    pub config: [u8; 16],
    pub file: Vec<u16>,
}

impl CrashDumpInfo {
    pub fn flags(&self) -> u64 {
        u64::from_le_bytes(self.config[8..16].try_into().unwrap())
    }
    pub fn reserved(&self) -> u64 {
        u64::from_le_bytes(self.config[0..8].try_into().unwrap())
    }
    pub fn encode(&self) -> Message {
        let mut b = vec![0u8; CRASH_DUMP_INFO_SIZE];
        b[..16].copy_from_slice(&self.config);
        for (i, c) in self.file.iter().take(CRASH_DUMP_PATH_CCH).enumerate() {
            b[0x10 + i * 2..0x12 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        Message::new(MSG_CRASH_DUMP_INFO, b)
    }
    pub fn decode(p: &[u8]) -> Self {
        let mut config = [0u8; 16];
        config.copy_from_slice(&p[..16]);
        let file: Vec<u16> = p[0x10..CRASH_DUMP_INFO_SIZE].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
        CrashDumpInfo { config, file }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchNewGame {
    pub exe: Vec<u16>,
    pub args: Vec<u16>,
}

impl LaunchNewGame {
    pub fn encode(&self) -> Message {
        Message::new(MSG_LAUNCH_NEW_GAME, Writer::default().wstr(&self.exe).wstr(&self.args).finish())
    }
    pub fn decode(p: &[u8]) -> Result<(Self, usize), i32> {
        let mut r = Reader::new(p);
        let exe = r.wstr()?;
        let args = r.wstr()?;
        Ok((LaunchNewGame { exe, args }, r.consumed()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchRestartOnCrash {
    pub args: Vec<u16>,
    pub value: u32,
}

impl LaunchRestartOnCrash {
    pub fn encode(&self) -> Message {
        Message::new(MSG_LAUNCH_RESTART_ON_CRASH, Writer::default().wstr(&self.args).u32(self.value).finish())
    }
    pub fn decode(p: &[u8]) -> Result<(Self, usize), i32> {
        let mut r = Reader::new(p);
        let args = r.wstr()?;
        let value = r.u32()?;
        Ok((LaunchRestartOnCrash { args, value }, r.consumed()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageMount {
    pub name: Vec<u16>,
    pub path: Vec<u16>,
}

impl PackageMount {
    pub fn encode(&self) -> Message {
        Message::new(MSG_PACKAGE_MOUNT, Writer::default().wstr(&self.name).wstr(&self.path).finish())
    }
    pub fn decode(p: &[u8]) -> Result<(Self, usize), i32> {
        let mut r = Reader::new(p);
        let name = r.wstr()?;
        let path = r.wstr()?;
        Ok((PackageMount { name, path }, r.consumed()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageMountResponse {
    pub hr: i32,
    pub name: Vec<u16>,
    pub device_path: Vec<u16>,
}

impl PackageMountResponse {
    pub fn encode(&self) -> Message {
        Message::new(MSG_PACKAGE_MOUNT, Writer::default().u32(self.hr as u32).wstr(&self.name).wstr(&self.device_path).finish())
    }
    pub fn decode(p: &[u8]) -> Result<Self, i32> {
        let mut r = Reader::new(p);
        let hr = r.u32()? as i32;
        let name = r.wstr()?;
        let device_path = r.wstr()?;
        Ok(PackageMountResponse { hr, name, device_path })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUnmount {
    pub name: Vec<u16>,
}

impl PackageUnmount {
    pub fn encode(&self) -> Message {
        Message::new(MSG_PACKAGE_UNMOUNT, Writer::default().wstr(&self.name).finish())
    }
    pub fn decode(p: &[u8]) -> Result<(Self, usize), i32> {
        let mut r = Reader::new(p);
        let name = r.wstr()?;
        Ok((PackageUnmount { name }, r.consumed()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUnmountResponse {
    pub hr: i32,
    pub name: Vec<u16>,
}

impl PackageUnmountResponse {
    pub fn encode(&self) -> Message {
        Message::new(MSG_PACKAGE_UNMOUNT, Writer::default().u32(self.hr as u32).wstr(&self.name).finish())
    }
    pub fn decode(p: &[u8]) -> Result<Self, i32> {
        let mut r = Reader::new(p);
        let hr = r.u32()? as i32;
        let name = r.wstr()?;
        Ok(PackageUnmountResponse { hr, name })
    }
}
