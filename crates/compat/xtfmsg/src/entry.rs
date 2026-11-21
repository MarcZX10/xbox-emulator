use crate::com::{self, metadata::OperationFn, Com, Obj, IID_XBTP_EXTENSION_METADATA};
use crate::sys::*;
use crate::xbtp;

pub struct EntrySite {
    pub file: &'static str,
    pub func: &'static str,
    pub bad_extension_line: u32,
    pub bad_metadata_line: u32,
    pub create_line: u32,
    pub handlers: &'static [(u32, OperationFn, &'static str, u32)],
    pub bad_extension_msg: &'static str,
    pub bad_metadata_msg: &'static str,
}

fn z(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

pub fn log_site(site: &EntrySite, line: u32, name: &str, hr: HRESULT, msg: &str) {
    let short = z(&format!("XtfCHK: {}: {}", site.func, line));
    let long = z(&format!("XtfCHK: {}: {}: {}", site.file, site.func, line));
    crate::chk::chk(&short, &long, &z(name), hr, &z(msg));
}

pub unsafe fn extension_entry_point(site: &EntrySite, want_ext: &GUID, ext: *const GUID, iid: *const GUID, out: *mut Obj) -> HRESULT {
    extension_entry_point_with(site, want_ext, ext, iid, out, &mut || S_OK)
}

pub unsafe fn extension_entry_point_with(site: &EntrySite, want_ext: &GUID, ext: *const GUID, iid: *const GUID, out: *mut Obj, pre: &mut dyn FnMut() -> HRESULT) -> HRESULT {
    if *ext != *want_ext {
        log_site(site, site.bad_extension_line, "E_UNEXPECTED", E_UNEXPECTED, site.bad_extension_msg);
        return E_UNEXPECTED;
    }
    if *iid != IID_XBTP_EXTENSION_METADATA {
        log_site(site, site.bad_metadata_line, "E_UNEXPECTED", E_UNEXPECTED, site.bad_metadata_msg);
        return E_UNEXPECTED;
    }
    let hr = pre();
    if hr < 0 {
        return hr;
    }
    let mut md = Com::null();
    let hr = xbtp::create_extension_metadata(&IID_XBTP_EXTENSION_METADATA, md.put());
    log_site(site, site.create_line, "XbtpCreateExtensionMetadata(IID_PPV_ARGS(&metadata))", hr, "");
    if hr < 0 {
        return hr;
    }
    for (id, f, text, line) in site.handlers {
        let hr = com::metadata::add_function(md.get(), *id, Some(*f));
        log_site(site, *line, text, hr, "");
        if hr < 0 {
            return hr;
        }
    }
    *out = md.detach();
    S_OK
}
