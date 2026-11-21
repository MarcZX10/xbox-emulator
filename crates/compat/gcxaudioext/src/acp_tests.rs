use super::acp_state::*;
use super::*;
use std::sync::Mutex;

pub(crate) static SERIAL: Mutex<()> = Mutex::new(());

pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn load_dll(name: &str) -> isize {
    crate::diagveh::install();
    let exe = std::env::current_exe().unwrap();
    let dll = exe.ancestors().map(|d| d.join(name)).find(|p| p.is_file());
    let dll = dll.unwrap_or_else(|| panic!("build the crate that makes {name} first (none above {})", exe.display()));
    let path = wide(dll.to_str().unwrap());
    let m = unsafe { LoadLibraryW(path.as_ptr()) };
    assert_ne!(m, 0);
    m
}

pub(crate) fn load_audio_dlls() -> isize {
    load_dll("xmem.dll");
    let m = load_dll("acphal.dll");
    unsafe {
        let (p, _) = apu_alloc(m, 0x100, 0x100, 1);
        let (q, _) = apu_alloc(m, 0x100, 0x100, 2);
        let free: unsafe extern "system" fn(*mut c_void) -> i32 = proc_of(m, b"ApuFree\0");
        free(p as *mut c_void);
        free(q as *mut c_void);
    }
    m
}

unsafe fn proc_of<T>(m: isize, name: &[u8]) -> T {
    let p = GetProcAddress(m, name.as_ptr());
    assert!(!p.is_null());
    core::mem::transmute_copy(&p)
}

unsafe fn heap_state(m: isize, heap: u32) -> [u32; 5] {
    let f: unsafe extern "system" fn(*mut u32, u32) -> i32 = proc_of(m, b"ApuHeapGetState\0");
    let mut o = [0u32; 5];
    assert_eq!(f(o.as_mut_ptr(), heap), 0);
    o
}

unsafe fn apu_alloc(m: isize, size: u32, align: u32, heap: u32) -> (usize, u32) {
    let f: unsafe extern "system" fn(*mut *mut c_void, *mut u32, u32, u32, u32) -> i32 = proc_of(m, b"ApuAlloc\0");
    let mut p: *mut c_void = core::ptr::null_mut();
    let mut a = 0u32;
    assert_eq!(f(&mut p, &mut a, size, align, heap), 0);
    (p as usize, a)
}

