use crate::chunk::{ChunkWriter, TAG_SQD2};
use crate::kmd;

pub struct SqCapture {
    pub data: usize,
    pub size: u32,
}

pub unsafe fn collect_sq(regions: (usize, usize), writer: Option<&mut ChunkWriter>) -> Option<SqCapture> {
    let args = [0u32; 4];
    let (p, n, status) = match kmd::request(regions, kmd::CMD_SQ, Some(&args)) {
        Ok(r) => r,
        Err(_) => return None,
    };
    if status != 0 {
        return None;
    }
    if let Some(w) = writer {
        w.write_chunk(TAG_SQD2, &[(p as *const u8, n)], 0);
    }
    Some(SqCapture { data: p, size: n })
}
