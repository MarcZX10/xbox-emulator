use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Media::MediaFoundation::*;

#[test]
fn probe_hevc_support() {
    let mut dev: Option<ID3D12Device> = None;
    unsafe { D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &mut dev) }.unwrap();
    let dev = dev.unwrap();
    let vd: ID3D12VideoDevice = windows::core::Interface::cast(&dev).unwrap();
    for (name, prof, fmt) in [("main", D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN, DXGI_FORMAT_NV12), ("main10", D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN10, DXGI_FORMAT_P010)] {
        let mut s = D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT {
            NodeIndex: 0,
            Configuration: D3D12_VIDEO_DECODE_CONFIGURATION { DecodeProfile: prof, BitstreamEncryption: D3D12_BITSTREAM_ENCRYPTION_TYPE_NONE, InterlaceType: D3D12_VIDEO_FRAME_CODED_INTERLACE_TYPE_NONE },
            Width: 640,
            Height: 360,
            DecodeFormat: fmt,
            FrameRate: windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL { Numerator: 30, Denominator: 1 },
            BitRate: 0,
            ..Default::default()
        };
        let r = unsafe { vd.CheckFeatureSupport(D3D12_FEATURE_VIDEO_DECODE_SUPPORT, &mut s as *mut _ as *mut _, core::mem::size_of_val(&s) as u32) };
        eprintln!("{name}: {r:?} support={:?} cfg={:?} tier={:?}", s.SupportFlags, s.ConfigurationFlags, s.DecodeTier);
    }
}

#[test]
fn probe_mfts() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        MFStartup(0x0002_0070, MFSTARTUP_FULL).unwrap();
        for (cat, inp, out) in [(MFT_CATEGORY_VIDEO_ENCODER, MFVideoFormat_NV12, MFVideoFormat_HEVC), (MFT_CATEGORY_VIDEO_DECODER, MFVideoFormat_HEVC, MFVideoFormat_NV12)] {
            let i = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: inp };
            let o = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: out };
            let mut acts: *mut Option<IMFActivate> = core::ptr::null_mut();
            let mut n = 0u32;
            MFTEnumEx(cat, MFT_ENUM_FLAG_ALL, Some(&i), Some(&o), &mut acts, &mut n).unwrap();
            for k in 0..n as usize {
                let a = (*acts.add(k)).as_ref().unwrap();
                let mut name = windows::core::PWSTR::null();
                let mut len = 0;
                let _ = a.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut len);
                let flags = a.GetUINT32(&MF_TRANSFORM_FLAGS_Attribute).unwrap_or(0);
                eprintln!("{:?} {} flags={flags:#x}", cat, name.to_string().unwrap_or_default());
            }
        }
    }
}
