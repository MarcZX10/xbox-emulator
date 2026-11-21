use audio_titles::xa::*;
use audio_titles::*;
use std::ffi::c_void;
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Default)]
struct Details {
    creation_flags: u32,
    active_flags: u32,
    channels: u32,
    rate: u32,
}

fn reverb_defaults() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(100f32.to_le_bytes());
    p.extend(5u32.to_le_bytes());
    p.extend([5u8, 5, 5, 6, 6, 27, 27, 8, 8, 8, 4, 8, 4]);
    for v in [5000f32, 0.0, 0.0, 0.0, 0.0, 1.0, 100.0, 100.0] {
        p.extend(v.to_le_bytes());
    }
    p.extend(0i32.to_le_bytes());
    p
}

fn wave(tag: u16, channels: u16) -> WaveFormatEx {
    let bits = if tag == 1 { 16 } else { 32 };
    let align = channels * bits / 8;
    WaveFormatEx { tag, channels, rate: 44100, bytes_per_sec: 44100 * align as u32, align, bits, cb: 0 }
}

#[test]
fn trackmania_xaudio2_engine_voice_pools_reverb_and_x3daudio() {
    let _g = serial();
    let x = xa();
    unsafe {
        let mut eng: *mut c_void = core::ptr::null_mut();
        assert_eq!((x.create)(&mut eng, 0, 0), S_OK, "XAudio2Create(&e, 0, 0) GbxGame 0x1415713c5 (SHAPE allowed, so the compat engine is wrapped)");
        let mut master: *mut c_void = core::ptr::null_mut();
        let cmv: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, *const u16, *const EffectChain, u32) -> i32 = slot(eng, 7);
        assert_eq!(cmv(eng, &mut master, 0, 0, 0, core::ptr::null(), core::ptr::null(), 6), S_OK, "CreateMasteringVoice(0, 0, 0, NULL, NULL, 6) 0x141571404");
        let set_volume: unsafe extern "system" fn(*mut c_void, f32, u32) -> i32 = slot(master, 12);
        assert_eq!(set_volume(master, 0.0, 0), S_OK);
        let mut d = Details::default();
        let details: unsafe extern "system" fn(*mut c_void, *mut Details) = slot(master, 0);
        details(master, &mut d);
        assert!(d.channels >= 1 && d.channels <= 8, "0x14157141f rejects more than 8 mastering channels, got {}", d.channels);
        assert!(d.rate >= 8000, "mastering rate {}", d.rate);
        let mut mask = 0u32;
        let get_mask: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 = slot(master, 19);
        assert_eq!(get_mask(master, &mut mask), S_OK, "IXAudio2MasteringVoice::GetChannelMask 0x141571437");
        assert_eq!(mask.count_ones(), d.channels, "channel mask {mask:#x} vs {} channels", d.channels);

        eprintln!("mastering {} ch {} Hz", d.channels, d.rate);
        let mut reverb: *mut c_void = core::ptr::null_mut();
        assert_eq!((x.reverb)(&mut reverb), S_OK, "XAudio2CreateReverb -> CreateAudioReverb (#2 via thunk 0x1417f7a6b)");
        let rd = [EffectDesc { effect: reverb, initial: 1, out_channels: 1 }];
        let chain = EffectChain { count: 1, descs: rd.as_ptr() };
        let mut submix: *mut c_void = core::ptr::null_mut();
        let csv: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, u32, *const Sends, *const EffectChain) -> i32 = slot(eng, 6);
        assert_eq!(csv(eng, &mut submix, 1, d.rate, 0, 0, core::ptr::null(), &chain), S_OK, "CreateSubmixVoice(1, master rate, 0, 0, NULL, reverb) 0x1415714b2");
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = slot(reverb, 2);
        rel(reverb);
        let params = reverb_defaults();
        assert_eq!(params.len(), 0x39);
        let set_fx: unsafe extern "system" fn(*mut c_void, u32, *const u8, u32, u32) -> i32 = slot(submix, 6);
        assert_eq!(set_fx(submix, 0, params.as_ptr(), 0x39, 0), S_OK, "SetEffectParameters(0, XAUDIO2FX_REVERB_PARAMETERS, 0x39) 0x14157154a");

        let mut inst = [0u8; 20];
        assert_eq!((x.x3d_init)(mask, 343.5, inst.as_mut_ptr()), S_OK, "X3DAudioInitialize(mask, 343.5) 0x141571584");
        let mut flags = if mask & 8 != 0 { 0x2003d } else { 0x3d };
        if d.channels == 2 {
            flags |= 2;
        }
        flags |= 0x40;
        let l = Listener { front: [0.0, 0.0, 1.0], top: [0.0, 1.0, 0.0], ..Default::default() };
        let e = Emitter { front: [0.0, 0.0, 1.0], top: [0.0, 1.0, 0.0], pos: [5.0, 0.0, 0.0], vel: [0.0, 0.0, 0.0], channels: 1, distance_scaler: 1.0, doppler_scaler: 1.0, ..Default::default() };
        let mut m = vec![0f32; d.channels as usize];
        let mut delays = vec![0f32; d.channels as usize];
        let mut dsp = Dsp { matrix: m.as_mut_ptr(), delays: delays.as_mut_ptr(), src: 1, dst: d.channels, lpf_direct: -1.0, lpf_reverb: -1.0, reverb: -1.0, doppler: -1.0, angle: -9.0, distance: 0.0, emitter_vel: 0.0, listener_vel: 0.0 };
        (x.x3d_calc)(inst.as_ptr(), &l, &e, flags, &mut dsp);
        let (dist, doppler, angle, lpf, rev) = (dsp.distance, dsp.doppler, dsp.angle, dsp.lpf_direct, dsp.reverb);
        eprintln!("mask {mask:#x} flags {flags:#x} matrix {m:?} delays {delays:?} distance {dist} doppler {doppler} angle {angle} lpf {lpf} reverb {rev}");
        assert!((dist - 5.0).abs() < 1e-4);
        assert!((doppler - 1.0).abs() < 1e-6, "X3DAUDIO_CALCULATE_DOPPLER with nothing moving gives 1.0, got {doppler}");
        assert!((angle - core::f32::consts::FRAC_PI_2).abs() < 1e-3, "X3DAUDIO_CALCULATE_EMITTER_ANGLE for an emitter on the right, got {angle}");
        assert!((0.0..=1.0).contains(&lpf) && (0.0..=1.0).contains(&rev));
        assert!(m.iter().sum::<f32>() > 0.0);
        let bits: Vec<u32> = (0..32).map(|b| 1u32 << b).filter(|b| mask & b != 0).collect();
        let side = |set: u32| bits.iter().zip(&m).filter(|(b, _)| **b & set != 0).map(|(_, v)| *v).sum::<f32>();
        let (left, right) = (side(0x1 | 0x10 | 0x40 | 0x200 | 0x1000 | 0x8000), side(0x2 | 0x20 | 0x80 | 0x400 | 0x4000 | 0x20000));
        assert!(right > 0.0 && left == 0.0, "emitter on the right: right speakers {right}, left speakers {left}");

        let csrc: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *const WaveFormatEx, u32, f32, *mut c_void, *const Sends, *const EffectChain) -> i32 = slot(eng, 5);
        let pools = [(1u16, 1u16, 35usize), (1, 2, 35), (3, 1, 12), (3, 2, 4)];
        let mut voices: Vec<Vec<*mut c_void>> = Vec::new();
        for (tag, ch, n) in pools {
            let fmt = wave(tag, ch);
            let mut v = Vec::new();
            for k in 0..n {
                let mut s: *mut c_void = core::ptr::null_mut();
                assert_eq!(csrc(eng, &mut s, &fmt, 0, 4.0, core::ptr::null_mut(), core::ptr::null(), core::ptr::null()), S_OK, "CreateSourceVoice tag {tag} {ch} ch 44100 Hz, max ratio 4.0, voice {k} of {n} (0x141571861)");
                v.push(s);
            }
            voices.push(v);
        }

        let mut bufs: Vec<Vec<u8>> = Vec::new();
        let mut plays = Vec::new();
        for (p, (tag, ch, _)) in pools.iter().enumerate() {
            for (j, ratio) in [(0usize, 1.0f32), (1, 2.0)] {
                let frames = (11025.0 * ratio) as u32;
                let fmt = wave(*tag, *ch);
                bufs.push(vec![0u8; frames as usize * fmt.align as usize]);
                let data = bufs.last().unwrap();
                let s = voices[p][j];
                let set_ratio: unsafe extern "system" fn(*mut c_void, f32, u32) -> i32 = slot(s, 26);
                assert_eq!(set_ratio(s, ratio, 0), S_OK);
                let buf = Buffer { flags: 0, bytes: data.len() as u32, data: data.as_ptr(), play_begin: 0, play_length: 0, loop_begin: 0, loop_length: 0, loop_count: 0, context: core::ptr::null_mut() };
                let submit: unsafe extern "system" fn(*mut c_void, *const Buffer, *const c_void) -> i32 = slot(s, 21);
                assert_eq!(submit(s, &buf, core::ptr::null()), S_OK);
                plays.push((s, *tag, *ch, ratio, frames));
            }
        }
        let t = Instant::now();
        for (s, ..) in &plays {
            let start: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 = slot(*s, 19);
            assert_eq!(start(*s, 0, 0), S_OK);
        }
        let mut done = vec![None; plays.len()];
        while done.iter().any(|d| d.is_none()) {
            assert!(t.elapsed() < Duration::from_secs(5), "voices never drained: {done:?}");
            for (k, (s, ..)) in plays.iter().enumerate() {
                if done[k].is_some() {
                    continue;
                }
                let mut st = VoiceState::default();
                let state: unsafe extern "system" fn(*mut c_void, *mut VoiceState, u32) = slot(*s, 25);
                state(*s, &mut st, 0);
                if st.queued == 0 {
                    let played = st.played;
                    done[k] = Some((t.elapsed(), played));
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        for (k, (_, tag, ch, ratio, frames)) in plays.iter().enumerate() {
            let (took, played) = done[k].unwrap();
            eprintln!("tag {tag} {ch} ch ratio {ratio}: {frames} frames in {took:?}, SamplesPlayed {played}");
            assert_eq!(played, *frames as u64, "tag {tag} {ch} ch ratio {ratio}: SamplesPlayed");
            assert!(took >= Duration::from_millis(220) && took <= Duration::from_millis(450), "tag {tag} {ch} ch ratio {ratio}: 0.25 s of output took {took:?}");
        }

        for v in voices.iter().flatten() {
            let destroy: unsafe extern "system" fn(*mut c_void) = slot(*v, 18);
            destroy(*v);
        }
        let destroy: unsafe extern "system" fn(*mut c_void) = slot(submix, 18);
        destroy(submix);
        let destroy: unsafe extern "system" fn(*mut c_void) = slot(master, 18);
        destroy(master);
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = slot(eng, 2);
        rel(eng);
    }
}
