use crate::transport::*;
use std::ffi::c_void;
use std::ptr::null_mut;

pub const INTERFACE_VERSION: u32 = 0x30017;

#[repr(C)]
pub struct Factory {
    pub vtbl: *const DeleteFn,
    pub transport: *mut Transport,
    pub init: u8,
    pub version: u32,
}

#[repr(C)]
pub struct CriticalSection {
    pub raw: [u64; 5],
}

#[repr(C)]
pub struct Encoder {
    pub vtbl: *const DeleteFn,
    pub handle: u64,
    pub transport: *mut Transport,
    pub init: u8,
    pub closed: u8,
    pub cs: CriticalSection,
}

const _: () = assert!(std::mem::size_of::<Factory>() == 0x18);
const _: () = assert!(std::mem::size_of::<Encoder>() == 0x48);

#[link(name = "kernel32")]
extern "system" {
    fn InitializeCriticalSection(cs: *mut CriticalSection);
    fn DeleteCriticalSection(cs: *mut CriticalSection);
}

static FACTORY_VTBL: DeleteFn = factory_delete;
static ENCODER_VTBL: DeleteFn = encoder_delete;

pub fn bad_pointer(legacy: i32) -> i32 {
    if legacy_build() { legacy } else { E_POINTER }
}

pub fn closed_code() -> i32 {
    if legacy_build() { E_FAIL } else { E_UNEXPECTED }
}

unsafe fn u32_at(p: *mut c_void, off: usize) -> u32 {
    *((p as *const u8).add(off) as *const u32)
}

unsafe fn u64_at(p: *mut c_void, off: usize) -> u64 {
    *((p as *const u8).add(off) as *const u64)
}

unsafe fn put_u32(p: *mut c_void, off: usize, v: u32) {
    *((p as *mut u8).add(off) as *mut u32) = v;
}

unsafe fn put_u64(p: *mut c_void, off: usize, v: u64) {
    *((p as *mut u8).add(off) as *mut u64) = v;
}

unsafe fn execute(t: *mut Transport, pkg: &mut Package, count: u32, res: *const *const Resource) -> i32 {
    ((*(*t).vtbl).execute)(t, pkg, count, res)
}

unsafe extern "system" fn factory_delete(this: *mut c_void, flags: u32) -> *mut c_void {
    let f = this as *mut Factory;
    (*f).vtbl = &FACTORY_VTBL;
    if !(*f).transport.is_null() {
        delete_object((*f).transport as *mut c_void);
        (*f).transport = null_mut();
    }
    (*f).init = 0;
    if flags & 1 != 0 {
        free_object(f);
    }
    this
}

unsafe extern "system" fn encoder_delete(this: *mut c_void, flags: u32) -> *mut c_void {
    let e = this as *mut Encoder;
    (*e).vtbl = &ENCODER_VTBL;
    if (*e).init != 0 {
        (*e).init = 0;
        DeleteCriticalSection(&mut (*e).cs);
    }
    if flags & 1 != 0 {
        free_object(e);
    }
    this
}

unsafe fn factory_init(f: &mut Factory, kind: u32, device: *mut c_void, args: *mut c_void, reply: *mut c_void) -> i32 {
    if args.is_null() || reply.is_null() {
        return E_INVALIDARG;
    }
    f.transport = null_mut();
    f.version = u32_at(args, 8);
    if kind != 3 {
        return E_INVALIDARG;
    }
    let hr = create_transport(device, &mut f.transport);
    if f.transport.is_null() {
        return E_OUTOFMEMORY;
    }
    if hr != 0 {
        delete_object(f.transport as *mut c_void);
        f.transport = null_mut();
        return hr;
    }
    let mut pkg = Package::new(1, 0, args, reply);
    let hr = execute(f.transport, &mut pkg, 0, std::ptr::null());
    if hr != 0 {
        delete_object(f.transport as *mut c_void);
        f.transport = null_mut();
        return hr;
    }
    if u32_at(reply, 8) != INTERFACE_VERSION {
        delete_object(f.transport as *mut c_void);
        f.transport = null_mut();
        return if legacy_build() { E_FAIL } else { E_INVALIDARG };
    }
    f.init = 1;
    0
}

pub unsafe fn create_factory(desc: *mut c_void, args: *mut c_void, out: *mut c_void, reply: *mut c_void) -> i32 {
    if desc.is_null() || args.is_null() || out.is_null() || reply.is_null() || u64_at(desc, 0x10) == 0 {
        return E_INVALIDARG;
    }
    put_u64(out, 8, 0);
    put_u32(out, 0x10, INTERFACE_VERSION);
    let f = alloc_object::<Factory>();
    if f.is_null() {
        return E_OUTOFMEMORY;
    }
    (*f).vtbl = &FACTORY_VTBL;
    let hr = factory_init(&mut *f, u32_at(desc, 8), u64_at(desc, 0x10) as *mut c_void, args, reply);
    if hr != 0 {
        delete_object(f as *mut c_void);
        return hr;
    }
    put_u64(out, 8, f as u64);
    0
}

pub unsafe fn destroy_object(holder: *mut c_void, out: *mut c_void) -> i32 {
    if holder.is_null() || out.is_null() || u64_at(holder, 8) == 0 {
        return E_INVALIDARG;
    }
    put_u32(out, 4, 0);
    delete_object(u64_at(holder, 8) as *mut c_void);
    0
}

