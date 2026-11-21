use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ffi::c_void;
use std::ptr::null_mut;

pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;

pub const VIDEO_ENCODE: u32 = 6;
pub const PARAMS_TAG: u32 = 0x64;
pub const MAX_RESOURCES: usize = 5;

pub const IID_DEVICE: [u8; 16] = guid(0x189819f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);
pub const IID_ALLOCATOR: [u8; 16] = guid(0x6102dee4, 0xaf59, 0x4b09, [0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24]);
pub const IID_ENCODE_LIST1: [u8; 16] = guid(0x94971eca, 0x2bdb, 0x4769, [0x88, 0xcf, 0x36, 0x75, 0xea, 0x75, 0x7e, 0xbc]);
pub const IID_QUEUE: [u8; 16] = guid(0x0ec870a6, 0x5d7e, 0x4c22, [0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed]);
pub const IID_VIDEO_DEVICE2: [u8; 16] = guid(0xf019ac49, 0xf838, 0x4a95, [0x9b, 0x17, 0x57, 0x94, 0x37, 0xc8, 0xf5, 0x13]);
pub const IID_EXTENSION_COMMAND: [u8; 16] = guid(0x554e41e8, 0xae8e, 0x4a8c, [0xb7, 0xd2, 0x5b, 0x4f, 0x27, 0x4a, 0x30, 0xe4]);
pub const IID_FENCE: [u8; 16] = guid(0x0a753dcf, 0xc4d8, 0x4b91, [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76]);
pub const AVE_COMMAND_ID: [u8; 16] = guid(0xa218961c, 0x091f, 0x494e, [0x91, 0x88, 0x65, 0xe2, 0x18, 0x7e, 0x69, 0xc0]);

const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventA(attrs: *mut c_void, manual: i32, initial: i32, name: *const u8) -> *mut c_void;
    fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
    fn CloseHandle(h: *mut c_void) -> i32;
}

#[repr(C)]
pub struct Package {
    pub cmd: u32,
    pub pad: u32,
    pub handle: u64,
    pub status: u32,
    pub pad2: u32,
    pub a: *mut c_void,
    pub b: *mut c_void,
}

impl Package {
    pub fn new(cmd: u32, handle: u64, a: *mut c_void, b: *mut c_void) -> Self {
        Package { cmd, pad: 0, handle, status: 0, pad2: 0, a, b }
    }
}

#[repr(C)]
pub struct Params {
    pub tag: u32,
    pub pad: u32,
    pub package: *mut Package,
    pub data: [*mut c_void; MAX_RESOURCES],
    pub sizes: [u32; MAX_RESOURCES],
    pub pad2: u32,
}

impl Params {
    fn new(package: *mut Package) -> Self {
        Params { tag: PARAMS_TAG, pad: 0, package, data: [null_mut(); MAX_RESOURCES], sizes: [0; MAX_RESOURCES], pad2: 0 }
    }
}

#[repr(C)]
pub struct Resource {
    pub data: *mut c_void,
    pub size: u32,
}

#[repr(C)]
pub struct InputList {
    pub reserved: u64,
    pub count: u32,
    pub pad: u32,
    pub items: *const Input,
}

