use super::*;
use crate::backend::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct MockDec;
impl OpusDecoderCodec for MockDec {
    fn decode(&mut self, packet: &[u8], pcm: &mut [i16]) -> Result<usize, i32> {
        if packet.is_empty() {
            let n = 480.min(pcm.len());
            pcm[..n].fill(0);
            return Ok(n);
        }
        let n = (packet.len() * 4).min(pcm.len());
        for i in 0..n {
            pcm[i] = (packet[i / 4] as i16) << 4;
        }
        Ok(n)
    }
}

struct MockEnc;
impl OpusEncoderCodec for MockEnc {
    fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize, i32> {
        let n = (pcm.len() / 4).min(out.len());
        for i in 0..n {
            out[i] = (pcm[i * 4] >> 4) as u8;
        }
        Ok(n)
    }
}

struct MockSilkDec;
impl SilkDecoderCodec for MockSilkDec {
    fn decode(&mut self, control: &mut [i32; 6], all_frames: bool, packet: &[u8], pcm: &mut [i16]) -> Result<usize, i32> {
        let per = if all_frames { 2 } else { 1 };
        let n = (packet.len() * per).min(pcm.len());
        for i in 0..n {
            pcm[i] = packet[i / per] as i16;
        }
        control[1] = n as i32;
        Ok(n)
    }
}

struct MockSilkEnc;
impl SilkEncoderCodec for MockSilkEnc {
    fn encode(&mut self, control: &mut [i32; 8], pcm: &[i16], out: &mut [u8]) -> Result<usize, i32> {
        let n = (pcm.len() / 2).min(out.len());
        for i in 0..n {
            out[i] = pcm[i * 2] as u8;
        }
        control[7] = 77;
        Ok(n)
    }
}

struct Mock {
    opus: bool,
    silk: bool,
    fail_init: AtomicBool,
}

impl Mock {
    fn new() -> Arc<Mock> {
        Arc::new(Mock { opus: true, silk: true, fail_init: AtomicBool::new(false) })
    }
}

const INIT_ERR: i32 = 0x8007_0001u32 as i32;

impl CodecProvider for Mock {
    fn opus_available(&self) -> bool {
        self.opus
    }
    fn silk_available(&self) -> bool {
        self.silk
    }
    fn opus_decoder(&self, _rate: u32, _channels: u32) -> Result<Box<dyn OpusDecoderCodec>, i32> {
        if self.fail_init.load(Ordering::SeqCst) { Err(INIT_ERR) } else { Ok(Box::new(MockDec)) }
    }
    fn opus_encoder(&self, _rate: u32, _channels: u32, _bitrate: u32, _flags: u32) -> Result<Box<dyn OpusEncoderCodec>, i32> {
        if self.fail_init.load(Ordering::SeqCst) { Err(INIT_ERR) } else { Ok(Box::new(MockEnc)) }
    }
    fn silk_decoder(&self) -> Result<Box<dyn SilkDecoderCodec>, i32> {
        Ok(Box::new(MockSilkDec))
    }
    fn silk_encoder(&self) -> Result<Box<dyn SilkEncoderCodec>, i32> {
        Ok(Box::new(MockSilkEnc))
    }
}

#[test]
fn opus_decoder_limits_and_slots() {
    let d = OpusDecoderInterface::new(Mock::new());
    let mut n = 0;
    assert_eq!(d.get_maximum_count(Some(&mut n)), 0);
    assert_eq!(n, 15);
    assert_eq!(d.get_maximum_count(None), E_POINTER);
    let mut idx = 0;
    assert_eq!(d.acquire(44100, 1, Some(&mut idx)), E_INVALIDARG);
    assert_eq!(d.acquire(24000, 0, Some(&mut idx)), E_INVALIDARG);
    assert_eq!(d.acquire(24000, 3, Some(&mut idx)), E_INVALIDARG);
    for want in 0..15u32 {
        assert_eq!(d.acquire([16000, 24000, 48000][want as usize % 3], 1 + want % 2, Some(&mut idx)), 0);
        assert_eq!(idx, want);
    }
    assert_eq!(d.acquire(24000, 1, Some(&mut idx)), opus::NO_FREE_SLOT);
    assert_eq!(d.release(15), opus::BAD_INDEX);
    assert_eq!(d.release(4), 0);
    assert_eq!(d.release(4), opus::BAD_INDEX);
    assert_eq!(d.acquire(24000, 1, Some(&mut idx)), 0);
    assert_eq!(idx, 4);
}