#[test]
fn create_and_destroy_xaudio_acp_state() {
    let _g = serial();
    let m = load_audio_dlls();
    unsafe {
        let mut s: *mut u8 = 0x55 as *mut u8;
        assert_eq!(CreateXAudioAcpState(core::ptr::null(), 0, &mut s), 0);
        assert!(!s.is_null());
        assert_eq!(*s.add(STATE_INITIALIZED), 0);
        assert_eq!(*s.add(STATE_FLAG), 0);
        assert!((*(s.add(STATE_HAL) as *const *mut c_void)).is_null());
        assert_eq!(core::ptr::read_unaligned(s.add(0x1b8) as *const u32), 0x40);
        assert_eq!(core::ptr::read_unaligned(s.add(STATE_POOL_B + 0x1b8) as *const u32), 0x80);
        assert_eq!(core::ptr::read_unaligned(s.add(0x1c0) as *const u64), 0x8000);
        assert_eq!(core::ptr::read_unaligned(s.add(0x1b0) as *const u64), 0x10);
        assert_eq!(core::ptr::read_unaligned(s.add(8) as *const usize), s as usize + 0x10);
        assert_eq!(core::ptr::read_unaligned(s as *const usize), s as usize + 0x10 + 8 + 15 * 0x18);
        DestroyXAudioAcpState(s);
        DestroyXAudioAcpState(core::ptr::null_mut());

        let h1 = heap_state(m, 1);
        let h2 = heap_state(m, 2);
        let (xma, xma_apu) = apu_alloc(m, 4 * 0x40, 0x100, 2);
        let mut table = ShapeTable::default();
        table.counts[4] = 4;
        table.ptrs[4] = xma;
        let after_contexts = (heap_state(m, 1), heap_state(m, 2));
        let c1 = ShapeConfig { table: &table, base: [0, 0, 0, 0, 0, 0], count: [0, 0, 0, 0, 2, 0] };
        let c2 = ShapeConfig { table: &table, base: [0, 0, 0, 0, 2, 0], count: [0, 0, 0, 0, 2, 0] };
        let mut a: *mut u8 = core::ptr::null_mut();
        let mut b: *mut u8 = core::ptr::null_mut();
        assert_eq!(CreateXAudioAcpState(&c1, 0, &mut a), 0);
        assert_eq!(CreateXAudioAcpState(&c2, 1, &mut b), 0);
        assert_eq!(*a.add(STATE_INITIALIZED), 1);
        assert_eq!(*b.add(STATE_FLAG), 1);
        let ta = shape_table();
        assert_eq!(ta.counts[4], 4);
        assert_eq!(ta.ptrs[4], xma);
        assert_eq!(ta.apus[4], xma_apu);
        assert_eq!(owner_flags(4), vec![6, 6, 4, 4]);
        assert_eq!(shape_objects(), 2);
        let (shadow, _) = shadow_arrays();
        assert_ne!(shadow[4], 0);
        let (r, w) = xma_side_arrays();
        assert!(r != 0 && w != 0);
        assert!(core::slice::from_raw_parts(r as *const u8, 16).iter().all(|&x| x == 0));
        let oa = &**(a.add(STATE_ALLOCATOR) as *const *const ShapeAllocator);
        let ob = &**(b.add(STATE_ALLOCATOR) as *const *const ShapeAllocator);
        assert_eq!((oa.begin[4], oa.end[4], oa.count[4], oa.local), (0, 2, 2, 1));
        assert_eq!((ob.begin[4], ob.end[4], ob.base[4], ob.local), (2, 4, 2, 0));
        assert_eq!(oa.table, shape_table_address());
        assert_ne!(heap_state(m, 1), after_contexts.0);
        DestroyXAudioAcpState(a);
        assert_eq!(shape_objects(), 1);
        assert_eq!(shape_table().ptrs[4], xma);
        DestroyXAudioAcpState(b);
        assert_eq!(shape_objects(), 0);
        assert_eq!(shape_table(), ShapeTable::default());
        assert_eq!((heap_state(m, 1), heap_state(m, 2)), after_contexts);
        let free: unsafe extern "system" fn(*mut c_void) -> i32 = proc_of(m, b"ApuFree\0");
        assert_eq!(free(xma as *mut c_void), 0);
        assert_eq!((heap_state(m, 1), heap_state(m, 2)), (h1, h2));
    }
}

#[test]
fn default_acp_state_allocates_shape_contexts_once() {
    let _g = serial();
    let m = load_audio_dlls();
    unsafe {
        let h1 = heap_state(m, 1);
        let h2 = heap_state(m, 2);
        let s1 = heap_alloc(ACP_STATE_SIZE);
        let s2 = heap_alloc(ACP_STATE_SIZE);
        construct_state(s1);
        construct_state(s2);
        assert_eq!(initialize_state(s1, core::ptr::null()), 0);
        assert_eq!(initialize_state(s1, core::ptr::null()), 0);
        assert_eq!(initialize_state(s2, core::ptr::null()), 0);
        let t = shape_table();
        assert_eq!(t.counts, [0, 0, 0, 0, 0x200, 0]);
        assert_ne!(t.ptrs[4], 0);
        assert_ne!(t.apus[4], 0);
        assert_eq!(owner_flags(4).len(), 0x200);
        assert!(owner_flags(4).iter().all(|&f| f == 6));
        let o = &**(s1.add(STATE_ALLOCATOR) as *const *const ShapeAllocator);
        assert_eq!((o.begin[4], o.end[4], o.count[4]), (0, 0x200, 0x200));
        destruct_state(s1);
        assert_eq!(shape_table().ptrs[4], t.ptrs[4]);
        destruct_state(s2);
        assert_eq!(shape_table(), ShapeTable::default());
        assert_eq!((heap_state(m, 1), heap_state(m, 2)), (h1, h2));
        heap_free(s1);
        heap_free(s2);
    }
}
