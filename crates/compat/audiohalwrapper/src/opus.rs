use crate::backend::{CodecProvider, OpusDecoderCodec, OpusEncoderCodec, E_INVALIDARG, E_POINTER};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

pub const HW_FAILED: i32 = 0x8aca_0001u32 as i32;
pub const BUSY: i32 = 0x8aca_0002u32 as i32;
pub const BAD_INDEX: i32 = 0x8aca_0003u32 as i32;
pub const NO_FREE_SLOT: i32 = 0x8aca_0004u32 as i32;
pub const INPUT_TOO_LARGE: i32 = 0x8aca_0007u32 as i32;
pub const BUFFER_TOO_SMALL: i32 = 0x8aca_0008u32 as i32;
pub const NO_OUTPUT: i32 = 0x8aca_0009u32 as i32;
pub const S_FALSE: i32 = 1;

pub const DECODER_SLOTS: usize = 15;
pub const ENCODER_SLOTS: usize = 3;
pub const MAX_PACKET_BYTES: usize = 0x5e0;
pub const MAX_PCM_BYTES: usize = 0x2d00;
pub const MAX_PCM_SAMPLES: usize = MAX_PCM_BYTES;
pub const ENCODER_OUTPUT_CAPACITY: usize = 0x2f0 - 8;

pub const ST_FREE: u32 = 0;
pub const ST_INIT: u32 = 1;
pub const ST_INIT_QUEUED: u32 = 2;
pub const ST_READY: u32 = 3;
pub const ST_BUSY: u32 = 4;
pub const ST_DONE: u32 = 5;
pub const ST_INIT_FAILED: u32 = 6;
pub const ST_INIT_FAILED_QUEUED: u32 = 7;

fn in_flight(state: u32) -> Option<bool> {
    match state {
        ST_INIT | ST_READY | ST_INIT_FAILED => Some(false),
        ST_INIT_QUEUED | ST_BUSY | ST_DONE | ST_INIT_FAILED_QUEUED => Some(true),
        _ => None,
    }
}

fn collectable(state: u32) -> bool {
    state <= 7 && (0xb4u32 >> state) & 1 == 1
}

fn valid_rate(rate: u32) -> bool {
    matches!(rate, 16000 | 24000 | 48000)
}

fn ticks_since(t: Instant) -> u32 {
    t.elapsed().as_micros().min(u32::MAX as u128) as u32
}

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct DecSlot {
    state: u32,
    codec: Option<Box<dyn OpusDecoderCodec>>,
    status: i32,
    ticks: u32,
    pcm: Vec<i16>,
    len: usize,
}

impl DecSlot {
    fn empty() -> DecSlot {
        DecSlot { state: ST_FREE, codec: None, status: 0, ticks: 0, pcm: Vec::new(), len: 0 }
    }
}

pub struct OpusDecoderInterface {
    provider: Arc<dyn CodecProvider>,
    slots: Mutex<Vec<DecSlot>>,
}

impl OpusDecoderInterface {
    pub fn new(provider: Arc<dyn CodecProvider>) -> OpusDecoderInterface {
        OpusDecoderInterface { provider, slots: Mutex::new((0..DECODER_SLOTS).map(|_| DecSlot::empty()).collect()) }
    }

    pub fn get_maximum_count(&self, out: Option<&mut u32>) -> i32 {
        match out {
            Some(o) => {
                *o = DECODER_SLOTS as u32;
                0
            }
            None => E_POINTER,
        }
    }

    pub fn acquire(&self, rate: u32, channels: u32, out_index: Option<&mut u32>) -> i32 {
        if !valid_rate(rate) || !(1..=2).contains(&channels) {
            return E_INVALIDARG;
        }
        let mut slots = guard(&self.slots);
        let Some(i) = slots.iter().position(|s| s.state == ST_FREE) else {
            return NO_FREE_SLOT;
        };
        let Some(out_index) = out_index else {
            return E_POINTER;
        };
        let s = &mut slots[i];
        *s = DecSlot::empty();
        let t = Instant::now();
        match self.provider.opus_decoder(rate, channels) {
            Ok(c) => {
                s.codec = Some(c);
                s.state = ST_READY;
            }
            Err(e) => {
                s.status = e;
                s.state = ST_INIT_FAILED;
            }
        }
        s.ticks = ticks_since(t);
        s.pcm = vec![0i16; MAX_PCM_SAMPLES];
        *out_index = i as u32;
        0
    }

