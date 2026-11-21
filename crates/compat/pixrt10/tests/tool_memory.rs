use core::ffi::c_void;
use pixrt10::PixDllInitializeToolMemory;

#[test]
fn pixrt10_tool_memory_has_no_guard_and_honours_flag_and_size() {
    unsafe {
        assert_eq!(PixDllInitializeToolMemory(0, 0, 0, core::ptr::null_mut(), 16), 0);
        let mut t = [9u64; 3];
        assert_eq!(PixDllInitializeToolMemory(0, 0, 0, t.as_mut_ptr() as *mut c_void, 0x18), 0);
        assert_eq!(t[..2], pixrt10::tool_table().map(|v| v as u64));
        assert_eq!(t[2], 9);
        let mut z = [9u64; 2];
        assert_eq!(PixDllInitializeToolMemory(0, 0, 1, z.as_mut_ptr() as *mut c_void, 16), 0);
        assert_eq!(z, [0, 0]);
        let mut n = [9u64; 2];
        assert_eq!(PixDllInitializeToolMemory(0, 0, 0, n.as_mut_ptr() as *mut c_void, 0), 0);
        assert_eq!(n, [9, 9]);
        let mut h = [0u8; 16];
        assert_eq!(PixDllInitializeToolMemory(0, 0, 0, h.as_mut_ptr() as *mut c_void, 3), 0);
        assert_eq!(h[..3], (pixrt10::tool_table()[0] as u64).to_le_bytes()[..3]);
        assert_eq!(h[3..], [0u8; 13]);
    }
}