#[repr(C)]
pub struct Input {
    pub kind: u32,
    pub pad: u32,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct FenceSync {
    pub signal_fence: *mut c_void,
    pub signal_value: u64,
    pub wait_fence: *mut c_void,
    pub wait_value: u64,
}

pub type DeleteFn = unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void;
pub type ExecuteFn = unsafe extern "system" fn(*mut Transport, *mut Package, u32, *const *const Resource) -> i32;

#[repr(C)]
pub struct TransportVtbl {
    pub delete: DeleteFn,
    pub execute: ExecuteFn,
}

#[repr(C)]
pub struct Transport {
    pub vtbl: *const TransportVtbl,
    pub init: u8,
    pub device: *mut c_void,
    pub queue: *mut c_void,
    pub list: *mut c_void,
    pub ext: *mut c_void,
    pub allocators: [*mut c_void; 3],
    pub fence_values: [u64; 3],
    pub index: u32,
    pub video: *mut c_void,
    pub fence: *mut c_void,
    pub value: u64,
    pub event: *mut c_void,
}

const _: () = assert!(std::mem::size_of::<Transport>() == 0x88);
const _: () = assert!(std::mem::size_of::<Package>() == 0x28);
const _: () = assert!(std::mem::size_of::<Params>() == 0x50);

pub static TRANSPORT_VTBL: TransportVtbl = TransportVtbl { delete: transport_delete, execute: transport_execute };

pub unsafe fn slot<F: Copy>(obj: *mut c_void, index: usize) -> F {
    let vtbl = *(obj as *const *const usize);
    std::mem::transmute_copy(&*vtbl.add(index))
}

pub unsafe fn delete_object(obj: *mut c_void) {
    let f: DeleteFn = slot(obj, 0);
    f(obj, 1);
}

pub unsafe fn release(obj: *mut c_void) -> u32 {
    let f: unsafe extern "system" fn(*mut c_void) -> u32 = slot(obj, 2);
    f(obj)
}

pub fn legacy_build() -> bool {
    build() < 22000
}

#[cfg(test)]
thread_local! {
    pub static BUILD_OVERRIDE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

fn build() -> u32 {
    #[cfg(test)]
    if let Some(b) = BUILD_OVERRIDE.with(|c| c.get()) {
        return b;
    }
    compat_common::title_os::title_os_build().unwrap_or(22621)
}

pub unsafe fn alloc_object<T>() -> *mut T {
    alloc_zeroed(Layout::new::<T>()) as *mut T
}

pub unsafe fn free_object<T>(p: *mut T) {
    dealloc(p as *mut u8, Layout::new::<T>());
}

unsafe fn release_slot(p: &mut *mut c_void) {
    if !p.is_null() {
        release(*p);
        *p = null_mut();
    }
}

unsafe extern "system" fn transport_delete(this: *mut c_void, flags: u32) -> *mut c_void {
    let t = this as *mut Transport;
    destroy_transport(&mut *t);
    if flags & 1 != 0 {
        free_object(t);
    }
    this
}

unsafe fn destroy_transport(t: &mut Transport) {
    t.init = 0;
    release_slot(&mut t.device);
    release_slot(&mut t.queue);
    release_slot(&mut t.list);
    release_slot(&mut t.ext);
    release_slot(&mut t.video);
    for a in t.allocators.iter_mut() {
        release_slot(a);
    }
    release_slot(&mut t.fence);
    if !t.event.is_null() {
        CloseHandle(t.event);
    }
}

pub unsafe fn create_transport(device: *mut c_void, out: *mut *mut Transport) -> i32 {
    let t = alloc_object::<Transport>();
    if t.is_null() {
        *out = null_mut();
        return E_OUTOFMEMORY;
    }
    (*t).vtbl = &TRANSPORT_VTBL;
    let hr = if device.is_null() {
        E_INVALIDARG
    } else {
        let qi: unsafe extern "system" fn(*mut c_void, *const [u8; 16], *mut *mut c_void) -> i32 = slot(device, 0);
        let q = qi(device, &IID_DEVICE, &mut (*t).device);
        if q < 0 {
            if legacy_build() { E_FAIL } else { q }
        } else {
            init_transport(&mut *t)
        }
    };
    *out = t;
    if hr != 0 { E_OUTOFMEMORY } else { 0 }
}

unsafe fn init_transport(t: &mut Transport) -> i32 {
    let hr = init_steps(t);
    if hr < 0 && legacy_build() { E_FAIL } else { hr }
}

unsafe fn init_steps(t: &mut Transport) -> i32 {
    let device = t.device;
    let create_allocator: unsafe extern "system" fn(*mut c_void, u32, *const [u8; 16], *mut *mut c_void) -> i32 = slot(device, 9);
    for i in 0..3 {
        let hr = create_allocator(device, VIDEO_ENCODE, &IID_ALLOCATOR, &mut t.allocators[i]);
        if hr < 0 {
            return hr;
        }
        t.fence_values[i] = 0;
    }
    t.index = 0;
    let list_params: [u32; 4] = [VIDEO_ENCODE, 0, 0, 0];
    let create_list: unsafe extern "system" fn(*mut c_void, *const [u32; 4], *mut c_void, *mut c_void, *const [u8; 16], *mut *mut c_void) -> i32 = slot(device, 73);
    let hr = create_list(device, &list_params, t.allocators[0], null_mut(), &IID_ENCODE_LIST1, &mut t.list);
    if hr < 0 {
        return hr;
    }
    let queue_desc: [u32; 4] = [VIDEO_ENCODE, 0, 0, 0];
    let create_queue: unsafe extern "system" fn(*mut c_void, *const [u32; 4], *const [u8; 16], *mut *mut c_void) -> i32 = slot(device, 8);
    let hr = create_queue(device, &queue_desc, &IID_QUEUE, &mut t.queue);
    if hr < 0 {
        return hr;
    }
    let qi: unsafe extern "system" fn(*mut c_void, *const [u8; 16], *mut *mut c_void) -> i32 = slot(device, 0);
    let hr = qi(device, &IID_VIDEO_DEVICE2, &mut t.video);
    if hr < 0 {
        return hr;
    }
    let mut desc = [0u8; 20];
    desc[4..].copy_from_slice(&AVE_COMMAND_ID);
    let creation: u32 = 0;
    let create_ext: unsafe extern "system" fn(*mut c_void, *const [u8; 20], *const u32, usize, *mut c_void, *const [u8; 16], *mut *mut c_void) -> i32 = slot(t.video, 12);
    let hr = create_ext(t.video, &desc, &creation, 4, null_mut(), &IID_EXTENSION_COMMAND, &mut t.ext);
    if hr < 0 {
        return hr;
    }
    let create_fence: unsafe extern "system" fn(*mut c_void, u64, u32, *const [u8; 16], *mut *mut c_void) -> i32 = slot(device, 36);
    let hr = create_fence(device, 0, 0, &IID_FENCE, &mut t.fence);
    if hr < 0 {
        return hr;
    }
    t.event = CreateEventA(null_mut(), 0, 0, std::ptr::null());
    let init_params = [0u8; 16];
    let init_ext: unsafe extern "system" fn(*mut c_void, *mut c_void, *const [u8; 16], usize) = slot(t.list, 25);
    init_ext(t.list, t.ext, &init_params, 0x10);
    let close: unsafe extern "system" fn(*mut c_void) -> i32 = slot(t.list, 9);
    close(t.list);
    let execute_lists: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) = slot(t.queue, 10);
    execute_lists(t.queue, 1, &t.list);
    t.value += 1;
    t.fence_values[t.index as usize] = t.value;
    let signal: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = slot(t.queue, 14);
    signal(t.queue, t.fence, t.value);
    t.index = (t.index + 1) % 3;
    t.init = 1;
    0
}

unsafe fn wait_fence(t: &Transport, target: u64) -> i32 {
    let completed: unsafe extern "system" fn(*mut c_void) -> u64 = slot(t.fence, 8);
    if completed(t.fence) < target {
        let on_completion: unsafe extern "system" fn(*mut c_void, u64, *mut c_void) -> i32 = slot(t.fence, 9);
        let hr = on_completion(t.fence, target, t.event);
        if hr < 0 {
            return hr;
        }
        WaitForSingleObject(t.event, u32::MAX);
    }
    0
}

unsafe fn reset_list(t: &Transport) {
    let allocator = t.allocators[t.index as usize];
    let reset_allocator: unsafe extern "system" fn(*mut c_void) -> i32 = slot(allocator, 8);
    reset_allocator(allocator);
    let reset: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 = slot(t.list, 10);
    reset(t.list, allocator);
}

unsafe fn record(t: &Transport, params: &Params) -> i32 {
    let execute_ext: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Params, usize) = slot(t.list, 26);
    execute_ext(t.list, t.ext, params, std::mem::size_of::<Params>());
    let close: unsafe extern "system" fn(*mut c_void) -> i32 = slot(t.list, 9);
    close(t.list)
}

