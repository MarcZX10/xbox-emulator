#![allow(dead_code)]

pub use XAudio2_9::xma_stream::silent_packet;
use std::io::{Read, Seek, SeekFrom};
use XAudio2_9::ffi::*;

pub const PACKET: usize = 2048;
pub const FRAME: u32 = 512;

pub fn silent_stream(channels: usize, packets_per_stream: usize, frames: u32) -> Vec<u8> {
    let streams = (channels + 1) / 2;
    let mut out = Vec::new();
    for _ in 0..packets_per_stream {
        for s in 0..streams {
            let ch = if s == streams - 1 && channels % 2 == 1 { 1 } else { 2 };
            let skip = if streams > 1 { (streams - 1) as u32 } else { 0 };
            out.extend(silent_packet(ch, frames, skip));
        }
    }
    out
}

pub fn expected_frames(packets_per_stream: usize, frames: u32) -> u32 {
    packets_per_stream as u32 * frames * FRAME
}

pub const DATA: &str = r"D:\xbox-pc-port\games\XDefiant\extracted\Mount\hunter\sdf\scarlett\data";

pub struct Riff {
    pub fmt: XMA2WAVEFORMATEX,
    pub data: Vec<u8>,
}

pub fn load(file: &str, off: u64, size: usize) -> Riff {
    let path = format!(r"{DATA}\{file}");
    let mut f = std::fs::File::open(&path).unwrap_or_else(|e| panic!("real XMA2 source {path}: {e}"));
    f.seek(SeekFrom::Start(off)).unwrap();
    let mut b = vec![0u8; size];
    f.read_exact(&mut b).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let mut q = 12;
    let mut fmt = None;
    let mut data = None;
    while q + 8 <= b.len() {
        let id = &b[q..q + 4];
        let len = u32::from_le_bytes(b[q + 4..q + 8].try_into().unwrap()) as usize;
        let body = &b[q + 8..(q + 8 + len).min(b.len())];
        if id == b"fmt " {
            assert!(body.len() >= 52);
            let mut x: XMA2WAVEFORMATEX = unsafe { core::ptr::read_unaligned(body.as_ptr() as *const XMA2WAVEFORMATEX) };
            assert_eq!({ x.wfx.n_block_align }, 2048, "XMA2 RIFF files store the packet size in nBlockAlign");
            x.wfx.n_block_align = x.wfx.n_channels * 2;
            x.wfx.cb_size = 34;
            fmt = Some(x);
        } else if id == b"data" {
            data = Some(body.to_vec());
        }
        q += 8 + len + (len & 1);
    }
    Riff { fmt: fmt.unwrap(), data: data.unwrap() }
}

pub const MONO: (&str, u64, usize) = ("sdf-A-0293.sdfdata", 0x1651e91, 16392);
pub const STEREO: (&str, u64, usize) = ("sdf-A-0288.sdfdata", 0x3959878, 45064);
pub const QUAD: (&str, u64, usize) = ("sdf-A-0287.sdfdata", 0xbc1b70, 1071112);
pub const SURROUND: (&str, u64, usize) = ("sdf-A-0299.sdfdata", 0xc58de8, 5627912);

