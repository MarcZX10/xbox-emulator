use crate::com::*;
use core::ffi::c_void;
use core::ptr::null_mut;

pub fn cvtt_i64(d: f64) -> i64 {
    if d.is_nan() || d >= 9.223372036854775808e18 || d < -9.223372036854775808e18 {
        i64::MIN
    } else {
        d as i64
    }
}

pub fn cvtt_i32(d: f64) -> i32 {
    if d.is_nan() || d >= 2147483648.0 || d <= -2147483649.0 {
        i32::MIN
    } else {
        d as i32
    }
}

unsafe fn obj(p: *mut c_void) -> &'static JsonObjectVtbl {
    vt::<JsonObjectVtbl>(p)
}

pub unsafe fn get_string(props: *mut c_void, name: &str, out: &mut Bstr) -> HRESULT {
    let n = wide(name);
    (obj(props).get_string)(props, n.as_ptr(), out.out())
}

pub unsafe fn set_string(props: *mut c_void, name: &str, value: &Bstr) -> HRESULT {
    let n = wide(name);
    (obj(props).set_string)(props, n.as_ptr(), value.0)
}

pub unsafe fn get_u32(props: *mut c_void, name: &str, out: &mut u32) -> HRESULT {
    if props.is_null() {
        return E_INVALIDARG;
    }
    let n = wide(name);
    let mut d = 0f64;
    let hr = (obj(props).get_number)(props, n.as_ptr(), &mut d);
    if hr >= 0 {
        *out = cvtt_i64(d) as u32;
    }
    hr
}

pub unsafe fn get_i32(props: *mut c_void, name: &str, out: &mut i32) -> HRESULT {
    if props.is_null() {
        return E_INVALIDARG;
    }
    let n = wide(name);
    let mut d = 0f64;
    let hr = (obj(props).get_number)(props, n.as_ptr(), &mut d);
    if hr >= 0 {
        *out = cvtt_i32(d);
    }
    hr
}

pub unsafe fn set_u32(props: *mut c_void, name: &str, v: u32) -> HRESULT {
    if props.is_null() {
        return E_INVALIDARG;
    }
    let n = wide(name);
    (obj(props).set_number)(props, n.as_ptr(), v as f64)
}

pub unsafe fn set_i32(props: *mut c_void, name: &str, v: i32) -> HRESULT {
    if props.is_null() {
        return E_INVALIDARG;
    }
    let n = wide(name);
    (obj(props).set_number)(props, n.as_ptr(), v as f64)
}

pub unsafe fn get_u64(props: *mut c_void, name: &str, out: &mut u64) -> HRESULT {
    if props.is_null() {
        return E_INVALIDARG;
    }
    let n = wide(name);
    let mut num = Com::null();
    let hr = (obj(props).get_object)(props, n.as_ptr(), num.out());
    if hr < 0 {
        return hr;
    }
    let l = wide("L");
    let h = wide("H");
    let mut d = 0f64;
    let hr = (obj(num.0).get_number)(num.0, l.as_ptr(), &mut d);
    if hr < 0 {
        return hr;
    }
    let lo = cvtt_i64(d) as u32;
    let hr = (obj(num.0).get_number)(num.0, h.as_ptr(), &mut d);
    if hr < 0 {
        return hr;
    }
    let hi = cvtt_i64(d) as u32;
    *out = ((hi as u64) << 32) | lo as u64;
    hr
}

pub unsafe fn set_u64(props: *mut c_void, name: &str, v: u64) -> HRESULT {
    if props.is_null() {
        return E_INVALIDARG;
    }
    let mut num = Com::null();
    let hr = XbtpCreateJsonObject(num.out());
    if hr < 0 {
        return hr;
    }
    let l = wide("L");
    let h = wide("H");
    let hr = (obj(num.0).set_number)(num.0, l.as_ptr(), (v as u32) as f64);
    if hr < 0 {
        return hr;
    }
    let hr = (obj(num.0).set_number)(num.0, h.as_ptr(), ((v >> 32) as u32 as i32) as f64);
    if hr < 0 {
        return hr;
    }
    let n = wide(name);
    (obj(props).set_object)(props, n.as_ptr(), num.0)
}

pub trait Props {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT;
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT;
}

pub unsafe fn deserialize_json_body(message: *mut c_void, out: &mut Com) -> HRESULT {
    if message.is_null() {
        return E_INVALIDARG;
    }
    let mut len = 0u32;
    let mut body: *mut u8 = null_mut();
    let hr = (vt::<MessageVtbl>(message).get_json_body)(message, &mut len, &mut body);
    if hr < 0 {
        return hr;
    }
    if body.is_null() {
        XbtpCreateJsonObject(out.out())
    } else {
        XbtpGetJsonObjectFromUtf8(body, out.out())
    }
}

pub unsafe fn deserialize<T: Props>(m: &mut T, message: *mut c_void) -> HRESULT {
    let mut props = Com::null();
    let hr = deserialize_json_body(message, &mut props);
    if hr < 0 {
        return hr;
    }
    m.deserialize_properties(props.0)
}

unsafe fn stringify(m: &impl Props) -> Result<Bstr, HRESULT> {
    let mut props = Com::null();
    let hr = XbtpCreateJsonObject(props.out());
    if hr < 0 {
        return Err(hr);
    }
    let hr = m.serialize_properties(props.0);
    if hr < 0 {
        return Err(hr);
    }
    let mut w = Bstr::null();
    let hr = (obj(props.0).stringify)(props.0, w.out());
    if hr < 0 {
        return Err(hr);
    }
    Ok(w)
}

