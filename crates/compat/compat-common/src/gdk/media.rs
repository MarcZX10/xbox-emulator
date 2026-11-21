use super::win::wide;
use super::{Guid, E_FAIL};
use core::ffi::c_void;
use std::path::Path;

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(clsid: *const Guid, outer: *mut c_void, ctx: u32, iid: *const Guid, out: *mut *mut c_void) -> i32;
    fn CreateStreamOnHGlobal(h: *mut c_void, del: i32, out: *mut *mut c_void) -> i32;
    fn GetHGlobalFromStream(s: *mut c_void, out: *mut *mut c_void) -> i32;
}

#[link(name = "shlwapi")]
extern "system" {
    fn SHCreateMemStream(p: *const u8, n: u32) -> *mut c_void;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalLock(h: *mut c_void) -> *mut u8;
    fn GlobalUnlock(h: *mut c_void) -> i32;
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}

const CLSID_WIC: Guid = Guid::parse("cacaf262-9370-4615-a13b-9f5539da4c0a");
const IID_WIC: Guid = Guid::parse("ec5ec8a9-c395-4314-9c77-54d7a935ff70");
const FORMAT_JPEG: Guid = Guid::parse("19e4a5aa-5662-4fc5-a0c0-1758028e1057");
const PIXEL_BGR24: Guid = Guid::parse("6fddc324-4e03-4bfe-b185-3d77768dc90c");

const MF_MT_MAJOR_TYPE: Guid = Guid::parse("48eba18e-f8c9-4687-bf11-0a74c9f96a8f");
const MF_MT_SUBTYPE: Guid = Guid::parse("f7e34c9a-42e8-4714-b74b-cb29d72c35e5");
const MF_MT_AVG_BITRATE: Guid = Guid::parse("20332624-fb0d-4d9e-bd0d-cbf6786c102e");
const MF_MT_INTERLACE_MODE: Guid = Guid::parse("e2724bb8-e676-4806-b4b2-a8d6efb44ccd");
const MF_MT_FRAME_SIZE: Guid = Guid::parse("1652c33d-d6b2-4012-b834-72030849a37d");
const MF_MT_FRAME_RATE: Guid = Guid::parse("c459a2e8-3d2c-4e44-b132-fee5156c7bb0");
const MF_MT_PIXEL_ASPECT_RATIO: Guid = Guid::parse("c6376a1e-8d0a-4027-be45-6d9a0ad39bb6");
const MF_MT_DEFAULT_STRIDE: Guid = Guid::parse("644b4e48-1e02-4516-b0eb-c01ca9d49ac6");
const MF_VIDEO: Guid = Guid::parse("73646976-0000-0010-8000-00aa00389b71");
const MF_H264: Guid = Guid::parse("34363248-0000-0010-8000-00aa00389b71");
const MF_RGB32: Guid = Guid::parse("00000016-0000-0010-8000-00aa00389b71");

unsafe fn slot(obj: *mut c_void, n: usize) -> usize {
    *(*(obj as *const *const usize)).add(n)
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(obj, 2));
        f(obj);
    }
}

struct Com(Vec<*mut c_void>);

impl Com {
    fn keep(&mut self, p: *mut c_void) -> *mut c_void {
        self.0.push(p);
        p
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        for p in self.0.drain(..).rev() {
            unsafe { release(p) };
        }
    }
}

fn check(hr: i32) -> Result<(), i32> {
    if hr < 0 { Err(hr) } else { Ok(()) }
}

pub fn com_thread<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    std::thread::spawn(move || unsafe {
        let hr = CoInitializeEx(core::ptr::null_mut(), 0);
        let r = f();
        if hr >= 0 {
            CoUninitialize();
        }
        r
    })
    .join()
    .ok()
}

pub fn init_com_here() {
    unsafe {
        CoInitializeEx(core::ptr::null_mut(), 0);
    }
}

unsafe fn wic(c: &mut Com) -> Result<*mut c_void, i32> {
    let mut f = core::ptr::null_mut();
    check(CoCreateInstance(&CLSID_WIC, core::ptr::null_mut(), 1, &IID_WIC, &mut f))?;
    Ok(c.keep(f))
}

