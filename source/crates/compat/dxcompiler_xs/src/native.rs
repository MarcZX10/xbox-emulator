use crate::com::*;
use crate::guids::*;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(p: *mut c_void);
}

enum Store {
    Owned(#[allow(dead_code)] Box<[u8]>),
    Pinned,
    Malloc(*mut c_void),
    CoTask,
    Parent(*mut c_void),
}

#[repr(C)]
struct BlobVtbl {
    unk: UnknownVtbl,
    get_buffer_pointer: unsafe extern "system" fn(*mut c_void) -> *mut c_void,
    get_buffer_size: unsafe extern "system" fn(*mut c_void) -> usize,
    get_encoding: unsafe extern "system" fn(*mut c_void, *mut i32, *mut u32) -> Hr,
    get_string_pointer: unsafe extern "system" fn(*mut c_void) -> *const c_void,
    get_string_length: unsafe extern "system" fn(*mut c_void) -> usize,
}

#[repr(C)]
pub struct Blob {
    vtbl: &'static BlobVtbl,
    refs: AtomicU32,
    data: *mut u8,
    len: usize,
    known: bool,
    cp: u32,
    store: Store,
}

static BLOB_VTBL: BlobVtbl = BlobVtbl {
    unk: UnknownVtbl { query_interface: blob_qi, add_ref: blob_add_ref, release: blob_release },
    get_buffer_pointer: blob_ptr,
    get_buffer_size: blob_size,
    get_encoding: blob_get_encoding,
    get_string_pointer: blob_str_ptr,
    get_string_length: blob_str_len,
};

impl Blob {
    fn boxed(data: *mut u8, len: usize, enc: Option<u32>, store: Store) -> *mut c_void {
        let b = Box::new(Blob { vtbl: &BLOB_VTBL, refs: AtomicU32::new(1), data, len, known: enc.is_some(), cp: enc.unwrap_or(DXC_CP_ACP), store });
        Box::into_raw(b) as *mut c_void
    }

    pub fn copy(bytes: &[u8], cp: u32) -> *mut c_void {
        let mut owned: Box<[u8]> = bytes.to_vec().into_boxed_slice();
        let (p, n) = (owned.as_mut_ptr(), owned.len());
        Self::boxed(p, n, Some(cp), Store::Owned(owned))
    }

    fn unit(&self) -> usize {
        if self.known && self.cp == DXC_CP_UTF16 {
            2
        } else {
            1
        }
    }

    fn is_text(&self, cp: u32) -> bool {
        self.known && self.cp == cp
    }
}

impl Drop for Blob {
    fn drop(&mut self) {
        unsafe {
            match self.store {
                Store::Malloc(m) => {
                    (vtbl::<MallocVtbl>(m).free)(m, self.data as *mut c_void);
                    release(m);
                }
                Store::CoTask => CoTaskMemFree(self.data as *mut c_void),
                Store::Parent(p) => {
                    release(p);
                }
                Store::Owned(_) | Store::Pinned => {}
            }
        }
    }
}

pub unsafe fn is_native_blob(p: *mut c_void) -> bool {
    !p.is_null() && core::ptr::eq(*(p as *const *const BlobVtbl), &BLOB_VTBL)
}

unsafe extern "system" fn blob_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let Some(iid) = Guid::read(iid as *const c_void) else { return E_POINTER };
    let b = &*(this as *const Blob);
    let ok = iid == IID_IUNKNOWN
        || iid == IID_IDXC_BLOB
        || iid == IID_IDXC_BLOB_ENCODING
        || (iid == IID_IDXC_BLOB_UTF8 && b.is_text(DXC_CP_UTF8))
        || (iid == IID_IDXC_BLOB_UTF16 && b.is_text(DXC_CP_UTF16));
    if !ok {
        return E_NOINTERFACE;
    }
    blob_add_ref(this);
    *out = this;
    S_OK
}

unsafe extern "system" fn blob_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const Blob)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn blob_release(this: *mut c_void) -> u32 {
    let n = (*(this as *const Blob)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this as *mut Blob));
    }
    n
}

