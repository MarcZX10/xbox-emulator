use super::loudness::*;
use super::xapo_format::*;
use super::Guid;
use core::ffi::c_void;

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(p: *mut c_void);
}

fn float_format(channels: u16, rate: u32) -> [u8; 18] {
    let mut f = [0u8; 18];
    f[0..2].copy_from_slice(&WAVE_FORMAT_IEEE_FLOAT.to_le_bytes());
    f[2..4].copy_from_slice(&channels.to_le_bytes());
    f[4..8].copy_from_slice(&rate.to_le_bytes());
    f[8..12].copy_from_slice(&(rate * channels as u32 * 4).to_le_bytes());
    f[12..14].copy_from_slice(&(channels * 4).to_le_bytes());
    f[14..16].copy_from_slice(&32u16.to_le_bytes());
    f
}

fn pcm16_format(channels: u16, rate: u32) -> [u8; 18] {
    let mut f = float_format(channels, rate);
    f[0..2].copy_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    f[8..12].copy_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    f[12..14].copy_from_slice(&(channels * 2).to_le_bytes());
    f[14..16].copy_from_slice(&16u16.to_le_bytes());
    f
}

unsafe fn vt(x: *mut LoudnessXapo) -> &'static XapoVtbl {
    &*(*x).vtbl
}

unsafe fn params_of(x: *mut LoudnessXapo) -> *mut c_void {
    let mut p: *mut c_void = core::ptr::null_mut();
    assert_eq!((vt(x).query_interface)(x, &IID_IXAPO_PARAMETERS, &mut p), 0);
    p
}

unsafe fn read_parameters(p: *mut c_void) -> LoudnessParameters {
    let v = &**(p as *const *const XapoParametersVtbl);
    let mut out = LoudnessParameters::default();
    (v.get_parameters)(p, &mut out as *mut LoudnessParameters as *mut c_void, PARAMETER_BLOCK_SIZE as u32);
    out
}

