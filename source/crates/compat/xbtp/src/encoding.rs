use crate::base::log_error_args;
use crate::sys::{E_INVALIDARG, GUID, HRESULT, HR_INSUFFICIENT_BUFFER, S_OK};

pub const E_XBTP_MEMCPY: HRESULT = 0x8898_040Au32 as i32;

pub const XBTP_MESSAGE_TYPE_DISCONNECT: u16 = 0xFFFF;
pub const XBTP_MESSAGE_TYPE_CREATE_SESSION_REQUEST: u16 = 0xFFFE;
pub const XBTP_MESSAGE_TYPE_CREATE_SESSION_RESPONSE: u16 = 0xFFFD;
pub const XBTP_MESSAGE_TYPE_CLOSE_SESSION: u16 = 0xFFFC;

pub const FIXED_CONNECT_REQUEST_BYTE_SIZE: u32 = 8;
pub const FIXED_CONNECT_RESPONSE_BYTE_SIZE: u32 = 8;
pub const FIXED_MESSAGE_HEADER_BYTE_SIZE: u32 = 8;
pub const FIXED_SESSION_REQUEST_BYTE_SIZE: u32 = 0x28;
pub const FIXED_SESSION_RESPONSE_BYTE_SIZE: u32 = 0x20;
pub const FIXED_DISCONNECT_MESSAGE_BYTE_SIZE: u32 = 0xC;
pub const FIXED_CLOSE_SESSION_MESSAGE_BYTE_SIZE: u32 = 0x18;
pub const FIXED_EXTENSION_MESSAGE_BYTE_SIZE: u32 = 0x28;
pub const FIXED_EXTENSION_BODY_BYTE_SIZE: u32 = 0x20;

const TOO_SMALL: &[u8] = b"Received buffer is too small %d - expected %d\0";
const MEMCPY_ERROR: &[u8] = b"Memory copy error %d\0";

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UTF8Text {
    pub length: u32,
    pub text: *mut u8,
}

