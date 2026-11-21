use compat_common::record;
use core::sync::atomic::{AtomicUsize, Ordering};

const RAYTRACING_AS: u32 = 0x40_0000;

fn fix_state(s: u32) -> u32 {
    if s & RAYTRACING_AS != 0 && s != RAYTRACING_AS {
        s & !RAYTRACING_AS
    } else {
        s
    }
}

pub unsafe fn sanitize(n: u32, barriers: *const u8) -> Option<Vec<u8>> {
    if n == 0 || barriers.is_null() {
        return None;
    }
    let len = n as usize * 32;
    let src = core::slice::from_raw_parts(barriers, len);
    let mut out: Option<Vec<u8>> = None;
    for i in 0..n as usize {
        let p = i * 32;
        let ty = u32::from_le_bytes(src[p..p + 4].try_into().unwrap());
        if ty == 1 {
            let before = u64::from_le_bytes(src[p + 8..p + 16].try_into().unwrap()) as usize;
            let after = u64::from_le_bytes(src[p + 16..p + 24].try_into().unwrap()) as usize;
            let committed = |r: usize| crate::placed::is_committed_placement(r as *mut core::ffi::c_void);
            if committed(before) || committed(after) {
                let v = out.get_or_insert_with(|| src.to_vec());
                v[p..p + 4].copy_from_slice(&2u32.to_le_bytes());
                v[p + 4..p + 8].copy_from_slice(&0u32.to_le_bytes());
                v[p + 8..p + 24].fill(0);
                static ALIASED: AtomicUsize = AtomicUsize::new(0);
                if ALIASED.fetch_add(1, Ordering::Relaxed) < 8 {
                    record("d3d12_xs", "xone.barrier", &format!("aliasing barrier before={before:#x} after={after:#x} -> global UAV barrier (placed resources are committed on PC, so they cannot alias)"));
                }
            }
            continue;
        }
        if ty != 0 {
            continue;
        }
        for off in [p + 20, p + 24] {
            let s = u32::from_le_bytes(src[off..off + 4].try_into().unwrap());
            let f = fix_state(s);
            if f != s {
                let v = out.get_or_insert_with(|| src.to_vec());
                v[off..off + 4].copy_from_slice(&f.to_le_bytes());
                static LOGGED: AtomicUsize = AtomicUsize::new(0);
                if LOGGED.fetch_add(1, Ordering::Relaxed) < 8 {
                    record("d3d12_xs", "xone.barrier", &format!("state {s:#x} -> {f:#x}: Xbox bit 0x400000 combined with other states (on PC it is RAYTRACING_ACCELERATION_STRUCTURE, valid only alone)"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition(res: u64, before: u32, after: u32) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[8..16].copy_from_slice(&res.to_le_bytes());
        b[16..20].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        b[20..24].copy_from_slice(&before.to_le_bytes());
        b[24..28].copy_from_slice(&after.to_le_bytes());
        b
    }

    #[test]
    fn strips_combined_xbox_bit_only() {
        assert_eq!(fix_state(0x4000c0), 0xc0);
        assert_eq!(fix_state(0x400000), 0x400000);
        assert_eq!(fix_state(0xc0), 0xc0);
        let b = transition(0x1000, 0x4, 0x4000c0);
        let out = unsafe { sanitize(1, b.as_ptr()) }.expect("rewritten");
        assert_eq!(u32::from_le_bytes(out[24..28].try_into().unwrap()), 0xc0);
        assert_eq!(u32::from_le_bytes(out[20..24].try_into().unwrap()), 0x4);
        let clean = transition(0x1000, 0x4, 0xc0);
        assert!(unsafe { sanitize(1, clean.as_ptr()) }.is_none());
    }

    #[test]
    fn aliasing_without_placed_resource_is_kept() {
        let mut b = [0u8; 32];
        b[0..4].copy_from_slice(&1u32.to_le_bytes());
        assert!(unsafe { sanitize(1, b.as_ptr()) }.is_none());
    }
}