    pub fn release(&self, index: u32) -> i32 {
        let mut slots = guard(&self.slots);
        match slots.get_mut(index as usize) {
            Some(s) if s.state != ST_FREE => {
                *s = DecSlot::empty();
                0
            }
            _ => BAD_INDEX,
        }
    }

    pub fn is_data_in_flight(&self, index: u32, out: Option<&mut u8>) -> i32 {
        let Some(out) = out else {
            return E_POINTER;
        };
        let slots = guard(&self.slots);
        let Some(s) = slots.get(index as usize).filter(|s| s.state != ST_FREE) else {
            return BAD_INDEX;
        };
        match in_flight(s.state) {
            Some(b) => {
                *out = b as u8;
                0
            }
            None => HW_FAILED,
        }
    }

    pub fn decode_packet(&self, index: u32, packet: Option<&[u8]>, size: u16) -> i32 {
        if size as usize > MAX_PACKET_BYTES {
            return INPUT_TOO_LARGE;
        }
        let mut slots = guard(&self.slots);
        let Some(s) = slots.get_mut(index as usize).filter(|s| s.state != ST_FREE) else {
            return BAD_INDEX;
        };
        match s.state {
            ST_READY => {}
            ST_INIT_FAILED => {
                s.state = ST_DONE;
                return 0;
            }
            _ => return BUSY,
        }
        let data: &[u8] = match packet {
            Some(p) if p.len() >= size as usize => &p[..size as usize],
            None if size == 0 => &[],
            _ => return E_POINTER,
        };
        let t = Instant::now();
        let r = match s.codec.as_mut() {
            Some(c) => c.decode(data, &mut s.pcm),
            None => Err(HW_FAILED),
        };
        match r {
            Ok(n) => {
                s.len = n.min(s.pcm.len());
                s.status = 0;
            }
            Err(e) => {
                s.len = 0;
                s.status = e;
            }
        }
        s.ticks = ticks_since(t);
        s.state = ST_DONE;
        0
    }

    pub fn get_decode_output(
        &self,
        index: u32,
        _wait: bool,
        ticks: Option<&mut u32>,
        out: Option<&mut [u8]>,
        size: Option<&mut u16>,
    ) -> i32 {
        let (Some(out), Some(size)) = (out, size) else {
            return E_POINTER;
        };
        let mut slots = guard(&self.slots);
        let Some(s) = slots.get_mut(index as usize).filter(|s| s.state != ST_FREE) else {
            return BAD_INDEX;
        };
        if !collectable(s.state) {
            return NO_OUTPUT;
        }
        if s.state != ST_DONE {
            *size = 0;
            return S_FALSE;
        }
        s.state = ST_READY;
        if let Some(t) = ticks {
            *t = s.ticks;
        }
        match s.status {
            0 => {
                let need = s.len * 2;
                if need > u16::MAX as usize || (*size as usize) < need || out.len() < need {
                    *size = need.min(u16::MAX as usize) as u16;
                    return BUFFER_TOO_SMALL;
                }
                for (i, v) in s.pcm[..s.len].iter().enumerate() {
                    out[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
                }
                *size = need as u16;
                0
            }
            1 => {
                *size = 0;
                0
            }
            e => e,
        }
    }
}

struct EncSlot {
    state: u32,
    codec: Option<Box<dyn OpusEncoderCodec>>,
    channels: usize,
    status: i32,
    ticks: u32,
    out: Vec<u8>,
    len: usize,
}

impl EncSlot {
    fn empty() -> EncSlot {
        EncSlot { state: ST_FREE, codec: None, channels: 1, status: 0, ticks: 0, out: Vec::new(), len: 0 }
    }
}

pub struct OpusEncoderInterface {
    provider: Arc<dyn CodecProvider>,
    slots: Mutex<Vec<EncSlot>>,
}

impl OpusEncoderInterface {
    pub fn new(provider: Arc<dyn CodecProvider>) -> OpusEncoderInterface {
        OpusEncoderInterface { provider, slots: Mutex::new((0..ENCODER_SLOTS).map(|_| EncSlot::empty()).collect()) }
    }

    pub fn get_maximum_count(&self, out: Option<&mut u32>) -> i32 {
        match out {
            Some(o) => {
                *o = ENCODER_SLOTS as u32;
                0
            }
            None => E_POINTER,
        }
    }

