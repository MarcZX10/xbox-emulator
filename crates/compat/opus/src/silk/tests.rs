use super::*;
use crate::entdec::Decoder;
use crate::ogg::parse_opus_file;

const REFS: &str = "D:\\xbox-pc-port\\emulator\\third_party\\opus-refs";

fn parse_frames(pkt: &[u8]) -> Option<(u8, Vec<&[u8]>)> {
    let toc = *pkt.first()?;
    let data = &pkt[1..];
    let code = toc & 3;
    let mut frames = Vec::new();
    let read_len = |d: &[u8]| -> Option<(usize, usize)> {
        let b0 = *d.first()? as usize;
        if b0 < 252 {
            Some((b0, 1))
        } else {
            let b1 = *d.get(1)? as usize;
            Some((b1 * 4 + b0, 2))
        }
    };
    match code {
        0 => frames.push(data),
        1 => {
            if data.len() % 2 != 0 {
                return None;
            }
            let h = data.len() / 2;
            frames.push(&data[..h]);
            frames.push(&data[h..]);
        }
        2 => {
            let (l, n) = read_len(data)?;
            let rest = &data[n..];
            if l > rest.len() {
                return None;
            }
            frames.push(&rest[..l]);
            frames.push(&rest[l..]);
        }
        _ => {
            let fc = *data.first()?;
            let vbr = fc & 0x80 != 0;
            let pad = fc & 0x40 != 0;
            let m = (fc & 0x3F) as usize;
            if m == 0 {
                return None;
            }
            let mut p = 1usize;
            let mut padding = 0usize;
            if pad {
                loop {
                    let b = *data.get(p)? as usize;
                    p += 1;
                    if b == 255 {
                        padding += 254;
                    } else {
                        padding += b;
                        break;
                    }
                }
            }
            let end = data.len().checked_sub(padding)?;
            if vbr {
                let mut lens = Vec::new();
                for _ in 0..m - 1 {
                    let (l, n) = read_len(data.get(p..)?)?;
                    p += n;
                    lens.push(l);
                }
                for l in lens {
                    if p + l > end {
                        return None;
                    }
                    frames.push(&data[p..p + l]);
                    p += l;
                }
                if p > end {
                    return None;
                }
                frames.push(&data[p..end]);
            } else {
                if end < p {
                    return None;
                }
                let total = end - p;
                if total % m != 0 {
                    return None;
                }
                let l = total / m;
                for i in 0..m {
                    frames.push(&data[p + i * l..p + (i + 1) * l]);
                }
            }
        }
    }
    Some((toc, frames))
}

fn frame_size_48k(toc: u8) -> usize {
    let config = (toc >> 3) as usize;
    if config < 12 {
        [480, 960, 1920, 2880][config & 3]
    } else if config < 16 {
        [480, 960][config & 1]
    } else {
        [120, 240, 480, 960][config & 3]
    }
}

struct StreamResult {
    channels: usize,
    pre_skip: usize,
    gain_q8: i16,
    pcm: Vec<f32>,
    excluded: Vec<(usize, usize)>,
    frame_starts: Vec<(usize, usize, usize)>,
    silk_frames: usize,
    other_frames: usize,
    redundant_frames: usize,
    errors: usize,
}

fn decode_stream(name: &str) -> Option<StreamResult> {
    decode_stream_ex(name, false)
}

