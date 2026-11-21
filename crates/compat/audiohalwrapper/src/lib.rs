#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

pub mod backend;
pub mod opus;
pub mod silk;

use backend::{provider, E_INVALIDARG, E_MOD_NOT_FOUND, E_POINTER};
use core::ffi::c_void;
use opus::{OpusDecoderInterface, OpusEncoderInterface};
use silk::SilkInterface;
use std::sync::{Arc, Mutex};

pub const COORDINATION_SLOTS: usize = 2;
pub const COORDINATION_BUSY: i32 = 0x8923_431Fu32 as i32;
pub const OPUS_HW_FAILED_PUBLIC: i32 = 0x8923_4313u32 as i32;
pub const SILK_HW_FAILED_PUBLIC: i32 = 0x8923_431Au32 as i32;

pub enum Interface {
    OpusDecoder(OpusDecoderInterface),
    OpusEncoder(OpusEncoderInterface),
    Silk(SilkInterface),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    OpusDecoder,
    OpusEncoder,
    Silk,
}

static TABLE: Mutex<[Option<Arc<Interface>>; COORDINATION_SLOTS]> = Mutex::new([None, None]);

fn table() -> std::sync::MutexGuard<'static, [Option<Arc<Interface>>; COORDINATION_SLOTS]> {
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn create_interface(kind: Kind, out: *mut *mut c_void) -> i32 {
    let mut t = table();
    let Some(slot) = t.iter().position(|s| s.is_none()) else {
        return COORDINATION_BUSY;
    };
    if out.is_null() {
        return E_POINTER;
    }
    let p = provider();
    let available = match kind {
        Kind::OpusDecoder | Kind::OpusEncoder => p.opus_available(),
        Kind::Silk => p.silk_available(),
    };
    if !available {
        return E_MOD_NOT_FOUND;
    }
    let iface = Arc::new(match kind {
        Kind::OpusDecoder => Interface::OpusDecoder(OpusDecoderInterface::new(p)),
        Kind::OpusEncoder => Interface::OpusEncoder(OpusEncoderInterface::new(p)),
        Kind::Silk => Interface::Silk(SilkInterface::new(p)),
    });
    let raw = Arc::as_ptr(&iface) as *mut c_void;
    t[slot] = Some(iface);
    unsafe { *out = raw };
    0
}

pub fn destroy_interface(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    let taken = {
        let mut t = table();
        t.iter_mut()
            .find(|s| s.as_ref().is_some_and(|a| Arc::as_ptr(a) as *mut c_void == handle))
            .and_then(|s| s.take())
    };
    drop(taken);
}

pub fn lookup(handle: *mut c_void) -> Option<Arc<Interface>> {
    if handle.is_null() {
        return None;
    }
    table().iter().flatten().find(|a| Arc::as_ptr(a) as *mut c_void == handle).cloned()
}

pub fn live_interfaces() -> usize {
    table().iter().flatten().count()
}

fn map_opus(hr: i32) -> i32 {
    if hr == opus::HW_FAILED { OPUS_HW_FAILED_PUBLIC } else { hr }
}

fn map_silk(hr: i32) -> i32 {
    if hr == silk::HW_FAILED { SILK_HW_FAILED_PUBLIC } else { hr }
}

unsafe fn opt<'a, T>(p: *mut T) -> Option<&'a mut T> {
    if p.is_null() { None } else { Some(&mut *p) }
}

unsafe fn slice_mut<'a, T>(p: *mut T, len: usize) -> Option<&'a mut [T]> {
    if p.is_null() { None } else { Some(core::slice::from_raw_parts_mut(p, len)) }
}

unsafe fn slice<'a, T>(p: *const T, len: usize) -> Option<&'a [T]> {
    if p.is_null() {
        None
    } else if len == 0 {
        Some(&[])
    } else {
        Some(core::slice::from_raw_parts(p, len))
    }
}

fn with_opus(h: *mut c_void, f: impl FnOnce(&Interface) -> i32) -> i32 {
    match lookup(h) {
        Some(i) if !matches!(*i, Interface::Silk(_)) => map_opus(f(&i)),
        Some(_) => E_INVALIDARG,
        None => E_POINTER,
    }
}

fn with_silk(h: *mut c_void, f: impl FnOnce(&SilkInterface) -> i32) -> i32 {
    match lookup(h) {
        Some(i) => match &*i {
            Interface::Silk(s) => map_silk(f(s)),
            _ => E_INVALIDARG,
        },
        None => E_POINTER,
    }
}

fn opus_create(kind: Kind, out: *mut *mut c_void) -> i32 {
    map_opus(create_interface(kind, out))
}

#[no_mangle]
pub unsafe extern "system" fn CreateOpusDecoderHardwareInterface(out: *mut *mut c_void) -> i32 {
    opus_create(Kind::OpusDecoder, out)
}

#[no_mangle]
pub unsafe extern "system" fn CreateOpusEncoderHardwareInterface(out: *mut *mut c_void) -> i32 {
    opus_create(Kind::OpusEncoder, out)
}

