use crate::{Family, Guid, E_NOINTERFACE, S_OK};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::collections::HashMap;
use std::sync::Mutex;

pub const KIND_BUFFER_DESC: u32 = 1;
pub const KIND_TEXTURE_DESC: u32 = 2;
pub const KIND_DESTROY_CALLBACK: u32 = 3;
pub const KIND_HEAP_MEMORY: u32 = 4;
pub const KIND_HEAP_COUNT: u32 = 5;

pub const IID_ID3D12_DESCRIPTOR_HEAP: Guid = Guid::new(0x8efb_471d, 0x616c, 0x4f49, [0x90, 0xf7, 0x12, 0x7b, 0xb7, 0x63, 0xfa, 0x51]);
pub const DESTROY_CALLBACK_GUID: Guid = Guid::new(0x5f1c_0d3a, 0x7b2e, 0x4c61, [0x9a, 0x40, 0x78, 0x67, 0x73, 0x31, 0x32, 0x58]);

extern "system" {
    fn GetModuleHandleW(p: *const u16) -> *mut c_void;
}

type WriteFn = unsafe extern "system" fn(usize, u32, u32, u32, u32) -> u32;
type ReadFn = unsafe extern "system" fn(usize, u32, u32) -> u32;
type RemoveFn = unsafe extern "system" fn(usize, u32) -> u32;

static LOCAL: Mutex<Option<HashMap<(usize, u32), Vec<u32>>>> = Mutex::new(None);

