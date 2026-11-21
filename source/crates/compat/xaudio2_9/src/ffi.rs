use core::ffi::c_void;

pub type HRESULT = i32;

pub const S_OK: HRESULT = 0;
pub const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as i32;
pub const E_POINTER: HRESULT = 0x8000_4003u32 as i32;
pub const E_OUTOFMEMORY: HRESULT = 0x8007_000Eu32 as i32;
pub const XAUDIO2_E_INVALID_CALL: HRESULT = 0x8896_0001u32 as i32;

pub const XAUDIO2_DO_NOT_USE_SHAPE: u32 = 0x0400;
pub const XAUDIO2_VOICE_NOSHAPE: u32 = 0x0400;
pub const XAUDIO2_CREATE_VALID_FLAGS: u32 = 0xAC01;
pub const XAUDIO2_SOURCE_VALID_FLAGS: u32 = 0x040E;
pub const XAUDIO2_END_OF_STREAM: u32 = 0x0040;
pub const XAUDIO2_CONSOLE_DEFAULT_PROCESSOR: u32 = 0x20;

pub const WAVE_FORMAT_PCM: u16 = 1;
pub const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
pub const WAVE_FORMAT_XMA2: u16 = 0x0166;

pub const XMA_BYTES_PER_PACKET: u32 = 2048;
pub const XMA_READBUFFER_MAX_BYTES: u32 = 0x7F_F800;
pub const XMA_SAMPLES_PER_SUBFRAME: u32 = 128;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const IID_IUNKNOWN: GUID = GUID { data1: 0x0000_0000, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
pub const IID_IXAUDIO2: GUID = GUID { data1: 0x2B02_E3CF, data2: 0x2E0B, data3: 0x4EC3, data4: [0xBE, 0x45, 0x1B, 0x2A, 0x3F, 0xE7, 0x21, 0x0D] };
pub const IID_IXAUDIO2_EXTENSION: GUID = GUID { data1: 0x84AC_29BB, data2: 0xD619, data3: 0x44D2, data4: [0xB1, 0x97, 0xE4, 0xAC, 0xF7, 0xDF, 0x3E, 0xD6] };
pub const KSDATAFORMAT_SUBTYPE_PCM: GUID = GUID { data1: 0x0000_0001, data2: 0x0000, data3: 0x0010, data4: [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71] };
pub const CLSID_CONSOLE_LOUDNESS_METER: GUID = GUID { data1: 0x54E3_4CC6, data2: 0x934C, data3: 0x4DCC, data4: [0xB5, 0x3D, 0x85, 0xC2, 0x08, 0x86, 0x44, 0xD3] };

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct WAVEFORMATEX {
    pub w_format_tag: u16,
    pub n_channels: u16,
    pub n_samples_per_sec: u32,
    pub n_avg_bytes_per_sec: u32,
    pub n_block_align: u16,
    pub w_bits_per_sample: u16,
    pub cb_size: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct WAVEFORMATEXTENSIBLE {
    pub format: WAVEFORMATEX,
    pub w_valid_bits_per_sample: u16,
    pub dw_channel_mask: u32,
    pub sub_format: GUID,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct XMA2WAVEFORMATEX {
    pub wfx: WAVEFORMATEX,
    pub num_streams: u16,
    pub channel_mask: u32,
    pub samples_encoded: u32,
    pub bytes_per_block: u32,
    pub play_begin: u32,
    pub play_length: u32,
    pub loop_begin: u32,
    pub loop_length: u32,
    pub loop_count: u8,
    pub encoder_version: u8,
    pub block_count: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct XAUDIO2_BUFFER {
    pub flags: u32,
    pub audio_bytes: u32,
    pub p_audio_data: *const u8,
    pub play_begin: u32,
    pub play_length: u32,
    pub loop_begin: u32,
    pub loop_length: u32,
    pub loop_count: u32,
    pub p_context: *mut c_void,
}

impl Default for XAUDIO2_BUFFER {
    fn default() -> Self {
        XAUDIO2_BUFFER {
            flags: 0,
            audio_bytes: 0,
            p_audio_data: core::ptr::null(),
            play_begin: 0,
            play_length: 0,
            loop_begin: 0,
            loop_length: 0,
            loop_count: 0,
            p_context: core::ptr::null_mut(),
        }
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct XAUDIO2_VOICE_STATE {
    pub p_current_buffer_context: *mut c_void,
    pub buffers_queued: u32,
    pub samples_played: u64,
}

impl Default for XAUDIO2_VOICE_STATE {
    fn default() -> Self {
        XAUDIO2_VOICE_STATE { p_current_buffer_context: core::ptr::null_mut(), buffers_queued: 0, samples_played: 0 }
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct XAUDIO2_VOICE_DETAILS {
    pub creation_flags: u32,
    pub active_flags: u32,
    pub input_channels: u32,
    pub input_sample_rate: u32,
}

pub type Fp = *const c_void;

#[repr(C)]
pub struct Unknown {
    pub vtbl: *const Fp,
}

pub unsafe fn slot(obj: *mut c_void, i: usize) -> Fp {
    *(*(obj as *mut Unknown)).vtbl.add(i)
}

pub mod xa {
    pub const QUERY_INTERFACE: usize = 0;
    pub const ADD_REF: usize = 1;
    pub const RELEASE: usize = 2;
    pub const REGISTER_FOR_CALLBACKS: usize = 3;
    pub const UNREGISTER_FOR_CALLBACKS: usize = 4;
    pub const CREATE_SOURCE_VOICE: usize = 5;
    pub const CREATE_SUBMIX_VOICE: usize = 6;
    pub const CREATE_MASTERING_VOICE: usize = 7;
    pub const START_ENGINE: usize = 8;
    pub const STOP_ENGINE: usize = 9;
    pub const COMMIT_CHANGES: usize = 10;
    pub const GET_PERFORMANCE_DATA: usize = 11;
    pub const SET_DEBUG_CONFIGURATION: usize = 12;
    pub const COUNT: usize = 13;
}

pub mod ext {
    pub const GET_PROCESSING_QUANTUM: usize = 3;
    pub const GET_PROCESSOR: usize = 4;
}

pub mod sv {
    pub const GET_VOICE_DETAILS: usize = 0;
    pub const SET_OUTPUT_VOICES: usize = 1;
    pub const SET_EFFECT_CHAIN: usize = 2;
    pub const ENABLE_EFFECT: usize = 3;
    pub const DISABLE_EFFECT: usize = 4;
    pub const GET_EFFECT_STATE: usize = 5;
    pub const SET_EFFECT_PARAMETERS: usize = 6;
    pub const GET_EFFECT_PARAMETERS: usize = 7;
    pub const SET_FILTER_PARAMETERS: usize = 8;
    pub const GET_FILTER_PARAMETERS: usize = 9;
    pub const SET_OUTPUT_FILTER_PARAMETERS: usize = 10;
    pub const GET_OUTPUT_FILTER_PARAMETERS: usize = 11;
    pub const SET_VOLUME: usize = 12;
    pub const GET_VOLUME: usize = 13;
    pub const SET_CHANNEL_VOLUMES: usize = 14;
    pub const GET_CHANNEL_VOLUMES: usize = 15;
    pub const SET_OUTPUT_MATRIX: usize = 16;
    pub const GET_OUTPUT_MATRIX: usize = 17;
    pub const DESTROY_VOICE: usize = 18;
    pub const START: usize = 19;
    pub const STOP: usize = 20;
    pub const SUBMIT_SOURCE_BUFFER: usize = 21;
    pub const FLUSH_SOURCE_BUFFERS: usize = 22;
    pub const DISCONTINUITY: usize = 23;
    pub const EXIT_LOOP: usize = 24;
    pub const GET_STATE: usize = 25;
    pub const SET_FREQUENCY_RATIO: usize = 26;
    pub const GET_FREQUENCY_RATIO: usize = 27;
    pub const SET_SOURCE_SAMPLE_RATE: usize = 28;
    pub const COUNT: usize = 29;
}

pub mod cb {
    pub const ON_VOICE_PROCESSING_PASS_START: usize = 0;
    pub const ON_VOICE_PROCESSING_PASS_END: usize = 1;
    pub const ON_STREAM_END: usize = 2;
    pub const ON_BUFFER_START: usize = 3;
    pub const ON_BUFFER_END: usize = 4;
    pub const ON_LOOP_END: usize = 5;
    pub const ON_VOICE_ERROR: usize = 6;
    pub const COUNT: usize = 7;
}