pub fn jpeg_encode(w: u32, h: u32, rgb: &[u8]) -> Result<Vec<u8>, i32> {
    if rgb.len() < (w * h * 3) as usize || w == 0 || h == 0 {
        return Err(super::E_INVALIDARG);
    }
    unsafe {
        let mut c = Com(Vec::new());
        let f = wic(&mut c)?;
        let mut stream = core::ptr::null_mut();
        check(CreateStreamOnHGlobal(core::ptr::null_mut(), 1, &mut stream))?;
        c.keep(stream);
        let create_encoder: unsafe extern "system" fn(*mut c_void, *const Guid, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(slot(f, 8));
        let mut enc = core::ptr::null_mut();
        check(create_encoder(f, &FORMAT_JPEG, core::ptr::null(), &mut enc))?;
        c.keep(enc);
        let init: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> i32 = core::mem::transmute(slot(enc, 3));
        check(init(enc, stream, 2))?;
        let new_frame: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut *mut c_void) -> i32 = core::mem::transmute(slot(enc, 10));
        let (mut fr, mut bag) = (core::ptr::null_mut(), core::ptr::null_mut());
        check(new_frame(enc, &mut fr, &mut bag))?;
        c.keep(fr);
        if !bag.is_null() {
            c.keep(bag);
        }
        let finit: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 = core::mem::transmute(slot(fr, 3));
        check(finit(fr, bag))?;
        let set_size: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 = core::mem::transmute(slot(fr, 4));
        check(set_size(fr, w, h))?;
        let set_fmt: unsafe extern "system" fn(*mut c_void, *mut Guid) -> i32 = core::mem::transmute(slot(fr, 6));
        let mut fmt = PIXEL_BGR24;
        check(set_fmt(fr, &mut fmt))?;
        if fmt != PIXEL_BGR24 {
            return Err(E_FAIL);
        }
        let mut bgr = rgb[..(w * h * 3) as usize].to_vec();
        for p in bgr.chunks_exact_mut(3) {
            p.swap(0, 2);
        }
        let write: unsafe extern "system" fn(*mut c_void, u32, u32, u32, *const u8) -> i32 = core::mem::transmute(slot(fr, 10));
        check(write(fr, h, w * 3, bgr.len() as u32, bgr.as_ptr()))?;
        let fcommit: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(fr, 12));
        check(fcommit(fr))?;
        let ecommit: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(enc, 11));
        check(ecommit(enc))?;
        let seek: unsafe extern "system" fn(*mut c_void, i64, u32, *mut u64) -> i32 = core::mem::transmute(slot(stream, 5));
        let mut len = 0u64;
        check(seek(stream, 0, 1, &mut len))?;
        let mut hg = core::ptr::null_mut();
        check(GetHGlobalFromStream(stream, &mut hg))?;
        let p = GlobalLock(hg);
        if p.is_null() {
            return Err(super::win::last_error_hr());
        }
        let out = std::slice::from_raw_parts(p, len as usize).to_vec();
        GlobalUnlock(hg);
        Ok(out)
    }
}

