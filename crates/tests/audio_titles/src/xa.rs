use crate::*;
use std::ffi::c_void;
use std::sync::OnceLock;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

pub const CLSID_FXEQ: Guid = Guid(0xF5E0_1117, 0xD6C4, 0x485A, [0xA3, 0xF5, 0x69, 0x51, 0x96, 0xF3, 0xDB, 0xFA]);

pub type CreateFn = unsafe extern "system" fn(*mut *mut c_void, u32, u32) -> i32;
pub type ReverbFn = unsafe extern "system" fn(*mut *mut c_void) -> i32;
pub type FxFn = unsafe extern "system" fn(*const Guid, *mut *mut c_void, *const c_void, u32) -> i32;
pub type X3dInitFn = unsafe extern "system" fn(u32, f32, *mut u8) -> i32;
pub type X3dCalcFn = unsafe extern "system" fn(*const u8, *const Listener, *const Emitter, u32, *mut Dsp);

pub struct X {
    pub create: CreateFn,
    pub reverb: ReverbFn,
    pub fx: FxFn,
    pub x3d_init: X3dInitFn,
    pub x3d_calc: X3dCalcFn,
}

pub fn xa() -> &'static X {
    static A: OnceLock<(Dll, X)> = OnceLock::new();
    &A.get_or_init(|| {
        let d = Dll::load("XAudio2_9.dll");
        let x = X { create: d.sym("XAudio2Create"), reverb: d.sym("CreateAudioReverb"), fx: d.sym("CreateFX"), x3d_init: d.sym("X3DAudioInitialize"), x3d_calc: d.sym("X3DAudioCalculate") };
        (d, x)
    })
    .1
}

pub unsafe fn slot<F: Copy>(obj: *mut c_void, i: usize) -> F {
    let vt = *(obj as *const *const usize);
    let f = *vt.add(i);
    core::mem::transmute_copy(&f)
}

#[repr(C, packed)]
pub struct WaveFormatEx {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub bytes_per_sec: u32,
    pub align: u16,
    pub bits: u16,
    pub cb: u16,
}

#[repr(C, packed)]
pub struct Buffer {
    pub flags: u32,
    pub bytes: u32,
    pub data: *const u8,
    pub play_begin: u32,
    pub play_length: u32,
    pub loop_begin: u32,
    pub loop_length: u32,
    pub loop_count: u32,
    pub context: *mut c_void,
}

#[repr(C, packed)]
#[derive(Default)]
pub struct VoiceState {
    pub context: usize,
    pub queued: u32,
    pub played: u64,
}

#[repr(C, packed)]
pub struct EffectDesc {
    pub effect: *mut c_void,
    pub initial: i32,
    pub out_channels: u32,
}

#[repr(C, packed)]
pub struct EffectChain {
    pub count: u32,
    pub descs: *const EffectDesc,
}

#[repr(C, packed)]
pub struct SendDesc {
    pub flags: u32,
    pub voice: *mut c_void,
}

#[repr(C, packed)]
pub struct Sends {
    pub count: u32,
    pub sends: *const SendDesc,
}

#[repr(C, packed)]
#[derive(Default)]
pub struct Listener {
    pub front: [f32; 3],
    pub top: [f32; 3],
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub cone: usize,
}

#[repr(C, packed)]
#[derive(Default)]
pub struct Emitter {
    pub cone: usize,
    pub front: [f32; 3],
    pub top: [f32; 3],
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub inner_radius: f32,
    pub inner_radius_angle: f32,
    pub channels: u32,
    pub channel_radius: f32,
    pub azimuths: usize,
    pub volume_curve: usize,
    pub lfe_curve: usize,
    pub lpf_direct_curve: usize,
    pub lpf_reverb_curve: usize,
    pub reverb_curve: usize,
    pub distance_scaler: f32,
    pub doppler_scaler: f32,
}

#[repr(C, packed)]
pub struct Dsp {
    pub matrix: *mut f32,
    pub delays: *mut f32,
    pub src: u32,
    pub dst: u32,
    pub lpf_direct: f32,
    pub lpf_reverb: f32,
    pub reverb: f32,
    pub doppler: f32,
    pub angle: f32,
    pub distance: f32,
    pub emitter_vel: f32,
    pub listener_vel: f32,
}

