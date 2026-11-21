use core::ffi::c_void;
use ndr_proxy::{Guid, Tables};

mod table;

pub static TABLES: Tables = Tables {
    blob: include_bytes!("../data/proxy_19041.bin"),
    proxy_file_list: table::PROXY_FILE_LIST,
    clsid: table::CLSID,
    factory: table::FACTORY,
    files: table::FILES,
    dlls: table::DLLS,
    fixups: table::FIXUPS,
    rdata_len: table::RDATA_LEN,
};

pub const SOURCE_RVA: u32 = table::SOURCE_RVA;
pub const FILE_COUNT: usize = table::FILE_COUNT;

#[no_mangle]
pub unsafe extern "system" fn GetProxyDllInfo(info: *mut *const c_void, id: *mut *const Guid) {
    ndr_proxy::get_proxy_dll_info(&TABLES, info, id)
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const Guid, iid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    ndr_proxy::dll_get_class_object(&TABLES, clsid, iid, ppv)
}

#[no_mangle]
pub unsafe extern "system" fn DllCanUnloadNow() -> i32 {
    ndr_proxy::dll_can_unload_now(&TABLES)
}

#[no_mangle]
pub unsafe extern "system" fn DllRegisterServer() -> i32 {
    ndr_proxy::dll_register_server(&TABLES)
}

#[no_mangle]
pub unsafe extern "system" fn DllUnregisterServer() -> i32 {
    ndr_proxy::dll_unregister_server(&TABLES)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