unsafe extern "system" fn blob_ptr(this: *mut c_void) -> *mut c_void {
    (*(this as *const Blob)).data as *mut c_void
}

unsafe extern "system" fn blob_size(this: *mut c_void) -> usize {
    (*(this as *const Blob)).len
}

unsafe extern "system" fn blob_get_encoding(this: *mut c_void, known: *mut i32, cp: *mut u32) -> Hr {
    let b = &*(this as *const Blob);
    if !known.is_null() {
        *known = b.known as i32;
    }
    if !cp.is_null() {
        *cp = b.cp;
    }
    S_OK
}

unsafe extern "system" fn blob_str_ptr(this: *mut c_void) -> *const c_void {
    let b = &*(this as *const Blob);
    if b.len == 0 {
        if b.unit() == 2 {
            static EMPTY16: u16 = 0;
            &EMPTY16 as *const u16 as *const c_void
        } else {
            b"\0".as_ptr() as *const c_void
        }
    } else {
        b.data as *const c_void
    }
}

unsafe extern "system" fn blob_str_len(this: *mut c_void) -> usize {
    let b = &*(this as *const Blob);
    let u = b.unit();
    let mut n = b.len / u;
    if n > 0 {
        let last = if u == 2 {
            u16::from_le_bytes([*b.data.add((n - 1) * 2), *b.data.add((n - 1) * 2 + 1)]) as u32
        } else {
            *b.data.add(n - 1) as u32
        };
        if last == 0 {
            n -= 1;
        }
    }
    n
}

pub unsafe fn create_blob(data: *const c_void, size: u32, cp: u32, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if data.is_null() && size != 0 {
        return E_POINTER;
    }
    let bytes = if size == 0 { &[][..] } else { core::slice::from_raw_parts(data as *const u8, size as usize) };
    *out = Blob::copy(bytes, cp);
    S_OK
}

pub unsafe fn create_pinned(data: *const c_void, size: u32, cp: u32, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if data.is_null() && size != 0 {
        return E_POINTER;
    }
    *out = Blob::boxed(data as *mut u8, size as usize, Some(cp), Store::Pinned);
    S_OK
}

pub unsafe fn move_to_blob(data: *const c_void, malloc: *mut c_void, size: u32, cp: u32, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if data.is_null() && size != 0 {
        return E_POINTER;
    }
    let store = if malloc.is_null() {
        Store::CoTask
    } else {
        add_ref(malloc);
        Store::Malloc(malloc)
    };
    *out = Blob::boxed(data as *mut u8, size as usize, Some(cp), store);
    S_OK
}

pub unsafe fn sub_blob(src: *mut c_void, offset: u32, length: u32, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if src.is_null() {
        return E_POINTER;
    }
    let bytes = blob_bytes(src);
    let (o, l) = (offset as usize, length as usize);
    if o > bytes.len() || l > bytes.len() - o {
        return E_INVALIDARG;
    }
    let enc = blob_encoding(src);
    add_ref(src);
    *out = Blob::boxed(bytes.as_ptr().add(o) as *mut u8, l, enc, Store::Parent(src));
    S_OK
}

fn decode_text(bytes: &[u8], cp: Option<u32>) -> String {
    match cp {
        Some(DXC_CP_UTF16) => {
            let w: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
            String::from_utf16_lossy(&w[..end])
        }
        _ => {
            let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
            let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf8_lossy(&b[..end]).into_owned()
        }
    }
}

pub unsafe fn as_utf8(src: *mut c_void, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if src.is_null() {
        return E_POINTER;
    }
    let cp = blob_encoding(src);
    let mut v = decode_text(blob_bytes(src), cp).into_bytes();
    v.push(0);
    *out = Blob::copy(&v, DXC_CP_UTF8);
    S_OK
}

pub unsafe fn as_utf16(src: *mut c_void, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if src.is_null() {
        return E_POINTER;
    }
    let cp = blob_encoding(src);
    let w = wide(&decode_text(blob_bytes(src), cp));
    let v: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    *out = Blob::copy(&v, DXC_CP_UTF16);
    S_OK
}

