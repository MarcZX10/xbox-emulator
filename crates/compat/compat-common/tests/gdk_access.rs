use compat_common::gdk::{self, access};
use core::ffi::c_void;

unsafe fn slot(n: usize) -> (*mut c_void, usize) {
    let mut obj: *mut c_void = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&access::CLSID, &access::IID, &mut obj), 0);
    let vt = *(obj as *const *const usize);
    (obj, *vt.add(n))
}

#[test]
fn accessibility_served_natively() {
    std::env::set_var("TOMBOX_GDK_ROOT", std::env::temp_dir().join(format!("tombox_gdk_access_{}", std::process::id())));
    unsafe {
        let (o, get) = slot(3);
        let get: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32 = core::mem::transmute(get);
        let (_, set) = slot(4);
        let set: unsafe extern "system" fn(*mut c_void, bool) -> i32 = core::mem::transmute(set);
        let mut props = [0xAAu8; 32];
        assert_eq!(get(o, props.as_mut_ptr()), 0);
        assert_eq!(props[28..], [0xAA; 4]);
        let scale = f32::from_le_bytes(props[20..24].try_into().unwrap());
        assert!([0.5, 1.0, 1.5, 2.0].contains(&scale));
        assert_eq!(set(o, true), 0);
        assert_eq!(get(o, props.as_mut_ptr()), 0);
        assert_eq!(props[24], 1);
        assert_eq!(set(o, false), 0);
        assert_eq!(get(o, props.as_mut_ptr()), 0);
        assert_eq!(props[24], 0);
        assert!(get(o, core::ptr::null_mut()) < 0);

        let (_, hc) = slot(5);
        let hc: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 = core::mem::transmute(hc);
        let mut mode = 99u32;
        assert_eq!(hc(o, &mut mode), 0);
        assert!(mode <= 3);

        let (_, pos) = slot(6);
        let pos: unsafe extern "system" fn(*mut c_void, u32) -> i32 = core::mem::transmute(pos);
        assert_eq!(pos(o, 2), 0);
        assert_eq!(access::speech_to_text_position(), 2);
        assert!(pos(o, 100) < 0);
    }
}

#[test]
fn speech_to_text_hypotheses() {
    std::env::set_var("TOMBOX_GDK_ROOT", std::env::temp_dir().join(format!("tombox_gdk_access_{}", std::process::id())));
    unsafe {
        let (o, begin) = slot(17);
        let begin: unsafe extern "system" fn(*mut c_void, *const u8, *const u8, u32, *mut u32) -> i32 = core::mem::transmute(begin);
        let (_, update) = slot(18);
        let update: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> i32 = core::mem::transmute(update);
        let (_, finalize) = slot(19);
        let finalize: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> i32 = core::mem::transmute(finalize);
        let (_, cancel) = slot(20);
        let cancel: unsafe extern "system" fn(*mut c_void, u32) -> i32 = core::mem::transmute(cancel);
        let (mut a, mut b) = (0u32, 0u32);
        assert_eq!(begin(o, b"Player\0".as_ptr(), b"hel\0".as_ptr(), 0, &mut a), 0);
        assert_eq!(begin(o, b"Player\0".as_ptr(), b"wor\0".as_ptr(), 0, &mut b), 0);
        assert_ne!(a, b);
        assert_eq!(update(o, a, b"hello\0".as_ptr()), 0);
        assert_eq!(access::hypothesis(a).unwrap().content, "hello");
        assert_eq!(cancel(o, b), 0);
        assert!(access::hypothesis(b).is_none());
        assert!(cancel(o, b) < 0);
        assert!(update(o, b, b"x\0".as_ptr()) < 0);
        assert_eq!(finalize(o, a, b"hello there\0".as_ptr()), 0);
        assert!(access::hypothesis(a).is_none());
        assert!(finalize(o, a, b"again\0".as_ptr()) < 0);
    }
}

