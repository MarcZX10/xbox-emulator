use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use compat_common::record;

use crate::device_wrap::Guid;

const FORMAT_NV12: u32 = 103;
const STATE_COPY_DEST: u32 = 0x400;
const STATE_COPY_SOURCE: u32 = 0x800;
const HEAP_TYPE_READBACK: u32 = 3;
const SENTINEL: u32 = 0xA5C3_E1F7;
const READBACK_COPIES: usize = 2;

const IID_ID3D12DEVICE: Guid = Guid { d1: 0x1898_19f1, d2: 0x1db6, d3: 0x4b57, d4: [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7] };
const IID_ID3D12RESOURCE: Guid = Guid { d1: 0x6964_42be, d2: 0xa72e, d3: 0x4059, d4: [0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad] };

static REAL_COPY: AtomicUsize = AtomicUsize::new(0);
static NV12_COPIES: AtomicUsize = AtomicUsize::new(0);
static READBACKS: AtomicUsize = AtomicUsize::new(0);

type CopyFn = unsafe extern "system" fn(*mut c_void, *const u8, u32, u32, u32, *const u8, *const u8);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Desc {
    dimension: u32,
    alignment: u64,
    width: u64,
    height: u32,
    depth: u16,
    mips: u16,
    format: u32,
    sample_count: u32,
    sample_quality: u32,
    layout: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Footprint {
    offset: u64,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    row_pitch: u32,
}

#[repr(C)]
struct Location {
    resource: *mut c_void,
    ty: u32,
    footprint: Footprint,
}

#[repr(C)]
struct Barrier {
    ty: u32,
    flags: u32,
    resource: *mut c_void,
    sub: u32,
    before: u32,
    after: u32,
}

#[repr(C)]
struct HeapProps {
    ty: u32,
    cpu_page: u32,
    pool: u32,
    creation_mask: u32,
    visible_mask: u32,
}

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn desc_of(res: *mut c_void) -> Desc {
    let mut d = Desc::default();
    let f: unsafe extern "system" fn(*mut c_void, *mut Desc) -> *mut Desc = core::mem::transmute(slot(res, 10));
    f(res, &mut d);
    d
}

unsafe fn release(obj: *mut c_void) {
    let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(obj, 2));
    f(obj);
}

fn bytes_per_texel(format: u32) -> u32 {
    match format {
        60..=65 => 1,
        48..=52 => 2,
        _ => 4,
    }
}

pub fn install(real_copy: usize) -> usize {
    REAL_COPY.store(real_copy, Ordering::Relaxed);
    copy_texture_region_dma as *const () as usize
}

static RESULTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[allow(dead_code)]
pub fn results() -> Vec<String> {
    RESULTS.lock().map(|r| r.clone()).unwrap_or_default()
}

fn report(msg: String) {
    if let Ok(mut r) = RESULTS.lock() {
        r.push(msg.clone());
    }
    record("d3d12_xs", "video.readback", &msg);
}

unsafe extern "system" fn copy_texture_region_dma(list: *mut c_void, dst: *const u8, x: u32, y: u32, z: u32, src: *const u8, bx: *const u8) {
    let real: CopyFn = core::mem::transmute(REAL_COPY.load(Ordering::Relaxed));
    real(list, dst, x, y, z, src, bx);
    if dst.is_null() || src.is_null() {
        return;
    }
    let s = &*(src as *const Location);
    let d = &*(dst as *const Location);
    if s.resource.is_null() || d.resource.is_null() || s.ty != 0 {
        return;
    }
    let sd = desc_of(s.resource);
    if sd.format != FORMAT_NV12 {
        return;
    }
    let n = NV12_COPIES.fetch_add(1, Ordering::Relaxed);
    let dd = desc_of(d.resource);
    let dsub = if d.ty == 0 { d.footprint.offset as u32 } else { u32::MAX };
    let ssub = s.footprint.offset as u32;
    if n < 4 || n == 60 || n == 600 || n == 6000 {
        record(
            "d3d12_xs",
            "video.copy",
            &format!(
                "NV12 copy #{n}: src {:p} sub={ssub} {}x{} -> dst {:p} type={} sub={dsub} format={} {}x{} at ({x},{y},{z}) box={}",
                s.resource, sd.width, sd.height, d.resource, d.ty, dd.format, dd.width, dd.height, !bx.is_null()
            ),
        );
    }
    if d.ty == 0 && READBACKS.load(Ordering::Relaxed) < READBACK_COPIES {
        let idx = READBACKS.fetch_add(1, Ordering::Relaxed);
        if idx < READBACK_COPIES {
            readback(list, d.resource, &dd, dsub, n);
        }
    }
}

