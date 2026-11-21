use crate::backend::{CodecProvider, SilkDecoderCodec, SilkEncoderCodec, E_POINTER};
use std::sync::{Arc, Mutex, MutexGuard};

pub const HW_FAILED: i32 = 0x8ac9_0001u32 as i32;
pub const BUSY: i32 = 0x8ac9_0002u32 as i32;
pub const BAD_INDEX: i32 = 0x8ac9_0003u32 as i32;
pub const NO_FREE_SLOT: i32 = 0x8ac9_0004u32 as i32;
pub const SYNC_AFTER_ASYNC: i32 = 0x8ac9_0006u32 as i32;
pub const INPUT_TOO_LARGE: i32 = 0x8ac9_0007u32 as i32;
pub const BUFFER_TOO_SMALL: i32 = 0x8ac9_0008u32 as i32;
pub const NO_OUTPUT: i32 = 0x8ac9_0009u32 as i32;
pub const S_FALSE: i32 = 1;

pub const DECODER_SLOTS: usize = 7;
pub const ENCODER_SLOTS: usize = 2;
pub const DECODER_INPUT_BYTES: u32 = 0xf00;
pub const DECODER_OUTPUT_BYTES: u32 = 0x2580;
pub const ENCODER_INPUT_BYTES: u32 = 0x2580;
pub const ENCODER_OUTPUT_BYTES: u32 = 0x500;

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct DecSlot {
    acquired: bool,
    in_flight: bool,
    done: bool,
    status: i32,
    codec: Option<Box<dyn SilkDecoderCodec>>,
    control: [i32; 6],
    pcm: Vec<i16>,
    len: usize,
}

impl DecSlot {
    fn empty() -> DecSlot {
        DecSlot { acquired: false, in_flight: false, done: false, status: 0, codec: None, control: [0; 6], pcm: Vec::new(), len: 0 }
    }
}

struct EncSlot {
    acquired: bool,
    in_flight: bool,
    done: bool,
    status: i32,
    codec: Option<Box<dyn SilkEncoderCodec>>,
    control: [i32; 8],
    out: Vec<u8>,
    len: usize,
}

impl EncSlot {
    fn empty() -> EncSlot {
        EncSlot { acquired: false, in_flight: false, done: false, status: 0, codec: None, control: [0; 8], out: Vec::new(), len: 0 }
    }
}

struct Inner {
    dec: Vec<DecSlot>,
    enc: Vec<EncSlot>,
    hw_failed: bool,
    async_used: bool,
}

pub struct SilkInterface {
    provider: Arc<dyn CodecProvider>,
    inner: Mutex<Inner>,
}

impl SilkInterface {
    pub fn new(provider: Arc<dyn CodecProvider>) -> SilkInterface {
        SilkInterface {
            provider,
            inner: Mutex::new(Inner {
                dec: (0..DECODER_SLOTS).map(|_| DecSlot::empty()).collect(),
                enc: (0..ENCODER_SLOTS).map(|_| EncSlot::empty()).collect(),
                hw_failed: false,
                async_used: false,
            }),
        }
    }

    pub fn get_maximum_decoder_count(&self, out: Option<&mut u32>) -> i32 {
        match out {
            Some(o) => {
                *o = DECODER_SLOTS as u32;
                0
            }
            None => E_POINTER,
        }
    }

    pub fn get_maximum_encoder_count(&self, out: Option<&mut u32>) -> i32 {
        match out {
            Some(o) => {
                *o = ENCODER_SLOTS as u32;
                0
            }
            None => E_POINTER,
        }
    }

    pub fn get_maximum_decoder_buffer_sizes(&self, input: Option<&mut u32>, output: Option<&mut u32>) -> i32 {
        match (input, output) {
            (Some(i), Some(o)) => {
                *i = DECODER_INPUT_BYTES;
                *o = DECODER_OUTPUT_BYTES;
                0
            }
            _ => E_POINTER,
        }
    }

    pub fn get_maximum_encoder_buffer_sizes(&self, input: Option<&mut u32>, output: Option<&mut u32>) -> i32 {
        match (input, output) {
            (Some(i), Some(o)) => {
                *i = ENCODER_INPUT_BYTES;
                *o = ENCODER_OUTPUT_BYTES;
                0
            }
            _ => E_POINTER,
        }
    }

