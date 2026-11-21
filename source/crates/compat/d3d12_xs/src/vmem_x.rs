use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use compat_common::record;
use crate::device_wrap::Guid;

const M: &str = "d3d12_xs";

const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const S_OK: i32 = 0;
const HRESULT_FROM_NT_SUCCESS: i32 = 0x1000_0000;
const MAX_PAGE_TABLE_PAGES: u32 = 0x21;
const MEM_COMMIT: u32 = 0x1000;
const WRITABLE: u32 = 0x04 | 0x08 | 0x40 | 0x80;
const CPU_ADDRESS_FLOOR: u64 = 0x100_0000_0000;

#[repr(C)]
#[derive(Default)]
struct Mbi {
    base: usize,
    alloc_base: usize,
    alloc_protect: u32,
    partition: u16,
    pad0: u16,
    size: usize,
    state: u32,
    protect: u32,
    kind: u32,
    pad1: u32,
}

extern "system" {
    fn VirtualQuery(addr: *const c_void, mbi: *mut Mbi, len: usize) -> usize;
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn OutputDebugStringA(s: *const u8);
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    fn CreateEventW(attrs: *const c_void, manual: i32, initial: i32, name: *const u16) -> *mut c_void;
    fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
    fn CloseHandle(h: *mut c_void) -> i32;
}