#[test]
fn history_frames_are_compressed_and_clips_are_h264() {
    std::env::set_var("TOMBOX_GDK_ROOT", std::env::temp_dir().join(format!("tombox_gdk_clip_{}", std::process::id())));
    let (w, h) = (480u32, 270u32);
    let r = gdk::media::com_thread(move || {
        let base = 1_000_000u64;
        for i in 0..30u64 {
            let mut rgb = vec![0u8; (w * h * 3) as usize];
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let o = (y * w as usize + x) * 3;
                    rgb[o] = ((x as u64 + i * 8) % 256) as u8;
                    rgb[o + 1] = (y % 256) as u8;
                    rgb[o + 2] = if (x / 40 + y / 40) % 2 == 0 { 200 } else { 30 };
                }
            }
            if i == 0 {
                let j = gdk::media::jpeg_encode(w, h, &rgb).unwrap();
                let (dw, dh, back) = gdk::media::jpeg_decode(&j).unwrap();
                assert_eq!((dw, dh), (w, h));
                let err: u64 = back.iter().zip(&rgb).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
                assert!(err / (rgb.len() as u64) < 12);
            }
            gdk::capture::push_history_frame(base + i * 100, w, h, &rgb).unwrap();
        }
        gdk::capture::encode_clip(base, base + 3000, "test")
    })
    .unwrap();
    assert!(gdk::capture::history_len() >= 30);
    let per = gdk::capture::history_bytes() / gdk::capture::history_len();
    assert!(per < (w * h * 3) as usize / 8, "{per} bytes per frame");
    let clip = r.unwrap();
    eprintln!("history {per} bytes/frame, clip {} bytes at {}", clip.size, clip.path.display());
    assert_eq!((clip.w, clip.h), (w, h));
    assert_eq!(clip.duration_ms, 3000);
    let data = std::fs::read(&clip.path).unwrap();
    assert_eq!(data.len() as u64, clip.size);
    assert_eq!(&data[4..8], b"ftyp");
    assert!(data.windows(4).any(|x| x == b"avc1"));
    assert!(clip.path.extension().unwrap() == "mp4");
}

#[test]
fn speech_synthesizer_produces_wav() {
    if access::voices().is_empty() {
        return;
    }
    unsafe {
        let (o, create) = slot(9);
        let create: unsafe extern "system" fn(*mut c_void, *mut usize) -> i32 = core::mem::transmute(create);
        let (_, dup) = slot(8);
        let dup: unsafe extern "system" fn(*mut c_void, usize, *mut usize) -> i32 = core::mem::transmute(dup);
        let (_, close) = slot(10);
        let close: unsafe extern "system" fn(*mut c_void, usize) -> i32 = core::mem::transmute(close);
        let (_, custom) = slot(12);
        let custom: unsafe extern "system" fn(*mut c_void, usize, *const u8) -> i32 = core::mem::transmute(custom);
        let (_, from_text) = slot(13);
        let from_text: unsafe extern "system" fn(*mut c_void, usize, *const u8, *mut usize) -> i32 = core::mem::transmute(from_text);
        let (_, sclose) = slot(14);
        let sclose: unsafe extern "system" fn(*mut c_void, usize) -> i32 = core::mem::transmute(sclose);
        let (_, size) = slot(15);
        let size: unsafe extern "system" fn(*mut c_void, usize, *mut usize) -> i32 = core::mem::transmute(size);
        let (_, data) = slot(16);
        let data: unsafe extern "system" fn(*mut c_void, usize, usize, *mut u8, *mut usize) -> i32 = core::mem::transmute(data);

        let mut h = 0usize;
        assert_eq!(create(o, &mut h), 0);
        let mut h2 = 0usize;
        assert_eq!(dup(o, h, &mut h2), 0);
        assert_eq!(close(o, h2), 0);
        let v = &access::voices()[0];
        let id = std::ffi::CString::new(v.token.clone()).unwrap();
        assert_eq!(custom(o, h, id.as_ptr() as *const u8), 0);
        assert!(custom(o, h, b"no-such-voice\0".as_ptr()) < 0);
        let mut s = 0usize;
        assert_eq!(from_text(o, h, b"Hello from the emulator.\0".as_ptr(), &mut s), 0);
        let mut n = 0usize;
        assert_eq!(size(o, s, &mut n), 0);
        assert!(n > 44);
        let mut small = vec![0u8; 4];
        assert!(data(o, s, small.len(), small.as_mut_ptr(), core::ptr::null_mut()) < 0);
        let mut buf = vec![0u8; n];
        let mut used = 0usize;
        assert_eq!(data(o, s, n, buf.as_mut_ptr(), &mut used), 0);
        assert_eq!(used, n);
        assert_eq!(&buf[0..4], b"RIFF");
        assert_eq!(&buf[8..12], b"WAVE");
        assert_eq!(sclose(o, s), 0);
        assert_eq!(close(o, h), 0);
        assert!(close(o, 0) < 0);
    }
}