pub unsafe fn factory_command(f: *mut Factory, cmd: u32, a: *mut c_void, b: *mut c_void, needs: Option<(bool, usize)>) -> i32 {
    if f.is_null() {
        return E_INVALIDARG;
    }
    if (*f).init == 0 {
        return E_FAIL;
    }
    if a.is_null() || b.is_null() {
        return bad_pointer(E_FAIL);
    }
    if let Some((in_b, off)) = needs {
        if u64_at(if in_b { b } else { a }, off) == 0 {
            return bad_pointer(E_FAIL);
        }
    }
    let mut pkg = Package::new(cmd, 0, a, b);
    let hr = execute((*f).transport, &mut pkg, 0, std::ptr::null());
    if hr != 0 { hr } else { pkg.status as i32 }
}

pub unsafe fn create_encoder(f: *mut Factory, a: *mut c_void, b: *mut c_void) -> i32 {
    if f.is_null() {
        return E_INVALIDARG;
    }
    if (*f).init == 0 {
        return E_FAIL;
    }
    if a.is_null() || b.is_null() || u64_at(a, 0x10) == 0 {
        return bad_pointer(E_FAIL);
    }
    put_u32(b, 4, 0);
    put_u64(b, 8, 0);
    let mut pkg = Package::new(5, 0, a, null_mut());
    let hr = execute((*f).transport, &mut pkg, 0, std::ptr::null());
    if hr != 0 {
        return hr;
    }
    if pkg.status != 0 {
        return pkg.status as i32;
    }
    let handle = pkg.b as u64;
    let e = alloc_object::<Encoder>();
    let mut hr = 0;
    if !e.is_null() {
        (*e).vtbl = &ENCODER_VTBL;
        (*e).handle = handle;
        (*e).transport = (*f).transport;
        if !(*e).transport.is_null() && handle != 0 {
            (*e).init = 1;
            InitializeCriticalSection(&mut (*e).cs);
        } else {
            hr = E_INVALIDARG;
        }
        if hr == 0 {
            put_u64(b, 8, e as u64);
            return 0;
        }
    }
    let mut info: [u32; 2] = [8, 0];
    let mut destroy = Package::new(6, 0, handle as *mut c_void, info.as_mut_ptr() as *mut c_void);
    execute((*f).transport, &mut destroy, 0, std::ptr::null());
    if e.is_null() {
        return E_OUTOFMEMORY;
    }
    delete_object(e as *mut c_void);
    hr
}

pub unsafe fn destroy_encoder(f: *mut Factory, holder: *mut c_void, b: *mut c_void) -> i32 {
    if f.is_null() {
        return E_INVALIDARG;
    }
    if (*f).init == 0 {
        return E_FAIL;
    }
    if holder.is_null() || b.is_null() || u64_at(holder, 8) == 0 {
        return bad_pointer(E_INVALIDARG);
    }
    let e = u64_at(holder, 8) as *mut Encoder;
    let mut pkg = Package::new(6, 0, (*e).handle as *mut c_void, b);
    execute((*f).transport, &mut pkg, 0, std::ptr::null());
    delete_object(e as *mut c_void);
    pkg.status as i32
}

#[derive(Clone, Copy)]
pub enum Check {
    None,
    A(usize),
    B(usize),
    ClosedAfter,
    ClosedFirst,
    BThenClosed(usize),
}

pub unsafe fn encoder_command(e: *mut Encoder, cmd: u32, a: *mut c_void, b: *mut c_void, check: Check) -> i32 {
    if e.is_null() {
        return E_INVALIDARG;
    }
    if (*e).init == 0 {
        return E_FAIL;
    }
    if matches!(check, Check::ClosedFirst) && (*e).closed != 0 {
        return closed_code();
    }
    if a.is_null() || b.is_null() {
        return bad_pointer(E_FAIL);
    }
    let field = match check {
        Check::A(off) => Some(u64_at(a, off)),
        Check::B(off) | Check::BThenClosed(off) => Some(u64_at(b, off)),
        _ => None,
    };
    if field == Some(0) {
        return bad_pointer(E_FAIL);
    }
    if matches!(check, Check::ClosedAfter | Check::BThenClosed(_)) && (*e).closed != 0 {
        return closed_code();
    }
    let mut pkg = Package::new(cmd, (*e).handle, a, b);
    let hr = execute((*e).transport, &mut pkg, 0, std::ptr::null());
    if hr != 0 { hr } else { pkg.status as i32 }
}

pub unsafe fn encode(e: *mut Encoder, a: *mut c_void, b: *mut c_void) -> i32 {
    if e.is_null() {
        return E_INVALIDARG;
    }
    if (*e).init == 0 {
        return E_FAIL;
    }
    if (*e).closed != 0 {
        return closed_code();
    }
    if a.is_null() || b.is_null() || u64_at(a, 0x10) == 0 {
        return E_INVALIDARG;
    }
    let mut pkg = Package::new(0xd, (*e).handle, a, b);
    let inputs = &*(a as *const InputList);
    let mut picked: Vec<*const Resource> = Vec::new();
    if picked.try_reserve_exact(inputs.count as usize).is_err() {
        return E_OUTOFMEMORY;
    }
    for i in 0..inputs.count as usize {
        let item = &*inputs.items.add(i);
        if item.kind == 2 || item.kind == 5 {
            picked.push(item.data as *const Resource);
        }
    }
    let hr = execute((*e).transport, &mut pkg, picked.len() as u32, picked.as_ptr());
    if hr != 0 { hr } else { pkg.status as i32 }
}
