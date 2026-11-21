use audio_titles::*;

const N: u32 = 512;
const SUBFRAMES: u32 = 8;
const RING_BLOCKS: u32 = 9;

fn clear(buf: u32) -> u64 {
    ((buf as u64 & 0x1fff) << 33) | 0x802001
}

fn src(s: u32, a: u32, b: u32) -> u64 {
    (((((b as u64 & 0x1fff) << 13) | (a as u64 & 0x1fff)) << 12 | (s as u64 & 0xfff)) << 13) | 2
}

fn dma(d: u32, buf: u32) -> u64 {
    ((((d as u64 & 0xfff) << 13) | (buf as u64 & 0x1fff)) << 14) | 0x10
}

struct Wwise {
    hal: Hal,
    ctx: ShapeContexts,
    words: ApuMem,
    tag_done: Vec<u64>,
}

impl Wwise {
    fn init() -> Wwise {
        heap(0x15E00000);
        let ctx = ShapeContexts::allocate([N, 0, 0, 2 * N, N, 0]);
        unsafe {
            core::ptr::write_bytes(ctx.va(SRC) as *mut u8, 0, (N << 5) as usize);
            core::ptr::write_bytes(ctx.va(DMA) as *mut u8, 0, (2 * N << 4) as usize);
            core::ptr::write_bytes(ctx.va(XMA) as *mut u8, 0, (N << 6) as usize);
        }
        let mut hal = Hal::create();
        hal.connect(0x4000, 0x4000);
        let words = ApuMem::new((N * 40 + 0xff) & !0xff, 0x100, 1);
        let mut w = Wwise { hal, ctx, words, tag_done: Vec::new() };
        assert_eq!(w.hal.submit(2, 0, u32::MAX, &0x72u32.to_le_bytes(), 0), S_OK, "0x14281b705: SubmitCommand(2, 0, -1, &0x72)");
        assert_eq!(w.hal.submit(8, 0x32, u32::MAX, &[0xffu8; 64], 0), S_OK, "0x14281b7a0: disable-all mask");
        w.pump();
        assert!(w.tag_done.contains(&0x32), "0x14281a220 waits for COMMAND_COMPLETED tag 0x32");
        w
    }

    fn pump(&mut self) -> Vec<[u8; 28]> {
        let mut v = Vec::new();
        while let Some(m) = self.hal.pop() {
            if msg_kind(&m) == 0x10 {
                self.tag_done.push(msg_id(&m));
            }
            v.push(m);
        }
        v
    }

    fn xma(&self, i: u32) -> usize {
        self.ctx.va(XMA) + i as usize * 0x40
    }

    fn src(&self, i: u32) -> usize {
        self.ctx.va(SRC) + i as usize * 0x20
    }

    fn dma(&self, d: u32) -> usize {
        self.ctx.va(DMA) + d as usize * 0x10
    }
}

fn rd(p: usize, off: usize) -> u32 {
    unsafe { core::ptr::read_volatile((p + off) as *const u32) }
}

fn wr(p: usize, off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((p + off) as *mut u32, v) }
}

fn set(p: usize, off: usize, shift: u32, width: u32, v: u32) {
    let m = ((1u64 << width) - 1) as u32;
    wr(p, off, (rd(p, off) & !(m << shift)) | ((v & m) << shift));
}

fn rate_code(rate: u32) -> u32 {
    match rate {
        24000 => 0,
        32000 => 1,
        44100 => 2,
        _ => 3,
    }
}

fn reference(x: &Xma2) -> Vec<i16> {
    let mut s = xma::Xma2Stream::new_hardware(48000, x.channels as usize).unwrap();
    let mut out = Vec::new();
    for p in x.data.chunks(2048) {
        s.decode_packet(p, &mut out);
    }
    out
}

struct V {
    x: Xma2,
    ch: u32,
    i: u32,
    xa: usize,
    da: usize,
    _bufs: [ApuMem; 3],
    ring_va: usize,
    rate: u32,
    ratio: f64,
    raw: u32,
    planes: Vec<Vec<f32>>,
    read: u32,
    starved: u32,
}

fn play(file: (&str, u64, usize), source_rate: Option<u32>) {
    play_many(&[(file, source_rate)]);
}