#[repr(C)]
pub struct VirtualMemoryConfiguration {
    pub flags: u32,
    pub page_table_pages: u32,
    pub frame_buffer_preserve_bytes: u32,
    pub reserved: u32,
    pub output0: u64,
    pub output1: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GraphicsMemoryConfiguration {
    page_table_bytes: u32,
    frame_buffer_preserve_bytes: u32,
    output0: u64,
    output1: u64,
}

static FRAME_BUFFER_PRESERVE: AtomicU32 = AtomicU32::new(0);

unsafe fn register_with_xmem(cfg: &GraphicsMemoryConfiguration) {
    let mut h = GetModuleHandleA(b"xmem.dll\0".as_ptr());
    if h.is_null() {
        h = LoadLibraryA(b"xmem.dll\0".as_ptr());
    }
    if h.is_null() {
        return;
    }
    let f = GetProcAddress(h, b"XMemRegisterGraphicsMemoryConfiguration\0".as_ptr());
    if f.is_null() {
        return;
    }
    let f: unsafe extern "system" fn(*const GraphicsMemoryConfiguration) = core::mem::transmute(f);
    f(cfg);
}

#[no_mangle]
pub unsafe extern "system" fn D3D12ConfigureVirtualMemoryX(desc: *mut VirtualMemoryConfiguration) -> i32 {
    if desc.is_null() || (*desc).flags & !1 != 0 {
        return E_INVALIDARG;
    }
    let d = &mut *desc;
    if d.page_table_pages > MAX_PAGE_TABLE_PAGES {
        let msg = format!(
            "ERROR: Can only support a maximum of {} MB ({} 4MB pages) for graphics page tables.\n\0",
            MAX_PAGE_TABLE_PAGES * 4,
            MAX_PAGE_TABLE_PAGES
        );
        OutputDebugStringA(msg.as_ptr());
        return E_INVALIDARG;
    }
    FRAME_BUFFER_PRESERVE.store(d.frame_buffer_preserve_bytes, Ordering::Release);
    d.output0 = 0;
    d.output1 = 0;
    let cfg = GraphicsMemoryConfiguration {
        page_table_bytes: d.page_table_pages << 22,
        frame_buffer_preserve_bytes: d.frame_buffer_preserve_bytes,
        output0: 0,
        output1: 0,
    };
    register_with_xmem(&cfg);
    record(M, "D3D12ConfigureVirtualMemoryX", &format!("flags={} pages={} fb_preserve={:#x}", d.flags, d.page_table_pages, d.frame_buffer_preserve_bytes));
    HRESULT_FROM_NT_SUCCESS
}

#[no_mangle]
pub extern "system" fn GetFrameBufferPreserveMemorySizeBytes() -> u32 {
    FRAME_BUFFER_PRESERVE.load(Ordering::Acquire)
}

#[no_mangle]
pub extern "system" fn D3D12XboxUnloadDriverModule() {
    if crate::driver_state::unload_driver_module() {
        record(M, "D3D12XboxUnloadDriverModule", "FreeLibrary on the xgs12 driver module this runtime loaded; driver state cleared");
    }
}

#[repr(C)]
pub struct ZeroRange {
    pub address: u64,
    pub size: u64,
}

#[repr(C)]
pub struct ZeroScratch {
    pub command_memory: u64,
    pub command_bytes: u32,
    pub pad0: u32,
    pub fence_memory: u64,
    pub fence_bytes: u32,
    pub pad1: u32,
}

const MEM_IMAGE: u32 = 0x100_0000;

fn regions(addr: u64, size: u64) -> Option<Vec<(u64, u64, u32)>> {
    let mut out = Vec::new();
    let mut p = addr;
    let end = addr.checked_add(size)?;
    while p < end {
        let mut m = Mbi::default();
        if unsafe { VirtualQuery(p as *const c_void, &mut m, core::mem::size_of::<Mbi>()) } == 0 {
            return None;
        }
        if m.state != MEM_COMMIT || m.kind == MEM_IMAGE {
            return None;
        }
        let stop = ((m.base + m.size) as u64).min(end);
        out.push((p, stop - p, m.protect));
        p = stop;
    }
    Some(out)
}

fn cpu_writable(addr: u64, size: u64) -> bool {
    match regions(addr, size) {
        Some(v) => v.iter().all(|&(_, _, pr)| pr & WRITABLE != 0 && pr & 0x100 == 0),
        None => false,
    }
}

unsafe fn gpu_fill_zero(addr: u64, size: u64) -> bool {
    let rs = match regions(addr, size) {
        Some(v) => v,
        None => return false,
    };
    for (p, n, pr) in rs {
        if pr & WRITABLE != 0 && pr & 0x100 == 0 {
            core::ptr::write_bytes(p as *mut u8, 0, n as usize);
            continue;
        }
        let mut old = 0u32;
        if VirtualProtect(p as *mut c_void, n as usize, 0x04, &mut old) == 0 {
            return false;
        }
        core::ptr::write_bytes(p as *mut u8, 0, n as usize);
        let mut tmp = 0u32;
        VirtualProtect(p as *mut c_void, n as usize, old, &mut tmp);
    }
    true
}

const PACKET_DWORDS: usize = 0x10;

struct CommandWriter {
    base: *mut u32,
    cap: usize,
    pos: usize,
}

impl CommandWriter {
    unsafe fn new(scratch: *const ZeroScratch) -> Option<Self> {
        if scratch.is_null() {
            return None;
        }
        let s = &*scratch;
        let cap = s.command_bytes as usize / 4;
        if !cpu_writable(s.command_memory, (cap * 4) as u64) {
            return None;
        }
        Some(Self { base: s.command_memory as *mut u32, cap, pos: 0 })
    }