#[test]
fn opus_decoder_state_machine() {
    let d = OpusDecoderInterface::new(Mock::new());
    let mut idx = 0;
    assert_eq!(d.acquire(24000, 1, Some(&mut idx)), 0);
    let mut fl = 9u8;
    assert_eq!(d.is_data_in_flight(idx, Some(&mut fl)), 0);
    assert_eq!(fl, 0);
    assert_eq!(d.is_data_in_flight(idx, None), E_POINTER);
    assert_eq!(d.is_data_in_flight(3, Some(&mut fl)), opus::BAD_INDEX);
    let mut buf = vec![0u8; 0x5a00];
    let mut size = buf.len() as u16;
    assert_eq!(d.get_decode_output(idx, false, None, Some(&mut buf), Some(&mut size)), opus::NO_OUTPUT);
    let big = vec![0u8; 0x5e1];
    assert_eq!(d.decode_packet(idx, Some(&big), 0x5e1), opus::INPUT_TOO_LARGE);
    let pkt = [1u8, 2, 3];
    assert_eq!(d.decode_packet(idx, Some(&pkt), 3), 0);
    assert_eq!(d.is_data_in_flight(idx, Some(&mut fl)), 0);
    assert_eq!(fl, 1);
    assert_eq!(d.decode_packet(idx, Some(&pkt), 3), opus::BUSY);
    let mut small = [0u8; 4];
    let mut ssize = 4u16;
    assert_eq!(d.get_decode_output(idx, true, None, Some(&mut small), Some(&mut ssize)), opus::BUFFER_TOO_SMALL);
    assert_eq!(ssize, 24);
    assert_eq!(d.is_data_in_flight(idx, Some(&mut fl)), 0);
    assert_eq!(fl, 0);
    assert_eq!(d.decode_packet(idx, Some(&pkt), 3), 0);
    let mut ticks = 0xFFFF_FFFFu32;
    assert_eq!(d.get_decode_output(idx, false, Some(&mut ticks), Some(&mut buf), Some(&mut size)), 0);
    assert_ne!(ticks, 0xFFFF_FFFF);
    assert_eq!(size, 24);
    let s0 = i16::from_le_bytes([buf[0], buf[1]]);
    let s11 = i16::from_le_bytes([buf[22], buf[23]]);
    assert_eq!(s0, 16);
    assert_eq!(s11, 48);
    size = buf.len() as u16;
    assert_eq!(d.get_decode_output(idx, false, None, Some(&mut buf), Some(&mut size)), opus::NO_OUTPUT);
    assert_eq!(d.decode_packet(idx, None, 0), 0);
    assert_eq!(d.get_decode_output(idx, false, None, Some(&mut buf), Some(&mut size)), 0);
    assert_eq!(size, 960);
    assert_eq!(d.get_decode_output(idx, false, None, None, Some(&mut size)), E_POINTER);
    assert_eq!(d.decode_packet(idx, None, 5), E_POINTER);
}

#[test]
fn opus_init_failure_reports_on_output() {
    let m = Mock::new();
    m.fail_init.store(true, Ordering::SeqCst);
    let d = OpusDecoderInterface::new(m.clone());
    let mut idx = 0;
    assert_eq!(d.acquire(48000, 2, Some(&mut idx)), 0);
    let mut fl = 1u8;
    assert_eq!(d.is_data_in_flight(idx, Some(&mut fl)), 0);
    assert_eq!(fl, 0);
    assert_eq!(d.decode_packet(idx, Some(&[9u8]), 1), 0);
    assert_eq!(d.is_data_in_flight(idx, Some(&mut fl)), 0);
    assert_eq!(fl, 1);
    let mut buf = vec![0u8; 64];
    let mut size = 64u16;
    assert_eq!(d.get_decode_output(idx, false, None, Some(&mut buf), Some(&mut size)), INIT_ERR);
    assert_eq!(d.decode_packet(idx, Some(&[9u8]), 1), 0);
    assert_eq!(d.get_decode_output(idx, false, None, Some(&mut buf), Some(&mut size)), opus::HW_FAILED);
    assert_eq!(d.release(idx), 0);
}