pub unsafe fn load_file(name: *const u16, cp_io: *mut u32, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let Some(path) = wide_to_string(name) else { return E_POINTER };
    let Ok(bytes) = std::fs::read(&path) else { return HRESULT_FILE_NOT_FOUND };
    let mut cp = if cp_io.is_null() { DXC_CP_ACP } else { *cp_io };
    if cp == DXC_CP_ACP {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            cp = DXC_CP_UTF8;
        } else if bytes.starts_with(&[0xFF, 0xFE]) {
            cp = DXC_CP_UTF16;
        }
    }
    if !cp_io.is_null() {
        *cp_io = cp;
    }
    *out = Blob::copy(&bytes, cp);
    S_OK
}

pub const DXBC_MAGIC: u32 = 0x4342_5844;

pub fn container_part(bytes: &[u8], fourcc: u32) -> Result<(usize, u32), Hr> {
    let rd = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(bytes.get(o..o + 4)?.try_into().ok()?)) };
    if rd(0) != Some(DXBC_MAGIC) {
        return Err(DXC_E_CONTAINER_INVALID);
    }
    let total = rd(24).ok_or(DXC_E_CONTAINER_INVALID)? as usize;
    let count = rd(28).ok_or(DXC_E_CONTAINER_INVALID)? as usize;
    if total > bytes.len() || 32 + count * 4 > total {
        return Err(DXC_E_CONTAINER_INVALID);
    }
    for i in 0..count {
        let off = rd(32 + i * 4).ok_or(DXC_E_CONTAINER_INVALID)? as usize;
        let (cc, sz) = match (rd(off), rd(off + 4)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(DXC_E_CONTAINER_INVALID),
        };
        if off + 8 + sz as usize > total {
            return Err(DXC_E_CONTAINER_INVALID);
        }
        if cc == fourcc {
            return Ok((off + 8, sz));
        }
    }
    Err(DXC_E_MISSING_PART)
}

pub unsafe fn get_container_part(buf: *const DxcBuffer, part: u32, data: *mut *mut c_void, size: *mut u32) -> Hr {
    if buf.is_null() || data.is_null() || size.is_null() {
        return E_POINTER;
    }
    let b = &*buf;
    if b.ptr.is_null() {
        return E_POINTER;
    }
    let bytes = core::slice::from_raw_parts(b.ptr as *const u8, b.size);
    match container_part(bytes, part) {
        Ok((off, sz)) => {
            *data = (b.ptr as *mut u8).add(off) as *mut c_void;
            *size = sz;
            S_OK
        }
        Err(hr) => hr,
    }
}

#[repr(C)]
struct UtilsVtbl {
    unk: UnknownVtbl,
    create_blob_from_blob: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr,
    create_blob_from_pinned: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut *mut c_void) -> Hr,
    move_to_blob: unsafe extern "system" fn(*mut c_void, *const c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr,
    create_blob: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut *mut c_void) -> Hr,
    load_file: unsafe extern "system" fn(*mut c_void, *const u16, *mut u32, *mut *mut c_void) -> Hr,
    create_read_only_stream_from_blob: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
    create_default_include_handler: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hr,
    get_blob_as_utf8: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
    get_blob_as_utf16: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
    get_dxil_container_part: unsafe extern "system" fn(*mut c_void, *const DxcBuffer, u32, *mut *mut c_void, *mut u32) -> Hr,
    create_reflection: unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const Guid, *mut *mut c_void) -> Hr,
    build_arguments: unsafe extern "system" fn(*mut c_void, *const u16, *const u16, *const u16, *const *const u16, u32, *const c_void, u32, *mut *mut c_void) -> Hr,
    get_pdb_contents: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void, *mut *mut c_void) -> Hr,
}

