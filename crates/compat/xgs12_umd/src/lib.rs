pub mod blob;
pub mod device;
pub mod driver_call;
pub mod host;
pub mod infoqueue;
pub mod mem;
pub mod rootsig;

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};

pub use compat_common::{E_FAIL, E_NOTIMPL, S_OK};
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const DXGI_ERROR_UNSUPPORTED: i32 = 0x887a_0004u32 as i32;

pub const M: &str = "xgs12";

pub static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

impl Guid {
    pub const fn new(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Self {
        Guid { d1, d2, d3, d4 }
    }
}

impl core::fmt::Display for Guid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let d = &self.d4;
        write!(f, "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", self.d1, self.d2, self.d3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    One,
    Scarlett,
}

impl Family {
    pub fn alloc_source(self) -> mem::Source {
        match self {
            Family::One => mem::Source::XMem,
            Family::Scarlett => mem::Source::Hooks,
        }
    }
}

static DEBUG_BITS: AtomicU64 = AtomicU64::new(0);
static DEBUG_BREAK_ENABLED: AtomicU32 = AtomicU32::new(0);
static NULL_GPU_MODE: AtomicU32 = AtomicU32::new(0);
static RESOURCE_ZERO_MEMORY: AtomicU8 = AtomicU8::new(0);

pub fn set_debug_bits(family: Family, bits: u64) {
    if family == Family::One {
        DEBUG_BITS.store(bits, Ordering::Release);
        if bits & 0x8000_0000 != 0 {
            DEBUG_BREAK_ENABLED.store(1, Ordering::Release);
        }
    }
}

pub fn debug_bits() -> (u64, u32) {
    (DEBUG_BITS.load(Ordering::Acquire), DEBUG_BREAK_ENABLED.load(Ordering::Acquire))
}

pub fn set_null_gpu_mode(mode: u32) {
    NULL_GPU_MODE.store(mode, Ordering::Release);
}

pub fn null_gpu_mode() -> u32 {
    NULL_GPU_MODE.load(Ordering::Acquire)
}

pub fn set_resource_zero_memory(v: u8) {
    RESOURCE_ZERO_MEMORY.store(v, Ordering::Release);
}

pub fn resource_zero_memory() -> u8 {
    RESOURCE_ZERO_MEMORY.load(Ordering::Acquire)
}

pub unsafe fn create_blob(family: Family, size: usize, pp: *mut *mut c_void) -> i32 {
    if pp.is_null() {
        return E_INVALIDARG;
    }
    blob::create(family.alloc_source(), size, pp)
}

#[macro_export]
macro_rules! umd_common_exports {
    ($family:expr) => {
        #[no_mangle]
        pub extern "system" fn D3D11on12_GetPresentMultiplaneOverlay() {}
        #[no_mangle]
        pub extern "system" fn D3D11on12_PresentMultiplaneOverlay() {}
        #[no_mangle]
        pub extern "system" fn D3D11on12_SetDisplayMode() {}
        #[no_mangle]
        pub extern "system" fn D3D11on12_SetPrimaryDesc() {}
        #[no_mangle]
        pub extern "system" fn OpenAdapter12(_args: *mut core::ffi::c_void) -> i32 {
            $crate::E_NOTIMPL
        }
        #[no_mangle]
        pub extern "system" fn SetDebugBits(bits: u64) {
            $crate::set_debug_bits($family, bits)
        }
        #[no_mangle]
        pub extern "system" fn SetResourceZeroMemory(v: u8) {
            $crate::set_resource_zero_memory(v)
        }
        #[no_mangle]
        pub extern "system" fn UMDSetNullGpuMode(mode: u32) {
            $crate::set_null_gpu_mode(mode)
        }
        #[no_mangle]
        pub extern "system" fn UMDPresentArray(_a: usize, _b: usize, _c: usize, _d: usize) -> i32 {
            $crate::DXGI_ERROR_UNSUPPORTED
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDInternalDriverCallX(code: u32, args: *mut u64) -> i32 {
            $crate::driver_call::call($family, code, args)
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDCreateBlob(size: usize, pp: *mut *mut core::ffi::c_void) -> i32 {
            $crate::create_blob($family, size, pp)
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDCreateDevice12(
            _ctx: *mut core::ffi::c_void,
            adapter: *mut core::ffi::c_void,
            params: *const core::ffi::c_void,
            riid: *const core::ffi::c_void,
            pp: *mut *mut core::ffi::c_void,
        ) -> i32 {
            $crate::device::create($family, adapter, params, riid, pp)
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDSerializeRootSignature(
            desc: *const core::ffi::c_void,
            version: u32,
            pp_blob: *mut *mut core::ffi::c_void,
            pp_error: *mut *mut core::ffi::c_void,
        ) -> i32 {
            $crate::rootsig::serialize(desc, version, pp_blob, pp_error)
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDSerializeVersionedRootSignature(
            desc: *const core::ffi::c_void,
            pp_blob: *mut *mut core::ffi::c_void,
            pp_error: *mut *mut core::ffi::c_void,
        ) -> i32 {
            $crate::rootsig::serialize_versioned($family.alloc_source(), desc, pp_blob, pp_error)
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDCreateRootSignatureDeserializer(
            data: *const core::ffi::c_void,
            size: usize,
            riid: *const core::ffi::c_void,
            pp: *mut *mut core::ffi::c_void,
        ) -> i32 {
            $crate::rootsig::deserializer(false, data, size, riid, pp)
        }
        #[no_mangle]
        pub unsafe extern "system" fn UMDCreateVersionedRootSignatureDeserializer(
            data: *const core::ffi::c_void,
            size: usize,
            riid: *const core::ffi::c_void,
            pp: *mut *mut core::ffi::c_void,
        ) -> i32 {
            $crate::rootsig::deserializer(true, data, size, riid, pp)
        }

        #[cfg(test)]
        mod umd_common_export_tests {
            use super::*;
            use core::ffi::c_void;

            unsafe fn vt<T: Copy>(o: *mut c_void, slot: usize) -> T {
                let v = *(o as *const *const usize);
                core::mem::transmute_copy(&*v.add(slot))
            }

            #[test]
            fn exported_entry_points() {
                let _g = $crate::serial();
                unsafe {
                    D3D11on12_GetPresentMultiplaneOverlay();
                    D3D11on12_PresentMultiplaneOverlay();
                    D3D11on12_SetDisplayMode();
                    D3D11on12_SetPrimaryDesc();
                    assert_eq!(OpenAdapter12(core::ptr::null_mut()), $crate::E_NOTIMPL);
                    assert_eq!(UMDInternalDriverCallX(0x7f, core::ptr::null_mut()), $crate::E_NOINTERFACE);
                    assert_eq!(UMDPresentArray(0, 0, 0, 0), $crate::DXGI_ERROR_UNSUPPORTED);
                    SetResourceZeroMemory(1);
                    assert_eq!($crate::resource_zero_memory(), 1);
                    UMDSetNullGpuMode(3);
                    assert_eq!($crate::null_gpu_mode(), 3);
                    let before = $crate::debug_bits();
                    SetDebugBits(0x8000_0005);
                    if $family == $crate::Family::One {
                        assert_eq!($crate::debug_bits(), (0x8000_0005, 1));
                        SetDebugBits(4);
                        assert_eq!($crate::debug_bits(), (4, 1));
                    } else {
                        assert_eq!($crate::debug_bits(), before);
                    }
                    let mut b: *mut c_void = core::ptr::null_mut();
                    assert_eq!(UMDCreateBlob(16, &mut b), $crate::S_OK);
                    let size: unsafe extern "system" fn(*mut c_void) -> usize = vt(b, 4);
                    assert_eq!(size(b), 16);
                    let rel: unsafe extern "system" fn(*mut c_void) -> u32 = vt(b, 2);
                    assert_eq!(rel(b), 0);
                    let mut d: *mut c_void = core::ptr::null_mut();
                    assert_eq!(UMDCreateDevice12(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null(), core::ptr::null(), &mut d), $crate::E_INVALIDARG);
                    let rs = [0u64, 0, 0, 0, 1];
                    let mut e: *mut c_void = core::ptr::null_mut();
                    assert_eq!(UMDSerializeRootSignature(rs.as_ptr() as *const c_void, 1, &mut b, &mut e), $crate::S_OK);
                    let ptr: unsafe extern "system" fn(*mut c_void) -> *const c_void = vt(b, 3);
                    let size: unsafe extern "system" fn(*mut c_void) -> usize = vt(b, 4);
                    let rel: unsafe extern "system" fn(*mut c_void) -> u32 = vt(b, 2);
                    let mut des: *mut c_void = core::ptr::null_mut();
                    let iid = $crate::Guid::new(0x34ab_647b, 0x3cc8, 0x46ac, [0x84, 0x1b, 0xc0, 0x96, 0x56, 0x45, 0xc0, 0x46]);
                    assert_eq!(UMDCreateRootSignatureDeserializer(ptr(b), size(b), &iid as *const _ as *const c_void, &mut des), $crate::S_OK);
                    vt::<unsafe extern "system" fn(*mut c_void) -> u32>(des, 2)(des);
                    let viid = $crate::Guid::new(0x7f91_ce67, 0x090c, 0x4bb7, [0xb7, 0x8e, 0xed, 0x8f, 0xf2, 0xe3, 0x1d, 0xa0]);
                    assert_eq!(UMDCreateVersionedRootSignatureDeserializer(ptr(b), size(b), &viid as *const _ as *const c_void, &mut des), $crate::S_OK);
                    vt::<unsafe extern "system" fn(*mut c_void) -> u32>(des, 2)(des);
                    rel(b);
                    let vrs = [2u64, 0, 0, 0, 0, 1];
                    assert_eq!(UMDSerializeVersionedRootSignature(vrs.as_ptr() as *const c_void, &mut b, &mut e), $crate::S_OK);
                    rel(b);
                    let bad = [7u64, 0, 0, 0, 0, 1];
                    assert_eq!(UMDSerializeVersionedRootSignature(bad.as_ptr() as *const c_void, &mut b, &mut e), $crate::E_FAIL);
                    assert!(!e.is_null());
                    vt::<unsafe extern "system" fn(*mut c_void) -> u32>(e, 2)(e);
                }
            }
        }
    };
}

#[macro_export]
macro_rules! umd_release_debug_interface_export {
    () => {
        #[no_mangle]
        pub extern "system" fn UMDGetDebugInterface(_riid: *const core::ffi::c_void, _pp: *mut *mut core::ffi::c_void) -> i32 {
            $crate::E_NOINTERFACE
        }

        #[cfg(test)]
        mod umd_release_debug_interface_tests {
            #[test]
            fn debug_interface_absent_in_release_driver() {
                let mut p: *mut core::ffi::c_void = 7 as *mut core::ffi::c_void;
                assert_eq!(super::UMDGetDebugInterface(core::ptr::null(), &mut p), $crate::E_NOINTERFACE);
                assert_eq!(p as usize, 7);
            }
        }
    };
}

#[macro_export]
macro_rules! umd_scarlett_exports {
    () => {
        #[no_mangle]
        pub extern "system" fn OpenDisplayAdapter1() {}
        #[no_mangle]
        pub unsafe extern "system" fn CpuMemAlloc(size: usize, align: u32) -> *mut core::ffi::c_void {
            $crate::mem::cpu_mem_alloc(size, align)
        }
        #[no_mangle]
        pub unsafe extern "system" fn CpuMemFree(p: *mut core::ffi::c_void) {
            $crate::mem::cpu_mem_free(p)
        }
        #[no_mangle]
        pub extern "system" fn GMemSetAllocationHooks(alloc: usize, free: usize) {
            $crate::mem::set_allocation_hooks(alloc, free)
        }

        #[cfg(test)]
        mod umd_scarlett_export_tests {
            use super::*;
            use core::ffi::c_void;
            use core::sync::atomic::{AtomicUsize, Ordering};

            static ALLOCS: AtomicUsize = AtomicUsize::new(0);
            static FREES: AtomicUsize = AtomicUsize::new(0);

            unsafe extern "system" fn a(size: usize, attr: u64) -> *mut c_void {
                ALLOCS.fetch_add(1, Ordering::SeqCst);
                assert!(attr == $crate::mem::ATTR_OBJECT || attr == $crate::mem::ATTR_BUFFER);
                CpuMemAlloc(size, 16)
            }

            unsafe extern "system" fn f(p: *mut c_void, _attr: u64) {
                FREES.fetch_add(1, Ordering::SeqCst);
                CpuMemFree(p)
            }

            #[test]
            fn scarlett_entry_points() {
                let _g = $crate::serial();
                unsafe {
                    OpenDisplayAdapter1();
                    let p = CpuMemAlloc(10, 256);
                    assert_eq!(p as usize % 256, 0);
                    CpuMemFree(p);
                    GMemSetAllocationHooks(a as usize, f as usize);
                    assert_eq!($crate::mem::allocation_hooks(), (a as usize, f as usize));
                    let mut b: *mut c_void = core::ptr::null_mut();
                    assert_eq!(UMDCreateBlob(8, &mut b), $crate::S_OK);
                    let v = *(b as *const *const usize);
                    let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*v.add(2));
                    assert_eq!(rel(b), 0);
                    GMemSetAllocationHooks(0, f as usize);
                    let (ha, hf) = $crate::mem::allocation_hooks();
                    assert!(ha != 0 && ha != a as usize && hf != f as usize);
                    assert!(ALLOCS.load(Ordering::SeqCst) >= 2 && FREES.load(Ordering::SeqCst) >= 2);
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_bits_follow_console_rule() {
        set_debug_bits(Family::Scarlett, 0x8000_0000);
        assert_eq!(debug_bits(), (0, 0));
        set_debug_bits(Family::One, 0x10);
        assert_eq!(debug_bits(), (0x10, 0));
        set_debug_bits(Family::One, 0x1_8000_0000);
        assert_eq!(debug_bits(), (0x1_8000_0000, 1));
        set_debug_bits(Family::One, 0);
        assert_eq!(debug_bits(), (0, 1));
    }

    #[test]
    fn stored_driver_modes() {
        set_null_gpu_mode(2);
        assert_eq!(null_gpu_mode(), 2);
        set_resource_zero_memory(1);
        assert_eq!(resource_zero_memory(), 1);
        set_resource_zero_memory(0);
        assert_eq!(resource_zero_memory(), 0);
    }

    #[test]
    fn create_blob_rejects_null_out() {
        unsafe {
            assert_eq!(create_blob(Family::One, 4, core::ptr::null_mut()), E_INVALIDARG);
        }
    }
}