#[test]
fn opus_encoder_and_roundtrip() {
    let p = Mock::new();
    let e = OpusEncoderInterface::new(p.clone());
    let d = OpusDecoderInterface::new(p);
    let mut n = 0;
    assert_eq!(e.get_maximum_count(Some(&mut n)), 0);
    assert_eq!(n, 3);
    let mut idx = 0;
    assert_eq!(e.acquire(24000, 1, 0, 0, Some(&mut idx)), E_INVALIDARG);
    assert_eq!(e.acquire(24000, 1, 510_001, 0, Some(&mut idx)), E_INVALIDARG);
    assert_eq!(e.acquire(24000, 1, 24000, 2, Some(&mut idx)), E_INVALIDARG);
    assert_eq!(e.acquire(8000, 1, 24000, 0, Some(&mut idx)), E_INVALIDARG);
    for want in 0..3 {
        assert_eq!(e.acquire(24000, 1, 510_000, 1, Some(&mut idx)), 0);
        assert_eq!(idx, want);
    }
    assert_eq!(e.acquire(24000, 1, 24000, 0, Some(&mut idx)), opus::NO_FREE_SLOT);
    assert_eq!(e.release(3), opus::BAD_INDEX);
    let pcm: Vec<i16> = (0..480).map(|i| ((i / 4) as i16 % 200) << 4).collect();
    assert_eq!(e.encode_frame(0, None, 960), E_POINTER);
    assert_eq!(e.encode_frame(5, Some(&pcm), 960), opus::BAD_INDEX);
    let huge = vec![0i16; 0x1681];
    assert_eq!(e.encode_frame(0, Some(&huge), 0x2d01), opus::INPUT_TOO_LARGE);
    assert_eq!(e.encode_frame(0, Some(&pcm), 960), 0);
    assert_eq!(e.encode_frame(0, Some(&pcm), 960), opus::BUSY);
    let mut fl = 0u8;
    assert_eq!(e.is_data_in_flight(0, Some(&mut fl)), 0);
    assert_eq!(fl, 1);
    let mut out = vec![0u8; 0x2e8];
    let mut size = 10u16;
    assert_eq!(e.get_encode_output(0, true, None, Some(&mut out), Some(&mut size)), opus::BUFFER_TOO_SMALL);
    assert_eq!(size, 120);
    assert_eq!(e.encode_frame(0, Some(&pcm), 960), 0);
    size = out.len() as u16;
    let mut ticks = 0;
    assert_eq!(e.get_encode_output(0, false, Some(&mut ticks), Some(&mut out), Some(&mut size)), 0);
    assert_eq!(size, 120);
    let mut di = 0;
    assert_eq!(d.acquire(24000, 1, Some(&mut di)), 0);
    assert_eq!(d.decode_packet(di, Some(&out[..size as usize]), size), 0);
    let mut pcm_out = vec![0u8; 0x5a00];
    let mut psize = pcm_out.len() as u16;
    assert_eq!(d.get_decode_output(di, false, None, Some(&mut pcm_out), Some(&mut psize)), 0);
    assert_eq!(psize as usize, pcm.len() * 2);
    let back: Vec<i16> = pcm_out[..psize as usize].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    assert_eq!(back, pcm);
}

