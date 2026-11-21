use compat_common::gameinput_ext::*;
use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

fn setup() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = std::env::temp_dir();
    p.push("tombox_gameinput_ext_test.log");
    std::env::set_var("TOMBOX_LOG", p);
    g
}

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn call0<R>(obj: *mut c_void, i: usize) -> R {
    let f: unsafe extern "system" fn(*mut c_void) -> R = core::mem::transmute(slot(obj, i));
    f(obj)
}

unsafe fn release(obj: *mut c_void) -> u32 {
    call0::<u32>(obj, 2)
}

#[test]
fn kind_names_decode_masks() {
    assert_eq!(kind_names(0), "Unknown");
    assert_eq!(kind_names(KIND_GAMEPAD), "Gamepad");
    assert_eq!(kind_names(0x40030), "Keyboard|Mouse|Gamepad");
    assert_eq!(kind_names(0x0E), "ControllerAxis|ControllerButton|ControllerSwitch");
    assert_eq!(kind_names(0x8000_0010), "Keyboard|0x80000000");
}

#[test]
fn kind_log_aggregates_per_api_kind_filter_target() {
    let mut l = KindLog::default();
    l.note(Api::RegisterDeviceCallback, 0x40030, 1, Target::AnyDevice);
    l.note(Api::GetCurrentReading, 0x40030, 0, Target::VirtualPad);
    let r = l.note(Api::GetCurrentReading, 0x40030, 0, Target::VirtualPad);
    assert_eq!(r.count, 2);
    l.note(Api::GetCurrentReading, KIND_GAMEPAD, 0, Target::AnyDevice);
    assert_eq!(l.records.len(), 3);
    assert_eq!(l.requested(), 0x40030);
    assert_eq!(l.requested_by(Api::RegisterDeviceCallback), 0x40030);
    assert_eq!(l.requested_by(Api::RegisterReadingCallback), 0);
    let rep = l.report();
    assert!(rep.contains("GetCurrentReading kind=0x40030 (Keyboard|Mouse|Gamepad) filter=0x0 target=virtual-pad calls=2"));
    assert!(rep.lines().next().unwrap().starts_with("GetCurrentReading kind=0x40000"));
    assert!(should_log_count(1));
    assert!(!should_log_count(2));
    assert!(!should_log_count(9));
    assert!(should_log_count(16));
    assert!(!should_log_count(17));
    assert!(should_log_count(200_000));
}

#[test]
fn abi_struct_sizes_match_console_gameinput() {
    assert_eq!(core::mem::size_of::<KeyState>(), 12);
    assert_eq!(core::mem::size_of::<MouseState>(), 0x28);
    assert_eq!(core::mem::size_of::<RumbleParams>(), 16);
    assert_eq!(core::mem::size_of::<KeyboardInfo>(), 40);
    assert_eq!(core::mem::size_of::<MouseInfo>(), 16);
    assert_eq!(core::mem::size_of::<GiString>(), 24);
    let m = MouseState { buttons: BUTTON_LEFT | BUTTON_5, reserved: 0, position_x: -5, position_y: 7, wheel_x: 1, wheel_y: -120 };
    let b = m.to_bytes();
    assert_eq!(u32::from_le_bytes(b[0..4].try_into().unwrap()), 0x11);
    assert_eq!(i64::from_le_bytes(b[8..16].try_into().unwrap()), -5);
    assert_eq!(i64::from_le_bytes(b[16..24].try_into().unwrap()), 7);
    assert_eq!(i64::from_le_bytes(b[32..40].try_into().unwrap()), -120);
    let raw: [u8; 40] = unsafe { core::mem::transmute(m) };
    assert_eq!(raw, b);
}