pub fn jpeg_decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), i32> {
    unsafe {
        let mut c = Com(Vec::new());
        let f = wic(&mut c)?;
        let stream = SHCreateMemStream(data.as_ptr(), data.len() as u32);
        if stream.is_null() {
            return Err(0x8007_000Eu32 as i32);
        }
        c.keep(stream);
        let from_stream: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, u32, *mut *mut c_void) -> i32 = core::mem::transmute(slot(f, 4));
        let mut dec = core::ptr::null_mut();
        check(from_stream(f, stream, core::ptr::null(), 0, &mut dec))?;
        c.keep(dec);
        let get_frame: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 = core::mem::transmute(slot(dec, 13));
        let mut fr = core::ptr::null_mut();
        check(get_frame(dec, 0, &mut fr))?;
        c.keep(fr);
        let make_conv: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 = core::mem::transmute(slot(f, 10));
        let mut conv = core::ptr::null_mut();
        check(make_conv(f, &mut conv))?;
        c.keep(conv);
        let cinit: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, u32, *mut c_void, f64, u32) -> i32 = core::mem::transmute(slot(conv, 8));
        check(cinit(conv, fr, &PIXEL_BGR24, 0, core::ptr::null_mut(), 0.0, 0))?;
        let get_size: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) -> i32 = core::mem::transmute(slot(conv, 3));
        let (mut w, mut h) = (0u32, 0u32);
        check(get_size(conv, &mut w, &mut h))?;
        let mut buf = vec![0u8; (w * h * 3) as usize];
        let copy: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut u8) -> i32 = core::mem::transmute(slot(conv, 7));
        check(copy(conv, core::ptr::null(), w * 3, buf.len() as u32, buf.as_mut_ptr()))?;
        for p in buf.chunks_exact_mut(3) {
            p.swap(0, 2);
        }
        Ok((w, h, buf))
    }
}

unsafe fn proc<T>(module: &str, name: &str) -> Result<T, i32> {
    let m = LoadLibraryW(wide(module).as_ptr());
    if m.is_null() {
        return Err(super::win::last_error_hr());
    }
    let n = format!("{name}\0");
    let p = GetProcAddress(m, n.as_ptr());
    if p.is_null() {
        return Err(super::win::last_error_hr());
    }
    Ok(core::mem::transmute_copy(&p))
}

unsafe fn media_type(c: &mut Com, create: unsafe extern "system" fn(*mut *mut c_void) -> i32, sub: &Guid, w: u32, h: u32, fps: u32, extra: &[(&Guid, u32)]) -> Result<*mut c_void, i32> {
    let mut t = core::ptr::null_mut();
    check(create(&mut t))?;
    c.keep(t);
    let set_u32: unsafe extern "system" fn(*mut c_void, *const Guid, u32) -> i32 = core::mem::transmute(slot(t, 21));
    let set_u64: unsafe extern "system" fn(*mut c_void, *const Guid, u64) -> i32 = core::mem::transmute(slot(t, 22));
    let set_guid: unsafe extern "system" fn(*mut c_void, *const Guid, *const Guid) -> i32 = core::mem::transmute(slot(t, 24));
    check(set_guid(t, &MF_MT_MAJOR_TYPE, &MF_VIDEO))?;
    check(set_guid(t, &MF_MT_SUBTYPE, sub))?;
    check(set_u32(t, &MF_MT_INTERLACE_MODE, 2))?;
    check(set_u64(t, &MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64))?;
    check(set_u64(t, &MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1))?;
    check(set_u64(t, &MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1))?;
    for (k, v) in extra {
        check(set_u32(t, *k, *v))?;
    }
    Ok(t)
}

pub fn even_dims(w: u32, h: u32) -> (u32, u32) {
    (w & !1, h & !1)
}