    pub fn acquire(&self, rate: u32, channels: u32, bitrate: u32, flags: u32, out_index: Option<&mut u32>) -> i32 {
        if !valid_rate(rate) || !(1..=2).contains(&channels) || !(1..=510_000).contains(&bitrate) || flags > 1 {
            return E_INVALIDARG;
        }
        let mut slots = guard(&self.slots);
        let Some(i) = slots.iter().position(|s| s.state == ST_FREE) else {
            return NO_FREE_SLOT;
        };
        let Some(out_index) = out_index else {
            return E_POINTER;
        };
        let s = &mut slots[i];
        *s = EncSlot::empty();
        s.channels = channels as usize;
        let t = Instant::now();
        match self.provider.opus_encoder(rate, channels, bitrate, flags) {
            Ok(c) => {
                s.codec = Some(c);
                s.state = ST_READY;
            }
            Err(e) => {
                s.status = e;
                s.state = ST_INIT_FAILED;
            }
        }
        s.ticks = ticks_since(t);
        s.out = vec![0u8; ENCODER_OUTPUT_CAPACITY];
        *out_index = i as u32;
        0
    }

    pub fn release(&self, index: u32) -> i32 {
        let mut slots = guard(&self.slots);
        match slots.get_mut(index as usize) {
            Some(s) if s.state != ST_FREE => {
                *s = EncSlot::empty();
                0
            }
            _ => BAD_INDEX,
        }
    }

    pub fn is_data_in_flight(&self, index: u32, out: Option<&mut u8>) -> i32 {
        let Some(out) = out else {
            return E_POINTER;
        };
        let slots = guard(&self.slots);
        let Some(s) = slots.get(index as usize).filter(|s| s.state != ST_FREE) else {
            return BAD_INDEX;
        };
        match in_flight(s.state) {
            Some(b) => {
                *out = b as u8;
                0
            }
            None => HW_FAILED,
        }
    }

    pub fn encode_frame(&self, index: u32, pcm: Option<&[i16]>, bytes: u16) -> i32 {
        let mut slots = guard(&self.slots);
        let Some(s) = slots.get_mut(index as usize).filter(|s| s.state != ST_FREE) else {
            return BAD_INDEX;
        };
        let Some(pcm) = pcm else {
            return E_POINTER;
        };
        if bytes as usize > MAX_PCM_BYTES {
            return INPUT_TOO_LARGE;
        }
        match s.state {
            ST_READY => {}
            ST_INIT_FAILED => {
                s.state = ST_DONE;
                return 0;
            }
            _ => return BUSY,
        }
        let frames = bytes as usize / s.channels / 2;
        let count = (frames * s.channels).min(pcm.len());
        let t = Instant::now();
        let r = match s.codec.as_mut() {
            Some(c) => c.encode(&pcm[..count], &mut s.out),
            None => Err(HW_FAILED),
        };
        match r {
            Ok(n) => {
                s.len = n.min(s.out.len());
                s.status = 0;
            }
            Err(e) => {
                s.len = 0;
                s.status = e;
            }
        }
        s.ticks = ticks_since(t);
        s.state = ST_DONE;
        0
    }

    pub fn get_encode_output(
        &self,
        index: u32,
        _wait: bool,
        ticks: Option<&mut u32>,
        out: Option<&mut [u8]>,
        size: Option<&mut u16>,
    ) -> i32 {
        let (Some(out), Some(size)) = (out, size) else {
            return E_POINTER;
        };
        let mut slots = guard(&self.slots);
        let Some(s) = slots.get_mut(index as usize).filter(|s| s.state != ST_FREE) else {
            return BAD_INDEX;
        };
        if !collectable(s.state) {
            return NO_OUTPUT;
        }
        if s.state != ST_DONE {
            *size = 0;
            return S_FALSE;
        }
        s.state = ST_READY;
        if let Some(t) = ticks {
            *t = s.ticks;
        }
        match s.status {
            0 => {
                let need = s.len;
                if (*size as usize) < need || out.len() < need {
                    *size = need as u16;
                    return BUFFER_TOO_SMALL;
                }
                out[..need].copy_from_slice(&s.out[..need]);
                *size = need as u16;
                0
            }
            1 => {
                *size = 0;
                0
            }
            e => e,
        }
    }
}