#[test]
fn silk_slots_and_sizes() {
    let s = SilkInterface::new(Mock::new());
    let (mut a, mut b) = (0, 0);
    assert_eq!(s.get_maximum_decoder_count(Some(&mut a)), 0);
    assert_eq!(s.get_maximum_encoder_count(Some(&mut b)), 0);
    assert_eq!((a, b), (7, 2));
    assert_eq!(s.get_maximum_decoder_buffer_sizes(Some(&mut a), Some(&mut b)), 0);
    assert_eq!((a, b), (0xf00, 0x2580));
    assert_eq!(s.get_maximum_encoder_buffer_sizes(Some(&mut a), Some(&mut b)), 0);
    assert_eq!((a, b), (0x2580, 0x500));
    assert_eq!(s.get_maximum_encoder_buffer_sizes(Some(&mut a), None), E_POINTER);
    let mut idx = 0;
    for want in 0..7 {
        assert_eq!(s.acquire_decoder(Some(&mut idx)), 0);
        assert_eq!(idx, want);
    }
    assert_eq!(s.acquire_decoder(Some(&mut idx)), silk::NO_FREE_SLOT);
    for want in 0..2 {
        assert_eq!(s.acquire_encoder(Some(&mut idx)), 0);
        assert_eq!(idx, want);
    }
    assert_eq!(s.acquire_encoder(Some(&mut idx)), silk::NO_FREE_SLOT);
    assert_eq!(s.acquire_encoder(None), E_POINTER);
    assert_eq!(s.release_decoder(7), silk::BAD_INDEX);
    assert_eq!(s.release_encoder(2), silk::BAD_INDEX);
    assert_eq!(s.release_decoder(3), 0);
    assert_eq!(s.release_decoder(3), silk::BAD_INDEX);
}

#[test]
fn silk_sync_then_async_rules() {
    let s = SilkInterface::new(Mock::new());
    let mut d = 0;
    assert_eq!(s.acquire_decoder(Some(&mut d)), 0);
    let mut ctl = [24000, 0, 0, 0, 0, 0];
    let mut pcm = vec![0i16; 4800];
    let mut n: i16 = 4800;
    let pkt = [5u8, 6, 7];
    assert_eq!(s.decode_frame(d, Some(&pkt), Some(&mut pcm), Some(&mut n), Some(&mut ctl), true, true), 0);
    assert_eq!(n, 6);
    assert_eq!(&pcm[..6], &[5, 5, 6, 6, 7, 7]);
    assert_eq!(ctl[1], 6);
    let mut tiny = vec![0i16; 2];
    let mut tn: i16 = 2;
    assert_eq!(s.decode_frame(d, Some(&pkt), Some(&mut tiny), Some(&mut tn), Some(&mut ctl), true, false), silk::BUFFER_TOO_SMALL);
    assert_eq!(s.decode_frame(d, Some(&pkt), None, None, Some(&mut ctl), true, false), E_POINTER);
    let big = vec![0u8; 0xf01];
    assert_eq!(s.decode_frame(d, Some(&big), Some(&mut pcm), Some(&mut n), Some(&mut ctl), true, false), silk::INPUT_TOO_LARGE);
    n = 4800;
    assert_eq!(s.get_decode_output(d, Some(&mut pcm), Some(&mut n), Some(&mut ctl)), silk::NO_OUTPUT);
    let mut ctl2 = [24000, 0, 0, 0, 0, 0];
    assert_eq!(s.decode_frame(d, Some(&pkt), None, None, Some(&mut ctl2), false, false), 0);
    assert_eq!(ctl2[1], 0);
    let mut fl = 0;
    assert_eq!(s.is_decoder_data_in_flight(d, Some(&mut fl)), 0);
    assert_eq!(fl, 1);
    assert_eq!(s.decode_frame(d, Some(&pkt), None, None, Some(&mut ctl2), false, false), silk::BUSY);
    assert_eq!(s.release_decoder(d), silk::BUSY);
    let mut small_n: i16 = 1;
    let mut small = vec![0i16; 1];
    assert_eq!(s.get_decode_output(d, Some(&mut small), Some(&mut small_n), Some(&mut ctl2)), silk::BUFFER_TOO_SMALL);
    assert_eq!(s.is_decoder_data_in_flight(d, Some(&mut fl)), 0);
    assert_eq!(fl, 0);
    assert_eq!(s.decode_frame(d, Some(&pkt), None, None, Some(&mut ctl2), false, false), 0);
    n = 4800;
    assert_eq!(s.get_decode_output(d, Some(&mut pcm), Some(&mut n), Some(&mut ctl2)), 0);
    assert_eq!(n, 3);
    assert_eq!(ctl2[1], 3);
    assert_eq!(s.decode_frame(d, Some(&pkt), Some(&mut pcm), Some(&mut n), Some(&mut ctl), true, true), silk::SYNC_AFTER_ASYNC);
    assert_eq!(s.release_decoder(d), 0);
}