pub fn h264_mp4(path: &Path, w: u32, h: u32, fps: u32, mut next: impl FnMut() -> Option<Vec<u8>>) -> Result<(u64, u32), i32> {
    let (ew, eh) = even_dims(w, h);
    if ew < 2 || eh < 2 || fps == 0 {
        return Err(super::E_INVALIDARG);
    }
    unsafe {
        let startup: unsafe extern "system" fn(u32, u32) -> i32 = proc("mfplat.dll", "MFStartup")?;
        let shutdown: unsafe extern "system" fn() -> i32 = proc("mfplat.dll", "MFShutdown")?;
        let create_type: unsafe extern "system" fn(*mut *mut c_void) -> i32 = proc("mfplat.dll", "MFCreateMediaType")?;
        let create_sample: unsafe extern "system" fn(*mut *mut c_void) -> i32 = proc("mfplat.dll", "MFCreateSample")?;
        let create_buffer: unsafe extern "system" fn(u32, *mut *mut c_void) -> i32 = proc("mfplat.dll", "MFCreateMemoryBuffer")?;
        let create_writer: unsafe extern "system" fn(*const u16, *mut c_void, *mut c_void, *mut *mut c_void) -> i32 = proc("mfreadwrite.dll", "MFCreateSinkWriterFromURL")?;
        check(startup(0x0002_0070, 0))?;
        let r = (|| {
            let mut c = Com(Vec::new());
            let _ = std::fs::remove_file(path);
            let p = wide(&path.to_string_lossy());
            let mut wr = core::ptr::null_mut();
            check(create_writer(p.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut(), &mut wr))?;
            c.keep(wr);
            let bitrate = ((ew as u64 * eh as u64 * fps as u64) / 4).clamp(250_000, 8_000_000) as u32;
            let out = media_type(&mut c, create_type, &MF_H264, ew, eh, fps, &[(&MF_MT_AVG_BITRATE, bitrate)])?;
            let add_stream: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut u32) -> i32 = core::mem::transmute(slot(wr, 3));
            let mut idx = 0u32;
            check(add_stream(wr, out, &mut idx))?;
            let inp = media_type(&mut c, create_type, &MF_RGB32, ew, eh, fps, &[(&MF_MT_DEFAULT_STRIDE, ew * 4)])?;
            let set_input: unsafe extern "system" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> i32 = core::mem::transmute(slot(wr, 4));
            check(set_input(wr, idx, inp, core::ptr::null_mut()))?;
            let begin: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(wr, 5));
            check(begin(wr))?;
            let write_sample: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32 = core::mem::transmute(slot(wr, 6));
            let dur = 10_000_000i64 / fps as i64;
            let size = ew * eh * 4;
            let mut n = 0u32;
            while let Some(rgb) = next() {
                if rgb.len() < (w * h * 3) as usize {
                    continue;
                }
                let mut fc = Com(Vec::new());
                let mut buf = core::ptr::null_mut();
                check(create_buffer(size, &mut buf))?;
                fc.keep(buf);
                let lock: unsafe extern "system" fn(*mut c_void, *mut *mut u8, *mut u32, *mut u32) -> i32 = core::mem::transmute(slot(buf, 3));
                let unlock: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(buf, 4));
                let set_len: unsafe extern "system" fn(*mut c_void, u32) -> i32 = core::mem::transmute(slot(buf, 6));
                let mut dst = core::ptr::null_mut();
                check(lock(buf, &mut dst, core::ptr::null_mut(), core::ptr::null_mut()))?;
                let d = std::slice::from_raw_parts_mut(dst, size as usize);
                for y in 0..eh as usize {
                    for x in 0..ew as usize {
                        let s = (y * w as usize + x) * 3;
                        let o = (y * ew as usize + x) * 4;
                        d[o] = rgb[s + 2];
                        d[o + 1] = rgb[s + 1];
                        d[o + 2] = rgb[s];
                        d[o + 3] = 255;
                    }
                }
                unlock(buf);
                check(set_len(buf, size))?;
                let mut smp = core::ptr::null_mut();
                check(create_sample(&mut smp))?;
                fc.keep(smp);
                let add_buffer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 = core::mem::transmute(slot(smp, 42));
                let set_time: unsafe extern "system" fn(*mut c_void, i64) -> i32 = core::mem::transmute(slot(smp, 36));
                let set_dur: unsafe extern "system" fn(*mut c_void, i64) -> i32 = core::mem::transmute(slot(smp, 38));
                check(add_buffer(smp, buf))?;
                check(set_time(smp, n as i64 * dur))?;
                check(set_dur(smp, dur))?;
                check(write_sample(wr, idx, smp))?;
                n += 1;
            }
            if n == 0 {
                return Err(super::E_INVALIDARG);
            }
            let finalize: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(wr, 11));
            check(finalize(wr))?;
            Ok(n)
        })();
        shutdown();
        let n = r?;
        let len = std::fs::metadata(path).map(|m| m.len()).map_err(|e| super::win::hr_from_win32(e.raw_os_error().unwrap_or(2) as u32))?;
        Ok((len, n))
    }
}
