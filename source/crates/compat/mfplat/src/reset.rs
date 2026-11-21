use core::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

pub const IID_IUNKNOWN: Guid = Guid { d1: 0x0000_0000, d2: 0x0000, d3: 0x0000, d4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46] };
pub const IID_ID3D12DEVICE: Guid = Guid { d1: 0x1898_19f1, d2: 0x1db6, d3: 0x4b57, d4: [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7] };
pub const IID_ID3D12DEVICE9: Guid = Guid { d1: 0x4c80_e962, d2: 0xf032, d3: 0x4f60, d4: [0xbc, 0x9e, 0xeb, 0xc2, 0xcf, 0xa1, 0xd8, 0x3c] };
pub const IID_ID3D12DEVICE10: Guid = Guid { d1: 0x517f_8718, d2: 0xaa66, d3: 0x49f9, d4: [0xb0, 0x2b, 0xa7, 0xab, 0x89, 0xc0, 0x60, 0x31] };
pub const IID_ID3D12DEVICE11: Guid = Guid { d1: 0x5405_c344, d2: 0xd457, d3: 0x444e, d4: [0xb4, 0xdd, 0x23, 0x66, 0xe4, 0x5a, 0xee, 0x39] };

pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

const SLOT_QI: usize = 0;
const SLOT_RELEASE: usize = 2;
const SLOT_RESET_DEVICE: usize = 7;

type QiFn = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
type ResetFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> i32;

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    unsafe { *(*(obj as *const *const usize)).add(i) }
}

pub unsafe fn query(obj: *mut c_void, iid: &Guid) -> Result<*mut c_void, i32> {
    let f: QiFn = unsafe { core::mem::transmute(slot(obj, SLOT_QI)) };
    let mut out: *mut c_void = core::ptr::null_mut();
    let hr = unsafe { f(obj, iid, &mut out) };
    if hr < 0 || out.is_null() {
        return Err(if hr < 0 { hr } else { E_NOINTERFACE });
    }
    Ok(out)
}

pub unsafe fn release(obj: *mut c_void) -> u32 {
    let f: ReleaseFn = unsafe { core::mem::transmute(slot(obj, SLOT_RELEASE)) };
    unsafe { f(obj) }
}

pub struct Unwrapped {
    pub device: *mut c_void,
    pub via: &'static str,
}

pub unsafe fn unwrap_device(device: *mut c_void) -> Result<Unwrapped, i32> {
    if device.is_null() {
        return Err(E_POINTER);
    }
    for (iid, via) in [(IID_ID3D12DEVICE9, "ID3D12Device9"), (IID_ID3D12DEVICE10, "ID3D12Device10"), (IID_ID3D12DEVICE11, "ID3D12Device11")] {
        if let Ok(higher) = unsafe { query(device, &iid) } {
            let base = unsafe { query(higher, &IID_ID3D12DEVICE) };
            unsafe { release(higher) };
            if let Ok(real) = base {
                return Ok(Unwrapped { device: real, via });
            }
        }
    }
    let real = unsafe { query(device, &IID_ID3D12DEVICE) }.map_err(|_| E_INVALIDARG)?;
    Ok(Unwrapped { device: real, via: "ID3D12Device (no higher interface)" })
}

pub struct ResetOutcome {
    pub hr: i32,
    pub real_device: usize,
    pub via: &'static str,
}

pub unsafe fn reset_device_manager(manager: *mut c_void, device: *mut c_void, token: u32) -> ResetOutcome {
    if manager.is_null() || device.is_null() {
        return ResetOutcome { hr: E_POINTER, real_device: 0, via: "null argument" };
    }
    let u = match unsafe { unwrap_device(device) } {
        Ok(u) => u,
        Err(hr) => return ResetOutcome { hr, real_device: 0, via: "device is not an ID3D12Device" },
    };
    let reset: ResetFn = unsafe { core::mem::transmute(slot(manager, SLOT_RESET_DEVICE)) };
    let hr = unsafe { reset(manager, u.device, token) };
    let real = u.device as usize;
    unsafe { release(u.device) };
    ResetOutcome { hr, real_device: real, via: u.via }
}
