use super::CeltDecoder;
use crate::entdec::Decoder;
use crate::ogg::parse_opus_file;

const REF_DIR: &str = "D:\\xbox-pc-port\\emulator\\third_party\\opus-refs";

fn parse_frames(pkt: &[u8]) -> Option<Vec<&[u8]>> {
    if pkt.is_empty() {
        return None;
    }
    let toc = pkt[0];
    let code = toc & 3;
    let body = &pkt[1..];
    match code {
        0 => Some(vec![body]),
        1 => {
            if body.len() % 2 != 0 {
                return None;
            }
            let h = body.len() / 2;
            Some(vec![&body[..h], &body[h..]])
        }
        2 => {
            let (l, used) = read_len(body)?;
            if used + l > body.len() {
                return None;
            }
            Some(vec![&body[used..used + l], &body[used + l..]])
        }
        _ => {
            if body.is_empty() {
                return None;
            }
            let ch = body[0];
            let vbr = ch & 0x80 != 0;
            let padding = ch & 0x40 != 0;
            let count = (ch & 0x3f) as usize;
            if count == 0 {
                return None;
            }
            let mut pos = 1usize;
            let mut pad = 0usize;
            if padding {
                loop {
                    let p = *body.get(pos)? as usize;
                    pos += 1;
                    if p == 255 {
                        pad += 254;
                    } else {
                        pad += p;
                        break;
                    }
                }
            }
            if pos + pad > body.len() {
                return None;
            }
            let end = body.len() - pad;
            let mut frames = Vec::new();
            if vbr {
                let mut lens = Vec::new();
                for _ in 0..count - 1 {
                    let (l, used) = read_len(&body[pos..end])?;
                    pos += used;
                    lens.push(l);
                }
                for l in lens {
                    if pos + l > end {
                        return None;
                    }
                    frames.push(&body[pos..pos + l]);
                    pos += l;
                }
                frames.push(&body[pos..end]);
            } else {
                let total = end - pos;
                if total % count != 0 {
                    return None;
                }
                let l = total / count;
                for i in 0..count {
                    frames.push(&body[pos + i * l..pos + (i + 1) * l]);
                }
            }
            Some(frames)
        }
    }
}

fn read_len(b: &[u8]) -> Option<(usize, usize)> {
    let b0 = *b.first()? as usize;
    if b0 < 252 {
        Some((b0, 1))
    } else {
        let b1 = *b.get(1)? as usize;
        Some((b1 * 4 + b0, 2))
    }
}

fn end_band_for(config: u8) -> i32 {
    match config {
        16..=19 => 13,
        20..=23 => 17,
        24..=27 => 19,
        _ => 21,
    }
}

fn decode_stream(name: &str) -> Option<(Vec<f32>, usize)> {
    let path = format!("{}\\{}", REF_DIR, name);
    let bytes = std::fs::read(&path).ok()?;
    let f = parse_opus_file(&bytes).expect("parse ogg");
    let ch = f.channels;
    let mut dec = CeltDecoder::new(48000, ch).expect("new");
    dec.set_signalling(false);
    let mut out = Vec::new();
    let mut pcm = vec![0.0f32; 960 * 2];
    for pkt in &f.packets {
        let toc = pkt[0];
        let config = toc >> 3;
        assert!(config >= 16, "non-CELT packet");
        let fsz = 120usize << (config & 3);
        let stereo = toc & 4 != 0;
        let frames = parse_frames(pkt).expect("frames");
        dec.set_start_band(0);
        dec.set_end_band(end_band_for(config));
        dec.set_stream_channels(if stereo { 2 } else { 1 });
        for fr in frames {
            let ret = if fr.len() <= 1 {
                dec.decode_with_ec(None, &mut pcm, fsz, None, false)
            } else {
                let mut ec = Decoder::new(fr);
                dec.decode_with_ec(Some(fr), &mut pcm, fsz, Some(&mut ec), false)
            };
            assert_eq!(ret, fsz as i32);
            out.extend_from_slice(&pcm[..fsz * ch]);
        }
    }
    let skip = (f.pre_skip * ch).min(out.len());
    Some((out[skip..].to_vec(), ch))
}

fn compare(name: &str, refname: &str) {
    let Some((got, ch)) = decode_stream(name) else {
        println!("{}: reference missing, skipped", name);
        return;
    };
    let rb = std::fs::read(format!("{}\\{}", REF_DIR, refname)).expect("ref");
    let refv: Vec<f32> = rb.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let n = got.len().min(refv.len());
    let mut max_err = 0.0f64;
    let mut sig = 0.0f64;
    let mut noise = 0.0f64;
    let mut first_bad: Option<usize> = None;
    for i in 0..n {
        let e = (got[i] as f64 - refv[i] as f64).abs();
        if e > max_err {
            max_err = e;
        }
        if e > 1e-3 && first_bad.is_none() {
            first_bad = Some(i / ch);
        }
        sig += (refv[i] as f64) * (refv[i] as f64);
        noise += e * e;
    }
    let snr = 10.0 * (sig / noise.max(1e-30)).log10();
    println!(
        "{}: samples got={} ref={} compared={} max_abs_err={:.3e} snr={:.2} dB first_bad_sample={:?}",
        name,
        got.len(),
        refv.len(),
        n,
        max_err,
        snr,
        first_bad
    );
    assert!(n > 0);
    assert!(max_err < 1e-3, "max err {}", max_err);
    assert!(snr > 70.0, "snr {}", snr);
}