    unsafe fn dma_zero(&mut self, addr: u64, size: u64) {
        if self.cap - self.pos < 0x18 {
            self.pos = 0;
        }
        let pkt: [u32; PACKET_DWORDS] = [
            0xc009_7600,
            0x240,
            addr as u32,
            (addr >> 32) as u32,
            size as u32,
            0x2101_4fac,
            0,
            0,
            0,
            0,
            0,
            0xc003_1500,
            (size >> 4) as u32,
            1,
            1,
            0x25,
        ];
        core::ptr::copy_nonoverlapping(pkt.as_ptr(), self.base.add(self.pos), PACKET_DWORDS);
        self.pos += PACKET_DWORDS;
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12ZeroMemoryX(count: u32, ranges: *const ZeroRange, scratch: *const ZeroScratch, flags: u32) -> i32 {
    if count == 0 {
        return S_OK;
    }
    if ranges.is_null() {
        return E_INVALIDARG;
    }
    let rs = core::slice::from_raw_parts(ranges, count as usize);
    if rs.iter().any(|r| r.address & 0xf != 0 || r.size & 0xf != 0) {
        return E_INVALIDARG;
    }
    if !scratch.is_null() {
        let s = &*scratch;
        if s.command_memory == 0 || s.command_bytes < 0x1_0000 || s.fence_memory == 0 || s.fence_bytes < 0x100 {
            return E_INVALIDARG;
        }
    }
    let cpu_only = flags & 1 != 0;
    let mut cmd = if cpu_only { None } else { CommandWriter::new(scratch) };
    let mut gpu_spans = Vec::new();
    for r in rs {
        if r.size == 0 {
            continue;
        }
        if cpu_only || r.address >= CPU_ADDRESS_FLOOR {
            if !cpu_writable(r.address, r.size) {
                record(M, "D3D12ZeroMemoryX", &format!("cpu range {:#x}+{:#x} not writable (flags={flags:#x})", r.address, r.size));
                return E_FAIL;
            }
            core::ptr::write_bytes(r.address as *mut u8, 0, r.size as usize);
            continue;
        }
        if let Some(c) = cmd.as_mut() {
            c.dma_zero(r.address, r.size);
        }
        if crate::gpu_va::lookup_gpu(r.address).is_some() {
            gpu_spans.push((r.address, r.size));
        } else if !gpu_fill_zero(r.address, r.size) {
            record(M, "D3D12ZeroMemoryX", &format!("gpu range {:#x}+{:#x} not mapped; DMA would fault", r.address, r.size));
            return E_FAIL;
        }
    }
    if !gpu_spans.is_empty() && !gpu_zero(&gpu_spans) {
        return E_FAIL;
    }
    S_OK
}

const IID_DEVICE: Guid = Guid { d1: 0x1898_19F1, d2: 0x1DB6, d3: 0x4B57, d4: [0xBE, 0x54, 0x18, 0x21, 0x33, 0x9B, 0x85, 0xF7] };
const IID_QUEUE: Guid = Guid { d1: 0x0ec8_70a6, d2: 0x5d7e, d3: 0x4c22, d4: [0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed] };
const IID_ALLOCATOR: Guid = Guid { d1: 0x6102_dee4, d2: 0xaf59, d3: 0x4b09, d4: [0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24] };
const IID_LIST: Guid = Guid { d1: 0x5b16_0d0f, d2: 0xac1b, d3: 0x4185, d4: [0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55] };
const IID_FENCE: Guid = Guid { d1: 0x0a75_3dcf, d2: 0xc4d8, d3: 0x4b91, d4: [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76] };
const GPU_TIMEOUT_MS: u32 = 10_000;

struct GpuCtx {
    queue: usize,
    alloc: usize,
    list: usize,
    fence: usize,
    event: usize,
    value: u64,
}

static GPU: Mutex<Vec<(usize, GpuCtx)>> = Mutex::new(Vec::new());

unsafe fn vt(o: usize) -> *const usize {
    *(o as *const *const usize)
}

unsafe fn device_of_resource(res: usize) -> usize {
    let f: unsafe extern "system" fn(usize, *const Guid, *mut usize) -> i32 = core::mem::transmute(*vt(res).add(7));
    let mut dev = 0usize;
    if f(res, &IID_DEVICE, &mut dev) < 0 || dev == 0 {
        return 0;
    }
    let release: unsafe extern "system" fn(usize) -> u32 = core::mem::transmute(*vt(dev).add(2));
    release(dev);
    dev
}

unsafe fn create_ctx(dev: usize) -> Option<GpuCtx> {
    let qdesc = [0u32; 4];
    let cq: unsafe extern "system" fn(usize, *const u32, *const Guid, *mut usize) -> i32 = core::mem::transmute(*vt(dev).add(8));
    let mut queue = 0usize;
    if cq(dev, qdesc.as_ptr(), &IID_QUEUE, &mut queue) < 0 || queue == 0 {
        return None;
    }
    let ca: unsafe extern "system" fn(usize, u32, *const Guid, *mut usize) -> i32 = core::mem::transmute(*vt(dev).add(9));
    let mut alloc = 0usize;
    if ca(dev, 0, &IID_ALLOCATOR, &mut alloc) < 0 || alloc == 0 {
        return None;
    }
    let cl: unsafe extern "system" fn(usize, u32, u32, usize, usize, *const Guid, *mut usize) -> i32 = core::mem::transmute(*vt(dev).add(12));
    let mut list = 0usize;
    if cl(dev, 0, 0, alloc, 0, &IID_LIST, &mut list) < 0 || list == 0 {
        return None;
    }
    let close: unsafe extern "system" fn(usize) -> i32 = core::mem::transmute(*vt(list).add(9));
    close(list);
    let cf: unsafe extern "system" fn(usize, u64, u32, *const Guid, *mut usize) -> i32 = core::mem::transmute(*vt(dev).add(36));
    let mut fence = 0usize;
    if cf(dev, 0, 0, &IID_FENCE, &mut fence) < 0 || fence == 0 {
        return None;
    }
    let event = CreateEventW(core::ptr::null(), 0, 0, core::ptr::null()) as usize;
    if event == 0 {
        return None;
    }
    Some(GpuCtx { queue, alloc, list, fence, event, value: 0 })
}

unsafe fn gpu_zero(spans: &[(u64, u64)]) -> bool {
    let mut by_dev: Vec<(usize, Vec<(u64, u32)>)> = Vec::new();
    for &(addr, size) in spans {
        let mut cur = addr;
        let end = addr + size;
        while cur < end {
            let (e, off) = match crate::gpu_va::lookup_gpu(cur) {
                Some(v) => v,
                None => {
                    record(M, "D3D12ZeroMemoryX", &format!("gpu va {cur:#x} not owned by any resource; DMA would fault"));
                    return false;
                }
            };
            let dev = device_of_resource(e.res);
            if dev == 0 {
                return false;
            }
            let n = (end - cur).min(e.len - off).min(0x8000_0000);
            match by_dev.iter_mut().find(|(d, _)| *d == dev) {
                Some((_, v)) => v.push((cur, n as u32)),
                None => by_dev.push((dev, vec![(cur, n as u32)])),
            }
            cur += n;
        }
    }
    let t0 = std::time::Instant::now();
    let mut pending: Vec<(usize, u64)> = Vec::new();
    {
        let mut g = GPU.lock().unwrap_or_else(|e| e.into_inner());
        for (dev, fills) in by_dev {
            if !g.iter().any(|(d, _)| *d == dev) {
                match create_ctx(dev) {
                    Some(c) => g.push((dev, c)),
                    None => {
                        record(M, "D3D12ZeroMemoryX", "could not create a zeroing queue on the resource's device");
                        return false;
                    }
                }
            }
            let c = &mut g.iter_mut().find(|(d, _)| *d == dev).unwrap().1;
            if !wait_fence(c.fence, c.value, c.event) {
                record(M, "D3D12ZeroMemoryX", "previous zeroing DMA did not complete before the fence timeout");
                return false;
            }
            let reset_alloc: unsafe extern "system" fn(usize) -> i32 = core::mem::transmute(*vt(c.alloc).add(8));
            let reset_list: unsafe extern "system" fn(usize, usize, usize) -> i32 = core::mem::transmute(*vt(c.list).add(10));
            let close: unsafe extern "system" fn(usize) -> i32 = core::mem::transmute(*vt(c.list).add(9));
            if reset_alloc(c.alloc) < 0 || reset_list(c.list, c.alloc, 0) < 0 {
                return false;
            }
            for (va, n) in fills {
                crate::fill::fill(c.list as *mut c_void, false, va, n, 0, 0);
            }
            if close(c.list) < 0 {
                return false;
            }
            let exec: unsafe extern "system" fn(usize, u32, *const usize) = core::mem::transmute(*vt(c.queue).add(10));
            exec(c.queue, 1, &c.list);
            c.value += 1;
            let signal: unsafe extern "system" fn(usize, usize, u64) -> i32 = core::mem::transmute(*vt(c.queue).add(14));
            if signal(c.queue, c.fence, c.value) < 0 {
                return false;
            }
            pending.push((c.fence, c.value));
        }
    }
    let submitted = t0.elapsed().as_secs_f64() * 1000.0;
    for (fence, value) in pending {
        let ev = CreateEventW(core::ptr::null(), 0, 0, core::ptr::null()) as usize;
        if ev == 0 {
            return false;
        }
        let ok = wait_fence(fence, value, ev);
        CloseHandle(ev as *mut c_void);
        if !ok {
            record(M, "D3D12ZeroMemoryX", "zeroing DMA did not complete before the fence timeout");
            return false;
        }
    }
    let total = t0.elapsed().as_secs_f64() * 1000.0;
    if total > 20.0 {
        record(M, "D3D12ZeroMemoryX", &format!("slow: {total:.1} ms ({submitted:.1} ms under the lock) for {} spans", spans.len()));
    }
    true
}

unsafe fn wait_fence(fence: usize, value: u64, event: usize) -> bool {
    let completed: unsafe extern "system" fn(usize) -> u64 = core::mem::transmute(*vt(fence).add(8));
    if completed(fence) >= value {
        return true;
    }
    let on: unsafe extern "system" fn(usize, u64, usize) -> i32 = core::mem::transmute(*vt(fence).add(9));
    on(fence, value, event) >= 0 && (WaitForSingleObject(event as *mut c_void, GPU_TIMEOUT_MS) == 0 || completed(fence) >= value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configure_validates_and_records_preserve_size() {
        assert_eq!(unsafe { D3D12ConfigureVirtualMemoryX(core::ptr::null_mut()) }, E_INVALIDARG);
        let mut d = VirtualMemoryConfiguration { flags: 2, page_table_pages: 1, frame_buffer_preserve_bytes: 0, reserved: 0, output0: 9, output1: 9 };
        assert_eq!(unsafe { D3D12ConfigureVirtualMemoryX(&mut d) }, E_INVALIDARG);
        d.flags = 1;
        d.page_table_pages = 0x22;
        assert_eq!(unsafe { D3D12ConfigureVirtualMemoryX(&mut d) }, E_INVALIDARG);
        d.page_table_pages = 0x21;
        d.frame_buffer_preserve_bytes = 0x30_0000;
        assert_eq!(unsafe { D3D12ConfigureVirtualMemoryX(&mut d) }, HRESULT_FROM_NT_SUCCESS);
        assert_eq!(GetFrameBufferPreserveMemorySizeBytes(), 0x30_0000);
        assert_eq!((d.output0, d.output1), (0, 0));
    }

    #[test]
    fn zero_memory_clears_cpu_ranges_and_checks_alignment() {
        let mut buf = vec![0xffu8; 0x100 + 16];
        let base = ((buf.as_mut_ptr() as usize + 15) & !15) as u64;
        let r = [ZeroRange { address: base, size: 0x40 }, ZeroRange { address: base + 0x80, size: 0x10 }];
        assert_eq!(unsafe { D3D12ZeroMemoryX(2, r.as_ptr(), core::ptr::null(), 0) }, S_OK);
        let s = unsafe { core::slice::from_raw_parts(base as *const u8, 0x100) };
        assert!(s[..0x40].iter().all(|&b| b == 0));
        assert!(s[0x40..0x80].iter().all(|&b| b == 0xff));
        assert!(s[0x80..0x90].iter().all(|&b| b == 0));
        let bad = [ZeroRange { address: base + 4, size: 0x10 }];
        assert_eq!(unsafe { D3D12ZeroMemoryX(1, bad.as_ptr(), core::ptr::null(), 0) }, E_INVALIDARG);
        let gpu = [ZeroRange { address: 0x10, size: 0x10 }];
        assert_eq!(unsafe { D3D12ZeroMemoryX(1, gpu.as_ptr(), core::ptr::null(), 0) }, E_FAIL);
        assert_eq!(unsafe { D3D12ZeroMemoryX(0, core::ptr::null(), core::ptr::null(), 0) }, S_OK);
        let sc = ZeroScratch { command_memory: 1, command_bytes: 0x100, pad0: 0, fence_memory: 1, fence_bytes: 0x100, pad1: 0 };
        assert_eq!(unsafe { D3D12ZeroMemoryX(1, r.as_ptr(), &sc, 0) }, E_INVALIDARG);
    }

    extern "system" {
        fn VirtualAlloc(a: *mut c_void, s: usize, t: u32, p: u32) -> *mut c_void;
        fn VirtualFree(a: *mut c_void, s: usize, t: u32) -> i32;
    }

    #[test]
    fn zero_memory_gpu_path_ignores_cpu_protection_and_emits_dma() {
        unsafe {
            let mut p = core::ptr::null_mut::<u8>();
            for hint in [0x10_0000_0000u64, 0x20_0000_0000, 0x40_0000_0000, 0x80_0000_0000, 0x8000_0000] {
                p = VirtualAlloc(hint as *mut c_void, 0x2000, 0x3000, 0x04) as *mut u8;
                if !p.is_null() {
                    break;
                }
            }
            assert!(!p.is_null());
            assert!((p as u64) < CPU_ADDRESS_FLOOR);
            core::ptr::write_bytes(p, 0xab, 0x2000);
            let mut old = 0u32;
            VirtualProtect(p as *mut c_void, 0x1000, 0x02, &mut old);
            let mut cmd = vec![0u32; 0x4000];
            let fence = vec![0u64; 0x20];
            let sc = ZeroScratch { command_memory: cmd.as_mut_ptr() as u64, command_bytes: 0x10000, pad0: 0, fence_memory: fence.as_ptr() as u64, fence_bytes: 0x100, pad1: 0 };
            let r = [ZeroRange { address: p as u64 + 0xff0, size: 0x20 }];
            assert_eq!(D3D12ZeroMemoryX(1, r.as_ptr(), &sc, 0), S_OK);
            let s = core::slice::from_raw_parts(p, 0x2000);
            assert!(s[0xff0..0x1010].iter().all(|&b| b == 0));
            assert_eq!(s[0xfef], 0xab);
            assert_eq!(s[0x1010], 0xab);
            let mut m = Mbi::default();
            VirtualQuery(p as *const c_void, &mut m, core::mem::size_of::<Mbi>());
            assert_eq!(m.protect, 0x02);
            assert_eq!(cmd[0], 0xc009_7600);
            assert_eq!(cmd[2], (p as u64 + 0xff0) as u32);
            assert_eq!(cmd[12], 2);
            assert_eq!(D3D12ZeroMemoryX(1, r.as_ptr(), core::ptr::null(), 1), E_FAIL);
            VirtualFree(p as *mut c_void, 0, 0x8000);
            let unmapped = [ZeroRange { address: p as u64, size: 0x10 }];
            assert_eq!(D3D12ZeroMemoryX(1, unmapped.as_ptr(), core::ptr::null(), 0), E_FAIL);
        }
    }
}