#[repr(C)]
struct LibraryVtbl {
    unk: UnknownVtbl,
    set_malloc: unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hr,
    create_blob_from_blob: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr,
    create_blob_from_file: unsafe extern "system" fn(*mut c_void, *const u16, *mut u32, *mut *mut c_void) -> Hr,
    create_blob_with_encoding_from_pinned: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut *mut c_void) -> Hr,
    create_blob_with_encoding_on_heap_copy: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut *mut c_void) -> Hr,
    create_blob_with_encoding_on_malloc: unsafe extern "system" fn(*mut c_void, *const c_void, *mut c_void, u32, u32, *mut *mut c_void) -> Hr,
    create_include_handler: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hr,
    create_stream_from_blob_read_only: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
    get_blob_as_utf8: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
    get_blob_as_utf16: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
}

#[repr(C)]
struct Utils {
    utils: &'static UtilsVtbl,
    library: &'static LibraryVtbl,
    refs: AtomicU32,
}

unsafe fn from_utils(this: *mut c_void) -> *mut Utils {
    this as *mut Utils
}

unsafe fn from_library(this: *mut c_void) -> *mut Utils {
    (this as *mut u8).sub(core::mem::size_of::<usize>()) as *mut Utils
}

unsafe fn utils_qi_impl(u: *mut Utils, iid: *const Guid, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let Some(iid) = Guid::read(iid as *const c_void) else { return E_POINTER };
    let p = if iid == IID_IUNKNOWN || iid == IID_IDXC_UTILS {
        &mut (*u).utils as *mut _ as *mut c_void
    } else if iid == IID_IDXC_LIBRARY {
        &mut (*u).library as *mut _ as *mut c_void
    } else {
        return E_NOINTERFACE;
    };
    (*u).refs.fetch_add(1, Ordering::AcqRel);
    *out = p;
    S_OK
}

unsafe fn utils_release_impl(u: *mut Utils) -> u32 {
    let n = (*u).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(u));
    }
    n
}

unsafe extern "system" fn u_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> Hr {
    utils_qi_impl(from_utils(this), iid, out)
}
unsafe extern "system" fn u_add_ref(this: *mut c_void) -> u32 {
    (*from_utils(this)).refs.fetch_add(1, Ordering::AcqRel) + 1
}
unsafe extern "system" fn u_release(this: *mut c_void) -> u32 {
    utils_release_impl(from_utils(this))
}
unsafe extern "system" fn l_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> Hr {
    utils_qi_impl(from_library(this), iid, out)
}
unsafe extern "system" fn l_add_ref(this: *mut c_void) -> u32 {
    (*from_library(this)).refs.fetch_add(1, Ordering::AcqRel) + 1
}
unsafe extern "system" fn l_release(this: *mut c_void) -> u32 {
    utils_release_impl(from_library(this))
}