#[test]
fn celt_ref_stereo() {
    compare("celt.opus", "celt.f32");
}

#[test]
fn celt_ref_mono() {
    compare("celtmono.opus", "celtmono.f32");
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

#[test]
fn celt_fuzz_no_panic() {
    let mut seeds: Vec<Vec<u8>> = Vec::new();
    for name in ["celt.opus", "celtmono.opus"] {
        if let Ok(b) = std::fs::read(format!("{}\\{}", REF_DIR, name)) {
            if let Some(f) = parse_opus_file(&b) {
                for p in f.packets.iter().take(200) {
                    if let Some(fr) = parse_frames(p) {
                        for x in fr {
                            seeds.push(x.to_vec());
                        }
                    }
                }
            }
        }
    }
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let rates = [48000, 24000, 16000, 12000, 8000];
    let mut pcm = vec![0.0f32; 960 * 2];
    let mut total = 0usize;
    for round in 0..12 {
        let ch = 1 + (round % 2);
        let rate = rates[(round / 2) % rates.len()];
        let ds = 48000 / rate as usize;
        let mut dec = CeltDecoder::new(rate, ch).unwrap();
        for it in 0..1500 {
            let lm = rng.below(4) as usize;
            let fsz = (120usize << lm) / ds;
            let mut pkt: Vec<u8> = match rng.below(4) {
                0 => {
                    let l = rng.below(1300) as usize;
                    (0..l).map(|_| rng.next() as u8).collect()
                }
                1 | 2 if !seeds.is_empty() => {
                    let s = seeds[rng.below(seeds.len() as u32) as usize].clone();
                    let mut s = s;
                    let flips = rng.below(8);
                    for _ in 0..flips {
                        if !s.is_empty() {
                            let i = rng.below(s.len() as u32) as usize;
                            s[i] ^= 1 << rng.below(8);
                        }
                    }
                    if rng.below(3) == 0 && !s.is_empty() {
                        let l = rng.below(s.len() as u32) as usize;
                        s.truncate(l);
                    }
                    s
                }
                _ => {
                    let l = rng.below(8) as usize;
                    (0..l).map(|_| if rng.below(2) == 0 { 0xff } else { 0 }).collect()
                }
            };
            if rng.below(50) == 0 {
                pkt.clear();
            }
            match rng.below(20) {
                0 => dec.set_start_band(rng.below(21) as i32),
                1 => dec.set_start_band(0),
                2 => dec.set_end_band(1 + rng.below(21) as i32),
                3 => dec.set_stream_channels(1 + rng.below(2) as usize),
                4 => dec.reset(),
                _ => {}
            }
            if it % 97 == 0 {
                dec.set_start_band(0);
                dec.set_end_band(21);
            }
            let accum = rng.below(5) == 0;
            let ret = match rng.below(4) {
                0 => {
                    let mut ec = Decoder::new(&pkt);
                    dec.decode_with_ec(Some(&pkt), &mut pcm, fsz, Some(&mut ec), accum)
                }
                3 => {
                    let mut ec = Decoder::new(&pkt);
                    let cut = rng.below(pkt.len() as u32 + 1) as usize;
                    dec.decode_with_ec(Some(&pkt[..cut]), &mut pcm, fsz, Some(&mut ec), accum)
                }
                1 => dec.decode_with_ec(None, &mut pcm, fsz, None, accum),
                _ => dec.decode_with_ec(Some(&pkt), &mut pcm, fsz, None, accum),
            };
            assert!(ret == fsz as i32 || ret < 0, "ret {}", ret);
            let bad = dec.decode_with_ec(Some(&pkt), &mut pcm, fsz + 1, None, false);
            assert!(bad < 0);
            let bad2 = dec.decode_with_ec(Some(&pkt), &mut pcm[..1], fsz, None, false);
            assert!(bad2 < 0);
            for v in pcm.iter_mut() {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
            total += 1;
        }
    }
    println!("fuzz: {} packets decoded without panic", total);
}

#[test]
fn celt_plc_returns_frame_size() {
    for ch in 1..=2usize {
        let mut dec = CeltDecoder::new(48000, ch).unwrap();
        let mut pcm = vec![0.0f32; 960 * 2];
        for fsz in [120usize, 240, 480, 960] {
            assert_eq!(dec.decode_with_ec(None, &mut pcm, fsz, None, false), fsz as i32);
        }
        if let Ok(b) = std::fs::read(format!("{}\\{}", REF_DIR, if ch == 2 { "celt.opus" } else { "celtmono.opus" })) {
            let f = parse_opus_file(&b).unwrap();
            let mut fsz_last = 960;
            for p in f.packets.iter().take(30) {
                let config = p[0] >> 3;
                let fsz = 120usize << (config & 3);
                fsz_last = fsz;
                dec.set_end_band(end_band_for(config));
                dec.set_stream_channels(if p[0] & 4 != 0 { 2 } else { 1 });
                for fr in parse_frames(p).unwrap() {
                    dec.decode_with_ec(Some(fr), &mut pcm, fsz, None, false);
                }
            }
            for _ in 0..6 {
                assert_eq!(dec.decode_with_ec(None, &mut pcm, fsz_last, None, false), fsz_last as i32);
                assert!(pcm[..fsz_last * ch].iter().all(|v| v.is_finite()));
            }
        }
    }
}
