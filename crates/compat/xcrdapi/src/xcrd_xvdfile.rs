use std::path::Path;

pub const HDR: usize = 0x200;
pub const HEAD_LEN: usize = 0x3000;
pub const PAGE: u64 = 0x1000;
pub const INVALID_BLOCK: u32 = 0xffff_ffff;
pub const XVD_FIXED: u32 = 0;
pub const XVD_DYNAMIC: u32 = 1;

pub const ST_CORRUPT: i32 = 0xc000_0102u32 as i32;
pub const ST_IO: i32 = 0xc000_0185u32 as i32;
pub const ST_NOT_FOUND: i32 = 0xc000_0034u32 as i32;

fn g32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn g64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn s32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn page_align(n: u64) -> u64 {
    n.div_ceil(PAGE) * PAGE
}

#[derive(Clone, Debug)]
pub struct XvdFile {
    pub head: Vec<u8>,
    pub user: Vec<u8>,
    pub bat: Vec<u32>,
}

impl XvdFile {
    pub fn new(header_page: &[u8]) -> XvdFile {
        let mut head = vec![0u8; HEAD_LEN];
        let n = header_page.len().min(0x1000);
        head[..n].copy_from_slice(&header_page[..n]);
        let mut f = XvdFile { head, user: Vec::new(), bat: Vec::new() };
        f.rebuild_bat();
        f
    }

    pub fn h32(&self, o: usize) -> u32 {
        g32(&self.head, HDR + o)
    }

    pub fn h64(&self, o: usize) -> u64 {
        g64(&self.head, HDR + o)
    }

    pub fn drive_size(&self) -> u64 {
        self.h64(0x18)
    }

    pub fn set_drive_size(&mut self, v: u64) {
        self.head[HDR + 0x18..HDR + 0x20].copy_from_slice(&v.to_le_bytes());
        self.rebuild_bat();
    }

    pub fn xvd_type(&self) -> u32 {
        self.h32(0x80)
    }

    pub fn block_size(&self) -> u32 {
        self.h32(0x98)
    }

    pub fn entries_for(drive_size: u64, block: u32) -> usize {
        if block == 0 {
            return 0;
        }
        drive_size.div_ceil(block as u64) as usize
    }

    pub fn rebuild_bat(&mut self) {
        let n = if self.xvd_type() == XVD_DYNAMIC { Self::entries_for(self.drive_size(), self.block_size()) } else { 0 };
        self.bat.resize(n, INVALID_BLOCK);
        let dyn_len = page_align(n as u64 * 4) as u32;
        s32(&mut self.head, HDR + 0x94, dyn_len);
        let ulen = self.user.len() as u32;
        s32(&mut self.head, HDR + 0x8c, ulen);
    }

    pub fn user_offset(&self) -> u64 {
        HEAD_LEN as u64 + self.h32(0x88) as u64 + self.head[HDR + 0x270] as u64 * PAGE
    }

    pub fn bat_offset(&self) -> u64 {
        self.user_offset() + page_align(self.h32(0x8c) as u64)
    }

    pub fn dynamic_header_len(&self) -> u32 {
        self.h32(0x94)
    }

    pub fn allocated(&self) -> u32 {
        self.bat.iter().filter(|&&b| b != INVALID_BLOCK).count() as u32
    }

    pub fn bat_bytes(&self) -> Vec<u8> {
        self.bat.iter().flat_map(|b| b.to_le_bytes()).collect()
    }

    pub fn file_len(&self) -> u64 {
        self.bat_offset() + self.dynamic_header_len() as u64
    }

    pub fn set_user(&mut self, data: Vec<u8>) {
        self.user = data;
        self.rebuild_bat();
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.head.clone();
        out.resize(self.user_offset() as usize, 0);
        out.extend_from_slice(&self.user);
        out.resize(self.bat_offset() as usize, 0);
        out.extend_from_slice(&self.bat_bytes());
        out.resize(self.file_len() as usize, 0);
        out
    }

    pub fn parse(b: &[u8]) -> Result<XvdFile, i32> {
        if b.len() < 0x1000 || unsafe { crate::xcrd_a::validate_xvd_header(b.as_ptr().add(HDR)) } < 0 {
            return Err(ST_CORRUPT);
        }
        let mut head = vec![0u8; HEAD_LEN];
        let n = b.len().min(HEAD_LEN);
        head[..n].copy_from_slice(&b[..n]);
        let mut f = XvdFile { head, user: Vec::new(), bat: Vec::new() };
        let uo = f.user_offset() as usize;
        let ul = f.h32(0x8c) as usize;
        if ul != 0 {
            f.user = b.get(uo..uo + ul).ok_or(ST_CORRUPT)?.to_vec();
        }
        let n = if f.xvd_type() == XVD_DYNAMIC { Self::entries_for(f.drive_size(), f.block_size()) } else { 0 };
        if n != 0 {
            let bo = f.bat_offset() as usize;
            let raw = b.get(bo..bo + n * 4).ok_or(ST_CORRUPT)?;
            f.bat = raw.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        }
        if f.dynamic_header_len() as u64 != page_align(n as u64 * 4) {
            return Err(ST_CORRUPT);
        }
        Ok(f)
    }

    pub fn load(p: &Path) -> Result<XvdFile, i32> {
        match std::fs::read(p) {
            Ok(b) => Self::parse(&b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ST_NOT_FOUND),
            Err(_) => Err(ST_IO),
        }
    }

    pub fn save(&self, p: &Path) -> Result<(), i32> {
        std::fs::write(p, self.to_bytes()).map_err(|_| ST_IO)
    }
}