unsafe extern "system" fn x_from_blob(_: *mut c_void, b: *mut c_void, o: u32, l: u32, out: *mut *mut c_void) -> Hr {
    sub_blob(b, o, l, out)
}
unsafe extern "system" fn x_pinned(_: *mut c_void, d: *const c_void, n: u32, cp: u32, out: *mut *mut c_void) -> Hr {
    create_pinned(d, n, cp, out)
}
unsafe extern "system" fn x_move(_: *mut c_void, d: *const c_void, m: *mut c_void, n: u32, cp: u32, out: *mut *mut c_void) -> Hr {
    move_to_blob(d, m, n, cp, out)
}
unsafe extern "system" fn x_create(_: *mut c_void, d: *const c_void, n: u32, cp: u32, out: *mut *mut c_void) -> Hr {
    create_blob(d, n, cp, out)
}
unsafe extern "system" fn x_load(_: *mut c_void, name: *const u16, cp: *mut u32, out: *mut *mut c_void) -> Hr {
    load_file(name, cp, out)
}
unsafe extern "system" fn x_unsupported2(_: *mut c_void, _: *mut c_void, out: *mut *mut c_void) -> Hr {
    put(out, core::ptr::null_mut());
    crate::note("native", "CreateReadOnlyStreamFromBlob/CreateStreamFromBlobReadOnly -> E_NOTIMPL (no host dxcompiler.dll)");
    E_NOTIMPL
}
unsafe extern "system" fn x_unsupported1(_: *mut c_void, out: *mut *mut c_void) -> Hr {
    put(out, core::ptr::null_mut());
    crate::note("native", "CreateDefaultIncludeHandler/CreateIncludeHandler -> E_NOTIMPL (no host dxcompiler.dll)");
    E_NOTIMPL
}
unsafe extern "system" fn x_utf8(_: *mut c_void, b: *mut c_void, out: *mut *mut c_void) -> Hr {
    as_utf8(b, out)
}
unsafe extern "system" fn x_utf16(_: *mut c_void, b: *mut c_void, out: *mut *mut c_void) -> Hr {
    as_utf16(b, out)
}
unsafe extern "system" fn x_part(_: *mut c_void, buf: *const DxcBuffer, part: u32, d: *mut *mut c_void, n: *mut u32) -> Hr {
    get_container_part(buf, part, d, n)
}
unsafe extern "system" fn x_reflection(_: *mut c_void, _: *const DxcBuffer, _: *const Guid, out: *mut *mut c_void) -> Hr {
    put(out, core::ptr::null_mut());
    crate::note("native", "IDxcUtils::CreateReflection -> E_NOTIMPL (no host dxcompiler.dll)");
    E_NOTIMPL
}
unsafe extern "system" fn x_build_args(
    _: *mut c_void,
    _: *const u16,
    _: *const u16,
    _: *const u16,
    _: *const *const u16,
    _: u32,
    _: *const c_void,
    _: u32,
    out: *mut *mut c_void,
) -> Hr {
    put(out, core::ptr::null_mut());
    crate::note("native", "IDxcUtils::BuildArguments -> E_NOTIMPL (no host dxcompiler.dll)");
    E_NOTIMPL
}
unsafe extern "system" fn x_pdb(_: *mut c_void, _: *mut c_void, h: *mut *mut c_void, c: *mut *mut c_void) -> Hr {
    put(h, core::ptr::null_mut());
    put(c, core::ptr::null_mut());
    crate::note("native", "IDxcUtils::GetPDBContents -> E_NOTIMPL (no host dxcompiler.dll)");
    E_NOTIMPL
}
unsafe extern "system" fn x_set_malloc(_: *mut c_void, _: *mut c_void) -> Hr {
    S_OK
}

static UTILS_VTBL: UtilsVtbl = UtilsVtbl {
    unk: UnknownVtbl { query_interface: u_qi, add_ref: u_add_ref, release: u_release },
    create_blob_from_blob: x_from_blob,
    create_blob_from_pinned: x_pinned,
    move_to_blob: x_move,
    create_blob: x_create,
    load_file: x_load,
    create_read_only_stream_from_blob: x_unsupported2,
    create_default_include_handler: x_unsupported1,
    get_blob_as_utf8: x_utf8,
    get_blob_as_utf16: x_utf16,
    get_dxil_container_part: x_part,
    create_reflection: x_reflection,
    build_arguments: x_build_args,
    get_pdb_contents: x_pdb,
};

static LIBRARY_VTBL: LibraryVtbl = LibraryVtbl {
    unk: UnknownVtbl { query_interface: l_qi, add_ref: l_add_ref, release: l_release },
    set_malloc: x_set_malloc,
    create_blob_from_blob: x_from_blob,
    create_blob_from_file: x_load,
    create_blob_with_encoding_from_pinned: x_pinned,
    create_blob_with_encoding_on_heap_copy: x_create,
    create_blob_with_encoding_on_malloc: x_move,
    create_include_handler: x_unsupported1,
    create_stream_from_blob_read_only: x_unsupported2,
    get_blob_as_utf8: x_utf8,
    get_blob_as_utf16: x_utf16,
};

pub unsafe fn create_utils(iid: *const Guid, out: *mut *mut c_void) -> Hr {
    let u = Box::into_raw(Box::new(Utils { utils: &UTILS_VTBL, library: &LIBRARY_VTBL, refs: AtomicU32::new(1) }));
    let hr = utils_qi_impl(u, iid, out);
    utils_release_impl(u);
    hr
}
