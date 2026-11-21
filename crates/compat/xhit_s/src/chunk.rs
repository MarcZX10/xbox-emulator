use std::io::Write;

pub const TAG_SQD2: u32 = 0x5351_4432;

pub struct ChunkWriter<'a> {
    pub out: Option<&'a mut dyn Write>,
    pub written: u64,
    pub compact: bool,
}

pub fn header(tag: u32, compact: bool, total: u32, extra: u32) -> [u64; 2] {
    let q1 = ((extra as u64) << 32) | total as u64;
    let mut q0 = ((compact as u64) << 32) | tag as u64;
    if !compact {
        q0 |= q1 << 33;
    }
    [q0, q1]
}

impl<'a> ChunkWriter<'a> {
    pub fn new(out: &'a mut dyn Write, compact: bool) -> Self {
        ChunkWriter { out: Some(out), written: 0, compact }
    }

    fn emit(&mut self, b: &[u8]) {
        if let Some(w) = self.out.as_mut() {
            if w.write_all(b).is_ok() {
                self.written += b.len() as u64;
            }
        }
    }

    pub unsafe fn write_chunk(&mut self, tag: u32, parts: &[(*const u8, u32)], extra: u32) -> i32 {
        if self.out.is_none() {
            return crate::E_FAIL;
        }
        let total = parts.iter().fold(0u32, |a, p| a.wrapping_add(p.1));
        let h = header(tag, self.compact, total, extra);
        let mut hb = [0u8; 16];
        hb[..8].copy_from_slice(&h[0].to_le_bytes());
        hb[8..].copy_from_slice(&h[1].to_le_bytes());
        self.emit(&hb);
        for &(p, n) in parts {
            if !p.is_null() && n != 0 {
                let s = core::slice::from_raw_parts(p, n as usize);
                self.emit(s);
            }
        }
        0
    }
}
