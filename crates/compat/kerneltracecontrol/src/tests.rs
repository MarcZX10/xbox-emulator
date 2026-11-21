use super::*;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_WMI_INSTANCE_NOT_FOUND: u32 = 4201;

#[link(name = "advapi32")]
extern "system" {
    fn ControlTraceW(h: u64, name: *const u16, p: *mut c_void, code: u32) -> u32;
}

fn base_props(flags: u32, name: &str) -> Props {
    let w = wide(name);
    let size = PROPS_MIN + w.len() * 2 + 1024;
    let mut p = Props::zeroed(size);
    p.set_u32(OFF_BUFFER_SIZE, size as u32);
    p.set_u32(0x2c, 0x0002_0000);
    p.set_u32(0x40, 0x100);
    p.set_u32(OFF_ENABLE_FLAGS, flags);
    p.set_u32(OFF_LOGGER_NAME, PROPS_MIN as u32);
    for (i, c) in w.iter().enumerate() {
        p.set_u16(PROPS_MIN + i * 2, *c);
    }
    p
}

fn ev(class: usize, kind: u8) -> ClassicEventId {
    ClassicEventId { guid: CLASSES[class], kind, reserved: [0; 7] }
}

#[test]
fn group_mask_maps_classic_flags() {
    let m = group_mask(0x10 | 0x1 | 0x80);
    assert_eq!(m[0], 0x1);
    assert_eq!(m[1], 0x4);
    assert_eq!(m[2], 0x40);
    assert_eq!(m[3..], [0; 5]);
}

#[test]
fn hook_ids_use_class_index_and_type() {
    let unknown = ClassicEventId { guid: [7; 16], kind: 1, reserved: [0; 7] };
    assert_eq!(hook_ids(&[ev(2, 1), unknown, ev(16, 0x21)], 0x100), vec![0x201, 0x1021]);
    assert_eq!(hook_ids(&[ev(1, 1), ev(1, 2), ev(1, 3)], 2), vec![0x101, 0x102]);
}

#[test]
fn build_without_extensions_clears_enable_flags() {
    let b = base_props(0, "x");
    let size0 = b.u32_at(0) as usize;
    let p = unsafe { build(b.bytes().as_ptr(), &[], &[], false) }.unwrap();
    assert_eq!(p.u32_at(0) as usize, size0 + 4);
    assert_eq!(p.u32_at(OFF_ENABLE_FLAGS), 0);
    assert_eq!(p.u32_at(size0), 1);
}

#[test]
fn build_lays_out_group_mask_pids_and_hooks() {
    let b = base_props(0x11, "x");
    let size0 = b.u32_at(0) as usize;
    let p = unsafe { build(b.bytes().as_ptr(), &[ev(3, 0x10)], &[42, 43], true) }.unwrap();
    assert_eq!(p.u32_at(0) as usize, size0 + 4 + 0x24 + 12 + 8);
    assert_eq!(p.u16_at(OFF_ENABLE_FLAGS) as usize, size0);
    assert_eq!(p.bytes()[OFF_ENABLE_FLAGS + 2], 0xff);
    assert_eq!(p.bytes()[OFF_ENABLE_FLAGS + 3], 0x80);
    assert_eq!(&p.bytes()[OFF_GUID..OFF_GUID + 16], &[0; 16]);
    assert_eq!(p.u16_at(size0), 1 + 9 + 3 + 2);
    assert_eq!(p.u16_at(size0 + 2), 3);
    let g = size0 + 4;
    assert_eq!((p.u16_at(g), p.u16_at(g + 2)), (9, 1));
    assert_eq!(p.u32_at(g + 4), 1);
    assert_eq!(p.u32_at(g + 8), 4);
    let q = g + 36;
    assert_eq!((p.u16_at(q), p.u16_at(q + 2)), (3, 2));
    assert_eq!((p.u32_at(q + 4), p.u32_at(q + 8)), (42, 43));
    let k = q + 12;
    assert_eq!((p.u16_at(k), p.u16_at(k + 2)), (2, 3));
    assert_eq!(p.u32_at(k + 4), 0x310);
}

#[test]
fn add_item_rejects_missing_extension_header() {
    let mut p = base_props(0, "x");
    assert_eq!(add_item(&mut p, &[1, 2, 3, 4], 1), Err(ERROR_INVALID_FLAGS));
    p.set_u32(OFF_ENABLE_FLAGS, EXTENSION_FLAG | 0xfff0);
    assert_eq!(add_item(&mut p, &[1, 2, 3, 4], 1), Err(ERROR_BAD_LENGTH));
}

