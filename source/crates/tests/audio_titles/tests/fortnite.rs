use audio_titles::*;

const CTX_OUT: u32 = 0x1f00;
const CTX_OVERLAP: u32 = 0x200;
const HALF: usize = 0x800;
const RING: u32 = 31;

struct Fortnite {
    hal: Hal,
    ctx: ShapeContexts,
    notify: ApuMem,
    overlap: ApuMem,
    out: ApuMem,
}

impl Fortnite {
    fn init() -> Fortnite {
        heap(0x4000000);
        let ctx = ShapeContexts::allocate([0, 0, 0, 0, 0x200, 0]);
        let notify = ApuMem::new(0x800, 0x100, 2);
        let overlap = ApuMem::new(0x40000, 0x80, 1);
        let out = ApuMem::new(0x3e0000, 0x40, 1);
        let mut hal = Hal::create();
        hal.connect(0x1000, 0x1000);
        assert_eq!(hal.submit(2, 0, u32::MAX, &200u32.to_le_bytes(), 0), S_OK, "Initialize 0x1406df0b1: SubmitCommand(2, 0, -1, &200)");
        Fortnite { hal, ctx, notify, overlap, out }
    }
}

struct Voice<'a> {
    fx: &'a Fortnite,
    i: u32,
    ctx: usize,
    input: ApuMem,
    channels: u32,
    src: &'a [u8],
    fed: usize,
    eof: bool,
    started: bool,
    submits: u32,
}