#[no_mangle]
pub unsafe extern "system" fn CreateSilkHardwareInterface(out: *mut *mut c_void) -> i32 {
    map_silk(create_interface(Kind::Silk, out))
}

#[no_mangle]
pub unsafe extern "system" fn DestroyOpusDecoderHardwareInterface(h: *mut c_void) {
    destroy_interface(h)
}

#[no_mangle]
pub unsafe extern "system" fn DestroyOpusEncoderHardwareInterface(h: *mut c_void) {
    destroy_interface(h)
}

#[no_mangle]
pub unsafe extern "system" fn DestroySilkHardwareInterface(h: *mut c_void) {
    destroy_interface(h)
}

unsafe fn opus_max_count(h: *mut c_void, out: *mut u32) -> i32 {
    with_opus(h, |i| match i {
        Interface::OpusDecoder(d) => d.get_maximum_count(opt(out)),
        Interface::OpusEncoder(e) => e.get_maximum_count(opt(out)),
        Interface::Silk(_) => E_INVALIDARG,
    })
}

#[no_mangle]
pub unsafe extern "system" fn GetMaximumDecoderCount(h: *mut c_void, out: *mut u32) -> i32 {
    opus_max_count(h, out)
}

#[no_mangle]
pub unsafe extern "system" fn GetMaximumEncoderCount(h: *mut c_void, out: *mut u32) -> i32 {
    opus_max_count(h, out)
}

#[no_mangle]
pub unsafe extern "system" fn AcquireDecoder(h: *mut c_void, rate: u32, channels: u32, out_index: *mut u32) -> i32 {
    with_opus(h, |i| match i {
        Interface::OpusDecoder(d) => d.acquire(rate, channels, opt(out_index)),
        _ => E_INVALIDARG,
    })
}

#[no_mangle]
pub unsafe extern "system" fn AcquireEncoder(
    h: *mut c_void,
    rate: u32,
    channels: u32,
    bitrate: u32,
    flags: u32,
    out_index: *mut u32,
) -> i32 {
    with_opus(h, |i| match i {
        Interface::OpusEncoder(e) => e.acquire(rate, channels, bitrate, flags, opt(out_index)),
        _ => E_INVALIDARG,
    })
}

unsafe fn opus_release(h: *mut c_void, index: u32) -> i32 {
    with_opus(h, |i| match i {
        Interface::OpusDecoder(d) => d.release(index),
        Interface::OpusEncoder(e) => e.release(index),
        Interface::Silk(_) => E_INVALIDARG,
    })
}

#[no_mangle]
pub unsafe extern "system" fn ReleaseDecoder(h: *mut c_void, index: u32) -> i32 {
    opus_release(h, index)
}

#[no_mangle]
pub unsafe extern "system" fn ReleaseEncoder(h: *mut c_void, index: u32) -> i32 {
    opus_release(h, index)
}

unsafe fn opus_in_flight(h: *mut c_void, index: u32, out: *mut u8) -> i32 {
    with_opus(h, |i| match i {
        Interface::OpusDecoder(d) => d.is_data_in_flight(index, opt(out)),
        Interface::OpusEncoder(e) => e.is_data_in_flight(index, opt(out)),
        Interface::Silk(_) => E_INVALIDARG,
    })
}

#[no_mangle]
pub unsafe extern "system" fn GetIsDataInFlightForDecoder(h: *mut c_void, index: u32, out: *mut u8) -> i32 {
    opus_in_flight(h, index, out)
}

#[no_mangle]
pub unsafe extern "system" fn GetIsDataInFlightForEncoder(h: *mut c_void, index: u32, out: *mut u8) -> i32 {
    opus_in_flight(h, index, out)
}

unsafe fn opus_submit(h: *mut c_void, index: u32, data: *const u8, size: u16) -> i32 {
    with_opus(h, |i| match i {
        Interface::OpusDecoder(d) => d.decode_packet(index, slice(data, size as usize), size),
        Interface::OpusEncoder(e) => {
            let ch = (size as usize) / 2;
            e.encode_frame(index, slice(data as *const i16, ch), size)
        }
        Interface::Silk(_) => E_INVALIDARG,
    })
}

#[no_mangle]
pub unsafe extern "system" fn DecodePacket(h: *mut c_void, index: u32, packet: *const u8, size: u16) -> i32 {
    opus_submit(h, index, packet, size)
}

#[no_mangle]
pub unsafe extern "system" fn EncodeFrame(h: *mut c_void, index: u32, pcm: *const i16, bytes: u16) -> i32 {
    opus_submit(h, index, pcm as *const u8, bytes)
}

unsafe fn opus_output(h: *mut c_void, index: u32, wait: u8, ticks: *mut u32, out: *mut u8, size: *mut u16) -> i32 {
    let cap = if size.is_null() { 0 } else { *size as usize };
    with_opus(h, |i| match i {
        Interface::OpusDecoder(d) => d.get_decode_output(index, wait != 0, opt(ticks), slice_mut(out, cap), opt(size)),
        Interface::OpusEncoder(e) => e.get_encode_output(index, wait != 0, opt(ticks), slice_mut(out, cap), opt(size)),
        Interface::Silk(_) => E_INVALIDARG,
    })
}