fn decode_stream_ex(name: &str, hybrid_silk_layer: bool) -> Option<StreamResult> {
    let path = format!("{}\\{}.opus", REFS, name);
    let data = std::fs::read(&path).ok()?;
    let f = parse_opus_file(&data)?;
    let ch = f.channels;
    let mut silk = SilkDecoder::new();
    let mut ctl = DecControl {
        n_channels_api: ch as i32,
        n_channels_internal: 1,
        api_sample_rate: 48000,
        internal_sample_rate: 16000,
        payload_size_ms: 20,
        prev_pitch_lag: 0,
    };
    let mut res = StreamResult {
        channels: ch,
        pre_skip: f.pre_skip,
        gain_q8: f.gain_q8,
        pcm: Vec::new(),
        excluded: Vec::new(),
        frame_starts: Vec::new(),
        silk_frames: 0,
        other_frames: 0,
        redundant_frames: 0,
        errors: 0,
    };
    for (pi, pkt) in f.packets.iter().enumerate() {
        let (toc, frames) = match parse_frames(pkt) {
            Some(v) => v,
            None => {
                res.errors += 1;
                continue;
            }
        };
        let config = toc >> 3;
        let fs = frame_size_48k(toc);
        for (fi, fr) in frames.iter().enumerate() {
            let start = res.pcm.len() / ch;
            res.frame_starts.push((start, pi, fi));
            let mut buf = vec![0f32; fs * ch];
            let is_hybrid = (12..16).contains(&config);
            if config >= 16 || (is_hybrid && !hybrid_silk_layer) {
                res.other_frames += 1;
                res.excluded.push((start, fs));
                res.pcm.extend_from_slice(&buf);
                continue;
            }
            if is_hybrid {
                res.other_frames += 1;
            } else {
                res.silk_frames += 1;
            }
            ctl.n_channels_internal = if toc & 4 != 0 { 2 } else { 1 };
            ctl.internal_sample_rate = match config / 4 {
                0 => 8000,
                1 => 12000,
                _ => 16000,
            };
            ctl.payload_size_ms = (1000 * fs as i32 / 48000).max(10);
            let mut dec = Decoder::new(fr);
            let mut decoded = 0usize;
            while decoded < fs {
                let mut n = 0i32;
                let r = silk.decode(&mut ctl, 0, decoded == 0, &mut dec, &mut buf[decoded * ch..], &mut n);
                if r != 0 || n <= 0 {
                    res.errors += 1;
                    break;
                }
                decoded += n as usize;
            }
            if !is_hybrid && dec.tell() + 17 <= 8 * fr.len() as i32 {
                res.redundant_frames += 1;
                res.excluded.push((start, fs));
            }
            res.pcm.extend_from_slice(&buf);
        }
    }
    Some(res)
}