#[test]
fn silk_encoder_paths() {
    let s = SilkInterface::new(Mock::new());
    let mut e = 0;
    assert_eq!(s.acquire_encoder(Some(&mut e)), 0);
    let pcm: Vec<i16> = (0..320).collect();
    let mut ctl = [24000, 24000, 480, 16000, 0, 2, 0, 0];
    let mut out = vec![0u8; 0x500];
    let mut nb: i16 = 0x500;
    assert_eq!(s.encode_frame(e, Some(&pcm), 320, Some(&mut out), Some(&mut nb), Some(&mut ctl), true), 0);
    assert_eq!(nb, 160);
    assert_eq!(out[1], 2);
    assert_eq!(ctl[7], 77);
    let big = vec![0i16; 0x12c1];
    assert_eq!(s.encode_frame(e, Some(&big), 0x12c1, Some(&mut out), Some(&mut nb), Some(&mut ctl), true), silk::INPUT_TOO_LARGE);
    assert_eq!(s.encode_frame(e, Some(&pcm), -1, Some(&mut out), Some(&mut nb), Some(&mut ctl), true), silk::INPUT_TOO_LARGE);
    let mut nb2: i16 = 10;
    assert_eq!(s.encode_frame(e, Some(&pcm), 320, Some(&mut out), Some(&mut nb2), Some(&mut ctl), true), silk::BUFFER_TOO_SMALL);
    let mut ctl2 = [24000, 24000, 480, 16000, 0, 2, 0, 0];
    assert_eq!(s.encode_frame(e, Some(&pcm), 320, None, None, Some(&mut ctl2), false), 0);
    assert_eq!(s.encode_frame(e, Some(&pcm), 320, None, None, Some(&mut ctl2), false), silk::BUSY);
    let mut fl = 0;
    assert_eq!(s.is_encoder_data_in_flight(e, Some(&mut fl)), 0);
    assert_eq!(fl, 1);
    nb = 0x4ff;
    assert_eq!(s.get_encode_output(e, Some(&mut out), Some(&mut nb), Some(&mut ctl2)), 0);
    assert_eq!(nb, 160);
    assert_eq!(ctl2[7], 77);
    assert_eq!(s.get_encode_output(e, Some(&mut out), Some(&mut nb), Some(&mut ctl2)), silk::NO_OUTPUT);
    assert_eq!(s.encode_frame(e, Some(&pcm), 320, Some(&mut out), Some(&mut nb), Some(&mut ctl), true), silk::SYNC_AFTER_ASYNC);
    assert_eq!(s.release_encoder(e), 0);
}