pub unsafe extern "system" fn transport_execute(this: *mut Transport, pkg: *mut Package, count: u32, res: *const *const Resource) -> i32 {
    let t = &mut *this;
    if t.init == 0 {
        return E_FAIL;
    }
    if pkg.is_null() || (count != 0 && res.is_null()) || count as usize > MAX_RESOURCES {
        return E_INVALIDARG;
    }
    let mut sync = FenceSync { signal_fence: null_mut(), signal_value: 0, wait_fence: null_mut(), wait_value: 0 };
    if (*pkg).cmd == 0xd {
        let inputs = (*pkg).a as *const InputList;
        for i in 0..(*inputs).count as usize {
            let item = &*(*inputs).items.add(i);
            if item.kind == 6 {
                let s = &*(item.data as *const FenceSync);
                sync = FenceSync { signal_fence: s.signal_fence, signal_value: s.signal_value, wait_fence: s.wait_fence, wait_value: s.wait_value };
            }
        }
    }
    let mut params = Params::new(pkg);
    let hr = wait_fence(t, t.fence_values[t.index as usize]);
    if hr < 0 {
        return hr;
    }
    reset_list(t);
    for i in 0..count as usize {
        let r = &**res.add(i);
        params.data[i] = r.data;
        params.sizes[i] = r.size;
    }
    let hr = record(t, &params);
    if hr < 0 {
        return hr;
    }
    if !sync.wait_fence.is_null() {
        let wait: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = slot(t.queue, 15);
        let hr = wait(t.queue, sync.wait_fence, sync.wait_value);
        if hr < 0 {
            return hr;
        }
    }
    let execute_lists: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) = slot(t.queue, 10);
    execute_lists(t.queue, 1, &t.list);
    t.value += 1;
    t.fence_values[t.index as usize] = t.value;
    t.index = (t.index + 1) % 3;
    let signal: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = slot(t.queue, 14);
    let mut hr = signal(t.queue, t.fence, t.value);
    if hr < 0 {
        return hr;
    }
    if !sync.signal_fence.is_null() {
        hr = signal(t.queue, sync.signal_fence, sync.signal_value);
        if hr < 0 {
            return hr;
        }
    }
    if (*pkg).cmd != 0x19 && (*pkg).cmd != 6 {
        return hr;
    }
    let hr = wait_fence(t, t.value);
    if hr < 0 {
        return hr;
    }
    let mut follow = Package::new(0x23, 0, (*pkg).a, null_mut());
    let params = Params::new(&mut follow);
    reset_list(t);
    record(t, &params)
}