fn read_f32(name: &str) -> Option<Vec<f32>> {
    let path = format!("{}\\{}.f32", REFS, name);
    let b = std::fs::read(&path).ok()?;
    Some(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

fn compare(name: &str) {
    let r = match decode_stream(name) {
        Some(r) => r,
        None => {
            println!("{}: reference stream missing, skipped", name);
            return;
        }
    };
    let reference = read_f32(name).expect("reference f32 missing");
    let ch = r.channels;
    let ours = &r.pcm[(r.pre_skip * ch).min(r.pcm.len())..];
    let n = ours.len().min(reference.len());
    let mut excluded = vec![false; n / ch + 1];
    for &(s, l) in &r.excluded {
        for t in s..s + l {
            if t >= r.pre_skip {
                let k = t - r.pre_skip;
                if k < excluded.len() {
                    excluded[k] = true;
                }
            }
        }
    }
    let mut max_err = 0f64;
    let mut sig = 0f64;
    let mut noise = 0f64;
    let mut first_bad: Option<usize> = None;
    let mut mismatches = 0usize;
    let mut compared = 0usize;
    for i in 0..n {
        if excluded[i / ch] {
            continue;
        }
        compared += 1;
        let a = ours[i] as f64;
        let b = reference[i] as f64;
        let e = (a - b).abs();
        if e > 0.0 {
            mismatches += 1;
            if first_bad.is_none() {
                first_bad = Some(i);
            }
        }
        if e > max_err {
            max_err = e;
        }
        sig += b * b;
        noise += (a - b) * (a - b);
    }
    let snr = if noise == 0.0 { f64::INFINITY } else { 10.0 * (sig / noise).log10() };
    println!(
        "{}: channels={} pre_skip={} gain_q8={} silk_frames={} non_silk_frames={} celt_redundancy_frames={} decode_errors={} ours_len={} ref_len={} compared={} mismatching_samples={} max_abs_err={:.3e} snr_db={:.2}",
        name, ch, r.pre_skip, r.gain_q8, r.silk_frames, r.other_frames, r.redundant_frames, r.errors, ours.len(), reference.len(), compared, mismatches, max_err, snr
    );
    if let Some(i) = first_bad {
        let t = i / ch + r.pre_skip;
        let mut loc = (0, 0, 0);
        for &(s, pi, fi) in &r.frame_starts {
            if s <= t {
                loc = (s, pi, fi);
            }
        }
        println!("{}: first mismatch at sample {} (packet {}, frame {}, offset {}) ours={} ref={}", name, i, loc.1, loc.2, t - loc.0, ours[i], reference[i]);
    }
    assert_eq!(r.errors, 0, "decode errors");
    assert!(compared > 0);
    assert!(max_err < 1e-3, "max abs error too large: {}", max_err);
    assert!(snr > 70.0, "snr too low: {}", snr);
}

#[test]
fn silk_toc_survey() {
    for name in ["silk", "silkst", "hybrid"] {
        let path = format!("{}\\{}.opus", REFS, name);
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let f = match parse_opus_file(&data) {
            Some(f) => f,
            None => continue,
        };
        let mut counts = std::collections::BTreeMap::new();
        for p in &f.packets {
            if let Some(&t) = p.first() {
                *counts.entry((t >> 3, (t >> 2) & 1, t & 3)).or_insert(0usize) += 1;
            }
        }
        println!("{}: packets={} (config, stereo, code) -> count: {:?}", name, f.packets.len(), counts);
    }
}

#[test]
fn silk_mono_matches_libopus() {
    compare("silk");
}

#[test]
fn silk_stereo_matches_libopus() {
    let r = match decode_stream("silkst") {
        Some(r) => r,
        None => return,
    };
    if r.silk_frames == 0 {
        println!("silkst: contains no SILK-only frames ({} hybrid/CELT frames); exact comparison not possible, see silk_stereo_hybrid_lowband_sanity", r.other_frames);
        return;
    }
    compare("silkst");
}

fn lowpass(x: &[f32], ch: usize, c: usize) -> Vec<f64> {
    let taps = 255usize;
    let fc = 5000.0 / 48000.0;
    let mut h = vec![0f64; taps];
    let m = (taps - 1) as f64 / 2.0;
    for (i, hv) in h.iter_mut().enumerate() {
        let t = i as f64 - m;
        let s = if t == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * t).sin() / (std::f64::consts::PI * t) };
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (taps - 1) as f64).cos();
        *hv = s * w;
    }
    let n = x.len() / ch;
    let mut y = vec![0f64; n];
    for i in 0..n {
        let mut acc = 0f64;
        for (k, hv) in h.iter().enumerate() {
            if i >= k {
                acc += hv * x[(i - k) * ch + c] as f64;
            }
        }
        y[i] = acc;
    }
    y
}

