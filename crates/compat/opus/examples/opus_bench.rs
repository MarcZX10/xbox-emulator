use opusdec::OpusDecoder;
use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bench <opus-refs dir>");
    let reps: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    for name in ["celt", "celtmono", "silk", "silkst", "hybrid"] {
        let Ok(file) = std::fs::read(std::path::Path::new(&dir).join(format!("{name}.opus"))) else { continue };
        let f = opusdec::ogg::parse_opus_file(&file).unwrap();
        let ch = f.channels;
        let mut pcm = vec![0f32; 5760 * ch];
        let mut best = u128::MAX;
        let mut frames = 0usize;
        let mut checksum = 0f64;
        for _ in 0..reps {
            let mut d = OpusDecoder::new(48000, ch).unwrap();
            frames = 0;
            checksum = 0.0;
            let t = Instant::now();
            for p in &f.packets {
                let n = d.decode_float(p, &mut pcm, 5760, false);
                if n > 0 {
                    frames += n as usize;
                    checksum += pcm[0] as f64;
                }
            }
            best = best.min(t.elapsed().as_nanos());
        }
        let audio = frames as f64 / 48000.0;
        println!(
            "{name}: {} packets, {:.2} s audio, {:.1} us/packet, {:.0}x real time (checksum {checksum:.6})",
            f.packets.len(),
            audio,
            best as f64 / f.packets.len() as f64 / 1e3,
            audio / (best as f64 / 1e9)
        );
    }
}