    pub fn acquire_decoder(&self, out_index: Option<&mut u32>) -> i32 {
        let Some(out_index) = out_index else {
            return E_POINTER;
        };
        let mut g = guard(&self.inner);
        if g.hw_failed {
            return HW_FAILED;
        }
        let Some(i) = g.dec.iter().position(|s| !s.acquired) else {
            return NO_FREE_SLOT;
        };
        let codec = match self.provider.silk_decoder() {
            Ok(c) => c,
            Err(e) => return e,
        };
        let s = &mut g.dec[i];
        *s = DecSlot::empty();
        s.acquired = true;
        s.codec = Some(codec);
        s.pcm = vec![0i16; DECODER_OUTPUT_BYTES as usize / 2];
        *out_index = i as u32;
        0
    }

    pub fn acquire_encoder(&self, out_index: Option<&mut u32>) -> i32 {
        let Some(out_index) = out_index else {
            return E_POINTER;
        };
        let mut g = guard(&self.inner);
        if g.hw_failed {
            return HW_FAILED;
        }
        let Some(i) = g.enc.iter().position(|s| !s.acquired) else {
            return NO_FREE_SLOT;
        };
        let codec = match self.provider.silk_encoder() {
            Ok(c) => c,
            Err(e) => return e,
        };
        let s = &mut g.enc[i];
        *s = EncSlot::empty();
        s.acquired = true;
        s.codec = Some(codec);
        s.out = vec![0u8; ENCODER_OUTPUT_BYTES as usize];
        *out_index = i as u32;
        0
    }

    pub fn release_decoder(&self, index: u32) -> i32 {
        let mut g = guard(&self.inner);
        let failed = g.hw_failed;
        let Some(s) = g.dec.get_mut(index as usize).filter(|s| s.acquired) else {
            return BAD_INDEX;
        };
        if failed {
            return HW_FAILED;
        }
        if s.in_flight || s.done {
            return BUSY;
        }
        *s = DecSlot::empty();
        0
    }

    pub fn release_encoder(&self, index: u32) -> i32 {
        let mut g = guard(&self.inner);
        let failed = g.hw_failed;
        let Some(s) = g.enc.get_mut(index as usize).filter(|s| s.acquired) else {
            return BAD_INDEX;
        };
        if failed {
            return HW_FAILED;
        }
        if s.in_flight || s.done {
            return BUSY;
        }
        *s = EncSlot::empty();
        0
    }

    pub fn is_decoder_data_in_flight(&self, index: u32, out: Option<&mut u8>) -> i32 {
        let Some(out) = out else {
            return E_POINTER;
        };
        let g = guard(&self.inner);
        match g.dec.get(index as usize).filter(|s| s.acquired) {
            Some(s) => {
                *out = s.in_flight as u8;
                0
            }
            None => BAD_INDEX,
        }
    }

