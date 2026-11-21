use compat_common::{record, E_FAIL, S_OK};
use core::ffi::c_void;
use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::E_INVALIDARG;

pub const ESRAM_PAGE: usize = 0x1_0000;
pub const ESRAM_PAGES: u32 = 512;
pub const ESRAM_BYTES: usize = ESRAM_PAGE * ESRAM_PAGES as usize;
const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;
const MEM_FREE: u32 = 0x1_0000;
const MEM_MAPPED: u32 = 0x4_0000;
const PAGE_READWRITE: u32 = 0x04;
const FILE_MAP_ALL_ACCESS: u32 = 0xF001F;
const M: &str = "d3d12_x";

#[repr(C)]
struct MemoryBasicInformation {
    base: *mut c_void,
    alloc_base: *mut c_void,
    alloc_protect: u32,
    partition: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    kind: u32,
}

extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, typ: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(addr: *mut c_void, size: usize, typ: u32) -> i32;
    fn VirtualQuery(addr: *const c_void, info: *mut MemoryBasicInformation, len: usize) -> usize;
    fn CreateFileMappingW(file: *mut c_void, sa: *const c_void, protect: u32, hi: u32, lo: u32, name: *const u16) -> *mut c_void;
    fn MapViewOfFileEx(map: *mut c_void, access: u32, hi: u32, lo: u32, size: usize, base: *mut c_void) -> *mut c_void;
    fn UnmapViewOfFile(base: *const c_void) -> i32;
}

struct Esram {
    section: usize,
    views: BTreeSet<usize>,
}

static ESRAM: Mutex<Esram> = Mutex::new(Esram { section: 0, views: BTreeSet::new() });

fn query(at: usize) -> Option<MemoryBasicInformation> {
    let mut mbi: MemoryBasicInformation = unsafe { core::mem::zeroed() };
    let n = unsafe { VirtualQuery(at as *const c_void, &mut mbi, core::mem::size_of::<MemoryBasicInformation>()) };
    (n != 0).then_some(mbi)
}

fn allocation_extent(base: usize) -> (usize, bool, Vec<(usize, usize, bool)>) {
    let mut at = base;
    let mut regions = Vec::new();
    while let Some(m) = query(at) {
        if m.alloc_base as usize != base || m.state == MEM_FREE {
            break;
        }
        regions.push((m.base as usize, m.region_size, m.state == MEM_COMMIT));
        at = m.base as usize + m.region_size;
    }
    let mapped = query(base).map_or(false, |m| m.kind == MEM_MAPPED);
    (at, mapped, regions)
}

fn clear_range(st: &mut Esram, va: usize, size: usize) -> Result<Vec<(usize, usize)>, String> {
    for p in (va..va + size).step_by(ESRAM_PAGE) {
        if st.views.remove(&p) {
            unsafe { UnmapViewOfFile(p as *const c_void) };
        }
    }
    let mut keep = Vec::new();
    let mut at = va;
    while at < va + size {
        let Some(m) = query(at) else { return Err(format!("VirtualQuery failed at {at:#x}")) };
        let next = m.base as usize + m.region_size;
        if m.state != MEM_FREE {
            let base = m.alloc_base as usize;
            let (end, mapped, regions) = allocation_extent(base);
            if mapped {
                return Err(format!("{base:#x} is a foreign mapped view"));
            }
            if regions.iter().any(|&(b, l, c)| c && (b < va || b + l > va + size)) {
                return Err(format!("allocation {base:#x}-{end:#x} has committed memory outside the ESRAM range"));
            }
            if unsafe { VirtualFree(base as *mut c_void, 0, MEM_RELEASE) } == 0 {
                return Err(format!("could not release allocation {base:#x}"));
            }
            if base < va {
                keep.push((base, va - base));
            }
            if end > va + size {
                keep.push((va + size, end - (va + size)));
            }
            at = end.max(next);
            continue;
        }
        at = next;
    }
    Ok(keep)
}