pub unsafe fn serialize<T: Props>(m: &T, message: *mut c_void) -> HRESULT {
    let w = match stringify(m) {
        Ok(w) => w,
        Err(e) => return e,
    };
    if message.is_null() {
        return E_INVALIDARG;
    }
    (vt::<MessageVtbl>(message).set_json_body_w)(message, w.0)
}

pub unsafe fn serialize_impl<T: Props>(m: &T, message: *mut c_void) -> HRESULT {
    let w = match stringify(m) {
        Ok(w) => w,
        Err(e) => return e,
    };
    if message.is_null() {
        return E_INVALIDARG;
    }
    (vt::<MessageImplVtbl>(message).set_json_body_w)(message, w.0)
}

#[derive(Default)]
pub struct MsgFileName {
    pub file_name: Bstr,
}

impl Default for Bstr {
    fn default() -> Self {
        Bstr::null()
    }
}

impl Props for MsgFileName {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        get_string(props, "FileName", &mut self.file_name)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        set_string(props, "FileName", &self.file_name)
    }
}

#[derive(Default)]
pub struct MsgFlags {
    pub base: MsgFileName,
    pub flags: u32,
}

impl Props for MsgFlags {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        get_u32(props, "Flags", &mut self.flags)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        set_u32(props, "Flags", self.flags)
    }
}

#[derive(Default)]
pub struct MsgModifyFileAttributes {
    pub base: MsgFileName,
    pub add: u32,
    pub remove: u32,
}

impl Props for MsgModifyFileAttributes {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        let hr = get_u32(props, "AddFileAttributes", &mut self.add);
        if hr < 0 {
            return hr;
        }
        get_u32(props, "RemoveFileAttributes", &mut self.remove)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        let hr = set_u32(props, "AddFileAttributes", self.add);
        if hr < 0 {
            return hr;
        }
        set_u32(props, "RemoveFileAttributes", self.remove)
    }
}

#[derive(Default)]
pub struct MsgSetFileAttributes {
    pub base: MsgFileName,
    pub attributes: u32,
}

impl Props for MsgSetFileAttributes {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        get_u32(props, "FileAttributes", &mut self.attributes)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        set_u32(props, "FileAttributes", self.attributes)
    }
}

#[derive(Default)]
pub struct MsgFindFiles {
    pub base: MsgFlags,
    pub include: u32,
    pub exclude: u32,
    pub recursion: u32,
}

impl Props for MsgFindFiles {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        let hr = get_u32(props, "IncludeFileAttributes", &mut self.include);
        if hr < 0 {
            return hr;
        }
        let hr = get_u32(props, "ExcludeFileAttributes", &mut self.exclude);
        if hr < 0 {
            return hr;
        }
        get_u32(props, "RecursionLevels", &mut self.recursion)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        let hr = set_u32(props, "IncludeFileAttributes", self.include);
        if hr < 0 {
            return hr;
        }
        let hr = set_u32(props, "ExcludeFileAttributes", self.exclude);
        if hr < 0 {
            return hr;
        }
        set_u32(props, "RecursionLevels", self.recursion)
    }
}

#[derive(Default)]
pub struct MsgStorageDevice {
    pub storage_device: Bstr,
}

impl Props for MsgStorageDevice {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        get_string(props, "StorageDevice", &mut self.storage_device)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        set_string(props, "StorageDevice", &self.storage_device)
    }
}

#[derive(Default)]
pub struct MsgResponse {
    pub result: i32,
}

impl Props for MsgResponse {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        get_i32(props, "Result", &mut self.result)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        set_i32(props, "Result", self.result)
    }
}

#[derive(Default)]
pub struct MsgGetFileAttributesResponse {
    pub base: MsgResponse,
    pub attributes: u32,
}

impl Props for MsgGetFileAttributesResponse {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        get_u32(props, "FileAttributes", &mut self.attributes)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        set_u32(props, "FileAttributes", self.attributes)
    }
}

#[derive(Default)]
pub struct MsgGetFileSizeResponse {
    pub base: MsgResponse,
    pub size: u64,
}

impl Props for MsgGetFileSizeResponse {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        get_u64(props, "FileSize", &mut self.size)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        set_u64(props, "FileSize", self.size)
    }
}

#[derive(Default)]
pub struct MsgStorageDeviceFreeSpaceResponse {
    pub base: MsgResponse,
    pub total: u64,
    pub free: u64,
}

impl Props for MsgStorageDeviceFreeSpaceResponse {
    unsafe fn deserialize_properties(&mut self, props: *mut c_void) -> HRESULT {
        let hr = self.base.deserialize_properties(props);
        if hr < 0 {
            return hr;
        }
        let hr = get_u64(props, "TotalNumberOfBytes", &mut self.total);
        if hr < 0 {
            return hr;
        }
        get_u64(props, "NumberOfFreeBytes", &mut self.free)
    }
    unsafe fn serialize_properties(&self, props: *mut c_void) -> HRESULT {
        let hr = self.base.serialize_properties(props);
        if hr < 0 {
            return hr;
        }
        let hr = set_u64(props, "TotalNumberOfBytes", self.total);
        if hr < 0 {
            return hr;
        }
        set_u64(props, "NumberOfFreeBytes", self.free)
    }
}
