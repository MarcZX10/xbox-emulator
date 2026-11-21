use audio_titles::xa::*;
use audio_titles::*;
use std::ffi::c_void;
use std::time::{Duration, Instant};

#[test]
fn fortnite_xaudio2_engine_plays_at_the_mixer_rate_through_reverb_and_eq() {
    let _g = serial();
    let x = xa();
    unsafe {
        let mut eng: *mut c_void = core::ptr::null_mut();
        assert_eq!((x.create)(&mut eng, 0x400, 8), S_OK, "XAudio2Create(0x400, 8) 0x144a207bb");
        let mut master: *mut c_void = core::ptr::null_mut();
        let cmv: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, *const u16, *const EffectChain, u32) -> i32 = slot(eng, 7);
        assert_eq!(cmv(eng, &mut master, 6, 44100, 0, core::ptr::null(), core::ptr::null(), 6), S_OK, "CreateMasteringVoice(6, 44100, 0, NULL, NULL, 6) 0x144a20829");
        let set_volume: unsafe extern "system" fn(*mut c_void, f32, u32) -> i32 = slot(master, 12);
        assert_eq!(set_volume(master, 0.0, 0), S_OK);

        let mut reverb: *mut c_void = core::ptr::null_mut();
        assert_eq!((x.reverb)(&mut reverb), S_OK, "CreateAudioReverb 0x144a1dcee");
        assert!(!reverb.is_null());
        let mut eq: *mut c_void = core::ptr::null_mut();
        assert_eq!((x.fx)(&CLSID_FXEQ, &mut eq, core::ptr::null(), 0), S_OK, "CreateFX(FXEQ, NULL, 0) 0x144a1d667");
        assert!(!eq.is_null());

        let rd = [EffectDesc { effect: reverb, initial: 1, out_channels: 2 }];
        let rchain = EffectChain { count: 1, descs: rd.as_ptr() };
        let mut submix: *mut c_void = core::ptr::null_mut();
        let csv: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, u32, *const Sends, *const EffectChain) -> i32 = slot(eng, 6);
        assert_eq!(csv(eng, &mut submix, 2, 44100, 0, 0, core::ptr::null(), &rchain), S_OK, "CreateSubmixVoice with the CreateAudioReverb XAPO");
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = slot(reverb, 2);
        rel(reverb);

        let ed = [EffectDesc { effect: eq, initial: 1, out_channels: 2 }];
        let echain = EffectChain { count: 1, descs: ed.as_ptr() };
        let sd = [SendDesc { flags: 0, voice: submix }];
        let sends = Sends { count: 1, sends: sd.as_ptr() };
        let fmt = WaveFormatEx { tag: 3, channels: 2, rate: 44100, bytes_per_sec: 44100 * 8, align: 8, bits: 32, cb: 0 };
        let mut src: *mut c_void = core::ptr::null_mut();
        let csrc: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *const WaveFormatEx, u32, f32, *mut c_void, *const Sends, *const EffectChain) -> i32 = slot(eng, 5);
        assert_eq!(csrc(eng, &mut src, &fmt, 0, 2.0, core::ptr::null_mut(), &sends, &echain), S_OK, "CreateSourceVoice float32 stereo with the CreateFX(FXEQ) XAPO");
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = slot(eq, 2);
        rel(eq);

        let frames = 22050u32;
        let pcm = vec![0f32; frames as usize * 2];
        let buf = Buffer { flags: 0, bytes: frames * 8, data: pcm.as_ptr() as *const u8, play_begin: 0, play_length: 0, loop_begin: 0, loop_length: 0, loop_count: 0, context: core::ptr::null_mut() };
        let submit: unsafe extern "system" fn(*mut c_void, *const Buffer, *const c_void) -> i32 = slot(src, 21);
        assert_eq!(submit(src, &buf, core::ptr::null()), S_OK);
        let start: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 = slot(src, 19);
        let state: unsafe extern "system" fn(*mut c_void, *mut VoiceState, u32) = slot(src, 25);
        let t = Instant::now();
        assert_eq!(start(src, 0, 0), S_OK);
        let mut st = VoiceState::default();
        loop {
            state(src, &mut st, 0);
            if st.queued == 0 {
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "source voice never finished its 0.5 s buffer");
            std::thread::sleep(Duration::from_millis(2));
        }
        let took = t.elapsed();
        let played = st.played;
        eprintln!("{frames} frames at 44100 Hz played in {took:?}, SamplesPlayed {played}");
        assert_eq!(played, frames as u64, "SamplesPlayed after the buffer ended");
        assert!(took >= Duration::from_millis(450), "0.5 s buffer finished in {took:?}: the engine consumes audio faster than real time");
        assert!(took <= Duration::from_millis(900), "0.5 s buffer took {took:?}");

        let destroy: unsafe extern "system" fn(*mut c_void) = slot(src, 18);
        destroy(src);
        let destroy: unsafe extern "system" fn(*mut c_void) = slot(submix, 18);
        destroy(submix);
        let destroy: unsafe extern "system" fn(*mut c_void) = slot(master, 18);
        destroy(master);
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = slot(eng, 2);
        rel(eng);
    }
}

#[test]
fn fortnite_x3daudio_pans_a_right_emitter_with_matrix_and_reverb_flags() {
    let x = xa();
    let mut inst = [0u8; 20];
    assert_eq!(unsafe { (x.x3d_init)(0x3f, 343.5, inst.as_mut_ptr()) }, S_OK, "X3DAudioInitialize(0x3f, 343.5) 0x144a1febd");
    let l = Listener { front: [0.0, 0.0, 1.0], top: [0.0, 1.0, 0.0], ..Default::default() };
    let e = Emitter { front: [0.0, 0.0, 1.0], top: [0.0, 1.0, 0.0], pos: [5.0, 0.0, 0.0], channels: 1, distance_scaler: 1.0, doppler_scaler: 1.0, ..Default::default() };
    let mut m = [0f32; 6];
    let mut d = Dsp { matrix: m.as_mut_ptr(), delays: core::ptr::null_mut(), src: 1, dst: 6, lpf_direct: 0.0, lpf_reverb: 0.0, reverb: -1.0, doppler: 0.0, angle: 0.0, distance: 0.0, emitter_vel: 0.0, listener_vel: 0.0 };
    unsafe { (x.x3d_calc)(inst.as_ptr(), &l, &e, 0x11, &mut d) };
    let (dist, rev) = (d.distance, d.reverb);
    eprintln!("matrix {m:?} distance {dist} reverb {rev}");
    assert!((dist - 5.0).abs() < 1e-4, "EmitterToListenerDistance {dist}");
    assert!(m[1] > m[0], "emitter on the right: FR {} must exceed FL {}", m[1], m[0]);
    assert!(m.iter().all(|v| v.is_finite() && *v >= 0.0));
    assert!(m.iter().sum::<f32>() > 0.0);
    assert!((0.0..=1.0).contains(&rev), "X3DAUDIO_CALCULATE_REVERB must write ReverbLevel, got {rev}");
}
