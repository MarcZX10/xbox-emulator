use core::ffi::c_void;
use compat_common::record;

use crate::device_wrap::Guid;

const M: &str = "d3d12_xs";

static IID_DRED_SETTINGS: Guid = Guid { d1: 0x82BC_481C, d2: 0x6B9B, d3: 0x4030, d4: [0xAE, 0xDB, 0x7E, 0xE3, 0xD1, 0xDF, 0x1E, 0x63] };
static IID_DRED: Guid = Guid { d1: 0x9893_1D33, d2: 0x5AE8, d3: 0x4791, d4: [0xAA, 0x3C, 0x1A, 0x73, 0xA2, 0x93, 0x4E, 0x71] };
static CLSID_DRED: Guid = Guid { d1: 0x4a75_bbc4, d2: 0x9ff4, d3: 0x4ad8, d4: [0x9f, 0x18, 0xab, 0xae, 0x84, 0xdc, 0x5f, 0xf2] };
static IID_FENCE: Guid = Guid { d1: 0x0a75_3dcf, d2: 0xc4d8, d3: 0x4b91, d4: [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76] };
const FORCED_ON: u32 = 2;
const MAX_NODES: usize = 256;
const MAX_ALLOCS: usize = 64;

extern "system" {
    fn CreateEventW(attr: *mut c_void, manual: i32, initial: i32, name: *const u16) -> *mut c_void;
    fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
}

pub fn requested() -> bool {
    std::env::var("TOMBOX_DRED").map_or(false, |v| v.trim() == "1")
}

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn release(obj: *mut c_void) {
    let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(obj, 2));
    f(obj);
}