static EXPORT_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn exports_end_to_end() {
    let _g = EXPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_codec_provider(Some(Mock::new()));
    unsafe {
        let mut dec: *mut c_void = core::ptr::null_mut();
        let mut enc: *mut c_void = core::ptr::null_mut();
        let mut silk: *mut c_void = core::ptr::null_mut();
        assert_eq!(CreateOpusDecoderHardwareInterface(&mut dec), 0);
        assert_eq!(CreateOpusEncoderHardwareInterface(&mut enc), 0);
        assert_eq!(CreateSilkHardwareInterface(&mut silk), COORDINATION_BUSY);
        assert!(silk.is_null());
        let mut n = 0u32;
        assert_eq!(GetMaximumDecoderCount(dec, &mut n), 0);
        assert_eq!(n, 15);
        assert_eq!(GetMaximumEncoderCount(dec, &mut n), 0);
        assert_eq!(n, 15);
        assert_eq!(GetMaximumDecoderCount(enc, &mut n), 0);
        assert_eq!(n, 3);
        let mut ei = 0u32;
        assert_eq!(AcquireEncoder(enc, 24000, 1, 24000, 0, &mut ei), 0);
        let pcm: Vec<i16> = (0..480).map(|i| ((i / 4) as i16 % 100) << 4).collect();
        assert_eq!(EncodeFrame(enc, ei, pcm.as_ptr(), 960), 0);
        let mut fl = 0u8;
        assert_eq!(GetIsDataInFlightForEncoder(enc, ei, &mut fl), 0);
        assert_eq!(fl, 1);
        let mut pkt = [0u8; 0x2e8];
        let mut psize = pkt.len() as u16;
        let mut ticks = 0u32;
        assert_eq!(GetEncodeOutput(enc, ei, 1, &mut ticks, pkt.as_mut_ptr(), &mut psize), 0);
        assert_eq!(psize, 120);
        let mut di = 0u32;
        assert_eq!(AcquireDecoder(dec, 24000, 1, &mut di), 0);
        assert_eq!(AcquireDecoder(enc, 24000, 1, &mut di), E_INVALIDARG);
        assert_eq!(DecodePacket(dec, di, pkt.as_ptr(), psize), 0);
        assert_eq!(GetIsDataInFlightForDecoder(dec, di, &mut fl), 0);
        assert_eq!(fl, 1);
        let mut out = vec![0u8; 0x5a00];
        let mut osize = out.len() as u16;
        assert_eq!(GetDecodeOutput(dec, di, 0, core::ptr::null_mut(), out.as_mut_ptr(), &mut osize), 0);
        assert_eq!(osize, 960);
        assert_eq!(ReleaseEncoder(dec, di), 0);
        assert_eq!(ReleaseDecoder(enc, ei), 0);
        assert_eq!(ReleaseDecoder(dec, di), opus::BAD_INDEX);
        assert_eq!(SilkAcquireDecoder(dec, &mut di), E_INVALIDARG);
        assert_eq!(live_interfaces(), 2);
        DestroyOpusEncoderHardwareInterface(enc);
        assert_eq!(live_interfaces(), 1);
        assert_eq!(GetMaximumEncoderCount(enc, &mut n), E_POINTER);
        assert_eq!(CreateSilkHardwareInterface(&mut silk), 0);
        let (mut a, mut b) = (0u32, 0u32);
        assert_eq!(SilkGetMaximumDecoderBufferSizes(silk, &mut a, &mut b), 0);
        assert_eq!((a, b), (0xf00, 0x2580));
        assert_eq!(SilkGetMaximumEncoderBufferSizes(silk, &mut a, &mut b), 0);
        assert_eq!((a, b), (0x2580, 0x500));
        assert_eq!(SilkGetMaximumDecoderCount(silk, &mut a), 0);
        assert_eq!(SilkGetMaximumEncoderCount(silk, &mut b), 0);
        assert_eq!((a, b), (7, 2));
        let mut se = 0u32;
        let mut sd = 0u32;
        assert_eq!(SilkAcquireEncoder(silk, &mut se), 0);
        assert_eq!(SilkAcquireDecoder(silk, &mut sd), 0);
        let mut ectl = [24000i32, 24000, 480, 16000, 0, 2, 0, 0];
        let spcm: Vec<i16> = (0..480).collect();
        assert_eq!(SilkEncodeFrame(silk, se, spcm.as_ptr(), 480, core::ptr::null_mut(), core::ptr::null_mut(), &mut ectl, 0), 0);
        assert_eq!(SilkGetIsEncoderDataInFlight(silk, se, &mut fl), 0);
        assert_eq!(fl, 1);
        let mut sbuf = [0u8; 0x4ff];
        let mut sb: i16 = 0x4ff;
        assert_eq!(SilkGetEncodeOutput(silk, se, sbuf.as_mut_ptr(), &mut sb, &mut ectl), 0);
        assert_eq!(sb, 240);
        let mut dctl = [24000i32, 0, 0, 0, 0, 0];
        assert_eq!(SilkDecodeFrame(silk, sd, sbuf.as_ptr(), sb as u32, core::ptr::null_mut(), core::ptr::null_mut(), &mut dctl, 0, 1), 0);
        assert_eq!(SilkGetIsDecoderDataInFlight(silk, sd, &mut fl), 0);
        assert_eq!(fl, 1);
        let mut dpcm = vec![0i16; 0x4000];
        let mut dn: i16 = 0x8000u16 as i16;
        assert_eq!(SilkGetDecodeOutput(silk, sd, dpcm.as_mut_ptr(), &mut dn, &mut dctl), 0);
        assert_eq!(dn, 480);
        assert_eq!(SilkDecodeFrame(silk, sd, sbuf.as_ptr(), sb as u32, core::ptr::null_mut(), core::ptr::null_mut(), &mut dctl, 0, 1), 0);
        dn = 4800 * 2;
        assert_eq!(SilkGetDecodeOutput(silk, sd, dpcm.as_mut_ptr(), &mut dn, &mut dctl), 0);
        assert_eq!(dn, 480);
        assert_eq!(SilkDecodeFrame(silk, sd, sbuf.as_ptr(), sb as u32, core::ptr::null_mut(), core::ptr::null_mut(), &mut dctl, 0, 1), 0);
        dn = 479 * 2;
        assert_eq!(SilkGetDecodeOutput(silk, sd, dpcm.as_mut_ptr(), &mut dn, &mut dctl), silk::BUFFER_TOO_SMALL);
        assert_eq!(SilkDecodeFrame(silk, sd, sbuf.as_ptr(), 4, dpcm.as_mut_ptr(), &mut dn, &mut dctl, 1, 0), silk::SYNC_AFTER_ASYNC);
        assert_eq!(SilkReleaseDecoder(silk, sd), 0);
        assert_eq!(SilkReleaseEncoder(silk, se), 0);
        assert_eq!(AcquireDecoder(silk, 24000, 1, &mut di), E_INVALIDARG);
        DestroySilkHardwareInterface(silk);
        DestroyOpusDecoderHardwareInterface(dec);
        DestroyOpusDecoderHardwareInterface(dec);
        assert_eq!(live_interfaces(), 0);
        assert_eq!(CreateOpusDecoderHardwareInterface(core::ptr::null_mut()), E_POINTER);
        set_codec_provider(Some(Arc::new(Mock { opus: false, silk: false, fail_init: AtomicBool::new(false) })));
        assert_eq!(CreateOpusDecoderHardwareInterface(&mut dec), E_MOD_NOT_FOUND);
        assert_eq!(CreateOpusEncoderHardwareInterface(&mut enc), E_MOD_NOT_FOUND);
        assert_eq!(CreateSilkHardwareInterface(&mut silk), E_MOD_NOT_FOUND);
        assert_eq!(live_interfaces(), 0);
    }
    set_codec_provider(None);
}