impl<'a> Voice<'a> {
    fn rd(&self, off: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.ctx + off) as *const u32) }
    }

    fn wr(&self, off: usize, v: u32) {
        unsafe { core::ptr::write_volatile((self.ctx + off) as *mut u32, v) }
    }

    fn set(&self, off: usize, shift: u32, width: u32, v: u32) {
        let m = ((1u64 << width) - 1) as u32;
        self.wr(off, (self.rd(off) & !(m << shift)) | ((v & m) << shift));
    }

    fn acquire(fx: &'a Fortnite, i: u32, channels: u32, src: &'a [u8]) -> Voice<'a> {
        let ctx = fx.ctx.va(XMA) + i as usize * 0x40;
        unsafe { core::ptr::write_bytes(ctx as *mut u8, 0, 0x40) };
        fx.out.bytes()[(i * CTX_OUT) as usize..((i + 1) * CTX_OUT) as usize].fill(0);
        fx.overlap.bytes()[(i * CTX_OVERLAP) as usize..((i + 1) * CTX_OVERLAP) as usize].fill(0);
        fx.notify.wr(i as usize * 4, 0);
        let input = ApuMem::new(0x1000, 0x800, 1);
        let v = Voice { fx, i, ctx, input, channels, src, fed: 0, eof: false, started: false, submits: 0 };
        v.wr(0x1c, fx.out.apu + i * CTX_OUT);
        v.wr(0x20, fx.overlap.apu + i * CTX_OVERLAP);
        v.set(0, 22, 5, RING);
        v.set(4, 31, 1, 1);
        v.set(0, 20, 1, 1);
        v.set(0, 21, 1, 0);
        v.set(4, 31, 1, 1);
        v.set(4, 29, 1, channels - 1);
        v.wr(4, v.rd(4) | 0x1800_0000);
        v.wr(0x14, v.input.apu);
        v.wr(0x18, v.input.apu);
        v.set(0, 0, 12, (HALF >> 11) as u32);
        v.set(4, 0, 12, 0);
        v.set(0x10, 31, 1, 0);
        v.set(4, 24, 3, 0);
        v.set(0x24, 0, 5, 0);
        v.wr(0x18, v.input.apu + HALF as u32);
        v.set(4, 0, 12, (HALF >> 11) as u32);
        v
    }

    fn feed(&mut self, half: usize, bytes: usize) {
        let n = bytes.min(self.src.len() - self.fed);
        let dst = &mut self.input.bytes()[half * HALF..half * HALF + bytes];
        dst.fill(0);
        dst[..n].copy_from_slice(&self.src[self.fed..self.fed + n]);
        self.fed += n;
        if self.fed >= self.src.len() {
            self.eof = true;
        }
    }

    fn wait(&self, what: &str) {
        let t = std::time::Instant::now();
        while self.fx.notify.rd(self.i as usize * 4) == 0 {
            assert!(t.elapsed().as_millis() < 1000, "{what}: notify fence not written within Fortnite's 1000 ms timeout (0x1406f335c)");
            std::thread::yield_now();
        }
    }

    fn decode_next_subframes(&mut self, req: usize, pcm: &mut Vec<i16>) -> usize {
        let bpsf = self.channels as usize * 256;
        let n = (req / bpsf).min(8);
        assert!(n > 0);
        let n = 1u32 << (31 - (n as u32).leading_zeros());
        self.set(4, 20, 4, n);
        let id = ((self.i as u64) << 8) | 0xff;
        let idx = self.i.to_le_bytes();
        let fence = self.fx.notify.apu + self.i * 4;
        let hr = self.fx.hal.submit(5, id, u32::MAX, &idx, fence);
        assert_eq!(hr, S_OK, "EnableOrDisableXmaContext(enable) 0x1406f35ea: SubmitCommand(5) after {} submits", self.submits);
        self.submits += 1;
        self.wait("enable");
        self.fx.notify.wr(self.i as usize * 4, 0);
        let hr = self.fx.hal.submit(7, id, u32::MAX, &idx, fence);
        assert_eq!(hr, S_OK, "EnableOrDisableXmaContext(disable) 0x1406f340c: SubmitCommand(7) after {} submits", self.submits);
        self.submits += 1;
        self.wait("disable");
        let r = self.rd(0x24) & 31;
        let w = self.rd(0) >> 27;
        let avail = if r == w {
            if self.rd(4) >> 31 != 0 {
                0
            } else {
                RING
            }
        } else {
            (w + RING - r) % RING
        };
        let bytes = (avail as usize * 256).min(req);
        let base = self.fx.out.va + (self.i * CTX_OUT) as usize;
        for k in 0..bytes / 2 {
            let off = (r as usize * 256 + k * 2) % CTX_OUT as usize;
            pcm.push(unsafe { core::ptr::read_unaligned((base + off) as *const i16) });
        }
        self.set(0x24, 0, 5, w);
        bytes
    }

    fn decode(&mut self, req: usize, pcm: &mut Vec<i16>) -> usize {
        let mut left = req;
        if !self.started {
            self.started = true;
            let first = (2 * HALF).min(self.src.len());
            self.feed(0, HALF);
            if first > HALF {
                self.feed(1, HALF);
            }
            self.set(0, 20, 1, 1);
            self.set(0, 21, 1, (first != HALF) as u32);
            self.set(0x10, 31, 1, 0);
            self.set(8, 0, 26, 32);
            self.set(0x24, 0, 5, 0);
            self.wr(0, self.rd(0) & 0x07ff_ffff);
        }
        let mut iters = 1000;
        while left > 0 {
            let cur = self.rd(0x10) >> 31;
            if (self.rd(0) >> (20 + cur)) & 1 == 0 {
                break;
            }
            left -= self.decode_next_subframes(left, pcm);
            iters -= 1;
            assert!(iters > 0, "XMA2 streaming error: 1000 iterations without filling the request (0x1406f2e40)");
        }
        let valid = (self.rd(0) >> 20) & 3;
        if valid != 3 && !self.eof {
            let cur = self.rd(0x10) >> 31;
            let n = if valid & (1 << cur) != 0 { cur ^ 1 } else { cur };
            self.feed(n as usize, HALF);
            self.set(0, 20 + n, 1, 1);
        }
        req - left
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

fn run_voice(fx: &Fortnite, i: u32, x: &Xma2, req_frames: usize) -> (Vec<i16>, usize) {
    let mut v = Voice::acquire(fx, i, x.channels, &x.data);
    let req = req_frames * x.channels as usize * 2;
    let mut pcm = Vec::new();
    let mut calls = 0;
    loop {
        let got = v.decode(req, &mut pcm);
        calls += 1;
        assert!(calls < 100_000);
        if got == 0 && v.eof && (v.rd(0) >> 20) & 3 == 0 {
            break;
        }
    }
    (pcm, calls)
}

fn replay(file: (&str, u64, usize), req_frames: usize) {
    let _g = serial();
    let x = load_xma2(file.0, file.1, file.2);
    let fx = Fortnite::init();
    let (pcm, calls) = run_voice(&fx, 7, &x, req_frames);
    check(file, &x, &pcm, req_frames, calls);
}

fn check(file: (&str, u64, usize), x: &Xma2, pcm: &[i16], req_frames: usize, calls: usize) {
    let r = reference(x);
    let ch = x.channels as usize;
    assert_eq!(r.len() / ch, x.samples_encoded as usize, "{}: hardware-mode decode of every packet vs SamplesEncoded", file.0);
    if let Some(k) = pcm.iter().zip(r.iter()).position(|(a, b)| a != b) {
        let probe = &pcm[k..(k + 16).min(pcm.len())];
        let found = r.windows(probe.len()).position(|w| w == probe);
        panic!("{}: PCM diverges at frame {} (requests of {} frames), output continues at reference frame {:?}: audio skipped or repeated", file.0, k / ch, req_frames, found.map(|f| f / ch));
    }
    let out = pcm.len() / ch;
    eprintln!("{}: {} of {} frames delivered, {} play length, {} Decode() calls", file.0, out, r.len() / ch, x.play_length, calls);
    assert!(out <= r.len() / ch);
    assert!(r.len() / ch - out <= 512, "{}: {} frames out of {}: the stream stopped {} frames early (the console stops at most one XMA frame early, when the last frame's bits are consumed)", file.0, out, r.len() / ch, r.len() / ch - out);
    for c in 0..ch {
        assert!(rms(&pcm, ch, c) > 50.0, "{}: channel {c} silent", file.0);
    }
}

#[test]
fn fortnite_mono_stream_duration_matches_input() {
    replay(MONO, 1024);
}

#[test]
fn fortnite_stereo_stream_duration_matches_input() {
    replay(STEREO, 1024);
}

#[test]
fn fortnite_stereo_non_power_of_two_requests() {
    replay(STEREO, 384);
}

#[test]
fn fortnite_mono_256_frame_requests() {
    replay(MONO, 256);
}

#[test]
fn fortnite_pool_threads_decode_voices_concurrently() {
    let _g = serial();
    let files = [MONO, STEREO, STEREO, MONO, STEREO, MONO];
    let xs: Vec<Xma2> = files.iter().map(|f| load_xma2(f.0, f.1, f.2)).collect();
    let fx = Fortnite::init();
    let t = std::time::Instant::now();
    let outs: Vec<(Vec<i16>, usize)> = std::thread::scope(|s| {
        let hs: Vec<_> = xs.iter().enumerate().map(|(k, x)| {
            let fx = &fx;
            s.spawn(move || run_voice(fx, 3 + k as u32 * 5, x, if k % 2 == 0 { 1024 } else { 384 }))
        }).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    eprintln!("{} voices on {} threads: {:?}", xs.len(), xs.len(), t.elapsed());
    for (k, (pcm, calls)) in outs.iter().enumerate() {
        check(files[k], &xs[k], pcm, if k % 2 == 0 { 1024 } else { 384 }, *calls);
    }
}

#[test]
fn fortnite_survives_more_decodes_than_the_message_ring() {
    let _g = serial();
    let x = load_xma2(STEREO.0, STEREO.1, STEREO.2);
    let fx = Fortnite::init();
    let mut pcm = Vec::new();
    let mut submits = 0;
    for round in 0..120 {
        let mut v = Voice::acquire(&fx, round % 4, x.channels, &x.data);
        let mut calls = 0;
        while !(v.decode(0x400, &mut pcm) == 0 && v.eof && (v.rd(0) >> 20) & 3 == 0) {
            calls += 1;
            assert!(calls < 100_000);
        }
        submits += v.submits;
    }
    assert!(submits > 0x1000 + 0x100, "only {submits} submits, the message ring is 0x1000");
}