pub fn enable_before_create() -> bool {
    if !requested() {
        return false;
    }
    let mut settings: *mut c_void = core::ptr::null_mut();
    let factory = crate::agility::factory();
    let (hr, via) = unsafe {
        if !factory.is_null() {
            let get: unsafe extern "system" fn(*mut c_void, *const Guid, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(slot(factory, 7));
            (get(factory, &CLSID_DRED, &IID_DRED_SETTINGS, &mut settings), "Agility device factory")
        } else {
            let p = crate::host_proc(b"D3D12GetDebugInterface\0");
            if p == 0 {
                record(M, "dred", "TOMBOX_DRED=1 but host d3d12.dll has no D3D12GetDebugInterface; DRED not enabled");
                return false;
            }
            let get: unsafe extern "system" fn(*const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(p);
            (get(&IID_DRED_SETTINGS, &mut settings), "D3D12GetDebugInterface")
        }
    };
    if hr < 0 || settings.is_null() {
        record(M, "dred", &format!("TOMBOX_DRED=1 but ID3D12DeviceRemovedExtendedDataSettings unavailable via {via} hr={hr:#010x}; DRED not enabled"));
        return false;
    }
    unsafe {
        let crumbs: unsafe extern "system" fn(*mut c_void, u32) = core::mem::transmute(slot(settings, 3));
        let faults: unsafe extern "system" fn(*mut c_void, u32) = core::mem::transmute(slot(settings, 4));
        crumbs(settings, FORCED_ON);
        faults(settings, FORCED_ON);
        release(settings);
    }
    record(M, "dred", &format!("DRED auto-breadcrumbs and page-fault reporting FORCED_ON via {via} (before device creation)"));
    true
}

pub fn watch(device: *mut c_void) -> bool {
    if !requested() || device.is_null() {
        return false;
    }
    unsafe {
        let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(slot(device, 0));
        let mut dred: *mut c_void = core::ptr::null_mut();
        let hr = qi(device, &IID_DRED, &mut dred);
        if hr < 0 || dred.is_null() {
            record(M, "dred", &format!("device has no ID3D12DeviceRemovedExtendedData (hr={hr:#010x}); removal data will not be reported"));
            return false;
        }
        let create_fence: unsafe extern "system" fn(*mut c_void, u64, u32, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(slot(device, 36));
        let mut fence: *mut c_void = core::ptr::null_mut();
        let hr = create_fence(device, 0, 0, &IID_FENCE, &mut fence);
        let ev = CreateEventW(core::ptr::null_mut(), 0, 0, core::ptr::null());
        if hr < 0 || fence.is_null() || ev.is_null() {
            record(M, "dred", &format!("removal watch fence/event failed (CreateFence hr={hr:#010x}, event={ev:p})"));
            release(dred);
            return false;
        }
        let on_done: unsafe extern "system" fn(*mut c_void, u64, *mut c_void) -> i32 = core::mem::transmute(slot(fence, 9));
        let hr = on_done(fence, u64::MAX, ev);
        if hr < 0 {
            record(M, "dred", &format!("removal watch SetEventOnCompletion(UINT64_MAX) hr={hr:#010x}"));
            release(fence);
            release(dred);
            return false;
        }
        let (d, r, e, f) = (device as usize, dred as usize, ev as usize, fence as usize);
        let spawned = std::thread::Builder::new().name("tombox-dred-watch".into()).spawn(move || {
            WaitForSingleObject(e as *mut c_void, u32::MAX);
            report(d as *mut c_void, r as *mut c_void);
            let _ = f;
        });
        if spawned.is_err() {
            record(M, "dred", "removal watch thread could not start");
            return false;
        }
    }
    record(M, "dred", "device-removal watch armed (fence UINT64_MAX)");
    true
}

unsafe fn report(device: *mut c_void, dred: *mut c_void) {
    let reason: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(slot(device, 37));
    let hr = reason(device);
    record(M, "dred.removed", &format!("device removed: GetDeviceRemovedReason={hr:#010x} ({})", removed_reason_name(hr)));
    let mut crumbs: usize = 0;
    let get_crumbs: unsafe extern "system" fn(*mut c_void, *mut usize) -> i32 = core::mem::transmute(slot(dred, 3));
    let hr = get_crumbs(dred, &mut crumbs);
    if hr < 0 {
        record(M, "dred.breadcrumb", &format!("GetAutoBreadcrumbsOutput hr={hr:#010x}"));
    } else {
        for l in format_breadcrumbs(crumbs as *const u8) {
            record(M, "dred.breadcrumb", &l);
        }
    }
    let mut pf = [0usize; 3];
    let get_pf: unsafe extern "system" fn(*mut c_void, *mut usize) -> i32 = core::mem::transmute(slot(dred, 4));
    let hr = get_pf(dred, pf.as_mut_ptr());
    if hr < 0 {
        record(M, "dred.pagefault", &format!("GetPageFaultAllocationOutput hr={hr:#010x}"));
    } else {
        for l in format_page_fault(pf.as_ptr() as *const u8) {
            record(M, "dred.pagefault", &l);
        }
    }
}

pub fn removed_reason_name(hr: i32) -> &'static str {
    match hr as u32 {
        0 => "S_OK (not removed)",
        0x887A_0005 => "DXGI_ERROR_DEVICE_REMOVED",
        0x887A_0006 => "DXGI_ERROR_DEVICE_HUNG",
        0x887A_0007 => "DXGI_ERROR_DEVICE_RESET",
        0x887A_0020 => "DXGI_ERROR_DRIVER_INTERNAL_ERROR",
        0x8007_0057 => "E_INVALIDARG",
        0x8007_000E => "E_OUTOFMEMORY",
        _ => "other",
    }
}

const OPS: [&str; 49] = [
    "SETMARKER", "BEGINEVENT", "ENDEVENT", "DRAWINSTANCED", "DRAWINDEXEDINSTANCED", "EXECUTEINDIRECT", "DISPATCH", "COPYBUFFERREGION",
    "COPYTEXTUREREGION", "COPYRESOURCE", "COPYTILES", "RESOLVESUBRESOURCE", "CLEARRENDERTARGETVIEW", "CLEARUNORDEREDACCESSVIEW",
    "CLEARDEPTHSTENCILVIEW", "RESOURCEBARRIER", "EXECUTEBUNDLE", "PRESENT", "RESOLVEQUERYDATA", "BEGINSUBMISSION", "ENDSUBMISSION",
    "DECODEFRAME", "PROCESSFRAMES", "ATOMICCOPYBUFFERUINT", "ATOMICCOPYBUFFERUINT64", "RESOLVESUBRESOURCEREGION", "WRITEBUFFERIMMEDIATE",
    "DECODEFRAME1", "SETPROTECTEDRESOURCESESSION", "DECODEFRAME2", "PROCESSFRAMES1", "BUILDRAYTRACINGACCELERATIONSTRUCTURE",
    "EMITRAYTRACINGACCELERATIONSTRUCTUREPOSTBUILDINFO", "COPYRAYTRACINGACCELERATIONSTRUCTURE", "DISPATCHRAYS", "INITIALIZEMETACOMMAND",
    "EXECUTEMETACOMMAND", "ESTIMATEMOTION", "RESOLVEMOTIONVECTORHEAP", "SETPIPELINESTATE1", "INITIALIZEEXTENSIONCOMMAND",
    "EXECUTEEXTENSIONCOMMAND", "DISPATCHMESH", "ENCODEFRAME", "RESOLVEENCODEROUTPUTMETADATA", "BARRIER", "BEGIN_COMMAND_LIST",
    "DISPATCHGRAPH", "SETPROGRAM",
];

pub fn op_name(op: u32) -> String {
    match op {
        52 => "PROCESSFRAMES2".into(),
        o if (o as usize) < OPS.len() => OPS[o as usize].into(),
        o => format!("OP{o}"),
    }
}

pub fn alloc_type_name(t: u32) -> String {
    let n = match t {
        19 => "COMMAND_QUEUE", 20 => "COMMAND_ALLOCATOR", 21 => "PIPELINE_STATE", 22 => "COMMAND_LIST", 23 => "FENCE",
        24 => "DESCRIPTOR_HEAP", 25 => "HEAP", 27 => "QUERY_HEAP", 28 => "COMMAND_SIGNATURE", 29 => "PIPELINE_LIBRARY",
        30 => "VIDEO_DECODER", 32 => "VIDEO_PROCESSOR", 34 => "RESOURCE", 35 => "PASS", 36 => "CRYPTOSESSION",
        37 => "CRYPTOSESSIONPOLICY", 38 => "PROTECTEDRESOURCESESSION", 39 => "VIDEO_DECODER_HEAP", 40 => "COMMAND_POOL",
        41 => "COMMAND_RECORDER", 42 => "STATE_OBJECT", 43 => "METACOMMAND", 44 => "SCHEDULINGGROUP", 45 => "VIDEO_MOTION_ESTIMATOR",
        46 => "VIDEO_MOTION_VECTOR_HEAP", 47 => "VIDEO_EXTENSION_COMMAND", 48 => "VIDEO_ENCODER", 49 => "VIDEO_ENCODER_HEAP",
        _ => return format!("TYPE{t}"),
    };
    n.into()
}

unsafe fn rd<T: Copy>(p: *const u8, off: usize) -> T {
    core::ptr::read_unaligned(p.add(off) as *const T)
}

unsafe fn name_at(p: *const u8, a: usize, w: usize) -> String {
    let wp: *const u16 = rd(p, w);
    if !wp.is_null() {
        let mut n = 0;
        while n < 256 && *wp.add(n) != 0 {
            n += 1;
        }
        return String::from_utf16_lossy(core::slice::from_raw_parts(wp, n));
    }
    let ap: *const u8 = rd(p, a);
    if !ap.is_null() {
        let mut n = 0;
        while n < 256 && *ap.add(n) != 0 {
            n += 1;
        }
        return String::from_utf8_lossy(core::slice::from_raw_parts(ap, n)).into_owned();
    }
    String::new()
}

pub unsafe fn format_breadcrumbs(head: *const u8) -> Vec<String> {
    let mut out = Vec::new();
    let (mut node, mut seen, mut complete) = (head, 0usize, 0usize);
    while !node.is_null() && seen < MAX_NODES {
        seen += 1;
        let list = name_at(node, 0, 8);
        let queue = name_at(node, 16, 24);
        let count: u32 = rd(node, 48);
        let last_p: *const u32 = rd(node, 56);
        let hist: *const u32 = rd(node, 64);
        let last = if last_p.is_null() { 0 } else { *last_p };
        if last >= count || hist.is_null() {
            complete += 1;
        } else {
            let lo = last.saturating_sub(4);
            let hi = (last + 3).min(count);
            let ops: Vec<String> = (lo..hi).map(|i| {
                let n = op_name(*hist.add(i as usize));
                if i == last { format!(">>{n}<<") } else { n }
            }).collect();
            out.push(format!(
                "incomplete command list list={:?} queue={:?} cmdlist={:#x} completed {last}/{count}, first unfinished op #{last} {}; ops {lo}..{hi}: {}",
                list, queue, rd::<usize>(node, 32), op_name(*hist.add(last as usize)), ops.join(" ")));
        }
        node = rd(node, 72);
    }
    out.push(format!("{seen} breadcrumb node(s){}, {complete} complete, {} incomplete", if !node.is_null() { " (truncated)" } else { "" }, seen - complete));
    out
}

unsafe fn alloc_list(mut n: *const u8, what: &str, out: &mut Vec<String>) {
    let mut k = 0;
    while !n.is_null() && k < MAX_ALLOCS {
        out.push(format!("{what} allocation {} {:?}", alloc_type_name(rd::<u32>(n, 16)), name_at(n, 0, 8)));
        n = rd(n, 24);
        k += 1;
    }
    if !n.is_null() {
        out.push(format!("{what} allocations truncated after {MAX_ALLOCS}"));
    }
}

pub unsafe fn format_page_fault(pf: *const u8) -> Vec<String> {
    let va: u64 = rd(pf, 0);
    let mut out = vec![if va == 0 { "no GPU page fault recorded (PageFaultVA=0)".to_string() } else { format!("GPU page fault at VA {va:#018x}") }];
    alloc_list(rd(pf, 8), "existing", &mut out);
    alloc_list(rd(pf, 16), "recently freed", &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct Node {
        la: *const u8,
        lw: *const u16,
        qa: *const u8,
        qw: *const u16,
        list: usize,
        queue: usize,
        count: u32,
        last: *const u32,
        hist: *const u32,
        next: *const Node,
    }

    #[repr(C)]
    struct Alloc {
        a: *const u8,
        w: *const u16,
        ty: u32,
        next: *const Alloc,
    }

    #[test]
    fn breadcrumbs_name_the_first_unfinished_op_of_incomplete_lists() {
        let hist = [19u32, 15, 3, 4, 6, 17, 20];
        let done = 7u32;
        let stuck = 3u32;
        let qn: Vec<u16> = "Direct".encode_utf16().chain(Some(0)).collect();
        let n2 = Node { la: b"post\0".as_ptr(), lw: core::ptr::null(), qa: core::ptr::null(), qw: qn.as_ptr(), list: 0x20, queue: 0x30, count: 7, last: &done, hist: hist.as_ptr(), next: core::ptr::null() };
        let n1 = Node { la: core::ptr::null(), lw: core::ptr::null(), qa: b"gfx\0".as_ptr(), qw: core::ptr::null(), list: 0x10, queue: 0x30, count: 7, last: &stuck, hist: hist.as_ptr(), next: &n2 };
        let out = unsafe { format_breadcrumbs(&n1 as *const Node as *const u8) };
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].contains("queue=\"gfx\""), "{}", out[0]);
        assert!(out[0].contains("completed 3/7, first unfinished op #3 DRAWINDEXEDINSTANCED"), "{}", out[0]);
        assert!(out[0].contains("BEGINSUBMISSION RESOURCEBARRIER DRAWINSTANCED >>DRAWINDEXEDINSTANCED<< DISPATCH PRESENT"), "{}", out[0]);
        assert_eq!(out[1], "2 breadcrumb node(s), 1 complete, 1 incomplete");
    }

    #[test]
    fn page_fault_lists_va_and_allocations() {
        let rn: Vec<u16> = "SceneColor".encode_utf16().chain(Some(0)).collect();
        let freed = Alloc { a: b"Staging\0".as_ptr(), w: core::ptr::null(), ty: 34, next: core::ptr::null() };
        let a2 = Alloc { a: core::ptr::null(), w: core::ptr::null(), ty: 25, next: core::ptr::null() };
        let a1 = Alloc { a: core::ptr::null(), w: rn.as_ptr(), ty: 34, next: &a2 };
        let pf: [usize; 3] = [0x1_2345_6000, &a1 as *const Alloc as usize, &freed as *const Alloc as usize];
        let out = unsafe { format_page_fault(pf.as_ptr() as *const u8) };
        assert_eq!(out, vec![
            "GPU page fault at VA 0x0000000123456000".to_string(),
            "existing allocation RESOURCE \"SceneColor\"".to_string(),
            "existing allocation HEAP \"\"".to_string(),
            "recently freed allocation RESOURCE \"Staging\"".to_string(),
        ]);
        let none: [usize; 3] = [0, 0, 0];
        assert_eq!(unsafe { format_page_fault(none.as_ptr() as *const u8) }, vec!["no GPU page fault recorded (PageFaultVA=0)".to_string()]);
    }

    #[test]
    fn names_and_env_gate() {
        assert_eq!(op_name(45), "BARRIER");
        assert_eq!(op_name(52), "PROCESSFRAMES2");
        assert_eq!(op_name(50), "OP50");
        assert_eq!(alloc_type_name(42), "STATE_OBJECT");
        assert_eq!(alloc_type_name(26), "TYPE26");
        assert_eq!(removed_reason_name(0x887A_0006u32 as i32), "DXGI_ERROR_DEVICE_HUNG");
        std::env::remove_var("TOMBOX_DRED");
        assert!(!requested());
        assert!(!enable_before_create());
        assert!(!watch(0x1000 as *mut c_void));
    }

    #[test]
    fn host_runtime_accepts_dred_settings_and_a_warp_device_arms_the_watch() {
        std::env::set_var("TOMBOX_DRED", "1");
        assert!(enable_before_create());
        let create = crate::host_proc(b"D3D12CreateDevice\0");
        assert_ne!(create, 0);
        let create: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(create) };
        let mut dev: *mut c_void = core::ptr::null_mut();
        let hr = unsafe { create(core::ptr::null_mut(), 0xb000, crate::IID_ID3D12Device.as_ptr() as *const c_void, &mut dev) };
        std::env::remove_var("TOMBOX_DRED");
        if hr < 0 || dev.is_null() {
            return;
        }
        std::env::set_var("TOMBOX_DRED", "1");
        assert!(watch(dev));
        std::env::remove_var("TOMBOX_DRED");
    }
}