impl Default for UTF8Text {
    fn default() -> Self {
        UTF8Text { length: 0, text: core::ptr::null_mut() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XBTP_MESSAGE_HEADER {
    pub signature: u16,
    pub message_type: u16,
    pub message_length: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_CONNECT_REQUEST {
    pub signature: u16,
    pub protocol_version: u16,
    pub agent: UTF8Text,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_CONNECT_RESPONSE {
    pub status: i32,
    pub agent: UTF8Text,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_CREATE_SESSION_REQUEST_MESSAGE {
    pub header: XBTP_MESSAGE_HEADER,
    pub session_id: GUID,
    pub extension_id: GUID,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_CREATE_SESSION_RESPONSE_MESSAGE {
    pub header: XBTP_MESSAGE_HEADER,
    pub session_id: GUID,
    pub status: i32,
    pub location: UTF8Text,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XBTP_MESSAGE_BODY {
    pub json_body_length: u32,
    pub binary_body_length: u32,
    pub json_body: *mut u8,
    pub binary_body: *mut u8,
}

impl Default for XBTP_MESSAGE_BODY {
    fn default() -> Self {
        XBTP_MESSAGE_BODY { json_body_length: 0, binary_body_length: 0, json_body: core::ptr::null_mut(), binary_body: core::ptr::null_mut() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_BEGIN_MESSAGE {
    pub header: XBTP_MESSAGE_HEADER,
    pub session_id: GUID,
    pub exchange_id: i32,
    pub operation_id: u32,
    pub body: XBTP_MESSAGE_BODY,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_CLOSE_SESSION_MESSAGE {
    pub header: XBTP_MESSAGE_HEADER,
    pub session_id: GUID,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct XBTP_DISCONNECT_MESSAGE {
    pub header: XBTP_MESSAGE_HEADER,
    pub status: i32,
}

extern "C" {
    fn _errno() -> *mut i32;
    fn _invalid_parameter_noinfo();
}

unsafe fn crt_fail(code: i32) -> i32 {
    *_errno() = code;
    _invalid_parameter_noinfo();
    code
}

unsafe fn memcpy_s(dst: *mut u8, dst_size: u64, src: *const u8, count: u64) -> i32 {
    if count == 0 {
        return 0;
    }
    if dst.is_null() {
        return crt_fail(22);
    }
    if !src.is_null() && dst_size >= count {
        core::ptr::copy(src, dst, count as usize);
        return 0;
    }
    core::ptr::write_bytes(dst, 0, dst_size as usize);
    if src.is_null() {
        return crt_fail(22);
    }
    if dst_size >= count {
        return 22;
    }
    crt_fail(34)
}

unsafe fn copy_result(err: i32) -> HRESULT {
    if err != 0 {
        log_error_args(MEMCPY_ERROR, &[err as u32 as usize]);
        E_XBTP_MEMCPY
    } else {
        S_OK
    }
}

unsafe fn write_data(value: *const u8, size: u32, buffer_length: u32, offset: *mut u32, buffer: *mut u8) -> HRESULT {
    let off = *offset;
    let pad = off & (size - 1);
    let pos = off.wrapping_add(pad);
    let err = memcpy_s(buffer.wrapping_add(pos as usize), buffer_length.wrapping_sub(pos) as u64, value, size as u64);
    *offset = off.wrapping_add(pad + size);
    copy_result(err)
}

unsafe fn read_data(buffer_length: u32, buffer: *const u8, offset: *mut u32, out: *mut u8, size: u32) -> HRESULT {
    let off = *offset;
    let pad = off & (size - 1);
    let pos = off.wrapping_add(pad);
    let mut err = 0;
    if pos.wrapping_add(size) <= buffer_length {
        err = memcpy_s(out, size as u64, buffer.wrapping_add(pos as usize), size as u64);
    }
    *offset = off.wrapping_add(pad + size);
    copy_result(err)
}

unsafe fn write_u16(v: u16, buffer_length: u32, offset: *mut u32, buffer: *mut u8) -> HRESULT {
    write_data(&v as *const u16 as *const u8, 2, buffer_length, offset, buffer)
}

unsafe fn write_u32(v: u32, buffer_length: u32, offset: *mut u32, buffer: *mut u8) -> HRESULT {
    write_data(&v as *const u32 as *const u8, 4, buffer_length, offset, buffer)
}

unsafe fn write_buffer(length: u32, data: *const u8, buffer_length: u32, offset: *mut u32, buffer: *mut u8) -> HRESULT {
    let off = *offset;
    let err = memcpy_s(buffer.wrapping_add(off as usize), buffer_length.wrapping_sub(off) as u64, data, length as u64);
    if err != 0 {
        log_error_args(MEMCPY_ERROR, &[err as u32 as usize]);
        return E_XBTP_MEMCPY;
    }
    *offset = off.wrapping_add(length);
    S_OK
}

unsafe fn read_u16(buffer_length: u32, buffer: *const u8, offset: *mut u32, out: *mut u16) -> HRESULT {
    read_data(buffer_length, buffer, offset, out as *mut u8, 2)
}

unsafe fn read_u32(buffer_length: u32, buffer: *const u8, offset: *mut u32, out: *mut u32) -> HRESULT {
    read_data(buffer_length, buffer, offset, out as *mut u8, 4)
}

unsafe fn write_header(signature: u16, message_type: u16, message_length: u32, buffer_length: u32, offset: *mut u32, buffer: *mut u8) -> HRESULT {
    let hr = write_u16(signature, buffer_length, offset, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u16(message_type, buffer_length, offset, buffer);
    if hr < 0 {
        return hr;
    }
    write_u32(message_length, buffer_length, offset, buffer)
}

unsafe fn write_guid(g: *const GUID, buffer_length: u32, offset: *mut u32, buffer: *mut u8) -> HRESULT {
    let p = g as *const u8;
    let hr = write_data(p, 4, buffer_length, offset, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_data(p.add(4), 2, buffer_length, offset, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_data(p.add(6), 2, buffer_length, offset, buffer);
    if hr < 0 {
        return hr;
    }
    let off = *offset;
    let err = memcpy_s(buffer.wrapping_add(off as usize), buffer_length.wrapping_sub(off) as u64, p.add(8), 8);
    if err != 0 {
        log_error_args(MEMCPY_ERROR, &[err as u32 as usize]);
        return E_XBTP_MEMCPY;
    }
    *offset = off.wrapping_add(8);
    S_OK
}

unsafe fn read_header(buffer_length: u32, buffer: *const u8, offset: *mut u32, header: *mut XBTP_MESSAGE_HEADER) -> HRESULT {
    let hr = read_u16(buffer_length, buffer, offset, &mut (*header).signature);
    if hr < 0 {
        return hr;
    }
    let hr = read_u16(buffer_length, buffer, offset, &mut (*header).message_type);
    if hr < 0 {
        return hr;
    }
    read_u32(buffer_length, buffer, offset, &mut (*header).message_length)
}

unsafe fn read_guid(buffer_length: u32, buffer: *const u8, offset: *mut u32, g: *mut GUID) -> HRESULT {
    let p = g as *mut u8;
    let hr = read_data(buffer_length, buffer, offset, p, 4);
    if hr < 0 {
        return hr;
    }
    let hr = read_data(buffer_length, buffer, offset, p.add(4), 2);
    if hr < 0 {
        return hr;
    }
    let hr = read_data(buffer_length, buffer, offset, p.add(6), 2);
    if hr < 0 {
        return hr;
    }
    let mut guid_offset = 0u32;
    let hr = write_buffer(8, buffer.wrapping_add(*offset as usize), 8, &mut guid_offset, p.add(8));
    if hr < 0 {
        return hr;
    }
    *offset = (*offset).wrapping_add(guid_offset);
    S_OK
}

unsafe fn check_valid_length(expected: u32, buffer_length: u32) -> HRESULT {
    if buffer_length < expected {
        log_error_args(TOO_SMALL, &[buffer_length as usize, expected as usize]);
        HR_INSUFFICIENT_BUFFER
    } else {
        S_OK
    }
}

unsafe fn size_check(buffer_length: u32, complete: u32, required: *mut u32) -> HRESULT {
    let hr = check_valid_length(complete, buffer_length);
    *required = complete;
    hr
}

#[export_name = "?SerializeConnectRequest@@YAJFFPEAUUTF8Text@@KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeConnectRequest(signature: i16, version: i16, agent: *mut UTF8Text, buffer_length: u32, required: *mut u32, buffer: *mut u8) -> HRESULT {
    if (buffer_length != 0 && buffer.is_null()) || required.is_null() || agent.is_null() || ((*agent).length != 0 && (*agent).text.is_null()) {
        return E_INVALIDARG;
    }
    let hr = size_check(buffer_length, (*agent).length.wrapping_add(8), required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = write_u16(signature as u16, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u16(version as u16, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32((*agent).length, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_buffer((*agent).length, (*agent).text, buffer_length, &mut off, buffer)
}

#[export_name = "?DeserializeFixedConnectRequest@@YAJKPEADPEAUXBTP_CONNECT_REQUEST@@@Z"]
pub unsafe extern "C" fn DeserializeFixedConnectRequest(buffer_length: u32, buffer: *mut u8, request: *mut XBTP_CONNECT_REQUEST) -> HRESULT {
    if buffer.is_null() || request.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length(FIXED_CONNECT_REQUEST_BYTE_SIZE, buffer_length);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = read_u16(buffer_length, buffer, &mut off, &mut (*request).signature);
    if hr < 0 {
        return hr;
    }
    let hr = read_u16(buffer_length, buffer, &mut off, &mut (*request).protocol_version);
    if hr < 0 {
        return hr;
    }
    read_u32(buffer_length, buffer, &mut off, &mut (*request).agent.length)
}

#[export_name = "?DeserializeConnectRequest@@YAJKPEADPEAUXBTP_CONNECT_REQUEST@@@Z"]
pub unsafe extern "C" fn DeserializeConnectRequest(buffer_length: u32, buffer: *mut u8, request: *mut XBTP_CONNECT_REQUEST) -> HRESULT {
    let hr = DeserializeFixedConnectRequest(buffer_length, buffer, request);
    if hr < 0 {
        return hr;
    }
    let remaining = buffer_length - 8;
    let len = (*request).agent.length;
    if remaining < len {
        log_error_args(TOO_SMALL, &[remaining as usize, len as usize]);
        return HR_INSUFFICIENT_BUFFER;
    }
    attach_text(buffer_length, 8, buffer, &mut (*request).agent);
    S_OK
}

unsafe fn attach_text(buffer_length: u32, fixed: u32, buffer: *mut u8, text: *mut UTF8Text) {
    if buffer_length > fixed {
        let p = buffer.add(fixed as usize);
        (*text).text = p;
        let n = (*text).length;
        if n != 0 {
            *p.add((n - 1) as usize) = 0;
        }
    } else {
        (*text).length = 0;
        (*text).text = core::ptr::null_mut();
    }
}

#[export_name = "?SerializeConnectResponse@@YAJW4XBTP_STATUS_CODE@@PEAUUTF8Text@@KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeConnectResponse(status: i32, agent: *mut UTF8Text, buffer_length: u32, required: *mut u32, buffer: *mut u8) -> HRESULT {
    if (buffer_length != 0 && buffer.is_null()) || required.is_null() || agent.is_null() || ((*agent).length != 0 && (*agent).text.is_null()) {
        return E_INVALIDARG;
    }
    let hr = size_check(buffer_length, (*agent).length.wrapping_add(8), required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = write_u32(status as u32, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32((*agent).length, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_buffer((*agent).length, (*agent).text, buffer_length, &mut off, buffer)
}

#[export_name = "?DeserializeFixedConnectResponse@@YAJKPEADPEAUXBTP_CONNECT_RESPONSE@@@Z"]
pub unsafe extern "C" fn DeserializeFixedConnectResponse(buffer_length: u32, buffer: *mut u8, response: *mut XBTP_CONNECT_RESPONSE) -> HRESULT {
    if buffer.is_null() || response.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length(FIXED_CONNECT_RESPONSE_BYTE_SIZE, buffer_length);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = read_u32(buffer_length, buffer, &mut off, &mut (*response).status as *mut i32 as *mut u32);
    if hr < 0 {
        return hr;
    }
    read_u32(buffer_length, buffer, &mut off, &mut (*response).agent.length)
}

#[export_name = "?DeserializeConnectResponse@@YAJKPEADPEAUXBTP_CONNECT_RESPONSE@@@Z"]
pub unsafe extern "C" fn DeserializeConnectResponse(buffer_length: u32, buffer: *mut u8, response: *mut XBTP_CONNECT_RESPONSE) -> HRESULT {
    let hr = DeserializeFixedConnectResponse(buffer_length, buffer, response);
    if hr < 0 {
        return hr;
    }
    let remaining = buffer_length - 8;
    let len = (*response).agent.length;
    if remaining < len {
        log_error_args(TOO_SMALL, &[remaining as usize, len as usize]);
        return HR_INSUFFICIENT_BUFFER;
    }
    attach_text(buffer_length, 8, buffer, &mut (*response).agent);
    S_OK
}

#[export_name = "?SerializeCreateSessionRequest@@YAJFAEBU_GUID@@0KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeCreateSessionRequest(signature: i16, session_id: *const GUID, extension_id: *const GUID, buffer_length: u32, required: *mut u32, buffer: *mut u8) -> HRESULT {
    if (buffer_length != 0 && buffer.is_null()) || required.is_null() {
        return E_INVALIDARG;
    }
    let hr = size_check(buffer_length, FIXED_SESSION_REQUEST_BYTE_SIZE, required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = write_header(signature as u16, XBTP_MESSAGE_TYPE_CREATE_SESSION_REQUEST, FIXED_SESSION_REQUEST_BYTE_SIZE - FIXED_MESSAGE_HEADER_BYTE_SIZE, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_guid(session_id, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_guid(extension_id, buffer_length, &mut off, buffer)
}

#[export_name = "?DeserializeCreateSessionRequest@@YAJKPEADPEAUXBTP_CREATE_SESSION_REQUEST_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeCreateSessionRequest(buffer_length: u32, buffer: *mut u8, request: *mut XBTP_CREATE_SESSION_REQUEST_MESSAGE) -> HRESULT {
    if buffer.is_null() || request.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length(FIXED_SESSION_REQUEST_BYTE_SIZE, buffer_length);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = read_header(buffer_length, buffer, &mut off, &mut (*request).header);
    if hr < 0 {
        return hr;
    }
    let hr = read_guid(buffer_length, buffer, &mut off, &mut (*request).session_id);
    if hr < 0 {
        return hr;
    }
    read_guid(buffer_length, buffer, &mut off, &mut (*request).extension_id)
}

#[export_name = "?SerializeCreateSessionResponse@@YAJFAEBU_GUID@@W4XBTP_STATUS_CODE@@PEAUUTF8Text@@KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeCreateSessionResponse(signature: i16, session_id: *const GUID, status: i32, location: *mut UTF8Text, buffer_length: u32, required: *mut u32, buffer: *mut u8) -> HRESULT {
    if location.is_null() || (buffer_length != 0 && buffer.is_null()) || required.is_null() {
        return E_INVALIDARG;
    }
    let complete = (*location).length.wrapping_add(FIXED_SESSION_RESPONSE_BYTE_SIZE);
    let hr = size_check(buffer_length, complete, required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = write_header(signature as u16, XBTP_MESSAGE_TYPE_CREATE_SESSION_RESPONSE, complete.wrapping_sub(FIXED_MESSAGE_HEADER_BYTE_SIZE), buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_guid(session_id, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32(status as u32, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32((*location).length, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_buffer((*location).length, (*location).text, buffer_length, &mut off, buffer)
}

#[export_name = "?DeserializeFixedCreateSessionResponse@@YAJKPEADPEAUXBTP_CREATE_SESSION_RESPONSE_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeFixedCreateSessionResponse(buffer_length: u32, buffer: *mut u8, response: *mut XBTP_CREATE_SESSION_RESPONSE_MESSAGE) -> HRESULT {
    if buffer.is_null() || response.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length(FIXED_SESSION_RESPONSE_BYTE_SIZE, buffer_length);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = read_header(buffer_length, buffer, &mut off, &mut (*response).header);
    if hr < 0 {
        return hr;
    }
    let hr = read_guid(buffer_length, buffer, &mut off, &mut (*response).session_id);
    if hr < 0 {
        return hr;
    }
    let hr = read_u32(buffer_length, buffer, &mut off, &mut (*response).status as *mut i32 as *mut u32);
    if hr < 0 {
        return hr;
    }
    read_u32(buffer_length, buffer, &mut off, &mut (*response).location.length)
}

#[export_name = "?DeserializeCreateSessionResponse@@YAJKPEADPEAUXBTP_CREATE_SESSION_RESPONSE_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeCreateSessionResponse(buffer_length: u32, buffer: *mut u8, response: *mut XBTP_CREATE_SESSION_RESPONSE_MESSAGE) -> HRESULT {
    let hr = DeserializeFixedCreateSessionResponse(buffer_length, buffer, response);
    if hr < 0 {
        return hr;
    }
    let hr = check_valid_length((*response).location.length.wrapping_add(FIXED_SESSION_RESPONSE_BYTE_SIZE), buffer_length);
    if hr < 0 {
        return hr;
    }
    attach_text(buffer_length, FIXED_SESSION_RESPONSE_BYTE_SIZE, buffer, &mut (*response).location);
    S_OK
}

#[export_name = "?SerializeDisconnect@@YAJFW4XBTP_STATUS_CODE@@KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeDisconnect(signature: i16, status: i32, buffer_length: u32, required: *mut u32, buffer: *mut u8) -> HRESULT {
    if (buffer_length != 0 && buffer.is_null()) || required.is_null() {
        return E_INVALIDARG;
    }
    let hr = size_check(buffer_length, FIXED_DISCONNECT_MESSAGE_BYTE_SIZE, required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = write_header(signature as u16, XBTP_MESSAGE_TYPE_DISCONNECT, FIXED_DISCONNECT_MESSAGE_BYTE_SIZE - FIXED_MESSAGE_HEADER_BYTE_SIZE, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_u32(status as u32, buffer_length, &mut off, buffer)
}

#[export_name = "?SerializeCloseSession@@YAJFAEBU_GUID@@KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeCloseSession(signature: i16, session_id: *const GUID, buffer_length: u32, required: *mut u32, buffer: *mut u8) -> HRESULT {
    if (buffer_length != 0 && buffer.is_null()) || required.is_null() {
        return E_INVALIDARG;
    }
    let hr = size_check(buffer_length, FIXED_CLOSE_SESSION_MESSAGE_BYTE_SIZE, required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let hr = write_header(signature as u16, XBTP_MESSAGE_TYPE_CLOSE_SESSION, FIXED_CLOSE_SESSION_MESSAGE_BYTE_SIZE - FIXED_MESSAGE_HEADER_BYTE_SIZE, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_guid(session_id, buffer_length, &mut off, buffer)
}

#[export_name = "?SerializeFixedExtensionMessage@@YAJFFAEBU_GUID@@HKAEBUXBTP_MESSAGE_BODY@@KPEAKPEAD@Z"]
pub unsafe extern "C" fn SerializeFixedExtensionMessage(
    signature: i16,
    message_type: i16,
    session_id: *const GUID,
    exchange_id: i32,
    operation_id: u32,
    body: *const XBTP_MESSAGE_BODY,
    buffer_length: u32,
    required: *mut u32,
    buffer: *mut u8,
) -> HRESULT {
    if (buffer_length != 0 && buffer.is_null()) || required.is_null() {
        return E_INVALIDARG;
    }
    let hr = size_check(buffer_length, FIXED_EXTENSION_MESSAGE_BYTE_SIZE, required);
    if hr < 0 {
        return hr;
    }
    let mut off = 0u32;
    let message_length = (*body).binary_body_length.wrapping_add((*body).json_body_length.wrapping_add(FIXED_EXTENSION_BODY_BYTE_SIZE));
    let hr = write_header(signature as u16, message_type as u16, message_length, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_guid(session_id, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32(exchange_id as u32, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32(operation_id, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    let hr = write_u32((*body).json_body_length, buffer_length, &mut off, buffer);
    if hr < 0 {
        return hr;
    }
    write_u32((*body).binary_body_length, buffer_length, &mut off, buffer)
}

#[export_name = "?DeserializeFixedExtensionMessage@@YAJAEBUXBTP_MESSAGE_HEADER@@KPEADAEAUXBTP_BEGIN_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeFixedExtensionMessage(header: *const XBTP_MESSAGE_HEADER, buffer_length: u32, body: *mut u8, message: *mut XBTP_BEGIN_MESSAGE) -> HRESULT {
    if buffer_length != 0 && body.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length(FIXED_EXTENSION_BODY_BYTE_SIZE, buffer_length);
    if hr < 0 {
        return hr;
    }
    (*message).header = *header;
    let mut off = 0u32;
    let hr = read_guid(buffer_length, body, &mut off, &mut (*message).session_id);
    if hr < 0 {
        return hr;
    }
    let hr = read_u32(buffer_length, body, &mut off, &mut (*message).exchange_id as *mut i32 as *mut u32);
    if hr < 0 {
        return hr;
    }
    let hr = read_u32(buffer_length, body, &mut off, &mut (*message).operation_id);
    if hr < 0 {
        return hr;
    }
    let hr = read_u32(buffer_length, body, &mut off, &mut (*message).body.json_body_length);
    if hr < 0 {
        return hr;
    }
    read_u32(buffer_length, body, &mut off, &mut (*message).body.binary_body_length)
}

#[export_name = "?DeserializeExtensionMessage@@YAJAEBUXBTP_MESSAGE_HEADER@@KPEADAEAUXBTP_BEGIN_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeExtensionMessage(header: *const XBTP_MESSAGE_HEADER, buffer_length: u32, body: *mut u8, message: *mut XBTP_BEGIN_MESSAGE) -> HRESULT {
    if buffer_length != 0 && body.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length((*header).message_length, buffer_length);
    if hr < 0 {
        return hr;
    }
    let hr = DeserializeFixedExtensionMessage(header, buffer_length, body, message);
    if hr < 0 {
        return hr;
    }
    let m = &mut *message;
    m.body.binary_body = core::ptr::null_mut();
    m.body.json_body = core::ptr::null_mut();
    let data = body.wrapping_add(FIXED_EXTENSION_BODY_BYTE_SIZE as usize);
    if m.body.json_body_length != 0 {
        m.body.json_body = data;
        *data.add((m.body.json_body_length - 1) as usize) = 0;
    }
    if m.body.binary_body_length > 0 {
        m.body.binary_body = data.wrapping_add(m.body.json_body_length as usize);
    }
    S_OK
}

#[export_name = "?DeserializeCloseSessionMessage@@YAJAEAUXBTP_MESSAGE_HEADER@@KPEADAEAUXBTP_CLOSE_SESSION_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeCloseSessionMessage(header: *mut XBTP_MESSAGE_HEADER, buffer_length: u32, body: *mut u8, message: *mut XBTP_CLOSE_SESSION_MESSAGE) -> HRESULT {
    if buffer_length != 0 && body.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length((*header).message_length, buffer_length);
    if hr < 0 {
        return hr;
    }
    (*message).header = *header;
    let mut off = 0u32;
    read_guid(buffer_length, body, &mut off, &mut (*message).session_id)
}

#[export_name = "?DeserializeDisconnectMessage@@YAJAEAUXBTP_MESSAGE_HEADER@@KPEADAEAUXBTP_DISCONNECT_MESSAGE@@@Z"]
pub unsafe extern "C" fn DeserializeDisconnectMessage(header: *mut XBTP_MESSAGE_HEADER, buffer_length: u32, body: *mut u8, message: *mut XBTP_DISCONNECT_MESSAGE) -> HRESULT {
    if buffer_length != 0 && body.is_null() {
        return E_INVALIDARG;
    }
    let hr = check_valid_length((*header).message_length, buffer_length);
    if hr < 0 {
        return hr;
    }
    (*message).header = *header;
    let mut off = 0u32;
    read_u32(buffer_length, body, &mut off, &mut (*message).status as *mut i32 as *mut u32)
}

pub fn parse_header(bytes: &[u8]) -> Option<XBTP_MESSAGE_HEADER> {
    if bytes.len() < 8 {
        return None;
    }
    Some(XBTP_MESSAGE_HEADER {
        signature: u16::from_le_bytes([bytes[0], bytes[1]]),
        message_type: u16::from_le_bytes([bytes[2], bytes[3]]),
        message_length: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    })
}

fn build(size: u32, f: impl FnOnce(u32, *mut u32, *mut u8) -> HRESULT) -> Result<Vec<u8>, HRESULT> {
    let mut out = vec![0u8; size as usize];
    let mut req = 0u32;
    let hr = f(size, &mut req, out.as_mut_ptr());
    if hr < 0 {
        Err(hr)
    } else {
        out.truncate(req as usize);
        Ok(out)
    }
}

pub fn build_connect_request(signature: u16, version: u16, agent: &[u8]) -> Result<Vec<u8>, HRESULT> {
    let mut t = UTF8Text { length: agent.len() as u32, text: agent.as_ptr() as *mut u8 };
    build(agent.len() as u32 + 8, |l, r, b| unsafe { SerializeConnectRequest(signature as i16, version as i16, &mut t, l, r, b) })
}

pub fn build_connect_response(status: i32, agent: &[u8]) -> Result<Vec<u8>, HRESULT> {
    let mut t = UTF8Text { length: agent.len() as u32, text: agent.as_ptr() as *mut u8 };
    build(agent.len() as u32 + 8, |l, r, b| unsafe { SerializeConnectResponse(status, &mut t, l, r, b) })
}

pub fn build_create_session_request(signature: u16, session_id: &GUID, extension_id: &GUID) -> Result<Vec<u8>, HRESULT> {
    build(FIXED_SESSION_REQUEST_BYTE_SIZE, |l, r, b| unsafe { SerializeCreateSessionRequest(signature as i16, session_id, extension_id, l, r, b) })
}

pub fn build_create_session_response(signature: u16, session_id: &GUID, status: i32, location: &[u8]) -> Result<Vec<u8>, HRESULT> {
    let mut t = UTF8Text { length: location.len() as u32, text: location.as_ptr() as *mut u8 };
    build(location.len() as u32 + FIXED_SESSION_RESPONSE_BYTE_SIZE, |l, r, b| unsafe { SerializeCreateSessionResponse(signature as i16, session_id, status, &mut t, l, r, b) })
}

pub fn build_disconnect(signature: u16, status: i32) -> Result<Vec<u8>, HRESULT> {
    build(FIXED_DISCONNECT_MESSAGE_BYTE_SIZE, |l, r, b| unsafe { SerializeDisconnect(signature as i16, status, l, r, b) })
}

pub fn build_close_session(signature: u16, session_id: &GUID) -> Result<Vec<u8>, HRESULT> {
    build(FIXED_CLOSE_SESSION_MESSAGE_BYTE_SIZE, |l, r, b| unsafe { SerializeCloseSession(signature as i16, session_id, l, r, b) })
}

pub fn build_extension_message(signature: u16, message_type: u16, session_id: &GUID, exchange_id: i32, operation_id: u32, json: &[u8], binary: &[u8]) -> Result<Vec<u8>, HRESULT> {
    let body = XBTP_MESSAGE_BODY { json_body_length: json.len() as u32, binary_body_length: binary.len() as u32, json_body: json.as_ptr() as *mut u8, binary_body: binary.as_ptr() as *mut u8 };
    let mut out = build(FIXED_EXTENSION_MESSAGE_BYTE_SIZE, |l, r, b| unsafe { SerializeFixedExtensionMessage(signature as i16, message_type as i16, session_id, exchange_id, operation_id, &body, l, r, b) })?;
    out.extend_from_slice(json);
    out.extend_from_slice(binary);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::GUID;

    const G1: GUID = GUID::new(0x11223344, 0x5566, 0x7788, [0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01]);
    const G2: GUID = GUID::new(0xa1a2a3a4, 0xb1b2, 0xc1c2, [1, 2, 3, 4, 5, 6, 7, 8]);

    #[test]
    fn layouts_match_console() {
        use core::mem::{offset_of, size_of};
        assert_eq!(size_of::<UTF8Text>(), 16);
        assert_eq!(offset_of!(XBTP_CONNECT_REQUEST, agent), 8);
        assert_eq!(offset_of!(XBTP_CONNECT_RESPONSE, agent), 8);
        assert_eq!(size_of::<XBTP_CREATE_SESSION_REQUEST_MESSAGE>(), 0x28);
        assert_eq!(offset_of!(XBTP_CREATE_SESSION_RESPONSE_MESSAGE, status), 0x18);
        assert_eq!(offset_of!(XBTP_CREATE_SESSION_RESPONSE_MESSAGE, location), 0x20);
        assert_eq!(size_of::<XBTP_MESSAGE_BODY>(), 0x18);
        assert_eq!(offset_of!(XBTP_BEGIN_MESSAGE, exchange_id), 0x18);
        assert_eq!(offset_of!(XBTP_BEGIN_MESSAGE, body), 0x20);
        assert_eq!(size_of::<XBTP_BEGIN_MESSAGE>(), 0x38);
        assert_eq!(offset_of!(XBTP_DISCONNECT_MESSAGE, status), 8);
        assert_eq!(size_of::<XBTP_CLOSE_SESSION_MESSAGE>(), 0x18);
    }

    #[test]
    fn connect_request_bytes_and_round_trip() {
        let b = build_connect_request(0x5842, 3, b"agent\0").unwrap();
        assert_eq!(b, [0x42, 0x58, 3, 0, 6, 0, 0, 0, b'a', b'g', b'e', b'n', b't', 0]);
        let mut buf = b.clone();
        let mut r = XBTP_CONNECT_REQUEST::default();
        assert_eq!(unsafe { DeserializeConnectRequest(buf.len() as u32, buf.as_mut_ptr(), &mut r) }, S_OK);
        assert_eq!((r.signature, r.protocol_version, r.agent.length), (0x5842, 3, 6));
        assert_eq!(r.agent.text, unsafe { buf.as_mut_ptr().add(8) });
        let mut f = XBTP_CONNECT_REQUEST::default();
        assert_eq!(unsafe { DeserializeFixedConnectRequest(8, buf.as_mut_ptr(), &mut f) }, S_OK);
        assert_eq!(f.agent.length, 6);
        assert!(f.agent.text.is_null());
    }

    #[test]
    fn connect_request_size_query_and_errors() {
        let mut t = UTF8Text { length: 4, text: b"abc\0".as_ptr() as *mut u8 };
        let mut req = 0u32;
        assert_eq!(unsafe { SerializeConnectRequest(1, 2, &mut t, 0, &mut req, core::ptr::null_mut()) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(req, 12);
        let mut buf = [0u8; 4];
        assert_eq!(unsafe { SerializeConnectRequest(1, 2, &mut t, 4, core::ptr::null_mut(), buf.as_mut_ptr()) }, E_INVALIDARG);
        assert_eq!(unsafe { SerializeConnectRequest(1, 2, &mut t, 4, &mut req, core::ptr::null_mut()) }, E_INVALIDARG);
        let mut bad = UTF8Text { length: 3, text: core::ptr::null_mut() };
        assert_eq!(unsafe { SerializeConnectRequest(1, 2, &mut bad, 0, &mut req, core::ptr::null_mut()) }, E_INVALIDARG);
        assert_eq!(unsafe { SerializeConnectRequest(1, 2, core::ptr::null_mut(), 0, &mut req, core::ptr::null_mut()) }, E_INVALIDARG);
        let mut r = XBTP_CONNECT_REQUEST::default();
        assert_eq!(unsafe { DeserializeFixedConnectRequest(7, buf.as_mut_ptr(), &mut r) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(unsafe { DeserializeFixedConnectRequest(8, core::ptr::null_mut(), &mut r) }, E_INVALIDARG);
        let mut short = [1u8, 0, 2, 0, 9, 0, 0, 0, b'x'];
        assert_eq!(unsafe { DeserializeConnectRequest(9, short.as_mut_ptr(), &mut r) }, HR_INSUFFICIENT_BUFFER);
        let mut empty = [1u8, 0, 2, 0, 0, 0, 0, 0];
        r.agent.text = 1 as *mut u8;
        assert_eq!(unsafe { DeserializeConnectRequest(8, empty.as_mut_ptr(), &mut r) }, S_OK);
        assert!(r.agent.text.is_null());
    }

    #[test]
    fn connect_response_bytes_and_round_trip() {
        let b = build_connect_response(-0x73eefcfd, b"srv\0").unwrap();
        assert_eq!(b, [0x03, 0x03, 0x11, 0x8c, 4, 0, 0, 0, b's', b'r', b'v', 0]);
        let mut buf = b.clone();
        let mut r = XBTP_CONNECT_RESPONSE::default();
        assert_eq!(unsafe { DeserializeConnectResponse(buf.len() as u32, buf.as_mut_ptr(), &mut r) }, S_OK);
        assert_eq!(r.status as u32, 0x8c110303);
        assert_eq!(r.agent.length, 4);
        let mut f = XBTP_CONNECT_RESPONSE::default();
        assert_eq!(unsafe { DeserializeFixedConnectResponse(8, buf.as_mut_ptr(), &mut f) }, S_OK);
        assert_eq!(f.status as u32, 0x8c110303);
        assert_eq!(unsafe { DeserializeFixedConnectResponse(3, buf.as_mut_ptr(), &mut f) }, HR_INSUFFICIENT_BUFFER);
        let mut req = 0;
        assert_eq!(unsafe { SerializeConnectResponse(0, core::ptr::null_mut(), 0, &mut req, core::ptr::null_mut()) }, E_INVALIDARG);
    }

    #[test]
    fn connect_text_is_nul_terminated_in_place() {
        let mut buf = [0u8, 0, 0, 0, 3, 0, 0, 0, b'a', b'b', b'c'];
        let mut r = XBTP_CONNECT_RESPONSE::default();
        assert_eq!(unsafe { DeserializeConnectResponse(11, buf.as_mut_ptr(), &mut r) }, S_OK);
        assert_eq!(buf[10], 0);
    }

    #[test]
    fn create_session_request_bytes_and_round_trip() {
        let b = build_create_session_request(0x5842, &G1, &G2).unwrap();
        let mut want = vec![0x42, 0x58, 0xfe, 0xff, 0x20, 0, 0, 0];
        want.extend_from_slice(&G1.to_bytes());
        want.extend_from_slice(&G2.to_bytes());
        assert_eq!(b, want);
        let mut buf = b.clone();
        let mut r = XBTP_CREATE_SESSION_REQUEST_MESSAGE::default();
        assert_eq!(unsafe { DeserializeCreateSessionRequest(0x28, buf.as_mut_ptr(), &mut r) }, S_OK);
        assert_eq!(r.header, XBTP_MESSAGE_HEADER { signature: 0x5842, message_type: 0xfffe, message_length: 0x20 });
        assert_eq!((r.session_id, r.extension_id), (G1, G2));
        assert_eq!(unsafe { DeserializeCreateSessionRequest(0x27, buf.as_mut_ptr(), &mut r) }, HR_INSUFFICIENT_BUFFER);
        let mut req = 0;
        assert_eq!(unsafe { SerializeCreateSessionRequest(1, &G1, &G2, 0x27, &mut req, buf.as_mut_ptr()) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(req, 0x28);
    }

    #[test]
    fn create_session_response_bytes_and_round_trip() {
        let b = build_create_session_response(7, &G2, 0, b"tcp://x\0").unwrap();
        let mut want = vec![7, 0, 0xfd, 0xff, 0x20, 0, 0, 0];
        want.extend_from_slice(&G2.to_bytes());
        want.extend_from_slice(&[0, 0, 0, 0, 8, 0, 0, 0]);
        want.extend_from_slice(b"tcp://x\0");
        assert_eq!(b, want);
        let mut buf = b.clone();
        let mut r = XBTP_CREATE_SESSION_RESPONSE_MESSAGE::default();
        assert_eq!(unsafe { DeserializeCreateSessionResponse(buf.len() as u32, buf.as_mut_ptr(), &mut r) }, S_OK);
        assert_eq!((r.session_id, r.status, r.location.length), (G2, 0, 8));
        assert_eq!(r.location.text, unsafe { buf.as_mut_ptr().add(0x20) });
        let mut f = XBTP_CREATE_SESSION_RESPONSE_MESSAGE::default();
        assert_eq!(unsafe { DeserializeFixedCreateSessionResponse(0x20, buf.as_mut_ptr(), &mut f) }, S_OK);
        assert_eq!(f.location.length, 8);
        assert_eq!(unsafe { DeserializeCreateSessionResponse(0x21, buf.as_mut_ptr(), &mut r) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(unsafe { DeserializeFixedCreateSessionResponse(0x1f, buf.as_mut_ptr(), &mut f) }, HR_INSUFFICIENT_BUFFER);
        let mut req = 0;
        assert_eq!(unsafe { SerializeCreateSessionResponse(0, &G2, 0, core::ptr::null_mut(), 0, &mut req, core::ptr::null_mut()) }, E_INVALIDARG);
    }

    #[test]
    fn disconnect_bytes_and_round_trip() {
        let b = build_disconnect(0x5842, 0x8c11030a_u32 as i32).unwrap();
        assert_eq!(b, [0x42, 0x58, 0xff, 0xff, 4, 0, 0, 0, 0x0a, 0x03, 0x11, 0x8c]);
        let mut h = parse_header(&b).unwrap();
        let mut body = b[8..].to_vec();
        let mut m = XBTP_DISCONNECT_MESSAGE::default();
        assert_eq!(unsafe { DeserializeDisconnectMessage(&mut h, 4, body.as_mut_ptr(), &mut m) }, S_OK);
        assert_eq!(m.status as u32, 0x8c11030a);
        assert_eq!(m.header, h);
        assert_eq!(unsafe { DeserializeDisconnectMessage(&mut h, 3, body.as_mut_ptr(), &mut m) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(unsafe { DeserializeDisconnectMessage(&mut h, 4, core::ptr::null_mut(), &mut m) }, E_INVALIDARG);
        let mut req = 0;
        assert_eq!(unsafe { SerializeDisconnect(0, 0, 0, &mut req, core::ptr::null_mut()) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(req, 0xc);
    }

    #[test]
    fn close_session_bytes_and_round_trip() {
        let b = build_close_session(0x5842, &G1).unwrap();
        let mut want = vec![0x42, 0x58, 0xfc, 0xff, 0x10, 0, 0, 0];
        want.extend_from_slice(&G1.to_bytes());
        assert_eq!(b, want);
        let mut h = parse_header(&b).unwrap();
        let mut body = b[8..].to_vec();
        let mut m = XBTP_CLOSE_SESSION_MESSAGE::default();
        assert_eq!(unsafe { DeserializeCloseSessionMessage(&mut h, 16, body.as_mut_ptr(), &mut m) }, S_OK);
        assert_eq!(m.session_id, G1);
        assert_eq!(unsafe { DeserializeCloseSessionMessage(&mut h, 15, body.as_mut_ptr(), &mut m) }, HR_INSUFFICIENT_BUFFER);
        let mut req = 0;
        assert_eq!(unsafe { SerializeCloseSession(0, &G1, 1, &mut req, core::ptr::null_mut()) }, E_INVALIDARG);
    }

    #[test]
    fn extension_message_bytes_and_round_trip() {
        let b = build_extension_message(0x5842, 0x0002, &G1, 5, 9, b"{}\0", &[0xde, 0xad]).unwrap();
        let mut want = vec![0x42, 0x58, 2, 0, 0x25, 0, 0, 0];
        want.extend_from_slice(&G1.to_bytes());
        want.extend_from_slice(&[5, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0]);
        want.extend_from_slice(b"{}\0");
        want.extend_from_slice(&[0xde, 0xad]);
        assert_eq!(b, want);
        let h = parse_header(&b).unwrap();
        let mut body = b[8..].to_vec();
        let mut m = XBTP_BEGIN_MESSAGE::default();
        assert_eq!(unsafe { DeserializeExtensionMessage(&h, body.len() as u32, body.as_mut_ptr(), &mut m) }, S_OK);
        assert_eq!((m.session_id, m.exchange_id, m.operation_id), (G1, 5, 9));
        assert_eq!((m.body.json_body_length, m.body.binary_body_length), (3, 2));
        assert_eq!(m.body.json_body, unsafe { body.as_mut_ptr().add(0x20) });
        assert_eq!(m.body.binary_body, unsafe { body.as_mut_ptr().add(0x23) });
        assert_eq!(m.header, h);
        let mut f = XBTP_BEGIN_MESSAGE::default();
        assert_eq!(unsafe { DeserializeFixedExtensionMessage(&h, 0x20, body.as_mut_ptr(), &mut f) }, S_OK);
        assert_eq!(f.body.json_body_length, 3);
        assert_eq!(unsafe { DeserializeFixedExtensionMessage(&h, 0, core::ptr::null_mut(), &mut f) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(unsafe { DeserializeFixedExtensionMessage(&h, 0x20, core::ptr::null_mut(), &mut f) }, E_INVALIDARG);
        assert_eq!(unsafe { DeserializeFixedExtensionMessage(&h, 0x1f, body.as_mut_ptr(), &mut f) }, HR_INSUFFICIENT_BUFFER);
        assert_eq!(unsafe { DeserializeExtensionMessage(&h, 0x24, body.as_mut_ptr(), &mut m) }, HR_INSUFFICIENT_BUFFER);
    }

    #[test]
    fn extension_message_without_bodies_leaves_pointers_null() {
        let b = build_extension_message(1, 3, &G2, -1, 0, &[], &[]).unwrap();
        assert_eq!(b.len(), 0x28);
        let h = parse_header(&b).unwrap();
        let mut body = b[8..].to_vec();
        let mut m = XBTP_BEGIN_MESSAGE::default();
        m.body.json_body = 1 as *mut u8;
        assert_eq!(unsafe { DeserializeExtensionMessage(&h, 0x20, body.as_mut_ptr(), &mut m) }, S_OK);
        assert!(m.body.json_body.is_null() && m.body.binary_body.is_null());
        assert_eq!(m.exchange_id, -1);
    }

    #[test]
    fn reads_past_end_leave_fields_untouched() {
        let mut buf = [1u8, 0];
        let mut off = 0u32;
        let mut v = 0xffffu16;
        assert_eq!(unsafe { read_u16(1, buf.as_mut_ptr(), &mut off, &mut v) }, S_OK);
        assert_eq!((v, off), (0xffff, 2));
        let mut off = 1u32;
        let mut w = 0u32;
        let src = [0u8, 0, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(unsafe { read_u32(6, src.as_ptr(), &mut off, &mut w) }, S_OK);
        assert_eq!((w, off), (0x1234_5678, 6));
    }
}