#[test]
fn silk_stereo_hybrid_lowband_sanity() {
    let r = match decode_stream_ex("silkst", true) {
        Some(r) => r,
        None => return,
    };
    let reference = match read_f32("silkst") {
        Some(v) => v,
        None => return,
    };
    let ch = r.channels;
    let ours = &r.pcm[(r.pre_skip * ch).min(r.pcm.len())..];
    let n = (ours.len().min(reference.len()) / ch) * ch;
    let mut total_snr = Vec::new();
    for c in 0..ch {
        let a = lowpass(&ours[..n], ch, c);
        let b = lowpass(&reference[..n], ch, c);
        let skip = 2000usize.min(a.len());
        let mut sig = 0f64;
        let mut noise = 0f64;
        for i in skip..a.len() {
            sig += b[i] * b[i];
            noise += (a[i] - b[i]) * (a[i] - b[i]);
        }
        let snr = 10.0 * (sig / noise.max(1e-30)).log10();
        total_snr.push(snr);
    }
    println!("silkst hybrid SILK layer vs libopus full decode, <5 kHz band: decode_errors={} snr_db per channel = {:?}", r.errors, total_snr);
    assert_eq!(r.errors, 0);
    for s in total_snr {
        assert!(s > 15.0, "stereo SILK low band does not track reference: {}", s);
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

#[test]
fn silk_fuzz_stateful_no_panic() {
    let mut streams: Vec<(usize, Vec<Vec<u8>>)> = Vec::new();
    for name in ["silk", "silkst", "hybrid"] {
        if let Ok(d) = std::fs::read(format!("{}\\{}.opus", REFS, name)) {
            if let Some(f) = parse_opus_file(&d) {
                streams.push((f.channels, f.packets));
            }
        }
    }
    let mut rng = Rng(0xD1B54A32D192ED03);
    let mut out = vec![0f32; 2 * 2880 + 64];
    let mut frames_done = 0usize;
    for pass in 0..12 {
        for (channels, packets) in &streams {
            let mut silk = SilkDecoder::new();
            let mut ctl = DecControl::default();
            let api_ch = if pass % 3 == 2 { 3 - *channels as i32 } else { *channels as i32 };
            ctl.n_channels_api = api_ch;
            ctl.api_sample_rate = [48000, 24000, 16000, 12000, 8000, 48000][pass % 6];
            let mut hold_rate: Option<i32> = None;
            let mut hold_ch: Option<i32> = None;
            let mut hold_left = 0;
            for pkt in packets {
                let (toc, frames) = match parse_frames(pkt) {
                    Some(v) => v,
                    None => continue,
                };
                let config = toc >> 3;
                if config >= 16 {
                    continue;
                }
                let fs = frame_size_48k(toc);
                if hold_left == 0 && rng.below(12) == 0 {
                    hold_left = 1 + rng.below(6);
                    hold_rate = Some([8000, 12000, 16000][rng.below(3) as usize]);
                    hold_ch = Some(1 + rng.below(2) as i32);
                }
                for fr in frames {
                    let mut data = fr.to_vec();
                    if rng.below(10) == 0 && !data.is_empty() {
                        for _ in 0..1 + rng.below(4) {
                            let i = rng.below(data.len() as u64) as usize;
                            data[i] ^= 1 << rng.below(8);
                        }
                    }
                    let lost = match rng.below(8) {
                        0 | 1 => 1,
                        2 => 2,
                        _ => 0,
                    };
                    if lost != 1 {
                        ctl.n_channels_internal = hold_ch.filter(|_| hold_left > 0).unwrap_or(if toc & 4 != 0 { 2 } else { 1 });
                        ctl.internal_sample_rate = hold_rate.filter(|_| hold_left > 0).unwrap_or(if config >= 12 {
                            16000
                        } else {
                            [8000, 12000, 16000][(config / 4) as usize]
                        });
                    }
                    ctl.payload_size_ms = (1000 * fs as i32 / 48000).max(10);
                    let mut dec = Decoder::new(&data);
                    let mut decoded = 0usize;
                    let mut calls = 0;
                    while decoded < fs && calls < 8 {
                        let mut n = 0i32;
                        let off = (decoded * api_ch as usize).min(out.len());
                        let r = silk.decode(&mut ctl, lost, decoded == 0, &mut dec, &mut out[off..], &mut n);
                        calls += 1;
                        if r != 0 || n <= 0 {
                            break;
                        }
                        decoded += n as usize * 48000 / ctl.api_sample_rate as usize;
                    }
                    frames_done += 1;
                    for v in out.iter() {
                        assert!(v.is_finite());
                    }
                }
                if hold_left > 0 {
                    hold_left -= 1;
                }
            }
        }
    }
    println!("silk stateful fuzz: {} frames (loss/LBRR/bitflip/rate and channel switches) without panic", frames_done);
}

#[test]
fn silk_fuzz_no_panic() {
    let mut real: Vec<Vec<u8>> = Vec::new();
    for name in ["silk", "silkst"] {
        if let Ok(d) = std::fs::read(format!("{}\\{}.opus", REFS, name)) {
            if let Some(f) = parse_opus_file(&d) {
                for p in f.packets {
                    if p.len() > 1 {
                        real.push(p[1..].to_vec());
                    }
                }
            }
        }
    }
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let rates = [8000, 12000, 16000, 24000, 44100, 48000, 96000, 0, -5, 7999];
    let internal = [8000, 12000, 16000, 16000, 8000, 24000, 0, -1];
    let sizes = [10, 20, 40, 60, 0, 20, 10, 5, 80];
    let mut out = vec![0f32; 2 * 2880 + 64];
    let mut iterations = 0usize;
    for round in 0..40 {
        let mut silk = SilkDecoder::new();
        let mut ctl = DecControl::default();
        for _ in 0..150 {
            iterations += 1;
            let valid = rng.below(4) != 0;
            ctl.n_channels_api = if valid { 1 + rng.below(2) as i32 } else { rng.below(4) as i32 };
            ctl.n_channels_internal = if valid { 1 + rng.below(2) as i32 } else { rng.below(4) as i32 };
            ctl.api_sample_rate = if valid { [8000, 12000, 16000, 24000, 48000][rng.below(5) as usize] } else { rates[rng.below(rates.len() as u64) as usize] };
            ctl.internal_sample_rate = if valid { [8000, 12000, 16000][rng.below(3) as usize] } else { internal[rng.below(internal.len() as u64) as usize] };
            ctl.payload_size_ms = if valid { [10, 20, 40, 60][rng.below(4) as usize] } else { sizes[rng.below(sizes.len() as u64) as usize] };
            let mode = rng.below(6);
            let pkt: Vec<u8> = if mode < 2 && !real.is_empty() {
                let mut p = real[rng.below(real.len() as u64) as usize].clone();
                let flips = rng.below(8);
                for _ in 0..flips {
                    if !p.is_empty() {
                        let i = rng.below(p.len() as u64) as usize;
                        p[i] ^= 1 << rng.below(8);
                    }
                }
                if rng.below(3) == 0 {
                    let l = rng.below(p.len() as u64 + 1) as usize;
                    p.truncate(l);
                }
                p
            } else if mode == 2 {
                vec![0xFF; rng.below(200) as usize]
            } else if mode == 3 {
                vec![0x00; rng.below(200) as usize]
            } else {
                let l = rng.below(if round % 2 == 0 { 40 } else { 400 }) as usize;
                (0..l).map(|_| rng.next() as u8).collect()
            };
            let lost = match rng.below(10) {
                0 | 1 | 2 => 1,
                3 => 2,
                4 => rng.below(7) as i32 - 2,
                _ => 0,
            };
            let frame_samples = (ctl.payload_size_ms.max(10).min(60) as usize) * 48;
            let out_len = if rng.below(8) == 0 { rng.below(out.len() as u64) as usize } else { out.len() };
            let mut dec = Decoder::new(&pkt);
            let mut decoded = 0usize;
            let mut calls = 0;
            while decoded < frame_samples && calls < 8 {
                let mut n = 0i32;
                let off = (decoded * ctl.n_channels_api.max(1) as usize).min(out_len);
                let new_packet = if rng.below(10) == 0 { rng.below(2) == 0 } else { calls == 0 };
                let r = silk.decode(&mut ctl, lost, new_packet, &mut dec, &mut out[off..out_len], &mut n);
                calls += 1;
                if r != 0 || n <= 0 {
                    break;
                }
                decoded += n as usize;
            }
            if rng.below(50) == 0 {
                silk.reset();
            }
            for v in out.iter() {
                assert!(v.is_finite());
            }
        }
    }
    println!("silk fuzz: {} packets decoded without panic", iterations);
}