fn play_many(voices: &[((&str, u64, usize), Option<u32>)]) {
    let _g = serial();
    let mut w = Wwise::init();
    let mut vs: Vec<V> = Vec::new();
    let mut mask = [0u8; 64];
    for (k, (file, source_rate)) in voices.iter().enumerate() {
        let x = load_xma2(file.0, file.1, file.2);
        let ch = x.channels;
        assert!(ch <= 2);
        let i = 5 + 4 * k as u32;
        let (xa, sa) = (w.xma(i), w.src(i));
        let (da, db) = (w.dma(2 * i), w.dma(2 * i + 1));
        let out = ApuMem::new(0x2100, 0x100, 1);
        let data = ApuMem::new(x.data.len() as u32, 0x800, 1);
        data.bytes().copy_from_slice(&x.data);
        let ring = ApuMem::new(RING_BLOCKS * ch * 512, 0x200, 1);
        let rate = source_rate.unwrap_or(x.rate);
        let ratio = rate as f64 / 48000.0;
        let raw = (ratio * 262144.0) as u32;
        wr(xa, 0x1c, out.apu);
        wr(xa, 0x20, out.apu + 0x1f00);
        set(xa, 0, 22, 5, 31);
        set(sa, 4, 0, 21, raw);
        set(sa, 0x10, 0, 21, raw);
        wr(xa, 0x14, data.apu);
        wr(xa, 0x18, data.apu);
        set(xa, 0, 0, 12, x.data.len() as u32 >> 11);
        set(xa, 0, 12, 8, 0);
        set(xa, 0, 20, 1, 1);
        set(xa, 0, 21, 1, 0);
        set(xa, 4, 0, 12, 0);
        set(xa, 4, 20, 4, 1);
        set(xa, 4, 24, 3, 0);
        set(xa, 4, 27, 2, rate_code(x.rate));
        set(xa, 4, 29, 1, ch - 1);
        set(xa, 4, 31, 1, 1);
        set(xa, 8, 0, 26, 32);
        set(xa, 0x24, 0, 5, 0);
        set(sa, 4, 22, 1, 1);
        set(sa, 4, 23, 1, 0);
        for (slot, d) in [da, db].into_iter().enumerate().take(ch as usize) {
            wr(d, 4, 0);
            wr(d, 8, ring.apu & !0x1ff);
            wr(d, 0xc, (RING_BLOCKS - 1) | (1 << 8) | ((ch - 1) << 16) | ((slot as u32) << 24));
        }
        mask[(i / 8) as usize] |= 1 << (i % 8);
        let ring_va = ring.va;
        vs.push(V { planes: vec![Vec::new(); ch as usize], x, ch, i, xa, da, _bufs: [out, data, ring], ring_va, rate, ratio, raw, read: 0, starved: 0 });
    }
    assert_eq!(w.hal.submit(6, 0x33, u32::MAX, &mask, 0), S_OK, "0x14281c41a: enable mask");
    w.pump();
    let mut frames = 0;
    while vs.iter().any(|v| v.starved < 2) {
        frames += 1;
        assert!(frames < 20000);
        let mut starved = vec![false; vs.len()];
        for j in 0..SUBFRAMES {
            let mut words: Vec<u64> = Vec::new();
            for (k, v) in vs.iter().enumerate() {
                let b = (k as u32 * SUBFRAMES + j) * 2;
                if v.ch == 1 {
                    words.extend([clear(b), src(v.i, b, b), dma(2 * v.i, b)]);
                } else {
                    words.extend([clear(b), clear(b + 1), src(v.i, b, b + 1), dma(2 * v.i, b), dma(2 * v.i + 1, b + 1)]);
                }
            }
            for (k, c) in words.iter().enumerate() {
                unsafe { core::ptr::write_unaligned((w.words.va + k * 8) as *mut u64, *c) };
            }
            let mut pl = [0u8; 16];
            pl[0..4].copy_from_slice(&(words.len() as u32).to_le_bytes());
            pl[4..8].copy_from_slice(&w.words.apu.to_le_bytes());
            assert_eq!(w.hal.submit(1, 0x34, u32::MAX, &pl, 0), S_OK, "0x14281c5bc: flowgraph");
            let msgs = w.pump();
            assert!(msgs.iter().any(|m| msg_kind(m) == 2), "FLOWGRAPH_COMPLETED for each submitted group");
            for m in msgs.iter().filter(|m| msg_kind(m) == 4) {
                let s = u32::from_le_bytes(m[12..16].try_into().unwrap());
                let k = vs.iter().position(|v| v.i == s).expect("SRC_BLOCKED names a playing SRC context");
                assert_eq!(rd(vs[k].xa, 0) & (3 << 20), 0, "SRC {s} blocked while its XMA input buffer is still valid: decode fell behind");
                starved[k] = true;
            }
            assert!(!msgs.iter().any(|m| msg_kind(m) == 8), "DMA_BLOCKED: the CPU consumes every block it is given");
        }
        for v in vs.iter_mut() {
            loop {
                let d1 = rd(v.da, 4);
                let wpos = (d1 >> 8) & 31;
                let full = (d1 >> 16) & 1 != 0;
                assert_eq!(d1 & 31, v.read, "DMA read index follows the type 0x10 commands");
                if wpos == v.read && !full {
                    break;
                }
                let blk = v.ring_va + (v.read * v.ch * 128 * 4) as usize;
                for c in 0..v.ch as usize {
                    let plane = unsafe { core::slice::from_raw_parts((blk + c * 512) as *const f32, 128) };
                    v.planes[c].extend_from_slice(plane);
                }
                v.read = (v.read + 1) % RING_BLOCKS;
                let mut p = [0u8; 8];
                p[0..4].copy_from_slice(&(2 * v.i).to_le_bytes());
                p[4..8].copy_from_slice(&1u32.to_le_bytes());
                assert_eq!(w.hal.submit(0x10, 0x21, u32::MAX, &p, 0), S_OK, "0x14281ded5: DMA A consumed");
                if v.ch == 2 {
                    p[0..4].copy_from_slice(&(2 * v.i + 1).to_le_bytes());
                    assert_eq!(w.hal.submit(0x10, 0x21, u32::MAX, &p, 0), S_OK, "0x14281deff: DMA B consumed");
                }
                w.pump();
            }
        }
        for (k, s) in starved.into_iter().enumerate() {
            if s {
                vs[k].starved += 1;
            }
        }
    }
    for v in &vs {
        let (x, ch, ratio, raw, rate) = (&v.x, v.ch, v.ratio, v.raw, v.rate);
        let r = reference(x);
        let n = r.len() / ch as usize;
        assert_eq!(n, x.samples_encoded as usize);
        for c in 0..ch as usize {
            let p = &v.planes[c];
            let ref_last = (0..n).rev().find(|&k| r[k * ch as usize + c] != 0).unwrap_or(0) + 1;
            let last = p.iter().rposition(|s| *s != 0.0).unwrap_or(0) + 1;
            let want_last = (ref_last as f64 / ratio) as usize;
            eprintln!("voice {} ch{c}: last audible output frame {}, input {} frames ({} audible) at {} Hz, ratio {:.5}, expected ~{}; {} frames delivered", v.i, last, n, ref_last, rate, ratio, want_last, p.len());
            assert!((last as i64 - want_last as i64).abs() <= 2, "voice {} ch{c}: audio ends at output frame {}, expected {} for {} input frames at {} Hz", v.i, last, want_last, ref_last, rate);
            assert!(p.len() >= (n as f64 / ratio) as usize - 1, "voice {} ch{c}: only {} frames delivered for {} input frames", v.i, p.len(), n);
            let e: f64 = p.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / last as f64;
            assert!(e.sqrt() > 0.002, "voice {} ch{c} silent", v.i);
            if raw == 262144 {
                for k in 0..n - 1 {
                    let want = r[k * ch as usize + c] as f32 / 32768.0;
                    assert!((p[k] - want).abs() < 1e-6, "voice {} ch{c} frame {k}: {} vs {}", v.i, p[k], want);
                }
            }
        }
    }
}

#[test]
fn xdefiant_mono_voice_through_src_and_dma() {
    play(MONO, None);
}

#[test]
fn xdefiant_stereo_voice_through_src_and_dma() {
    play(STEREO, None);
}

#[test]
fn xdefiant_src_ratio_0_91875_stretches_duration() {
    play(STEREO, Some(44100));
}

#[test]
fn xdefiant_src_ratio_0_66667_stretches_duration() {
    play(MONO, Some(32000));
}

#[test]
fn xdefiant_four_voices_mixed_in_one_flowgraph_decode_in_parallel() {
    play_many(&[(STEREO, None), (MONO, None), (STEREO, Some(44100)), (MONO, Some(32000))]);
}
#[test]
fn xdefiant_src_ratio_1_5_pitches_up_and_shortens_duration() {
    play(STEREO, Some(72000));
}