fn local<R>(f: impl FnOnce(&mut HashMap<(usize, u32), Vec<u32>>) -> R) -> R {
    let mut g = LOCAL.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

fn runtime(name: &[u8]) -> usize {
    let h = unsafe { GetModuleHandleW(crate::host::wide("d3d12_xs.dll").as_ptr()) };
    crate::host::proc_in(h, name)
}

pub fn state_write(obj: usize, kind: u32, index: u32, mask: u32, value: u32) -> u32 {
    let p = runtime(b"TomBoxDriverStateWrite\0");
    if p != 0 {
        let f: WriteFn = unsafe { core::mem::transmute(p) };
        return unsafe { f(obj, kind, index, mask, value) };
    }
    local(|t| {
        let w = t.entry((obj, kind)).or_default();
        let i = index as usize;
        if w.len() <= i {
            w.resize(i + 1, 0);
        }
        w[i] = (w[i] & !mask) | (value & mask);
        w[i]
    })
}

pub fn state_read(obj: usize, kind: u32, index: u32) -> u32 {
    let p = runtime(b"TomBoxDriverStateRead\0");
    if p != 0 {
        let f: ReadFn = unsafe { core::mem::transmute(p) };
        return unsafe { f(obj, kind, index) };
    }
    local(|t| t.get(&(obj, kind)).and_then(|w| w.get(index as usize).copied()).unwrap_or(0))
}

pub fn state_remove(obj: usize, kind: u32) {
    let p = runtime(b"TomBoxDriverStateRemove\0");
    if p != 0 {
        let f: RemoveFn = unsafe { core::mem::transmute(p) };
        unsafe { f(obj, kind) };
        return;
    }
    local(|t| t.remove(&(obj, kind)));
}

fn write_u64(obj: usize, kind: u32, index: u32, v: u64) {
    state_write(obj, kind, index, u32::MAX, v as u32);
    state_write(obj, kind, index + 1, u32::MAX, (v >> 32) as u32);
}

fn read_u64(obj: usize, kind: u32, index: u32) -> u64 {
    state_read(obj, kind, index) as u64 | (state_read(obj, kind, index + 1) as u64) << 32
}

fn bit(v: u32, from: u32, to: u32) -> u32 {
    ((v >> from) & 1) << to
}

pub fn texture_x_index(family: Family) -> u32 {
    match family {
        Family::One => 0x8c / 4,
        Family::Scarlett => 0xac / 4,
    }
}

#[repr(C)]
pub struct DescriptorUpdate {
    pub sub: u32,
    pub flags: u32,
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub d: u32,
    pub e: u32,
}

pub unsafe fn update_descriptor(family: Family, obj: usize, s: *const DescriptorUpdate) {
    let s = &*s;
    let f = s.flags;
    match s.sub {
        1 => {
            let k = KIND_BUFFER_DESC;
            state_write(obj, k, 1, 1 << 14 | 1 << 13 | 1 << 26, bit(f, 0, 14) | bit(f, 1, 13) | bit(f, 2, 26));
            state_write(obj, k, 0, 7, bit(f, 3, 0) | bit(f, 4, 1) | bit(f, 5, 2));
            state_write(obj, k, 2, u32::MAX, s.a);
            state_write(obj, k, 3, u32::MAX, s.b);
        }
        3 => {
            let k = KIND_TEXTURE_DESC;
            let e = s.e;
            state_write(obj, k, 1, 0x60, bit(f, 0, 6) | bit(f, 1, 5));
            state_write(obj, k, texture_x_index(family), 0xf, bit(f, 2, 0) | bit(f, 3, 1) | (e & 0xc));
            state_write(obj, k, 0, 0x3f, (e & 3) | bit(f, 4, 2) | bit(f, 5, 3) | bit(f, 6, 4) | bit(f, 7, 5));
            state_write(obj, k, 3, u32::MAX, s.a);
            state_write(obj, k, 2, u32::MAX, s.b);
            state_write(obj, k, 4, u32::MAX, s.c);
            state_write(obj, k, 5, u32::MAX, s.d);
        }
        _ => {}
    }
}

#[repr(C)]
struct HeapDesc {
    ty: u32,
    num: u32,
    flags: u32,
    node_mask: u32,
}

pub unsafe fn create_heap(family: Family, device: *mut c_void, memory: u64, desc: u64, out: *mut *mut c_void) -> i32 {
    let ty = desc as u32;
    let d = HeapDesc { ty, num: (desc >> 32) as u32, flags: shader_visible_flags(ty), node_mask: 0 };
    let vt = *(device as *const *const usize);
    let f: unsafe extern "system" fn(*mut c_void, *const HeapDesc, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(*vt.add(14));
    let mut heap: *mut c_void = core::ptr::null_mut();
    let hr = f(device, &d, &IID_ID3D12_DESCRIPTOR_HEAP, &mut heap);
    if hr < 0 {
        return hr;
    }
    write_u64(heap as usize, KIND_HEAP_MEMORY, 0, memory);
    let n = state_read(device as usize, KIND_HEAP_COUNT, 0);
    state_write(device as usize, KIND_HEAP_COUNT, 0, u32::MAX, n.wrapping_add(1));
    let reg = runtime(b"TomBoxTitleHeapRegister\0");
    if reg != 0 {
        let f: unsafe extern "system" fn(usize, u64, u64, u32) -> i32 = core::mem::transmute(reg);
        f(heap as usize, memory, desc, (family == Family::Scarlett) as u32);
    }
    *out = heap;
    S_OK
}

pub fn shader_visible_flags(ty: u32) -> u32 {
    matches!(ty, 0 | 1 | 4) as u32
}

pub fn heap_memory(heap: usize) -> u64 {
    read_u64(heap, KIND_HEAP_MEMORY, 0)
}

#[repr(C)]
struct Sentinel {
    vtbl: *const SentinelVtbl,
    refs: AtomicU32,
    device: usize,
    armed: AtomicBool,
}

static SENTINELS: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);

fn sentinels<R>(f: impl FnOnce(&mut HashMap<usize, usize>) -> R) -> R {
    let mut g = SENTINELS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

const IID_IUNKNOWN: Guid = Guid::new(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);

unsafe extern "system" fn s_qi(this: *mut c_void, riid: *const Guid, pp: *mut *mut c_void) -> i32 {
    if pp.is_null() {
        return crate::E_POINTER;
    }
    if !riid.is_null() && *riid == IID_IUNKNOWN {
        s_addref(this);
        *pp = this;
        return S_OK;
    }
    *pp = core::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn s_addref(this: *mut c_void) -> u32 {
    (*(this as *const Sentinel)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn s_release(this: *mut c_void) -> u32 {
    let s = this as *mut Sentinel;
    let prev = (*s).refs.fetch_sub(1, Ordering::AcqRel);
    if prev != 1 {
        return prev - 1;
    }
    let dev = (*s).device;
    sentinels(|m| {
        if m.get(&dev) == Some(&(s as usize)) {
            m.remove(&dev);
        }
    });
    if (*s).armed.load(Ordering::Acquire) {
        let cb = read_u64(dev, KIND_DESTROY_CALLBACK, 0) as usize;
        let ctx = read_u64(dev, KIND_DESTROY_CALLBACK, 2) as usize;
        state_remove(dev, KIND_DESTROY_CALLBACK);
        if cb != 0 {
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = core::mem::transmute(cb);
            f(dev as *mut c_void, ctx as *mut c_void);
        }
    }
    drop(Box::from_raw(s));
    0
}

struct SentinelVtbl([*const (); 3]);

unsafe impl Sync for SentinelVtbl {}

static SENTINEL_VTBL: SentinelVtbl = SentinelVtbl([s_qi as *const (), s_addref as *const (), s_release as *const ()]);

pub unsafe fn set_destroy_callback(device: *mut c_void, cb: usize, ctx: usize) -> i32 {
    let dev = device as usize;
    let ctx = if cb != 0 { ctx } else { 0 };
    write_u64(dev, KIND_DESTROY_CALLBACK, 0, cb as u64);
    write_u64(dev, KIND_DESTROY_CALLBACK, 2, ctx as u64);
    if let Some(old) = sentinels(|m| m.remove(&dev)) {
        (*(old as *const Sentinel)).armed.store(false, Ordering::Release);
    }
    let vt = *(device as *const *const usize);
    let set: unsafe extern "system" fn(*mut c_void, *const Guid, *mut c_void) -> i32 = core::mem::transmute(*vt.add(5));
    if cb == 0 {
        set(device, &DESTROY_CALLBACK_GUID, core::ptr::null_mut());
        return S_OK;
    }
    let s = Box::into_raw(Box::new(Sentinel { vtbl: &SENTINEL_VTBL, refs: AtomicU32::new(1), device: dev, armed: AtomicBool::new(true) }));
    sentinels(|m| m.insert(dev, s as usize));
    set(device, &DESTROY_CALLBACK_GUID, s as *mut c_void);
    s_release(s as *mut c_void);
    S_OK
}

pub fn destroy_callback(device: usize) -> (usize, usize) {
    (read_u64(device, KIND_DESTROY_CALLBACK, 0) as usize, read_u64(device, KIND_DESTROY_CALLBACK, 2) as usize)
}

pub unsafe fn call(family: Family, code: u32, args: *mut u64) -> i32 {
    match (code, family) {
        (1, _) => {
            update_descriptor(family, *args as usize, args.add(1) as *const DescriptorUpdate);
            S_OK
        }
        (2, _) => create_heap(family, *args as *mut c_void, *args.add(2), *args.add(3), *args.add(4) as *mut *mut c_void),
        (3, Family::Scarlett) => set_destroy_callback(*args as *mut c_void, *args.add(1) as usize, *args.add(2) as usize),
        _ => E_NOINTERFACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicUsize;

    #[repr(C)]
    struct FakeDevice {
        vtbl: *const usize,
        private: AtomicUsize,
        heaps: AtomicU32,
        last_desc: [AtomicU32; 4],
    }

    unsafe extern "system" fn fd_set_iface(this: *mut c_void, g: *const Guid, p: *mut c_void) -> i32 {
        assert_eq!(*g, DESTROY_CALLBACK_GUID);
        let d = this as *mut FakeDevice;
        if !p.is_null() {
            s_addref(p);
        }
        let old = (*d).private.swap(p as usize, Ordering::AcqRel);
        if old != 0 {
            s_release(old as *mut c_void);
        }
        S_OK
    }

    unsafe extern "system" fn fd_create_heap(this: *mut c_void, desc: *const HeapDesc, riid: *const Guid, pp: *mut *mut c_void) -> i32 {
        assert_eq!(*riid, IID_ID3D12_DESCRIPTOR_HEAP);
        let d = this as *mut FakeDevice;
        let n = (*d).heaps.fetch_add(1, Ordering::AcqRel);
        (*d).last_desc[0].store((*desc).ty, Ordering::Release);
        (*d).last_desc[1].store((*desc).num, Ordering::Release);
        (*d).last_desc[2].store((*desc).flags, Ordering::Release);
        (*d).last_desc[3].store((*desc).node_mask, Ordering::Release);
        *pp = (0x7700_0000 + n as usize * 0x100) as *mut c_void;
        S_OK
    }

    fn fake_device() -> Box<FakeDevice> {
        let mut vt = Box::new([0usize; 15]);
        vt[5] = fd_set_iface as usize;
        vt[14] = fd_create_heap as usize;
        let vt = Box::leak(vt);
        Box::new(FakeDevice { vtbl: vt.as_ptr(), private: AtomicUsize::new(0), heaps: AtomicU32::new(0), last_desc: Default::default() })
    }

    unsafe fn destroy(d: &FakeDevice) {
        let p = d.private.swap(0, Ordering::AcqRel);
        if p != 0 {
            s_release(p as *mut c_void);
        }
    }

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static LAST: Mutex<(usize, usize)> = Mutex::new((0, 0));

    unsafe extern "system" fn on_destroy(dev: *mut c_void, ctx: *mut c_void) {
        CALLS.fetch_add(1, Ordering::SeqCst);
        *LAST.lock().unwrap() = (dev as usize, ctx as usize);
    }

    #[test]
    fn code1_buffer_descriptor_bits() {
        let obj = 0x1000_0040usize;
        let mut args = [obj as u64, 0, 0, 0, 0];
        let s = args.as_mut_ptr().wrapping_add(1) as *mut DescriptorUpdate;
        unsafe {
            *s = DescriptorUpdate { sub: 1, flags: 0b101101, a: 0x1111, b: 0x2222, c: 9, d: 9, e: 9 };
            assert_eq!(call(Family::One, 1, args.as_mut_ptr()), S_OK);
        }
        assert_eq!(state_read(obj, KIND_BUFFER_DESC, 1), 1 << 14 | 1 << 26);
        assert_eq!(state_read(obj, KIND_BUFFER_DESC, 0), 0b101);
        assert_eq!(state_read(obj, KIND_BUFFER_DESC, 2), 0x1111);
        assert_eq!(state_read(obj, KIND_BUFFER_DESC, 3), 0x2222);
        state_write(obj, KIND_BUFFER_DESC, 1, u32::MAX, 0xffff_ffff);
        unsafe {
            *s = DescriptorUpdate { sub: 1, flags: 0b000010, a: 1, b: 2, c: 0, d: 0, e: 0 };
            call(Family::One, 1, args.as_mut_ptr());
        }
        assert_eq!(state_read(obj, KIND_BUFFER_DESC, 1), 0xffff_ffff & !(1 << 14 | 1 << 26));
        unsafe {
            *s = DescriptorUpdate { sub: 2, flags: 0xff, a: 7, b: 7, c: 7, d: 7, e: 7 };
            assert_eq!(call(Family::One, 1, args.as_mut_ptr()), S_OK);
        }
        assert_eq!(state_read(obj, KIND_BUFFER_DESC, 2), 1);
    }

    #[test]
    fn code1_texture_descriptor_bits_per_family() {
        for (fam, obj) in [(Family::One, 0x2000_0000usize), (Family::Scarlett, 0x2000_1000usize)] {
            let mut args = [obj as u64, 0, 0, 0, 0];
            let s = args.as_mut_ptr().wrapping_add(1) as *mut DescriptorUpdate;
            unsafe {
                *s = DescriptorUpdate { sub: 3, flags: 0b1010_1101, a: 0xa, b: 0xb, c: 0xc, d: 0xd, e: 0b1110 };
                assert_eq!(call(fam, 1, args.as_mut_ptr()), S_OK);
            }
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, 1), 0x40);
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, texture_x_index(fam)), 0xf);
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, 0), 0x2a);
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, 3), 0xa);
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, 2), 0xb);
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, 4), 0xc);
            assert_eq!(state_read(obj, KIND_TEXTURE_DESC, 5), 0xd);
        }
        assert_eq!(texture_x_index(Family::One), 35);
        assert_eq!(texture_x_index(Family::Scarlett), 43);
    }

    #[test]
    fn code2_descriptor_heap_over_title_memory() {
        let dev = fake_device();
        let dp = &*dev as *const FakeDevice as *mut c_void;
        let mut out: *mut c_void = core::ptr::null_mut();
        let mut args = [dp as u64, 0, 0xdead_0000_1000, (64u64 << 32) | 1, &mut out as *mut _ as u64];
        unsafe {
            assert_eq!(call(Family::Scarlett, 2, args.as_mut_ptr()), S_OK);
        }
        assert!(!out.is_null());
        assert_eq!(dev.last_desc.iter().map(|a| a.load(Ordering::Acquire)).collect::<Vec<_>>(), vec![1, 64, 1, 0]);
        assert_eq!(heap_memory(out as usize), 0xdead_0000_1000);
        assert_eq!(state_read(dp as usize, KIND_HEAP_COUNT, 0), 1);
        args[2] = 0x5000;
        unsafe {
            assert_eq!(call(Family::One, 2, args.as_mut_ptr()), S_OK);
        }
        assert_eq!(heap_memory(out as usize), 0x5000);
        assert_eq!(state_read(dp as usize, KIND_HEAP_COUNT, 0), 2);
        args[3] = (8u64 << 32) | 2;
        unsafe {
            assert_eq!(call(Family::One, 2, args.as_mut_ptr()), S_OK);
        }
        assert_eq!(dev.last_desc[2].load(Ordering::Acquire), 0);
        assert_eq!((shader_visible_flags(0), shader_visible_flags(4), shader_visible_flags(3)), (1, 1, 0));
    }

    #[test]
    fn code3_destroy_callback_and_unknown_codes() {
        let _g = crate::serial();
        let dev = fake_device();
        let dp = &*dev as *const FakeDevice as *mut c_void;
        CALLS.store(0, Ordering::SeqCst);
        let mut args = [dp as u64, on_destroy as usize as u64, 0x77, 0, 0];
        unsafe {
            assert_eq!(call(Family::One, 3, args.as_mut_ptr()), E_NOINTERFACE);
            assert_eq!(call(Family::Scarlett, 4, args.as_mut_ptr()), E_NOINTERFACE);
            assert_eq!(call(Family::Scarlett, 0, args.as_mut_ptr()), E_NOINTERFACE);
            assert_eq!(call(Family::Scarlett, 3, args.as_mut_ptr()), S_OK);
            assert_eq!(destroy_callback(dp as usize), (on_destroy as usize, 0x77));
            args[2] = 0x88;
            assert_eq!(call(Family::Scarlett, 3, args.as_mut_ptr()), S_OK);
            assert_eq!(CALLS.load(Ordering::SeqCst), 0);
            destroy(&dev);
            assert_eq!(CALLS.load(Ordering::SeqCst), 1);
            assert_eq!(*LAST.lock().unwrap(), (dp as usize, 0x88));
            let dev2 = fake_device();
            let dp2 = &*dev2 as *const FakeDevice as *mut c_void;
            let mut a2 = [dp2 as u64, on_destroy as usize as u64, 5, 0, 0];
            assert_eq!(call(Family::Scarlett, 3, a2.as_mut_ptr()), S_OK);
            a2[1] = 0;
            assert_eq!(call(Family::Scarlett, 3, a2.as_mut_ptr()), S_OK);
            assert_eq!(destroy_callback(dp2 as usize), (0, 0));
            assert_eq!(dev2.private.load(Ordering::Acquire), 0);
            destroy(&dev2);
            assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        }
    }
}
