use super::*;

#[test]
fn get_register_offsets_x64_table_and_other_arch_null() {
    let p = GetRegisterOffsets(ARCH_X64);
    assert_eq!(p, TTD_X64_REGISTER_OFFSETS.as_ptr());
    let t = unsafe { core::slice::from_raw_parts(p, REGISTER_COUNT + 1) };
    assert_eq!(&t[..4], &[0x70, 0x58, 0x60, 0x78]);
    assert_eq!(t[REGISTER_COUNT - 1], 0x3d0);
    assert_eq!(t[REGISTER_COUNT], 0);
    assert!(t[..REGISTER_COUNT].iter().all(|&v| v != 0));
    for a in [0u8, 1, 3, 0xff] {
        assert!(GetRegisterOffsets(a).is_null());
    }
}

unsafe extern "system" fn user_cb() {}

#[test]
fn register_instrumentation_callbacks_stores_and_defaults_nulls() {
    let mut table = [0usize; CALLBACK_SLOTS];
    table[0] = user_cb as *const () as usize;
    table[9] = 0x1234_5678;
    unsafe { RegisterInstrumentationCallbacks(table.as_ptr()) };
    assert_eq!(instrumentation_callback(0), user_cb as *const () as usize);
    assert_eq!(instrumentation_callback(9), 0x1234_5678);
    for s in 1..9 {
        assert_eq!(instrumentation_callback(s), default_callback(s));
    }
    assert_ne!(default_callback(STRUCT_DEFAULT_SLOT), default_callback(1));
    let f: unsafe extern "system" fn(*mut Pair) -> *mut Pair = unsafe { core::mem::transmute(instrumentation_callback(STRUCT_DEFAULT_SLOT)) };
    let mut p = Pair { a: 7, b: 9 };
    let r = unsafe { f(&mut p) };
    assert_eq!(r, &mut p as *mut Pair);
    assert_eq!(p, Pair::default());
    let g: unsafe extern "system" fn() = unsafe { core::mem::transmute(instrumentation_callback(1)) };
    unsafe { g() };
}