#[test]
fn parses_keys_buttons_and_commands() {
    let w = parse_key("W").unwrap();
    assert_eq!((w.vk, w.scan, w.lower, w.upper), (0x57, 0x11, b'w' as u32, b'W' as u32));
    let one = parse_key("1").unwrap();
    assert_eq!((one.vk, one.scan, one.upper), (b'1', 0x02, b'!' as u32));
    let zero = parse_key("0").unwrap();
    assert_eq!((zero.scan, zero.upper), (0x0B, b')' as u32));
    assert_eq!(parse_key("LShift").unwrap().vk, 0xA0);
    assert_eq!(parse_key("up").unwrap().scan, 0xE048);
    assert_eq!(parse_key("page_up").unwrap().vk, 0x21);
    assert_eq!(parse_key("f12").unwrap().scan, 0x58);
    assert_eq!(parse_key("vk:0x5A").unwrap().vk, 0x5A);
    assert!(parse_key("nope").is_none());
    assert_eq!(parse_button("Right"), Some(BUTTON_RIGHT));
    assert_eq!(parse_button("x2"), Some(BUTTON_5));
    assert_eq!(parse_kbm_command("key lshift+w 300").unwrap(), Some(KbmCmd::KeyTap(vec![parse_key("lshift").unwrap(), w], 300)));
    assert_eq!(parse_kbm_command("tap space").unwrap(), Some(KbmCmd::KeyTap(vec![parse_key("space").unwrap()], DEFAULT_TAP_MS)));
    assert_eq!(parse_kbm_command("keydown esc").unwrap(), Some(KbmCmd::KeyDown(vec![parse_key("esc").unwrap()])));
    assert_eq!(parse_kbm_command("move 10 -4").unwrap(), Some(KbmCmd::Move(10, -4)));
    assert_eq!(parse_kbm_command("click left 50ms").unwrap(), Some(KbmCmd::Click(BUTTON_LEFT, 50)));
    assert_eq!(parse_kbm_command("mousedown middle").unwrap(), Some(KbmCmd::ButtonDown(BUTTON_MIDDLE)));
    assert_eq!(parse_kbm_command("wheel -120").unwrap(), Some(KbmCmd::Wheel(0, -120)));
    assert_eq!(parse_kbm_command("wheel 120 3").unwrap(), Some(KbmCmd::Wheel(3, 120)));
    assert_eq!(parse_kbm_command("connect mouse").unwrap(), Some(KbmCmd::Connect(KIND_MOUSE)));
    assert_eq!(parse_kbm_command("unplug").unwrap(), Some(KbmCmd::Disconnect(KIND_KBM)));
    assert_eq!(parse_kbm_command("# hi").unwrap(), None);
    assert!(parse_kbm_command("key").is_err());
    assert!(parse_kbm_command("move 1").is_err());
    assert!(parse_kbm_command("click side").is_err());
    assert!(parse_kbm_command("connect pad").is_err());
    assert!(parse_kbm_command("jump").is_err());
}

#[test]
fn kbm_state_tracks_keys_shift_buttons_and_relative_motion() {
    let mut s = KbmState::default();
    let a = parse_key("a").unwrap();
    assert!(s.key(a, true));
    assert!(!s.key(a, true));
    assert_eq!(s.key_states()[0].code_point, b'a' as u32);
    assert!(s.key(parse_key("rshift").unwrap(), true));
    let ks = s.key_states();
    assert_eq!(ks.len(), 2);
    assert_eq!(ks[0], KeyState { scan_code: 0x1E, code_point: b'A' as u32, virtual_key: 0x41, is_dead_key: 0, reserved: [0; 2] });
    assert_eq!(ks[1].code_point, 0);
    assert!(s.key(a, false));
    assert!(!s.key(a, false));
    for c in b"bcdefghijklmnopqrstu" {
        s.key(parse_key(&(*c as char).to_string()).unwrap(), true);
    }
    assert_eq!(s.keys.len(), MAX_KEYS);
    assert!(s.button(BUTTON_LEFT | BUTTON_RIGHT, true));
    assert!(!s.button(BUTTON_LEFT, true));
    assert!(s.button(BUTTON_LEFT, false));
    assert_eq!(s.mouse.buttons, BUTTON_RIGHT);
    assert!(!s.button(0x80, true));
    assert!(s.move_by(10, -3));
    assert!(s.move_by(-4, 1));
    assert!(!s.move_by(0, 0));
    assert_eq!((s.mouse.position_x, s.mouse.position_y), (6, -2));
    s.wheel(0, 120);
    s.wheel(0, -240);
    assert_eq!(s.mouse.wheel_y, -120);
    assert_eq!(s.release(), (true, true));
    assert_eq!(s.release(), (false, false));
    assert_eq!(s.mouse.position_x, 6);
}

