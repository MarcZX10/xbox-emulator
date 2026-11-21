use crate::com::*;
use crate::imports;
use crate::messaging::*;
use crate::sys::*;
use core::ffi::c_void;
use core::ptr::null_mut;

pub const XVMCTRL_DEVICE: &str = "\\\\.\\XVmCtrl";
pub const IOCTL_XVMCTRL_DEBUG_ACTION: u32 = 0x0015_0284;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct DebugActionRequest {
    pub flags: u64,
    pub version: u64,
    pub correlation: GUID,
    pub reserved: u64,
}

pub fn request_for(flags: u64, correlation: GUID) -> DebugActionRequest {
    DebugActionRequest { flags: flags & 7, version: 4, correlation, reserved: 0 }
}

pub unsafe fn send_vm_debug_action(flags: u64, correlation: GUID) -> HRESULT {
    let name = wz(XVMCTRL_DEVICE);
    let h = CreateFileW(name.as_ptr(), 0xC000_0000, 3, null_mut(), 3, 0x80, null_mut());
    let mut hr = S_OK;
    if h == INVALID_HANDLE_VALUE {
        hr = hr_last_error();
        if hr < 0 {
            return hr;
        }
    }
    let req = request_for(flags, correlation);
    if DeviceIoControl(h, IOCTL_XVMCTRL_DEBUG_ACTION, &req as *const DebugActionRequest as *const c_void, 0x28, null_mut(), 0, null_mut(), null_mut()) == 0 {
        hr = hr_last_error();
    }
    if h != INVALID_HANDLE_VALUE {
        CloseHandle(h);
    }
    hr
}

pub unsafe fn get_correlation_guid(o: Obj, out: *mut GUID) -> HRESULT {
    if o.is_null() {
        return E_INVALIDARG;
    }
    let mut s = Bstr::null();
    let hr = obj_get_string(o, wz("CorrelationGuid").as_ptr(), s.out());
    if hr == E_JSON_NOT_FOUND || hr < 0 {
        return hr;
    }
    let mut g: Vec<u16> = Vec::new();
    g.push(b'{' as u16);
    g.extend_from_slice(wslice(s.0));
    g.push(b'}' as u16);
    g.push(0);
    let f = Bstr(SysAllocString(g.as_ptr()));
    if f.0.is_null() {
        return E_OUTOFMEMORY;
    }
    IIDFromString(f.0, out)
}

pub fn vm_flags(flags: u64) -> (u32, u64) {
    let mut handled = 0u32;
    let mut vm = 0u64;
    if flags & 1 != 0 {
        handled = 1;
    }
    if flags & 2 != 0 {
        handled |= 2;
        vm |= 2;
    }
    if flags & 4 != 0 {
        handled |= 4;
        vm |= 4;
    }
    if flags & 8 != 0 {
        handled |= 8;
        vm |= 1;
    }
    (handled, vm)
}

pub unsafe extern "system" fn debug_action_handler(_session: usize, request: usize, reply: usize) -> HRESULT {
    let mut reply_body = Ptr::null();
    let mut body = Ptr::null();
    let mut correlation = GUID::default();
    let mut hr;
    'done: {
        let i = match imports::get() {
            Ok(i) => i,
            Err(e) => {
                hr = e;
                break 'done;
            }
        };
        hr = (i.create_json_object)(reply_body.out());
        if hr < 0 {
            break 'done;
        }
        hr = deserialize_body(request as *mut c_void, body.out());
        if hr < 0 {
            break 'done;
        }
        let mut d = 0f64;
        hr = obj_get_number(body.0, wz("Flags").as_ptr(), &mut d);
        if hr < 0 {
            break 'done;
        }
        hr = get_correlation_guid(body.0, &mut correlation);
        if hr == E_JSON_NOT_FOUND {
            hr = S_OK;
        }
        let flags = cvtt_i64(d) as u64;
        let (handled, mut vm) = vm_flags(flags);
        if flags & 1 != 0 {
            core::arch::asm!("int3");
        }
        if vm != 0 {
            hr = send_vm_debug_action(vm, correlation);
            if hr == STATUS_VM_PARTITION_UNAVAILABLE {
                vm &= !4;
                if vm == 0 {
                    hr = E_TITLE_VM_NOT_AVAILABLE;
                    break 'done;
                }
                hr = send_vm_debug_action(vm, correlation);
            }
            if hr < 0 {
                break 'done;
            }
        }
        if flags as u32 != handled {
            hr = E_NOTIMPL;
        }
    }
    complete_operation_handler(reply as *mut c_void, reply_body.0, &mut hr);
    S_OK
}
