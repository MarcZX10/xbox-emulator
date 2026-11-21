use core::mem::ManuallyDrop;
use windows::core::{Interface, GUID};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

pub const W: u32 = 640;
pub const H: u32 = 360;
pub const FRAMES: u32 = 24;

pub fn frame_nv12(i: u32) -> Vec<u8> {
    let mut f = vec![0u8; (W * H * 3 / 2) as usize];
    let sq = (i * 12) % (W - 96);
    for y in 0..H {
        for x in 0..W {
            let mut v = ((x + y + i * 4) & 0xff) as u8 / 2 + 40;
            if x >= sq && x < sq + 96 && y >= 120 && y < 216 {
                v = 220;
            }
            f[(y * W + x) as usize] = v;
        }
    }
    let uv = &mut f[(W * H) as usize..];
    for y in 0..H / 2 {
        for x in 0..W / 2 {
            uv[(y * W + x * 2) as usize] = (96 + (i * 5) % 64) as u8;
            uv[(y * W + x * 2 + 1) as usize] = (160 - (x / 8) % 32) as u8;
        }
    }
    f
}

fn find_encoder(name: &str) -> IMFTransform {
    unsafe {
        let i = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_NV12 };
        let o = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_HEVC };
        let mut acts: *mut Option<IMFActivate> = core::ptr::null_mut();
        let mut n = 0u32;
        MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_ALL, Some(&i), Some(&o), &mut acts, &mut n).unwrap();
        for k in 0..n as usize {
            let a = (*acts.add(k)).as_ref().unwrap();
            let mut p = windows::core::PWSTR::null();
            let mut len = 0;
            let _ = a.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut p, &mut len);
            if p.to_string().unwrap_or_default() == name {
                return a.ActivateObject().unwrap();
            }
        }
        panic!("no {name}");
    }
}

fn set_codec(t: &IMFTransform, key: &GUID, v: u32) {
    let api: ICodecAPI = t.cast().unwrap();
    let var = windows::core::VARIANT::from(v);
    let _ = unsafe { api.SetValue(key, &var) };
}

#[test]
#[ignore]
fn make_hevc_clip() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(0x0002_0070, MFSTARTUP_FULL).unwrap();
        let t = find_encoder("HEVCVideoExtensionEncoder");
        set_codec(&t, &CODECAPI_AVEncMPVDefaultBPictureCount, 2);
        set_codec(&t, &CODECAPI_AVEncMPVGOPSize, 12);
        let out = MFCreateMediaType().unwrap();
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_HEVC).unwrap();
        out.SetUINT64(&MF_MT_FRAME_SIZE, ((W as u64) << 32) | H as u64).unwrap();
        out.SetUINT64(&MF_MT_FRAME_RATE, (30u64 << 32) | 1).unwrap();
        out.SetUINT32(&MF_MT_AVG_BITRATE, 2_000_000).unwrap();
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).unwrap();
        t.SetOutputType(0, &out, 0).unwrap();
        let inp = MFCreateMediaType().unwrap();
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
        inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).unwrap();
        inp.SetUINT64(&MF_MT_FRAME_SIZE, ((W as u64) << 32) | H as u64).unwrap();
        inp.SetUINT64(&MF_MT_FRAME_RATE, (30u64 << 32) | 1).unwrap();
        inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).unwrap();
        t.SetInputType(0, &inp, 0).unwrap();
        t.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).unwrap();
        t.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).unwrap();
        let mut es = Vec::new();
        let pull = |t: &IMFTransform, es: &mut Vec<u8>| loop {
            let info = t.GetOutputStreamInfo(0).unwrap();
            let b = MFCreateMemoryBuffer(info.cbSize.max(1 << 20)).unwrap();
            let s = MFCreateSample().unwrap();
            s.AddBuffer(&b).unwrap();
            let mut o = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(Some(s)), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
            let mut st = 0;
            let r = t.ProcessOutput(0, &mut o, &mut st);
            let s = ManuallyDrop::take(&mut o[0].pSample).unwrap();
            let _ = ManuallyDrop::take(&mut o[0].pEvents);
            if r.is_err() {
                break;
            }
            let b = s.ConvertToContiguousBuffer().unwrap();
            let mut p = core::ptr::null_mut();
            let mut len = 0;
            b.Lock(&mut p, None, Some(&mut len)).unwrap();
            es.extend_from_slice(core::slice::from_raw_parts(p, len as usize));
            b.Unlock().unwrap();
        };
        for i in 0..FRAMES {
            let f = frame_nv12(i);
            let b = MFCreateMemoryBuffer(f.len() as u32).unwrap();
            let mut p = core::ptr::null_mut();
            b.Lock(&mut p, None, None).unwrap();
            core::ptr::copy_nonoverlapping(f.as_ptr(), p, f.len());
            b.Unlock().unwrap();
            b.SetCurrentLength(f.len() as u32).unwrap();
            let s = MFCreateSample().unwrap();
            s.AddBuffer(&b).unwrap();
            s.SetSampleTime(i as i64 * 333_333).unwrap();
            s.SetSampleDuration(333_333).unwrap();
            t.ProcessInput(0, &s, 0).unwrap();
            pull(&t, &mut es);
        }
        t.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0).unwrap();
        t.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0).unwrap();
        pull(&t, &mut es);
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/gradient_640x360_24f.h265");
        std::fs::write(dir, &es).unwrap();
        eprintln!("wrote {} bytes", es.len());
    }
}