#[test]
fn platform_specific_xapofx_factory_creates_console_loudness_meter() {
    unsafe {
        let mut out = 0x77usize as *mut c_void;
        let other = Guid(0x5039_d740, 0xf736, 0x449a, [0x84, 0xd3, 0xa5, 0x62, 0x02, 0x55, 0x7b, 0x87]);
        assert_eq!(PlatformSpecificXAPOFXFactory(&other, &mut out, core::ptr::null(), 0), E_INVALIDARG);
        assert!(out.is_null());
        assert_eq!(PlatformSpecificXAPOFXFactory(&CLSID_LOUDNESS_METER, &mut out, core::ptr::null(), 0), 0);
        let x = out as *mut LoudnessXapo;
        let v = vt(x);
        let mut reg: *mut RegistrationProperties = core::ptr::null_mut();
        assert_eq!((v.get_registration_properties)(x, &mut reg), 0);
        assert_eq!(core::mem::size_of::<RegistrationProperties>(), 0x42c);
        let r = &*reg;
        let name = r.friendly_name;
        assert_eq!(String::from_utf16_lossy(&name[..14]), "Loudness Meter");
        let (clsid, flags, major, minor) = (r.clsid, r.flags, r.major, r.minor);
        assert_eq!((clsid, flags, major, minor), (CLSID_LOUDNESS_METER, 0x3f, 1, 0));
        CoTaskMemFree(reg as *mut c_void);
        let mut none: *mut c_void = 0x1usize as *mut c_void;
        let bogus = Guid(1, 2, 3, [4; 8]);
        assert_eq!((v.query_interface)(x, &bogus, &mut none), E_NOINTERFACE);
        assert!(none.is_null());
        let p = params_of(x);
        assert_eq!(p as usize, x as usize + 8);

        let stereo = float_format(2, 48000);
        assert_eq!((v.is_input_format_supported)(x, stereo.as_ptr(), stereo.as_ptr(), core::ptr::null_mut()), 0);
        let pcm = pcm16_format(2, 48000);
        let mut sup: *mut u8 = core::ptr::null_mut();
        assert_eq!((v.is_input_format_supported)(x, stereo.as_ptr(), pcm.as_ptr(), &mut sup), XAPO_E_FORMAT_UNSUPPORTED);
        assert!(!sup.is_null());
        assert_eq!(format_tag(sup), WAVE_FORMAT_IEEE_FLOAT);
        assert_eq!(core::ptr::read_unaligned(sup as *const u16), WAVE_FORMAT_EXTENSIBLE);
        assert_eq!(core::ptr::read_unaligned(sup.add(0xe) as *const u16), 32);
        CoTaskMemFree(sup as *mut c_void);
        let mono = float_format(1, 48000);
        assert_eq!((v.is_output_format_supported)(x, stereo.as_ptr(), mono.as_ptr(), core::ptr::null_mut()), XAPO_E_FORMAT_UNSUPPORTED);
        let mut broken = stereo;
        broken[12] = 3;
        assert_eq!((v.is_input_format_supported)(x, stereo.as_ptr(), broken.as_ptr(), core::ptr::null_mut()), E_INVALIDARG);
        assert_eq!((v.calc_input_frames)(x, 480), 480);
        assert_eq!((v.calc_output_frames)(x, 123), 123);
        assert_eq!((v.initialize)(x, core::ptr::null(), 0), 0);

        let lock = LockParameters { format: stereo.as_ptr(), max_frames: 480 };
        assert_eq!((v.lock_for_process)(x, 1, &lock, 1, &lock), 0);
        let rate = 48000.0f64;
        let mut t = 0usize;
        let mut buf = vec![0.0f32; 480 * 2];
        let mut shorts = Vec::new();
        let mut last_short = f64::NAN;
        for _ in 0..(48000 * 5 / 480) {
            for i in 0..480 {
                buf[i * 2] = (2.0 * core::f64::consts::PI * 997.0 * t as f64 / rate).sin() as f32;
                buf[i * 2 + 1] = 0.0;
                t += 1;
            }
            let copy = buf.clone();
            let inp = BufferParameters { buffer: buf.as_mut_ptr() as *mut c_void, flags: XAPO_BUFFER_VALID, valid_frames: 480 };
            let mut outp = BufferParameters { buffer: buf.as_mut_ptr() as *mut c_void, flags: 0, valid_frames: 0 };
            (v.process)(x, 1, &inp, 1, &mut outp, 1);
            assert_eq!((outp.flags, outp.valid_frames), (XAPO_BUFFER_VALID, 480));
            assert_eq!(buf, copy);
            let now = read_parameters(p);
            if now.short_term != 0.0 && now.short_term.to_bits() != last_short.to_bits() {
                shorts.push(now.short_term);
                last_short = now.short_term;
            }
        }
        let m = read_parameters(p);
        assert!((m.momentary + 3.01).abs() < 0.05, "momentary {}", m.momentary);
        assert!((m.short_term + 3.01).abs() < 0.05, "short-term {}", m.short_term);
        assert!((m.range_high + 3.01).abs() < 0.05, "range high {}", m.range_high);
        assert!(m.range_low < -3.0, "range low {}", m.range_low);
        assert!(m.true_peak.abs() > 0.99 && m.true_peak.abs() < 1.1, "true peak {}", m.true_peak);
        assert!(!shorts.is_empty());

        let silent = vec![0.0f32; 480 * 2];
        let before = read_parameters(p);
        let inp = BufferParameters { buffer: silent.as_ptr() as *mut c_void, flags: XAPO_BUFFER_VALID, valid_frames: 480 };
        let mut outp = BufferParameters { buffer: buf.as_mut_ptr() as *mut c_void, flags: 0, valid_frames: 0 };
        (v.process)(x, 1, &inp, 1, &mut outp, 0);
        assert_eq!(read_parameters(p), before);
        assert!(buf[..480].iter().all(|&s| s == 0.0));

        (v.reset)(x);
        let r = read_parameters(p);
        let count = meter_count(x);
        assert_eq!(count, 0);
        assert!((r.integrated + 3.01).abs() < 0.1, "integrated {}", r.integrated);
        (v.reset)(x);
        assert!(read_parameters(p).integrated.is_nan());
        (v.unlock_for_process)(x);
        assert!((*x).meter.lock().unwrap().ring.is_none());
        let pv = &**(p as *const *const XapoParametersVtbl);
        assert_eq!((pv.release)(p), 1);
        assert_eq!((v.release)(x), 0);
    }
}

unsafe fn meter_count(x: *mut LoudnessXapo) -> u32 {
    (*x).meter.lock().unwrap().short_term_count
}

#[test]
fn loudness_pairs_follow_console_lock_and_reset_rounding() {
    let mut m = Meter::default();
    m.lock(1, 48000);
    assert_eq!((m.pairs, m.momentary_frames, m.short_term_frames, m.frames_per_ms), (0, 19200, 144000, 48));
    m.reset();
    assert_eq!(m.pairs, 1);
    let mut s = Meter::default();
    s.lock(6, 44100);
    assert_eq!((s.pairs, s.momentary_frames, s.short_term_frames, s.frames_per_ms), (3, 17640, 132300, 44));
    assert_eq!((s.range_low, s.range_high), (0.0, -80.0));
}
