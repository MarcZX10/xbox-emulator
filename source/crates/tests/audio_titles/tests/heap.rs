use audio_titles::*;

fn state() -> [u32; 5] {
    let mut s = [0u32; 5];
    assert_eq!(unsafe { (acphal().heap_state)(s.as_mut_ptr(), 1) }, S_OK, "ApuHeapGetState(&state, 1) 0x140964b68");
    s
}

#[test]
fn fortnite_apu_heap_state_tracks_alloc_and_free() {
    let _g = serial();
    heap(0x4000000);
    let s0 = state();
    assert!(s0[0] + s0[1] + s0[2] <= 0x4000000, "{s0:x?}");
    assert!(s0[3] <= s0[0]);
    let a = ApuMem::new(0x10000, 0x100, 1);
    assert_eq!(a.apu & 0xff, 0);
    let s1 = state();
    assert_eq!(s1[4], s0[4] + 1, "allocation count");
    assert_eq!(s1[1], s0[1] + 0x10000, "bytes in use");
    assert!(s0[0] - s1[0] >= 0x10000 && s0[0] - s1[0] < 0x10100, "free bytes {:#x} -> {:#x}", s0[0], s1[0]);
    let b = ApuMem::new(0x3e0000, 0x40, 1);
    let s2 = state();
    assert_eq!(s2[4], s0[4] + 2);
    drop(a);
    let s3 = state();
    assert_eq!(s3[4], s0[4] + 1);
    assert_eq!(s3[1], s0[1] + 0x3e0000);
    drop(b);
    assert_eq!(state(), s0, "ApuFree returns every byte and coalesces the free list");
    let mut s = [0u32; 5];
    assert_ne!(unsafe { (acphal().heap_state)(s.as_mut_ptr(), 3) }, S_OK, "heap 3 does not exist");
}

#[test]
fn fortnite_apu_heap_reuses_freed_blocks() {
    let _g = serial();
    heap(0x4000000);
    let a = ApuMem::new(0x40000, 0x80, 1);
    let at = a.apu;
    drop(a);
    let b = ApuMem::new(0x40000, 0x80, 1);
    assert_eq!(b.apu, at, "a freed overlap buffer is handed out again");
}

#[test]
fn xdefiant_apu_map_apu_address_round_trips_with_map_virtual_address() {
    let _g = serial();
    heap(0x4000000);
    let a = ApuMem::new(0x2100, 0x100, 1);
    let h = acphal();
    for off in [0u32, 4, 0x1f00, 0x20fc] {
        let va = unsafe { (h.map_apu)(a.apu + off) } as usize;
        assert_eq!(va, a.va + off as usize, "ApuMapApuAddress({:#x})", a.apu + off);
        assert_eq!(unsafe { (h.map_va)(va as *const _) }, a.apu + off, "ApuMapVirtualAddress({va:#x})");
    }
    let outside = 0u8;
    assert_eq!(unsafe { (h.map_va)(&outside as *const u8 as *const _) }, 0, "a stack address is not APU memory");
}