#[test]
fn key_state_fill_writes_min_of_count_and_pressed() {
    let st = [KeyState { scan_code: 1, ..Default::default() }, KeyState { scan_code: 2, ..Default::default() }, KeyState { scan_code: 3, ..Default::default() }];
    let mut out = [KeyState { scan_code: 99, ..Default::default() }; 4];
    assert_eq!(fill_key_states(&st, 2, out.as_mut_ptr()), 2);
    assert_eq!((out[0].scan_code, out[1].scan_code, out[2].scan_code), (1, 2, 99));
    assert_eq!(fill_key_states(&st, 10, out.as_mut_ptr()), 3);
    assert_eq!(fill_key_states(&st, 10, core::ptr::null_mut()), 0);
}

#[test]
fn device_info_layout_matches_gdk_v0_offsets() {
    let b = device_info_bytes(KIND_KEYBOARD);
    assert_eq!(u32::from_le_bytes(b[0..4].try_into().unwrap()), 0x140);
    assert_eq!(u16::from_le_bytes(b[4..6].try_into().unwrap()), 0x045E);
    assert_eq!(u16::from_le_bytes(b[14..16].try_into().unwrap()), 6);
    assert_eq!(&b[INFO_DEVICE_ID..INFO_DEVICE_ID + 32], &KEYBOARD_ID);
    assert_eq!(&b[INFO_ROOT_ID..INFO_ROOT_ID + 32], &KEYBOARD_ID);
    assert_eq!(i32::from_le_bytes(b[INFO_FAMILY..INFO_FAMILY + 4].try_into().unwrap()), 3);
    assert_eq!(u32::from_le_bytes(b[INFO_SUPPORTED_INPUT..INFO_SUPPORTED_INPUT + 4].try_into().unwrap()), KIND_KEYBOARD);
    assert_eq!(u32::from_le_bytes(b[INFO_SUPPORTED_RUMBLE..INFO_SUPPORTED_RUMBLE + 4].try_into().unwrap()), 0);
    let m = device_info_bytes(KIND_MOUSE);
    assert_eq!(u16::from_le_bytes(m[14..16].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(m[INFO_SUPPORTED_INPUT..INFO_SUPPORTED_INPUT + 4].try_into().unwrap()), KIND_MOUSE);
    assert_eq!(keyboard_info().max_simultaneous_keys as usize, MAX_KEYS);
    assert_eq!(mouse_info().supported_buttons, BUTTONS_ALL);
}

#[test]
fn vtable_layouts_per_console_os_build() {
    assert_eq!(device_method_count(19041), 21);
    assert_eq!(device_method_count(22000), 21);
    assert_eq!(device_method_count(22621), 22);
    assert_eq!(igameinput_method_count(19041), 23);
    assert_eq!(igameinput_method_count(22000), 22);
    assert_eq!(igameinput_method_count(22621), 22);
    assert_eq!(PATCHED_SLOTS, [4, 5, 6, 7, 8, 9, 12, 13, 16, 17]);
}

#[test]
fn rumble_and_force_feedback_are_stored_with_console_results() {
    let _g = setup();
    let p = RumbleParams { low_frequency: 1.5, high_frequency: -1.0, left_trigger: f32::NAN, right_trigger: 0.25 };
    assert_eq!(p.clamped(), RumbleParams { low_frequency: 1.0, high_frequency: 0.0, left_trigger: 0.0, right_trigger: 0.25 });
    let dev = 0x1234usize as *mut c_void;
    unsafe {
        dev_set_rumble_state(dev, &p);
        dev_set_rumble_state(dev, &p);
        let f = feedback_state(0x1234).unwrap();
        assert_eq!(f.rumble, p.clamped());
        assert_eq!(f.rumble_calls, 2);
        assert_eq!(dev_set_haptic_motor_state(dev, 0, core::ptr::null()), E_BOUNDS);
        let mut eff = 1usize as *mut c_void;
        assert_eq!(dev_create_force_feedback_effect(dev, 0, core::ptr::null(), &mut eff), E_BOUNDS);
        assert!(eff.is_null());
        assert!(!dev_is_force_feedback_motor_powered_on(dev, 0));
        dev_set_force_feedback_motor_gain(dev, 1, 0.5);
        dev_set_force_feedback_motor_gain(dev, 9, 0.5);
        dev_set_input_synchronization_state(dev, 1);
        dev_send_input_synchronization_hint(dev);
        let f = feedback_state(0x1234).unwrap();
        assert_eq!(f.motor_gain, [1.0, 0.5, 1.0, 1.0]);
        assert!(f.sync_enabled);
        assert_eq!(f.sync_hints, 1);
        assert_eq!(f.haptic_calls, 1);
        let mut vt = [0usize; 22];
        install_pad_feedback(vt.as_mut_ptr());
        assert_eq!(vt[10], dev_set_rumble_state as *const () as usize);
        assert_eq!(vt[9], dev_set_haptic_motor_state as *const () as usize);
        assert_eq!(vt[6], dev_create_force_feedback_effect as *const () as usize);
        assert_eq!(vt[5], 0);
        assert_eq!(vt[14], 0);
    }
    assert!(feedback_state(0x9999).is_none());
}

#[test]
fn registration_filter_and_tokens() {
    let r = DeviceRegistration { token: 5, ctx: 0, cb: 1, kind: KIND_KEYBOARD, filter: STATUS_CONNECTED };
    assert!(registration_wants(&r, KIND_KEYBOARD, STATUS_CONNECTED | STATUS_INPUT_ENABLED, 0));
    assert!(registration_wants(&r, KIND_KEYBOARD, 0, STATUS_CONNECTED));
    assert!(!registration_wants(&r, KIND_MOUSE, STATUS_CONNECTED, 0));
    assert!(!registration_wants(&r, KIND_KEYBOARD, STATUS_CONNECTED | STATUS_INPUT_ENABLED, STATUS_CONNECTED));
    assert!(!registration_wants(&DeviceRegistration { cb: 0, ..r }, KIND_KEYBOARD, STATUS_CONNECTED, 0));
    let mut l = Vec::new();
    regs_add(&mut l, r);
    regs_add(&mut l, DeviceRegistration { token: 6, ..r });
    regs_add(&mut l, DeviceRegistration { token: 5, ctx: 9, ..r });
    assert_eq!(l.len(), 2);
    assert_eq!(l[1].ctx, 9);
    assert!(regs_remove(&mut l, 6));
    assert!(!regs_remove(&mut l, 6));
    assert!(!regs_remove(&mut l, 0));
    let t = synthetic_token();
    assert!(is_synthetic_token(t));
    assert_ne!(t, synthetic_token());
    assert!(!is_synthetic_token(0x1_0000_0001));
}

static CB_CALLS: AtomicU32 = AtomicU32::new(0);
static CB_LAST: Mutex<Vec<(usize, u32, u32)>> = Mutex::new(Vec::new());

unsafe extern "system" fn device_cb(_token: u64, ctx: *mut c_void, device: *mut c_void, _ts: u64, cur: u32, prev: u32) {
    CB_CALLS.fetch_add(1, Ordering::SeqCst);
    assert_eq!(ctx as usize, 0xC0);
    CB_LAST.lock().unwrap().push((device as usize, cur, prev));
}

#[test]
fn virtual_keyboard_and_mouse_serve_console_style_readings() {
    let _g = setup();
    unsafe {
        let kb = keyboard_device();
        let ms = mouse_device();
        assert!(is_ext_device(kb) && is_ext_device(ms));
        assert_eq!(classify(kb), Target::VirtualKeyboard);
        assert_eq!(classify(core::ptr::null_mut()), Target::AnyDevice);
        let mut out = core::ptr::null_mut();
        set_connected(KIND_KBM, false);
        assert_eq!(on_get_current_reading(KIND_KEYBOARD, kb, &mut out), Some(DEVICE_DISCONNECTED));
        assert!(out.is_null());
        assert_eq!(on_get_current_reading(KIND_KEYBOARD, core::ptr::null_mut(), &mut out), None);
        assert_eq!(on_get_current_reading(KIND_GAMEPAD, core::ptr::null_mut(), &mut out), None);
        assert_eq!(call0::<u32>(kb, 4), 0);

        let mut tok = 0x77u64;
        assert_eq!(device_callback_pre(core::ptr::null_mut(), 0x40030, STATUS_CONNECTED), None);
        device_callback_post(S_OK, core::ptr::null_mut(), 0x40030, STATUS_CONNECTED, 2, 0xC0 as *mut c_void, device_cb as *const () as usize, &mut tok);
        assert_eq!(CB_CALLS.load(Ordering::SeqCst), 0);
        assert!(registrations().iter().any(|r| r.token == 0x77 && r.kind == KIND_KBM));
        set_connected(KIND_KBM, true);
        assert_eq!(CB_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(CB_LAST.lock().unwrap()[0], (kb as usize, STATUS_CONNECTED | STATUS_INPUT_ENABLED, 0));
        let mut tok2 = 0x78u64;
        device_callback_post(S_OK, core::ptr::null_mut(), KIND_MOUSE, STATUS_CONNECTED, 2, 0xC0 as *mut c_void, device_cb as *const () as usize, &mut tok2);
        assert_eq!(CB_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(CB_LAST.lock().unwrap()[2].0, ms as usize);
        device_callback_post(S_OK, core::ptr::null_mut(), KIND_GAMEPAD, STATUS_CONNECTED, 2, 0xC0 as *mut c_void, device_cb as *const () as usize, &mut tok2);
        assert_eq!(CB_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(device_callback_pre(kb, KIND_KEYBOARD, STATUS_CONNECTED), Some(S_OK));
        assert_eq!(call0::<u32>(kb, 4), STATUS_CONNECTED | STATUS_INPUT_ENABLED);

        let info = call0::<*const u8>(kb, 3);
        assert_eq!(core::slice::from_raw_parts(info.add(INFO_DEVICE_ID), 32), &KEYBOARD_ID);
        let ki = *(info.add(INFO_KEYBOARD_INFO) as *const *const KeyboardInfo);
        assert_eq!((*ki).layout, 0x0409);
        assert_eq!(*(info.add(INFO_MOUSE_INFO) as *const usize), 0);
        let name = *(info.add(INFO_DISPLAY_NAME) as *const *const GiString);
        assert_eq!(std::slice::from_raw_parts((*name).data, (*name).size_in_bytes as usize), b"TomBox Virtual Keyboard");
        let minfo = call0::<*const u8>(ms, 3);
        let mi = *(minfo.add(INFO_MOUSE_INFO) as *const *const MouseInfo);
        assert!((*mi).has_wheel_y);
        let mut batt = [0x55u8; 20];
        let fb: unsafe extern "system" fn(*mut c_void, *mut u8) = core::mem::transmute(slot(kb, 5));
        fb(kb, batt.as_mut_ptr());
        assert_eq!(&batt[0..16], &[0u8; 16]);
        assert_eq!(i32::from_le_bytes(batt[16..20].try_into().unwrap()), -1);

        update_state(|s| s.release());
        key_event(parse_key("w").unwrap(), true);
        key_event(parse_key("lshift").unwrap(), true);
        assert_eq!(on_get_current_reading(0x40030, kb, &mut out), Some(S_OK));
        let r1 = out;
        assert!(is_ext_reading(r1));
        assert_eq!(call0::<u32>(r1, 3), KIND_KEYBOARD);
        assert_eq!(call0::<u32>(r1, 14), 2);
        let mut ks = [KeyState::default(); 4];
        let gks: unsafe extern "system" fn(*mut c_void, u32, *mut KeyState) -> u32 = core::mem::transmute(slot(r1, 15));
        assert_eq!(gks(r1, 4, ks.as_mut_ptr()), 2);
        assert_eq!((ks[0].virtual_key, ks[0].code_point, ks[0].scan_code), (0x57, b'W' as u32, 0x11));
        assert_eq!(gks(r1, 1, ks.as_mut_ptr()), 1);
        let mut m = MouseState { buttons: 9, ..Default::default() };
        let gms: unsafe extern "system" fn(*mut c_void, *mut MouseState) -> bool = core::mem::transmute(slot(r1, 16));
        assert!(!gms(r1, &mut m));
        assert_eq!(m, MouseState::default());
        let mut pad = [0xAAu8; 32];
        let gpad: unsafe extern "system" fn(*mut c_void, *mut u8) -> bool = core::mem::transmute(slot(r1, 22));
        assert!(!gpad(r1, pad.as_mut_ptr()));
        assert_eq!(&pad[..28], &[0u8; 28]);
        assert_eq!(pad[28], 0xAA);
        let gseq: unsafe extern "system" fn(*mut c_void, u32) -> u64 = core::mem::transmute(slot(r1, 4));
        let s1 = gseq(r1, KIND_KEYBOARD);
        assert!(s1 > 0);
        assert_eq!(gseq(r1, KIND_MOUSE), 0);
        let mut dev = core::ptr::null_mut();
        let gdev: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) = core::mem::transmute(slot(r1, 6));
        gdev(r1, &mut dev);
        assert_eq!(dev, kb);

        let mut same = core::ptr::null_mut();
        assert_eq!(on_get_current_reading(KIND_KEYBOARD, kb, &mut same), Some(S_OK));
        assert_eq!(same, r1);
        release(same);

        let mut nx = core::ptr::null_mut();
        assert_eq!(on_get_neighbour_reading(true, r1, KIND_KEYBOARD, kb, &mut nx), Some(READING_NOT_FOUND));
        key_event(parse_key("w").unwrap(), false);
        assert_eq!(on_get_neighbour_reading(true, r1, KIND_KEYBOARD, kb, &mut nx), Some(S_OK));
        assert_eq!(call0::<u32>(nx, 14), 1);
        assert!(gseq(nx, KIND_KEYBOARD) > s1);
        let mut pv = core::ptr::null_mut();
        assert_eq!(on_get_neighbour_reading(false, nx, KIND_KEYBOARD, core::ptr::null_mut(), &mut pv), Some(S_OK));
        assert_eq!(gseq(pv, KIND_KEYBOARD), s1);
        assert_eq!(on_get_neighbour_reading(true, nx, KIND_MOUSE, core::ptr::null_mut(), &mut pv), Some(READING_NOT_FOUND));
        assert!(pv.is_null());
        release(nx);
        release(r1);

        assert_eq!(on_get_current_reading(KIND_MOUSE, kb, &mut out), Some(READING_NOT_FOUND));
        assert!(out.is_null());
        mouse_move(15, -2);
        mouse_button(BUTTON_LEFT, true);
        mouse_wheel(0, 120);
        assert_eq!(on_get_current_reading(KIND_MOUSE, core::ptr::null_mut(), &mut out), Some(S_OK));
        assert_eq!(call0::<u32>(out, 3), KIND_MOUSE);
        let gms2: unsafe extern "system" fn(*mut c_void, *mut MouseState) -> bool = core::mem::transmute(slot(out, 16));
        let mut m2 = MouseState::default();
        assert!(gms2(out, &mut m2));
        let base = state().mouse;
        assert_eq!(m2, base);
        assert_eq!(m2.buttons, BUTTON_LEFT);
        assert_eq!(call0::<u32>(out, 14), 0);
        release(out);
        mouse_button(BUTTON_LEFT, false);

        let mut tmp = core::ptr::null_mut();
        assert_eq!(on_get_temporal_reading(kb, &mut tmp), Some(E_NOTIMPL));
        let mut t = 5u64;
        assert_eq!(on_register_reading_callback(kb, KIND_KEYBOARD, &mut t), Some(E_NOTIMPL));
        assert_eq!(t, 0);
        assert_eq!(on_register_reading_callback(core::ptr::null_mut(), KIND_KEYBOARD, &mut t), None);
        let mut found = core::ptr::null_mut();
        assert_eq!(on_find_device_from_id(&MOUSE_ID, &mut found), Some(S_OK));
        assert_eq!(found, ms);
        assert_eq!(on_find_device_from_id(&[0u8; 32], &mut found), None);
        assert_eq!(on_find_device_from_object(kb, &mut found), Some(S_OK));
        assert_eq!(found, kb);

        let before = CB_CALLS.load(Ordering::SeqCst);
        set_connected(KIND_KEYBOARD, false);
        let after = CB_CALLS.load(Ordering::SeqCst);
        assert_eq!(after, before + 1);
        assert_eq!(*CB_LAST.lock().unwrap().last().unwrap(), (kb as usize, 0, STATUS_CONNECTED | STATUS_INPUT_ENABLED));
        on_unregister(0x77);
        on_unregister(0x78);
        assert!(registrations().iter().all(|r| r.token != 0x77 && r.token != 0x78));
        set_connected(KIND_MOUSE, false);
        assert_eq!(CB_CALLS.load(Ordering::SeqCst), after);

        let recs = kind_records();
        assert!(recs.iter().any(|r| r.api == Api::RegisterDeviceCallback && r.kind == 0x40030 && r.filter == STATUS_CONNECTED && r.target == Target::AnyDevice));
        assert!(recs.iter().any(|r| r.api == Api::GetCurrentReading && r.kind == 0x40030 && r.target == Target::VirtualKeyboard));
        assert!(requested_kinds() & 0x40030 == 0x40030);
        assert!(kind_report().contains("RegisterDeviceCallback kind=0x40030"));
    }
}

#[test]
fn command_application_updates_shared_state() {
    let _g = setup();
    update_state(|s| s.release());
    let before = state().mouse;
    apply_command(&parse_kbm_command("move 3 4").unwrap().unwrap());
    apply_command(&parse_kbm_command("keydown a+b").unwrap().unwrap());
    apply_command(&parse_kbm_command("mousedown right").unwrap().unwrap());
    let s = state();
    assert_eq!(s.mouse.position_x, before.position_x + 3);
    assert_eq!(s.mouse.position_y, before.position_y + 4);
    assert_eq!(s.keys.len(), 2);
    assert_eq!(s.mouse.buttons, BUTTON_RIGHT);
    apply_command(&KbmCmd::Release);
    let s = state();
    assert!(s.keys.is_empty());
    assert_eq!(s.mouse.buttons, 0);
}

const DUMPS: &str = "D:\\xbox-pc-port\\games";

fn titles() -> Vec<(&'static str, String, String)> {
    let t = "Terraria\\extracted\\(Terraria) 505GAMESS.P.A.84240166-5fcf-4ee1-b892-f5ff6a6094e1_4.4.14.1081_neutral__tefn33qh9azfc_x";
    let m = "Trackmania\\extracted\\Ubisoft.TrackmaniaNext_1.6.2.0_neutral__ngz4m417e0mpw";
    vec![
        ("Fortnite", "Fortnite\\extracted".into(), "Fortnite\\extracted\\Mount\\FortniteGame\\Binaries\\XSX\\FortniteClient-XSX-Shipping.exe".into()),
        ("XDefiant", "XDefiant\\extracted".into(), "XDefiant\\extracted\\Mount\\Rogue_scarlett_rtx.exe".into()),
        ("Terraria", t.into(), format!("{t}\\Mount\\Terraria.exe")),
        ("Trackmania", m.into(), format!("{m}\\Mount\\GbxGame.exe")),
    ]
}

#[test]
fn console_gameinput_ordinal_1_is_gameinputcreate_and_titles_import_it() {
    let mut checked = 0;
    for (name, root, exe) in titles() {
        let dll = format!("{DUMPS}\\{root}\\EmbeddedXvd\\Windows\\System32\\GameInput.dll");
        let Ok(bytes) = std::fs::read(&dll) else { continue };
        let (export, rva) = pe_export_by_ordinal(&bytes, 1).unwrap_or_else(|| panic!("{name}: no ordinal 1"));
        assert_eq!(export, "GameInputCreate", "{name}");
        assert!(rva != 0);
        assert!(pe_export_by_ordinal(&bytes, 2).is_none(), "{name}: console GameInput.dll exports only ordinal 1");
        if let Ok(exe_bytes) = std::fs::read(format!("{DUMPS}\\{exe}")) {
            let ords = pe_import_ordinals(&exe_bytes, "GameInput.dll").unwrap_or_else(|| panic!("{name}: no GameInput.dll import"));
            assert_eq!(ords, vec![1u16], "{name}");
        }
        checked += 1;
    }
    eprintln!("console GameInput.dll ordinal evidence checked for {checked} title dump(s)");
}

static INNER_CALLS: Mutex<Vec<(usize, u32)>> = Mutex::new(Vec::new());

unsafe extern "system" fn inner_ts(_this: *mut c_void) -> u64 {
    4242
}
unsafe extern "system" fn inner_current(_this: *mut c_void, kind: u32, _device: *mut c_void, out: *mut *mut c_void) -> i32 {
    INNER_CALLS.lock().unwrap().push((4, kind));
    *out = core::ptr::null_mut();
    READING_NOT_FOUND
}
unsafe extern "system" fn inner_device_cb(_this: *mut c_void, _device: *mut c_void, kind: u32, _filter: u32, _enumk: u32, _ctx: *mut c_void, _cb: usize, token: *mut u64) -> i32 {
    INNER_CALLS.lock().unwrap().push((9, kind));
    *token = 0x5150;
    S_OK
}
unsafe extern "system" fn inner_unregister(_this: *mut c_void, token: u64, _timeout: u64) -> u32 {
    INNER_CALLS.lock().unwrap().push((13, token as u32));
    1
}
unsafe extern "system" fn inner_other(_this: *mut c_void) -> i32 {
    INNER_CALLS.lock().unwrap().push((99, 0));
    E_NOTIMPL
}

#[repr(C)]
struct FakeGi {
    vtbl: *mut usize,
}

#[test]
fn attach_layers_over_an_existing_igameinput_vtable() {
    let _g = setup();
    let mut dir = std::env::temp_dir();
    dir.push("tombox_gameinput_ext_test_kbm");
    std::env::set_var("TOMBOX_KBM_DIR", &dir);
    let vt: &'static mut [usize] = Box::leak(vec![inner_other as *const () as usize; 23].into_boxed_slice());
    vt[3] = inner_ts as *const () as usize;
    vt[4] = inner_current as *const () as usize;
    vt[9] = inner_device_cb as *const () as usize;
    vt[13] = inner_unregister as *const () as usize;
    let untouched = vt[22];
    let gi = Box::leak(Box::new(FakeGi { vtbl: vt.as_mut_ptr() })) as *mut FakeGi as *mut c_void;
    unsafe {
        assert!(attach(gi));
        assert!(attach(gi));
        let v = *(gi as *const *const usize);
        assert_ne!(*v.add(4), inner_current as *const () as usize);
        assert_eq!(*v.add(3), inner_ts as *const () as usize);
        assert_eq!(*v.add(22), untouched);
        assert_eq!(now_us(), 4242);
        let cur: unsafe extern "system" fn(*mut c_void, u32, *mut c_void, *mut *mut c_void) -> i32 = core::mem::transmute(*v.add(4));
        let mut out = core::ptr::null_mut();
        assert_eq!(cur(gi, KIND_GAMEPAD, core::ptr::null_mut(), &mut out), READING_NOT_FOUND);
        assert_eq!(INNER_CALLS.lock().unwrap().last(), Some(&(4, KIND_GAMEPAD)));
        set_connected(KIND_KEYBOARD, true);
        let n = INNER_CALLS.lock().unwrap().len();
        assert_eq!(cur(gi, KIND_KEYBOARD, keyboard_device(), &mut out), S_OK);
        assert_eq!(INNER_CALLS.lock().unwrap().len(), n);
        assert!(is_ext_reading(out));
        let r: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*(*(out as *const *const usize)).add(2));
        r(out);
        let dcb: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, u32, *mut c_void, usize, *mut u64) -> i32 = core::mem::transmute(*v.add(9));
        let mut tok = 0u64;
        assert_eq!(dcb(gi, core::ptr::null_mut(), 0x40030, STATUS_CONNECTED, 0, 0xC0 as *mut c_void, device_cb as *const () as usize, &mut tok), S_OK);
        assert_eq!(tok, 0x5150);
        assert!(registrations().iter().any(|x| x.token == 0x5150));
        let mut tok2 = 0u64;
        assert_eq!(dcb(gi, keyboard_device(), KIND_KEYBOARD, STATUS_CONNECTED, 0, 0xC0 as *mut c_void, device_cb as *const () as usize, &mut tok2), S_OK);
        assert!(is_synthetic_token(tok2));
        let calls_before = INNER_CALLS.lock().unwrap().len();
        let unreg: unsafe extern "system" fn(*mut c_void, u64, u64) -> u32 = core::mem::transmute(*v.add(13));
        assert_eq!(unreg(gi, tok2, 0), 1);
        assert_eq!(INNER_CALLS.lock().unwrap().len(), calls_before);
        assert_eq!(unreg(gi, 0x5150, 0), 1);
        assert_eq!(INNER_CALLS.lock().unwrap().last(), Some(&(13, 0x5150)));
        assert!(registrations().iter().all(|x| x.token != 0x5150 && x.token != tok2));
        set_connected(KIND_KEYBOARD, false);
        let other: &'static mut [usize] = Box::leak(vec![inner_other as *const () as usize; 23].into_boxed_slice());
        let gi2 = Box::leak(Box::new(FakeGi { vtbl: other.as_mut_ptr() })) as *mut FakeGi as *mut c_void;
        assert!(!attach(gi2));
        assert_eq!(other[4], inner_other as *const () as usize);
    }
}