    pub fn is_encoder_data_in_flight(&self, index: u32, out: Option<&mut u8>) -> i32 {
        let Some(out) = out else {
            return E_POINTER;
        };
        let g = guard(&self.inner);
        match g.enc.get(index as usize).filter(|s| s.acquired) {
            Some(s) => {
                *out = s.in_flight as u8;
                0
            }
            None => BAD_INDEX,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decode_frame(
        &self,
        index: u32,
        input: Option<&[u8]>,
        out: Option<&mut [i16]>,
        out_samples: Option<&mut i16>,
        control: Option<&mut [i32; 6]>,
        sync: bool,
        all_frames: bool,
    ) -> i32 {
        let (Some(input), Some(control)) = (input, control) else {
            return E_POINTER;
        };
        let mut g = guard(&self.inner);
        if !g.dec.get(index as usize).is_some_and(|s| s.acquired) {
            return BAD_INDEX;
        }
        if g.hw_failed {
            return HW_FAILED;
        }
        let i = index as usize;
        if sync {
            if out.is_none() || out_samples.is_none() {
                return E_POINTER;
            }
            if g.async_used {
                return SYNC_AFTER_ASYNC;
            }
        } else {
            g.async_used = true;
            if g.dec[i].in_flight {
                return BUSY;
            }
        }
        if input.len() > DECODER_INPUT_BYTES as usize {
            return INPUT_TOO_LARGE;
        }
        let s = &mut g.dec[i];
        let mut ctl = *control;
        let r = match s.codec.as_mut() {
            Some(c) => c.decode(&mut ctl, all_frames, input, &mut s.pcm),
            None => Err(HW_FAILED),
        };
        let (status, len) = match r {
            Ok(n) => (0, n.min(s.pcm.len())),
            Err(e) => (e, 0),
        };
        if sync {
            if status < 0 {
                return status;
            }
            let (Some(out), Some(out_samples)) = (out, out_samples) else {
                return E_POINTER;
            };
            let cap = *out_samples as u16 as usize;
            if cap < len || out.len() < len {
                return BUFFER_TOO_SMALL;
            }
            out[..len].copy_from_slice(&s.pcm[..len]);
            *control = ctl;
            *out_samples = len as i16;
            return status;
        }
        s.in_flight = true;
        s.done = true;
        s.status = status;
        s.len = len;
        s.control = ctl;
        0
    }

    pub fn get_decode_output(
        &self,
        index: u32,
        out: Option<&mut [i16]>,
        out_samples: Option<&mut i16>,
        control: Option<&mut [i32; 6]>,
    ) -> i32 {
        let (Some(out), Some(out_samples), Some(control)) = (out, out_samples, control) else {
            return E_POINTER;
        };
        let mut g = guard(&self.inner);
        let failed = g.hw_failed;
        let Some(s) = g.dec.get_mut(index as usize).filter(|s| s.acquired) else {
            return BAD_INDEX;
        };
        if failed {
            return HW_FAILED;
        }
        if !s.done {
            return if s.in_flight { S_FALSE } else { NO_OUTPUT };
        }
        s.in_flight = false;
        s.done = false;
        if s.status < 0 {
            return s.status;
        }
        let cap = *out_samples as u16 as usize;
        if cap < s.len || out.len() < s.len {
            return BUFFER_TOO_SMALL;
        }
        out[..s.len].copy_from_slice(&s.pcm[..s.len]);
        *control = s.control;
        *out_samples = s.len as i16;
        s.status
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode_frame(
        &self,
        index: u32,
        pcm: Option<&[i16]>,
        samples: i16,
        out: Option<&mut [u8]>,
        out_bytes: Option<&mut i16>,
        control: Option<&mut [i32; 8]>,
        sync: bool,
    ) -> i32 {
        let (Some(pcm), Some(control)) = (pcm, control) else {
            return E_POINTER;
        };
        let mut g = guard(&self.inner);
        if !g.enc.get(index as usize).is_some_and(|s| s.acquired) {
            return BAD_INDEX;
        }
        if g.hw_failed {
            return HW_FAILED;
        }
        let i = index as usize;
        if sync {
            if out.is_none() || out_bytes.is_none() {
                return E_POINTER;
            }
            if g.async_used {
                return SYNC_AFTER_ASYNC;
            }
        } else {
            g.async_used = true;
            if g.enc[i].in_flight {
                return BUSY;
            }
        }
        let bytes = (samples as i32 * 2) as u32;
        if bytes > ENCODER_INPUT_BYTES {
            return INPUT_TOO_LARGE;
        }
        let n = (samples.max(0) as usize).min(pcm.len());
        let s = &mut g.enc[i];
        let mut ctl = *control;
        let r = match s.codec.as_mut() {
            Some(c) => c.encode(&mut ctl, &pcm[..n], &mut s.out),
            None => Err(HW_FAILED),
        };
        let (status, len) = match r {
            Ok(n) => (0, n.min(s.out.len())),
            Err(e) => (e, 0),
        };
        if sync {
            if status < 0 {
                return status;
            }
            let (Some(out), Some(out_bytes)) = (out, out_bytes) else {
                return E_POINTER;
            };
            let cap = *out_bytes as u16 as usize;
            if cap < len || out.len() < len {
                return BUFFER_TOO_SMALL;
            }
            out[..len].copy_from_slice(&s.out[..len]);
            *control = ctl;
            *out_bytes = len as i16;
            return status;
        }
        s.in_flight = true;
        s.done = true;
        s.status = status;
        s.len = len;
        s.control = ctl;
        0
    }

    pub fn get_encode_output(
        &self,
        index: u32,
        out: Option<&mut [u8]>,
        out_bytes: Option<&mut i16>,
        control: Option<&mut [i32; 8]>,
    ) -> i32 {
        let (Some(out), Some(out_bytes), Some(control)) = (out, out_bytes, control) else {
            return E_POINTER;
        };
        let mut g = guard(&self.inner);
        let failed = g.hw_failed;
        let Some(s) = g.enc.get_mut(index as usize).filter(|s| s.acquired) else {
            return BAD_INDEX;
        };
        if failed {
            return HW_FAILED;
        }
        if !s.done {
            return if s.in_flight { S_FALSE } else { NO_OUTPUT };
        }
        s.in_flight = false;
        s.done = false;
        if s.status < 0 {
            return s.status;
        }
        let cap = *out_bytes as u16 as usize;
        if cap < s.len || out.len() < s.len {
            return BUFFER_TOO_SMALL;
        }
        out[..s.len].copy_from_slice(&s.out[..s.len]);
        *control = s.control;
        *out_bytes = s.len as i16;
        s.status
    }
}