#[no_mangle]
pub extern "system" fn D3DMapEsramMemory(flags: u32, va: *mut c_void, num_pages: u32, page_list: *const u32) -> i32 {
    if flags != 1 && flags != 3 {
        record(M, "D3DMapEsramMemory", &format!("flags={flags:#x} not 1 or 3 -> E_FAIL"));
        return E_FAIL;
    }
    let addr = va as usize;
    if addr == 0 || addr % ESRAM_PAGE != 0 || num_pages == 0 || num_pages > ESRAM_PAGES || page_list.is_null() {
        record(M, "D3DMapEsramMemory", &format!("va={va:p} pages={num_pages} list={page_list:p} -> E_INVALIDARG"));
        return E_INVALIDARG;
    }
    let pages = unsafe { core::slice::from_raw_parts(page_list, num_pages as usize) }.to_vec();
    if pages.iter().any(|&p| p >= ESRAM_PAGES) {
        record(M, "D3DMapEsramMemory", "page index beyond the 32 MiB ESRAM -> E_INVALIDARG");
        return E_INVALIDARG;
    }
    let size = num_pages as usize * ESRAM_PAGE;
    let mut st = ESRAM.lock().unwrap_or_else(|e| e.into_inner());
    if st.section == 0 {
        let h = unsafe { CreateFileMappingW(-1isize as *mut c_void, core::ptr::null(), PAGE_READWRITE, 0, ESRAM_BYTES as u32, core::ptr::null()) };
        if h.is_null() {
            record(M, "D3DMapEsramMemory", "could not create the 32 MiB ESRAM section -> E_FAIL");
            return E_FAIL;
        }
        st.section = h as usize;
    }
    let keep = match clear_range(&mut st, addr, size) {
        Ok(k) => k,
        Err(e) => {
            record(M, "D3DMapEsramMemory", &format!("va={va:p} pages={num_pages}: {e} -> E_FAIL"));
            return E_FAIL;
        }
    };
    let mut mapped = Vec::with_capacity(pages.len());
    for (i, &p) in pages.iter().enumerate() {
        let at = addr + i * ESRAM_PAGE;
        let off = p as usize * ESRAM_PAGE;
        let v = unsafe { MapViewOfFileEx(st.section as *mut c_void, FILE_MAP_ALL_ACCESS, 0, off as u32, ESRAM_PAGE, at as *mut c_void) };
        if v as usize != at {
            for m in &mapped {
                unsafe { UnmapViewOfFile(*m as *const c_void) };
                st.views.remove(m);
            }
            record(M, "D3DMapEsramMemory", &format!("view of ESRAM page {p} at {at:#x} failed -> E_FAIL"));
            return E_FAIL;
        }
        st.views.insert(at);
        mapped.push(at);
    }
    for (b, l) in keep {
        unsafe { VirtualAlloc(b as *mut c_void, l, MEM_RESERVE, PAGE_READWRITE) };
    }
    record(M, "D3DMapEsramMemory", &format!("flags={flags} va={va:p} pages={num_pages} first={} -> ESRAM pages aliased into the VA", pages[0]));
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn reserve(n: usize) -> usize {
        unsafe { VirtualAlloc(core::ptr::null_mut(), n * ESRAM_PAGE, MEM_RESERVE, PAGE_READWRITE) as usize }
    }

    fn free_span(base: usize, pages: usize) {
        for p in 0..pages {
            let a = base + p * ESRAM_PAGE;
            let mut st = ESRAM.lock().unwrap();
            if st.views.remove(&a) {
                unsafe { UnmapViewOfFile(a as *const c_void) };
            }
        }
        unsafe { VirtualFree(base as *mut c_void, 0, MEM_RELEASE) };
    }

    #[test]
    fn same_page_at_two_addresses_aliases() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let a = reserve(2);
        let b = reserve(1);
        assert_eq!(D3DMapEsramMemory(1, a as *mut c_void, 2, [40u32, 41].as_ptr()), S_OK);
        assert_eq!(D3DMapEsramMemory(3, b as *mut c_void, 1, [41u32].as_ptr()), S_OK);
        unsafe {
            *((a + ESRAM_PAGE + 8) as *mut u32) = 0xfeed_beef;
            assert_eq!(*((b + 8) as *const u32), 0xfeed_beef);
            *(b as *mut u8) = 9;
            assert_eq!(*((a + ESRAM_PAGE) as *const u8), 9);
        }
        free_span(a, 2);
        free_span(b, 1);
    }

    #[test]
    fn contents_survive_remap_and_follow_page_index() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let a = reserve(1);
        assert_eq!(D3DMapEsramMemory(1, a as *mut c_void, 1, [100u32].as_ptr()), S_OK);
        unsafe { *(a as *mut u64) = 0x1111 };
        assert_eq!(D3DMapEsramMemory(1, a as *mut c_void, 1, [101u32].as_ptr()), S_OK);
        unsafe { *(a as *mut u64) = 0x2222 };
        assert_eq!(D3DMapEsramMemory(1, a as *mut c_void, 1, [100u32].as_ptr()), S_OK);
        assert_eq!(unsafe { *(a as *const u64) }, 0x1111);
        free_span(a, 1);
    }

    #[test]
    fn larger_reservation_keeps_its_remainder() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let a = reserve(4);
        let mid = a + ESRAM_PAGE;
        assert_eq!(D3DMapEsramMemory(1, mid as *mut c_void, 2, [7u32, 8].as_ptr()), S_OK);
        assert_eq!(query(a).unwrap().state, MEM_RESERVE);
        assert_eq!(query(a + 3 * ESRAM_PAGE).unwrap().state, MEM_RESERVE);
        assert_eq!(query(mid).unwrap().kind, MEM_MAPPED);
        unsafe { *(mid as *mut u8) = 1 };
        free_span(mid, 2);
        unsafe { VirtualFree(a as *mut c_void, 0, MEM_RELEASE) };
        unsafe { VirtualFree((a + 3 * ESRAM_PAGE) as *mut c_void, 0, MEM_RELEASE) };
    }

    #[test]
    fn refuses_to_destroy_committed_memory_outside_range() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let a = unsafe { VirtualAlloc(core::ptr::null_mut(), 2 * ESRAM_PAGE, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) as usize };
        unsafe { *(a as *mut u8) = 5 };
        assert_eq!(D3DMapEsramMemory(1, (a + ESRAM_PAGE) as *mut c_void, 1, [3u32].as_ptr()), E_FAIL);
        assert_eq!(unsafe { *(a as *const u8) }, 5);
        unsafe { VirtualFree(a as *mut c_void, 0, MEM_RELEASE) };
    }

    #[test]
    fn maps_unreserved_address() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let a = reserve(1);
        unsafe { VirtualFree(a as *mut c_void, 0, MEM_RELEASE) };
        assert_eq!(D3DMapEsramMemory(1, a as *mut c_void, 1, [0u32].as_ptr()), S_OK);
        unsafe { *(a as *mut u32) = 3 };
        free_span(a, 1);
    }

    #[test]
    fn rejects_like_the_real_runtime() {
        let list = [0u32, 512];
        let va = 0x10_0000_0000usize as *mut c_void;
        assert_eq!(D3DMapEsramMemory(0, va, 1, list.as_ptr()), E_FAIL);
        assert_eq!(D3DMapEsramMemory(2, va, 1, list.as_ptr()), E_FAIL);
        assert_eq!(D3DMapEsramMemory(1, core::ptr::null_mut(), 1, list.as_ptr()), E_INVALIDARG);
        assert_eq!(D3DMapEsramMemory(1, (va as usize + 0x1000) as *mut c_void, 1, list.as_ptr()), E_INVALIDARG);
        assert_eq!(D3DMapEsramMemory(1, va, 0, list.as_ptr()), E_INVALIDARG);
        assert_eq!(D3DMapEsramMemory(1, va, 2, list.as_ptr()), E_INVALIDARG);
        assert_eq!(D3DMapEsramMemory(1, va, 513, list.as_ptr()), E_INVALIDARG);
        assert_eq!(D3DMapEsramMemory(1, va, 1, core::ptr::null()), E_INVALIDARG);
    }
}