#[test]
fn error_code_remaps() {
    assert_eq!(map_opus(opus::HW_FAILED), 0x8923_4313u32 as i32);
    assert_eq!(map_silk(silk::HW_FAILED), 0x8923_431Au32 as i32);
    assert_eq!(map_opus(opus::BUSY), opus::BUSY);
    assert_eq!(map_silk(silk::BUSY), silk::BUSY);
}

#[test]
fn opus_frame_sizes() {
    assert!(opus_frame_ok(48000, 960));
    assert!(opus_frame_ok(24000, 120));
    assert!(opus_frame_ok(16000, 1920));
    assert!(!opus_frame_ok(24000, 500));
    assert_eq!(opus_padded_frame(24000, 500), Some(960));
    assert_eq!(opus_padded_frame(24000, 1500), Some(1920));
    assert_eq!(opus_padded_frame(48000, 5761), None);
    assert_eq!(opus_padded_frame(16000, 0), Some(40));
}

#[test]
fn dynamic_provider_loads_without_panicking() {
    let p = DynProvider::load();
    if !p.opus_available() {
        assert_eq!(p.opus_decoder(24000, 1).err(), Some(E_MOD_NOT_FOUND));
    }
    if !p.silk_available() {
        assert_eq!(p.silk_encoder().err(), Some(E_MOD_NOT_FOUND));
    }
}

#[test]
fn silk_capacities_follow_gamechat2_word_units() {
    let gc2 = 0x8000u16 as i16;
    assert_eq!(unsafe { crate::silk_decode_capacity(&gc2) }, 0x4000);
    let samples_bytes: i16 = 9600;
    assert_eq!(unsafe { crate::silk_decode_capacity(&samples_bytes) }, 4800);
    let enc = 0x7fffi16;
    assert_eq!(unsafe { crate::silk_encode_capacity(&enc) }, 0x7fff);
    let full = 0xffffu16 as i16;
    assert_eq!(unsafe { crate::silk_encode_capacity(&full) }, 0xffff);
    assert_eq!(unsafe { crate::silk_decode_capacity(core::ptr::null()) }, 0);
}
