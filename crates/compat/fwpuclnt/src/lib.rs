#![allow(non_snake_case)]

use core::ffi::c_void;
use std::sync::Mutex;

pub const FWP_E_NULL_POINTER: u32 = 0x8032_001C;
pub const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const RPC_X_NULL_REF_POINTER: u32 = 1780;
pub const NO_INDEX: u8 = 0xFF;
const EXCEPTION_NONCONTINUABLE: u32 = 1;
const BITS: u8 = 0x40;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const FWPM_CONDITION_IP_LOCAL_ADDRESS: Guid = Guid { data1: 0xd9ee00de, data2: 0xc1ef, data3: 0x4617, data4: [0xbf, 0xe3, 0xff, 0xd8, 0xf5, 0xa0, 0x89, 0x57] };
pub const FWPM_CONDITION_IP_LOCAL_PORT: Guid = Guid { data1: 0x0c1ba1af, data2: 0x5765, data3: 0x453f, data4: [0xaf, 0x22, 0xa8, 0xf7, 0x91, 0xac, 0x77, 0x5b] };
pub const FWPM_CONDITION_IP_REMOTE_ADDRESS: Guid = Guid { data1: 0xb235ae9a, data2: 0x1d64, data3: 0x49b8, data4: [0xa4, 0x4c, 0x5f, 0xf3, 0xd9, 0x09, 0x50, 0x45] };
pub const FWPM_CONDITION_IP_REMOTE_PORT: Guid = Guid { data1: 0xc35a604d, data2: 0xd22b, data3: 0x4e1a, data4: [0x91, 0xb4, 0x68, 0xf6, 0x74, 0xee, 0x67, 0x4b] };

const MAPS: [Guid; 4] = [FWPM_CONDITION_IP_LOCAL_ADDRESS, FWPM_CONDITION_IP_LOCAL_PORT, FWPM_CONDITION_IP_REMOTE_ADDRESS, FWPM_CONDITION_IP_REMOTE_PORT];

static BITMAPS: Mutex<[u64; 4]> = Mutex::new([0; 4]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Return(u32),
    Raise(u32),
}

#[link(name = "kernel32")]
extern "system" {
    fn RaiseException(code: u32, flags: u32, nargs: u32, args: *const usize);
}

fn map_slot(map: &Guid) -> Option<usize> {
    MAPS.iter().position(|m| m == map)
}

fn bitmaps() -> std::sync::MutexGuard<'static, [u64; 4]> {
    BITMAPS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn service_get(map: &Guid) -> u8 {
    let mut maps = bitmaps();
    let Some(slot) = map_slot(map) else { return NO_INDEX };
    let bits = maps[slot];
    if bits == u64::MAX {
        return NO_INDEX;
    }
    let index = (!bits).trailing_zeros() as u8;
    maps[slot] = bits | (1u64 << index);
    index
}

pub fn service_free(map: &Guid, index: u8) -> u32 {
    if index >= BITS {
        return STATUS_INVALID_PARAMETER;
    }
    let mut maps = bitmaps();
    match map_slot(map) {
        Some(slot) => {
            maps[slot] &= !(1u64 << index);
            0
        }
        None => STATUS_INVALID_PARAMETER,
    }
}

pub unsafe fn bitmap_index_get(engine: *mut c_void, map: *const Guid, index: *mut u8) -> Outcome {
    if engine.is_null() {
        return Outcome::Return(FWP_E_NULL_POINTER);
    }
    if map.is_null() || index.is_null() {
        return Outcome::Raise(RPC_X_NULL_REF_POINTER);
    }
    *index = service_get(&*map);
    Outcome::Return(0)
}

pub unsafe fn bitmap_index_free(engine: *mut c_void, map: *const Guid, index: *const u8) -> Outcome {
    if engine.is_null() {
        return Outcome::Return(FWP_E_NULL_POINTER);
    }
    if map.is_null() || index.is_null() {
        return Outcome::Raise(RPC_X_NULL_REF_POINTER);
    }
    match service_free(&*map, *index) {
        0 => Outcome::Return(0),
        status => Outcome::Raise(status),
    }
}

unsafe fn deliver(o: Outcome) -> u32 {
    match o {
        Outcome::Return(code) => code,
        Outcome::Raise(code) => {
            RaiseException(code, EXCEPTION_NONCONTINUABLE, 0, core::ptr::null());
            code
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn FwpmBitmapIndexGet0(engine: *mut c_void, map: *const Guid, index: *mut u8) -> u32 {
    deliver(bitmap_index_get(engine, map, index))
}

#[no_mangle]
pub unsafe extern "system" fn FwpmBitmapIndexFree0(engine: *mut c_void, map: *const Guid, index: *mut u8) -> u32 {
    deliver(bitmap_index_free(engine, map, index))
}

#[cfg(test)]
mod tests;