#[test]
fn start_kernel_trace_validates_like_console() {
    let mut h = 0u64;
    let mut p = base_props(0, "k");
    assert_eq!(unsafe { StartKernelTrace(&mut h, p.bytes_mut().as_mut_ptr(), core::ptr::null(), 0x101) }, ERROR_INVALID_PARAMETER);
    let mut p = base_props(0x8000_0001, "k");
    assert_eq!(unsafe { StartKernelTrace(&mut h, p.bytes_mut().as_mut_ptr(), core::ptr::null(), 0) }, ERROR_INVALID_FLAGS);
    let mut p = base_props(0, "k");
    p.bytes_mut()[OFF_GUID..OFF_GUID + 16].copy_from_slice(&SYSTEM_TRACE_PROVIDER);
    p.set_u32(OFF_LOGGER_NAME, 0);
    assert_eq!(unsafe { StartKernelTrace(&mut h, p.bytes_mut().as_mut_ptr(), core::ptr::null(), 0) }, ERROR_INVALID_PARAMETER);
    p.set_u32(OFF_LOGGER_NAME, 0x10000);
    assert_eq!(unsafe { StartKernelTrace(&mut h, p.bytes_mut().as_mut_ptr(), core::ptr::null(), 0) }, ERROR_BAD_LENGTH);
}

#[test]
fn start_heap_trace_validates_like_console() {
    let mut h = 0u64;
    let n = wide("dbg-heap");
    let mut p = base_props(0, "dbg-heap");
    assert_eq!(unsafe { StartHeapTrace(&mut h, p.bytes_mut().as_mut_ptr(), core::ptr::null(), core::ptr::null(), 0, core::ptr::null(), 0) }, ERROR_INVALID_PARAMETER);
    assert_eq!(unsafe { StartHeapTrace(core::ptr::null_mut(), p.bytes_mut().as_mut_ptr(), n.as_ptr(), core::ptr::null(), 0, core::ptr::null(), 0) }, ERROR_INVALID_PARAMETER);
    assert_eq!(unsafe { StartHeapTrace(&mut h, p.bytes_mut().as_mut_ptr(), n.as_ptr(), core::ptr::null(), 0, core::ptr::null(), 0x101) }, ERROR_INVALID_PARAMETER);
    let mut f = base_props(0x8000_0000, "dbg-heap");
    assert_eq!(unsafe { StartHeapTrace(&mut h, f.bytes_mut().as_mut_ptr(), n.as_ptr(), core::ptr::null(), 0, core::ptr::null(), 0) }, ERROR_INVALID_FLAGS);
}

#[test]
fn start_heap_trace_reaches_etw() {
    let name = format!("dbg-heap-{}", std::process::id());
    let n = wide(&name);
    let mut h = 0u64;
    let mut p = base_props(0, &name);
    let pid = std::process::id();
    let st = unsafe { StartHeapTrace(&mut h, p.bytes_mut().as_mut_ptr(), n.as_ptr(), &pid, 1, core::ptr::null(), 0) };
    if st == 0 {
        assert_ne!(h, 0);
        let mut s = base_props(0, &name);
        assert_eq!(unsafe { ControlTraceW(0, n.as_ptr(), s.ptr(), 1) }, 0);
    } else {
        assert_eq!(st, ERROR_ACCESS_DENIED);
    }
}

#[test]
fn update_heap_trace_without_session_reports_etw_error() {
    let n = wide("dbg-heap-missing-session");
    let mut p = base_props(0, "dbg-heap-missing-session");
    let st = unsafe { UpdateHeapTrace(p.bytes_mut().as_mut_ptr(), n.as_ptr(), core::ptr::null(), 0, core::ptr::null(), 0) };
    assert!(st == ERROR_WMI_INSTANCE_NOT_FOUND || st == ERROR_ACCESS_DENIED, "{st}");
    assert_eq!(unsafe { UpdateHeapTrace(p.bytes_mut().as_mut_ptr(), core::ptr::null(), core::ptr::null(), 0, core::ptr::null(), 0) }, ERROR_INVALID_PARAMETER);
}

#[test]
fn start_kernel_trace_reaches_etw() {
    let mut h = 0u64;
    let mut p = base_props(1, "NT Kernel Logger");
    let st = unsafe { StartKernelTrace(&mut h, p.bytes_mut().as_mut_ptr(), core::ptr::null(), 0) };
    if st == 0 {
        let n = wide(KERNEL_LOGGER);
        let mut s = base_props(0, KERNEL_LOGGER);
        unsafe { ControlTraceW(0, n.as_ptr(), s.ptr(), 1) };
    } else {
        assert!(st == ERROR_ACCESS_DENIED || st == 183, "{st}");
    }
}

#[test]
fn dll_can_unload_with_no_objects() {
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn ext_limit_matches_host_version() {
    let (ma, _) = os_version();
    assert!(ma >= 6);
    assert_eq!(ext_limit(), 0x100);
}