unsafe fn readback(list: *mut c_void, dst: *mut c_void, dd: &Desc, sub: u32, copy_index: usize) {
    let mut dev: *mut c_void = core::ptr::null_mut();
    let get_device: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(slot(list, 7));
    if get_device(list, &IID_ID3D12DEVICE, &mut dev) < 0 || dev.is_null() {
        record("d3d12_xs", "video.readback", "GetDevice on the DMA list failed");
        return;
    }
    let mut fp = Footprint::default();
    let mut rows = 0u32;
    let mut total = 0u64;
    let footprints: unsafe extern "system" fn(*mut c_void, *const Desc, u32, u32, u64, *mut Footprint, *mut u32, *mut u64, *mut u64) =
        core::mem::transmute(slot(dev, 38));
    footprints(dev, dd, sub, 1, 0, &mut fp, &mut rows, core::ptr::null_mut(), &mut total);
    if total == 0 {
        release(dev);
        record("d3d12_xs", "video.readback", &format!("no copyable footprint for dst sub={sub} format={}", dd.format));
        return;
    }
    let heap = HeapProps { ty: HEAP_TYPE_READBACK, cpu_page: 0, pool: 0, creation_mask: 0, visible_mask: 0 };
    let bd = Desc { dimension: 1, width: total, height: 1, depth: 1, mips: 1, sample_count: 1, layout: 1, ..Default::default() };
    let mut rb: *mut c_void = core::ptr::null_mut();
    let create: unsafe extern "system" fn(*mut c_void, *const HeapProps, u32, *const Desc, u32, *const c_void, *const Guid, *mut *mut c_void) -> i32 =
        core::mem::transmute(slot(dev, 27));
    let hr = create(dev, &heap, 0, &bd, STATE_COPY_DEST, core::ptr::null(), &IID_ID3D12RESOURCE, &mut rb);
    release(dev);
    if hr < 0 || rb.is_null() {
        record("d3d12_xs", "video.readback", &format!("CreateCommittedResource(readback {total} bytes) hr={:#010x}", hr as u32));
        return;
    }
    let mut p: *mut c_void = core::ptr::null_mut();
    let map: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut c_void) -> i32 = core::mem::transmute(slot(rb, 8));
    if map(rb, 0, core::ptr::null(), &mut p) < 0 || p.is_null() {
        record("d3d12_xs", "video.readback", "Map(readback) failed");
        return;
    }
    core::ptr::write_volatile(p as *mut u32, SENTINEL);
    let barrier: unsafe extern "system" fn(*mut c_void, u32, *const Barrier) = core::mem::transmute(slot(list, 26));
    let to_src = Barrier { ty: 0, flags: 0, resource: dst, sub, before: STATE_COPY_DEST, after: STATE_COPY_SOURCE };
    barrier(list, 1, &to_src);
    let real: CopyFn = core::mem::transmute(REAL_COPY.load(Ordering::Relaxed));
    let to = Location { resource: rb, ty: 1, footprint: fp };
    let mut from = Location { resource: dst, ty: 0, footprint: Footprint::default() };
    from.footprint.offset = sub as u64;
    real(list, &to as *const Location as *const u8, 0, 0, 0, &from as *const Location as *const u8, core::ptr::null());
    let back = Barrier { ty: 0, flags: 0, resource: dst, sub, before: STATE_COPY_SOURCE, after: STATE_COPY_DEST };
    barrier(list, 1, &back);
    record(
        "d3d12_xs",
        "video.readback",
        &format!("queued readback of video dst {dst:p} sub={sub} format={} {}x{} rows={rows} pitch={} after NV12 copy #{copy_index}", fp.format, fp.width, fp.height, fp.row_pitch),
    );
    let (base, rb) = (p as usize, rb as usize);
    std::thread::spawn(move || {
        let ptr = base as *const u8;
        for _ in 0..400 {
            if unsafe { core::ptr::read_volatile(ptr as *const u32) } != SENTINEL {
                let bpp = bytes_per_texel(fp.format);
                let mut sums = [0u64; 4];
                let mut mins = [255u8; 4];
                let mut maxs = [0u8; 4];
                let mut count = 0u64;
                let used = rows.min(fp.height.max(1));
                for row in (0..used).step_by(4) {
                    for col in (0..fp.width).step_by(4) {
                        let at = unsafe { ptr.add((row * fp.row_pitch + col * bpp) as usize) };
                        for c in 0..bpp.min(4) as usize {
                            let v = unsafe { core::ptr::read_volatile(at.add(c)) };
                            sums[c] += v as u64;
                            mins[c] = mins[c].min(v);
                            maxs[c] = maxs[c].max(v);
                        }
                        count += 1;
                    }
                }
                let ch = bpp.min(4) as usize;
                let avg: Vec<String> = (0..ch).map(|c| format!("c{c} avg={} min={} max={}", sums[c] / count.max(1), mins[c], maxs[c])).collect();
                report(format!("video dst readback after NV12 copy #{copy_index} format={} {}x{}: {} ({count} texels sampled)", fp.format, fp.width, fp.height, avg.join(", ")));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = rb;
        report(format!("video dst readback after NV12 copy #{copy_index} never completed within 20 s (list not executed or GPU hung)"));
    });
}
