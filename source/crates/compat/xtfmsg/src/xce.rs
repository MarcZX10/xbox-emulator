use crate::sys::*;
use crate::tlg::*;
use crate::xbtp;
use core::ffi::c_void;

#[repr(C)]
pub struct XceEventData {
    pub extension_id: GUID,
    pub session_id: GUID,
    pub operation_id: u32,
}

pub unsafe fn get_current_event_data(out: *mut *mut c_void) -> HRESULT {
    crate::xtfchk!("xbox\\devex\\xtf\\xce\\xtfxcehelper.cpp", "XceGetCurrentEventData", 164, "XceInitialize()", S_OK, "");
    let hr = xbtp::get_current_tls_event_data(out);
    crate::xtfchk!("xbox\\devex\\xtf\\xce\\xtfxcehelper.cpp", "XceGetCurrentEventData", 165, "XbtpGetCurrentTlsEventData(ppData)", hr, "");
    hr
}

pub unsafe fn write_operation_result(version: *const u16, ext: &GUID, session: &GUID, op: u32, task_name: *const u16, task_value: *const u16, hr: HRESULT) {
    let p = &CORE_DEV_TOOLS;
    if !p.enabled(OPERATION_RESULT.level, OPERATION_RESULT.keyword) {
        return;
    }
    let tags = PRIV_TAGS;
    let h = hr as u32;
    let d = [d_u64(&tags), d_wstr(version), d_guid(ext), d_guid(session), d_u32(&op), d_wstr(task_name), d_wstr(task_value), d_u32(&h)];
    p.write(&OPERATION_RESULT, core::ptr::null(), core::ptr::null(), &d);
}

pub unsafe fn log_operation_result(version: *const u16, task_name: *const u16, task_value: *const u16, hr: HRESULT) -> HRESULT {
    let mut data: *mut c_void = core::ptr::null_mut();
    let r = get_current_event_data(&mut data);
    crate::xtfchk!("xbox\\devex\\xtf\\xce\\xtfxcehelper.cpp", "XceLogOperationResult", 26, "XceGetCurrentEventData(&pEventData)", r, "");
    if r >= 0 {
        let e = &*(data as *const XceEventData);
        write_operation_result(version, &e.extension_id, &e.session_id, e.operation_id, task_name, task_value, hr);
    }
    r
}
