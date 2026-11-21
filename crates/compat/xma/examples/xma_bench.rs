use std::time::Instant;
use xma::{Xma2Stream, XMA_PACKET_BYTES};

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bench <dir with *_Nch_RATE.xma>");
    let reps: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let mut all_ns = 0u128;
    let mut all_audio = 0f64;
    let mut all_pk = 0usize;
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "xma")).collect();
    paths.sort();
    for p in paths {
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let ch = if name.contains("_2ch_") { 2 } else { 1 };
        let rate: u32 = name.rsplit('_').next().unwrap().parse().unwrap();
        let data = std::fs::read(&p).unwrap();
        let mut frames = 0usize;
        let mut best = u128::MAX;
        let mut checksum = 0i64;
        for _ in 0..reps {
            let mut s = Xma2Stream::new_hardware(rate, ch).unwrap();
            let mut out = Vec::with_capacity(1 << 20);
            let t = Instant::now();
            for pkt in data.chunks(XMA_PACKET_BYTES) {
                s.decode_packet(pkt, &mut out);
            }
            best = best.min(t.elapsed().as_nanos());
            frames = out.len() / ch;
            checksum = out.iter().map(|&v| v as i64).sum();
        }
        let pk = data.len().div_ceil(XMA_PACKET_BYTES);
        let audio = frames as f64 / rate as f64;
        println!(
            "{name}: {pk} packets, {:.2} s audio, {:.1} us/packet, {:.2} us per 512-sample frame, {:.0}x real time (checksum {checksum})",
            audio,
            best as f64 / pk as f64 / 1e3,
            best as f64 / (frames as f64 / 512.0) / 1e3,
            audio / (best as f64 / 1e9)
        );
        all_ns += best;
        all_audio += audio;
        all_pk += pk;
    }
    println!("total: {all_pk} packets, {:.2} s audio, {:.1} us/packet, {:.0}x real time per stream", all_audio, all_ns as f64 / all_pk as f64 / 1e3, all_audio / (all_ns as f64 / 1e9));
}