#[no_mangle]
pub unsafe extern "system" fn GetDecodeOutput(
    h: *mut c_void,
    index: u32,
    wait: u8,
    ticks: *mut u32,
    out: *mut u8,
    size: *mut u16,
) -> i32 {
    opus_output(h, index, wait, ticks, out, size)
}

#[no_mangle]
pub unsafe extern "system" fn GetEncodeOutput(
    h: *mut c_void,
    index: u32,
    wait: u8,
    ticks: *mut u32,
    out: *mut u8,
    size: *mut u16,
) -> i32 {
    opus_output(h, index, wait, ticks, out, size)
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetMaximumDecoderCount(h: *mut c_void, out: *mut u32) -> i32 {
    with_silk(h, |s| s.get_maximum_decoder_count(opt(out)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetMaximumEncoderCount(h: *mut c_void, out: *mut u32) -> i32 {
    with_silk(h, |s| s.get_maximum_encoder_count(opt(out)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetMaximumDecoderBufferSizes(h: *mut c_void, input: *mut u32, output: *mut u32) -> i32 {
    with_silk(h, |s| s.get_maximum_decoder_buffer_sizes(opt(input), opt(output)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetMaximumEncoderBufferSizes(h: *mut c_void, input: *mut u32, output: *mut u32) -> i32 {
    with_silk(h, |s| s.get_maximum_encoder_buffer_sizes(opt(input), opt(output)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkAcquireDecoder(h: *mut c_void, out_index: *mut u32) -> i32 {
    with_silk(h, |s| s.acquire_decoder(opt(out_index)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkAcquireEncoder(h: *mut c_void, out_index: *mut u32) -> i32 {
    with_silk(h, |s| s.acquire_encoder(opt(out_index)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkReleaseDecoder(h: *mut c_void, index: u32) -> i32 {
    with_silk(h, |s| s.release_decoder(index))
}

#[no_mangle]
pub unsafe extern "system" fn SilkReleaseEncoder(h: *mut c_void, index: u32) -> i32 {
    with_silk(h, |s| s.release_encoder(index))
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetIsDecoderDataInFlight(h: *mut c_void, index: u32, out: *mut u8) -> i32 {
    with_silk(h, |s| s.is_decoder_data_in_flight(index, opt(out)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetIsEncoderDataInFlight(h: *mut c_void, index: u32, out: *mut u8) -> i32 {
    with_silk(h, |s| s.is_encoder_data_in_flight(index, opt(out)))
}

pub unsafe fn silk_decode_capacity(out_samples: *const i16) -> usize {
    if out_samples.is_null() { 0 } else { *(out_samples as *const u16) as usize / 2 }
}

pub unsafe fn silk_encode_capacity(out_bytes: *const i16) -> usize {
    if out_bytes.is_null() { 0 } else { *(out_bytes as *const u16) as usize }
}

#[no_mangle]
pub unsafe extern "system" fn SilkDecodeFrame(
    h: *mut c_void,
    index: u32,
    input: *const u8,
    input_bytes: u32,
    out: *mut i16,
    out_samples: *mut i16,
    control: *mut [i32; 6],
    sync: u8,
    all_frames: u8,
) -> i32 {
    let cap = silk_decode_capacity(out_samples);
    let in_len = (input_bytes as usize).min(silk::DECODER_INPUT_BYTES as usize + 1);
    with_silk(h, |s| {
        s.decode_frame(index, slice(input, in_len), slice_mut(out, cap), opt(out_samples), opt(control), sync != 0, all_frames != 0)
    })
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetDecodeOutput(
    h: *mut c_void,
    index: u32,
    out: *mut i16,
    out_samples: *mut i16,
    control: *mut [i32; 6],
) -> i32 {
    let cap = silk_decode_capacity(out_samples);
    with_silk(h, |s| s.get_decode_output(index, slice_mut(out, cap), opt(out_samples), opt(control)))
}

#[no_mangle]
pub unsafe extern "system" fn SilkEncodeFrame(
    h: *mut c_void,
    index: u32,
    pcm: *const i16,
    samples: i16,
    out: *mut u8,
    out_bytes: *mut i16,
    control: *mut [i32; 8],
    sync: u8,
) -> i32 {
    let cap = silk_encode_capacity(out_bytes);
    let n = (samples.max(0) as usize).min(silk::ENCODER_INPUT_BYTES as usize / 2 + 1);
    with_silk(h, |s| s.encode_frame(index, slice(pcm, n), samples, slice_mut(out, cap), opt(out_bytes), opt(control), sync != 0))
}

#[no_mangle]
pub unsafe extern "system" fn SilkGetEncodeOutput(
    h: *mut c_void,
    index: u32,
    out: *mut u8,
    out_bytes: *mut i16,
    control: *mut [i32; 8],
) -> i32 {
    let cap = silk_encode_capacity(out_bytes);
    with_silk(h, |s| s.get_encode_output(index, slice_mut(out, cap), opt(out_bytes), opt(control)))
}

#[cfg(test)]
mod tests;
