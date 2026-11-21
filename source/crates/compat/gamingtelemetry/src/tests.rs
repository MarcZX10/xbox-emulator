use super::*;
use crate::cv::Cv;
use crate::sys::Guid;
use crate::tlg::*;
use std::mem::{size_of, MaybeUninit};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard};

const EXPORTS: &str = include_str!("../exports.txt");
static LOCK: Mutex<()> = Mutex::new(());

const NOT_REG: i32 = 0x80EB0011u32 as i32;

fn fx() -> MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        UnregisterGamingTelemetryProvider();
        ResetExtendedData();
    }
    g
}

fn cfg(use_group: u32, group: [u8; 16], b: u32) -> [u8; 0x24] {
    let mut c = [0u8; 0x24];
    c[4..8].copy_from_slice(&b.to_le_bytes());
    c[0x10..0x14].copy_from_slice(&use_group.to_le_bytes());
    c[0x14..0x24].copy_from_slice(&group);
    c
}

fn reg() {
    let c = cfg(0, [0; 16], 0);
    let n = wz("TestComp");
    assert_eq!(unsafe { RegisterGamingTelemetryProvider(n.as_ptr(), null(), c.as_ptr() as *const c_void) }, S_OK);
}

fn enable_p1() {
    let st = state();
    let p = &st.dynp.as_ref().unwrap()[0];
    unsafe { enable_cb(null(), 1, 4, u64::MAX, 0, null(), &**p as *const Prov as *mut c_void) };
}

unsafe fn mk<T>(f: impl FnOnce(*mut T) -> *mut T) -> T {
    let mut v = MaybeUninit::<T>::uninit();
    let r = f(v.as_mut_ptr());
    assert_eq!(r, v.as_mut_ptr());
    v.assume_init()
}

fn cv_string() -> String {
    let mut b = [0u8; 0x81];
    assert_eq!(unsafe { GetDeviceInteractionVector(b.as_mut_ptr()) }, S_OK);
    unsafe { String::from_utf8(cstr(b.as_ptr()).to_vec()).unwrap() }
}

#[test]
fn export_table_matches_reference_dll() {
    xone_exports::check_crate(env!("CARGO_MANIFEST_DIR"), "gamingtelemetry.dll", EXPORTS);
}

#[test]
fn built_dll_has_reference_ordinals() {
    xone_exports::check_built(env!("CARGO_MANIFEST_DIR"), "gamingtelemetry.dll", EXPORTS);
}

#[test]
fn abi_layouts_match_msvc() {
    assert_eq!(size_of::<Prim>(), 0x18);
    assert_eq!(size_of::<Coll>(), 0x20);
    assert_eq!(size_of::<BVar>(), 0x28);
    assert_eq!(size_of::<Value>(), 0x40);
    assert_eq!(size_of::<MapValue>(), 0x38);
    assert_eq!(size_of::<PrimMapValue>(), 0x28);
    assert_eq!(size_of::<Ctrl>(), 0x18);
    assert_eq!(size_of::<Node>(), 0x20);
}

#[test]
fn string_builder_owns_copy_and_null_is_static_empty() {
    unsafe {
        let n = wz("k");
        let s = wz("hello");
        let v = mk(|o| GamingTelemetryString(o, n.as_ptr(), s.as_ptr()));
        assert_eq!(v.aidx, 0);
        assert_eq!(v.name.wtext(), w("k").as_slice());
        let p = v.bvar().prim();
        assert_eq!(p.idx, 3);
        assert_ne!(p.d[0], s.as_ptr() as u64);
        assert_eq!(wstr(p.d[0] as *const u16), w("hello").as_slice());
        assert_eq!((*p.as_sp().ctrl).uses.load(std::sync::atomic::Ordering::SeqCst), 1);
        value_drop(&v);
        let e = mk(|o| GamingTelemetryString(o, n.as_ptr(), null()));
        assert_eq!(e.bvar().prim().idx, 1);
        assert_eq!(e.bvar().prim().d[0], EMPTY_W.as_ptr() as u64);
        value_drop(&e);
    }
}

#[test]
fn value_builder_normalises_narrow_strings() {
    unsafe {
        let n = wz("v");
        let ascii = b"abc\0";
        let v = mk(|o| GamingTelemetryValue(o, n.as_ptr(), &mut BVar::from_prim(Prim::nref(ascii.as_ptr()))));
        assert_eq!(v.bvar().prim().idx, 2);
        assert_eq!(cstr(v.bvar().prim().d[0] as *const u8), b"abc");
        value_drop(&v);
        let utf = "h\u{e9}\0".as_bytes();
        let u = mk(|o| GamingTelemetryValue(o, n.as_ptr(), &mut BVar::from_prim(Prim::nref(utf.as_ptr()))));
        assert_eq!(u.bvar().prim().idx, 3);
        assert_eq!(wstr(u.bvar().prim().d[0] as *const u16), w("h\u{e9}").as_slice());
        value_drop(&u);
        let num = mk(|o| GamingTelemetryValue(o, n.as_ptr(), &mut BVar::from_prim(Prim::u32v(9))));
        assert_eq!((num.bvar().prim().idx, num.bvar().prim().d[0]), (9, 9));
        value_drop(&num);
    }
}

#[test]
fn byvalue_builders_consume_their_argument() {
    unsafe {
        let n = wz("k");
        let s = Sp::wide(&w("owned"));
        let uses = |s: &Sp| (*s.ctrl).uses.load(std::sync::atomic::Ordering::SeqCst);
        s.clone_ref();
        s.clone_ref();
        s.clone_ref();
        let mut b = BVar::from_prim(Prim::sp(3, s));
        let v = mk(|o| GamingTelemetryValue(o, n.as_ptr(), &mut b));
        assert_eq!(uses(&s), 4);
        assert_eq!(v.bvar().prim().d[0], s.ptr as u64);
        value_drop(&v);
        assert_eq!(uses(&s), 3);
        let mut b2 = BVar::from_prim(Prim::sp(3, s));
        let mv = mk(|o| GamingTelemetryMapValue(o, n.as_ptr(), &mut b2));
        assert_eq!(uses(&s), 3);
        mv.name.release();
        bvar_drop(&mv.b);
        assert_eq!(uses(&s), 2);
        let mut p = Prim::sp(3, s);
        let pm = mk(|o| GamingTelemetryPrimitiveMapValue(o, n.as_ptr(), &mut p));
        assert_eq!(uses(&s), 2);
        pm.name.release();
        pm.p.drop_ref();
        assert_eq!(uses(&s), 1);
        s.release();
        let mut c = BVar::from_coll(Coll::new(1, raw_vec(&[Prim::u32v(1), Prim::u32v(2)])));
        let cv = mk(|o| GamingTelemetryValue(o, n.as_ptr(), &mut c));
        assert_eq!(vec_items::<Prim>(&cv.bvar().coll().d).len(), 2);
        value_drop(&cv);
    }
}

#[test]
fn primitive_vector_is_exact_capacity_vector() {
    unsafe {
        let n = wz("vec");
        let items: Vec<Prim> = (0..200).map(Prim::u32v).collect();
        let span = Span { first: items.as_ptr(), last: items.as_ptr().add(items.len()) };
        let v = mk(|o| GamingTelemetryPrimitiveVector(o, n.as_ptr(), &span));
        let c = v.bvar().coll();
        assert_eq!(c.idx, 1);
        assert_eq!(c.d[1] - c.d[0], 200 * 0x18);
        assert_eq!(c.d[2], c.d[1]);
        assert_eq!(c.d[0] % 32, 0);
        assert_eq!(vec_items::<Prim>(&c.d)[199].d[0], 199);
        value_drop(&v);
    }
}

#[test]
fn primitive_map_value_and_map_assign_duplicates() {
    unsafe {
        let k = wz("a");
        let k2 = wz("b");
        let e1 = mk(|o| GamingTelemetryPrimitiveMapValue(o, k.as_ptr(), &mut Prim::u32v(1)));
        let e2 = mk(|o| GamingTelemetryPrimitiveMapValue(o, k2.as_ptr(), &mut Prim::u32v(2)));
        let e3 = mk(|o| GamingTelemetryPrimitiveMapValue(o, k.as_ptr(), &mut Prim::u32v(3)));
        assert_eq!(e1.name.wtext(), w("a").as_slice());
        let es = [e1, e2, e3];
        let span = Span { first: es.as_ptr(), last: es.as_ptr().add(3) };
        let n = wz("pm");
        let v = mk(|o| GamingTelemetryPrimitiveMap(o, n.as_ptr(), &span));
        let m = v.bvar().coll().as_map();
        assert_eq!(m.size, 2);
        let vals: Vec<(Vec<u16>, u64)> = m.nodes().iter().map(|n| ((*nkey(*n)).wtext().to_vec(), (*nval::<Prim>(*n)).d[0])).collect();
        assert_eq!(vals, vec![(w("a"), 3), (w("b"), 2)]);
        value_drop(&v);
        for e in &es {
            e.name.release();
        }
    }
}

#[test]
fn map_value_map_basedata_basetype_keep_first() {
    unsafe {
        let k = wz("x");
        let s = wz("one");
        let mv1 = mk(|o| GamingTelemetryMapValue(o, k.as_ptr(), &mut BVar::from_prim(Prim::wref(s.as_ptr()))));
        assert_eq!(mv1.b.prim().idx, 3);
        let mv2 = mk(|o| GamingTelemetryMapValue(o, k.as_ptr(), &mut BVar::from_prim(Prim::u32v(5))));
        let es = [mv1, mv2];
        let span = Span { first: es.as_ptr(), last: es.as_ptr().add(2) };
        let n = wz("m");
        let v = mk(|o| GamingTelemetryMap(o, n.as_ptr(), &span));
        assert_eq!(v.aidx, 1);
        assert_eq!(v.map().size, 1);
        assert_eq!((*nval::<BVar>(v.map().nodes()[0])).prim().idx, 3);
        value_drop(&v);
        let bd = mk(|o| GamingTelemetryBaseData(o, &span));
        assert_eq!(bd.name.wtext(), w("baseData").as_slice());
        value_drop(&bd);
        let t = wz("PageViewData");
        let bt = mk(|o| GamingTelemetryBaseType(o, t.as_ptr()));
        assert_eq!(bt.name.wtext(), w("baseType").as_slice());
        assert_eq!(wstr(bt.bvar().prim().d[0] as *const u16), w("PageViewData").as_slice());
        value_drop(&bt);
        for e in &es {
            e.name.release();
            bvar_drop(&e.b);
        }
    }
}

#[test]
fn encoder_bytes_match_tracelogging_layout() {
    unsafe {
        let m = prim_map_of(vec![("x", Prim::i32v(1))]);
        let vals = Vals(vec![v_prim("a", Prim::u32v(7)), v_coll("m", Coll::map(m)), v_tag(4)]);
        let e = build_event(&w("E"), 0, 5, &vals.0).unwrap();
        let mut want = vec![0u8, 0, 0, b'E', 0];
        want.extend_from_slice(b"PartA_PrivTags\0\x0aa\0\x08m\0\x98\x01x\0\x07");
        want[0] = want.len() as u8;
        assert_eq!(e.meta, want);
        let mut p = 5u64.to_le_bytes().to_vec();
        p.extend_from_slice(&7u32.to_le_bytes());
        p.extend_from_slice(&1i32.to_le_bytes());
        assert_eq!(e.payload, p);
        let mut t = Vec::new();
        tags_bytes(&mut t, 0x200000);
        assert_eq!(t, vec![0x01]);
        let mut t2 = Vec::new();
        tags_bytes(&mut t2, 0x200001);
        assert_eq!(t2, vec![0x81, 0x80, 0x80, 0x01]);
        let mixed = Coll::new(1, raw_vec(&[Prim::u32v(1), Prim::u64v(2)]));
        let bad = Vals(vec![v_coll("v", mixed)]);
        assert_eq!(build_event(&w("E"), 0, 0, &bad.0).err(), Some(0x80EB0014u32 as i32));
        let m1 = prim_map_of(vec![("a", Prim::u32v(1))]);
        let m2 = prim_map_of(vec![("a", Prim::u32v(1)), ("b", Prim::u32v(2))]);
        let arr = Vals(vec![v_coll("s", Coll::new(2, raw_vec(&[m1, m2])))]);
        assert_eq!(build_event(&w("E"), 0, 0, &arr.0).err(), Some(0x80EB0016u32 as i32));
    }
}

#[test]
fn static_metadata_sizes_match_console_blobs() {
    assert_eq!(meta_error_short()[0], 0x81);
    assert_eq!(meta_provider_unregistered()[0], 0x45);
    assert_eq!(meta_provider_registered()[0], 0x39);
    assert_eq!(meta_active_user()[0], 0x4d);
    assert_eq!(meta_error_full()[0], 0x85);
    assert_eq!(meta_session_started()[0], 0x5c);
    assert_eq!(meta_session_stopped()[0], 0x89);
}

#[test]
fn provider_name_hash_matches_etw_name_hashing() {
    assert_eq!(name_guid(&w("MyCompany.MyComponent")), Guid::new(0xce5fa4ea, 0xab00, 0x5402, [0x8b, 0x76, 0x9f, 0x76, 0xac, 0x85, 0x8f, 0xb5]));
    assert_eq!(name_guid(&w("mycompany.mycomponent")), name_guid(&w("MyCompany.MyComponent")));
}

#[test]
fn correlation_vector_rules() {
    let cv = Cv::new_v2(&[0u8; 16]);
    assert_eq!(cv.format(cv.packed).unwrap(), b"AAAAAAAAAAAAAAAAAAAAAA.0");
    let mut c = Cv::extend(b"AAAAAAAAAAAAAAAAAAAAAA.5").unwrap();
    assert_eq!(c.format(c.packed).unwrap(), b"AAAAAAAAAAAAAAAAAAAAAA.5.0");
    for _ in 0..10 {
        c.increment();
    }
    assert_eq!(c.format(c.packed).unwrap(), b"AAAAAAAAAAAAAAAAAAAAAA.5.10");
    assert!(Cv::extend(b"short").is_none());
    let long = [b"AAAAAAAAAAAAAAAAAAAAAA.".to_vec(), vec![b'1'; 0x81 - 2 - 23]].concat();
    let o = Cv::extend(&long).unwrap();
    assert_eq!(o.format(o.packed).unwrap(), [long.clone(), b"!".to_vec()].concat());
    let v1 = Cv::extend(b"AAAAAAAAAAAAAAAA.1").unwrap();
    assert_eq!(v1.max, 0x41);
}

#[test]
fn register_validation_and_idempotence() {
    let _g = fx();
    unsafe {
        let c = cfg(0, [0; 16], 0);
        let cp = c.as_ptr() as *const c_void;
        assert_eq!(RegisterGamingTelemetryProvider(null(), null(), cp), E_INVALIDARG);
        assert_eq!(RegisterGamingTelemetryProvider(wz("").as_ptr(), null(), cp), E_INVALIDARG);
        assert_eq!(RegisterGamingTelemetryProvider(wz(&"a".repeat(53)).as_ptr(), null(), cp), E_INVALIDARG);
        assert_eq!(RegisterGamingTelemetryProvider(wz(&"a".repeat(52)).as_ptr(), null(), cp), 0x8007007Au32 as i32);
        let z = cfg(1, [0; 16], 0);
        assert_eq!(RegisterGamingTelemetryProvider(wz("G").as_ptr(), null(), z.as_ptr() as *const c_void), 0x80EB0018u32 as i32);
        reg();
        assert_eq!(state().provider, w("Microsoft.Gaming.TestComp"));
        assert_eq!(state().guid, name_guid(&w("Microsoft.Gaming.TestComp")));
        assert_eq!(RegisterGamingTelemetryProvider(wz("TestComp").as_ptr(), null(), cp), S_OK);
        assert_eq!(RegisterGamingTelemetryProvider(wz("Other").as_ptr(), null(), cp), 0x80EB0012u32 as i32);
        assert_eq!(state().keywords[0], 0x400000000000);
        assert_eq!(state().privacy[2], 0x1000000);
        UnregisterGamingTelemetryProvider();
        assert!(!state().registered);
        assert!(state().dynp.is_none());
    }
}

#[test]
fn privacy_keyword_tag_overrides() {
    let _g = fx();
    assert_eq!(OverridePrivacyFlagsForEvent(0, 1), NOT_REG);
    assert_eq!(OverrideTagsForEvent(0, 1), NOT_REG);
    assert_eq!(OverrideKeywordsForEvent(0, 0x400000000000), NOT_REG);
    assert_eq!(ResetPrivacyFlagsForEvent(0), NOT_REG);
    reg();
    assert_eq!(OverridePrivacyFlagsForEvent(8, 1), E_INVALIDARG);
    assert_eq!(OverridePrivacyFlagsForEvent(-1, 1), E_INVALIDARG);
    assert_eq!(OverrideKeywordsForEvent(1, 0x1), E_INVALIDARG);
    assert_eq!(OverrideKeywordsForEvent(1, 0x200000000001), S_OK);
    assert_eq!(state().keywords[1], 0x200000000001);
    assert_eq!(OverrideTagsForEvent(3, 0x10), S_OK);
    assert_eq!(state().tags[3], 0x10);
    assert_eq!(OverridePrivacyFlagsForEvent(4, 0x7), S_OK);
    assert_eq!(state().privacy[4], 7);
    assert_eq!(ResetPrivacyFlagsForEvent(4), S_OK);
    assert_eq!(state().privacy[4], 0x2000000);
    assert_eq!(ResetPrivacyFlagsForEvent(8), E_INVALIDARG);
}

#[test]
fn extended_data_slots_and_json_summary() {
    let _g = fx();
    unsafe {
        assert_eq!(SetExtendedDataString(null(), wz("x").as_ptr()), E_INVALIDARG);
        assert_eq!(SetExtendedDataString(wz("s").as_ptr(), null()), E_INVALIDARG);
        assert_eq!(SetExtendedDataString(wz("s").as_ptr(), wz("v").as_ptr()), S_OK);
        assert_eq!(SetExtendedDataInt(wz("i").as_ptr(), -2), S_OK);
        assert_eq!(SetExtendedDataUInt(wz("u").as_ptr(), 3), S_OK);
        assert_eq!(SetExtendedDataDouble(wz("d").as_ptr(), 1.5), S_OK);
        assert_eq!(SetExtendedDataBoolean(wz("b").as_ptr(), 1), S_OK);
        assert_eq!(SetExtendedDataInt(wz("s").as_ptr(), 7), S_OK);
        assert_eq!(state().slots[0].ty, 1);
        assert_eq!(state().count, 5);
        assert_eq!(SetExtendedDataString(wz("s").as_ptr(), wz("v").as_ptr()), S_OK);
        let j = String::from_utf16(&state().json().unwrap()).unwrap();
        assert_eq!(
            j,
            "{ \"s\": \"v\", \"i\": -2, \"u\": 3, \"d\": 1.500000, \"b\": true, \"sessionId\": \"00000000-0000-0000-0000-000000000000\", \"activeUserId\": \"\" }"
        );
        for i in 0..10 {
            assert_eq!(SetExtendedDataUInt(wz(&format!("n{i}")).as_ptr(), i), S_OK);
        }
        assert_eq!(SetExtendedDataUInt(wz("over").as_ptr(), 1), 0x8007000Eu32 as i32);
        assert_eq!(ClearExtendedDataValue(wz("nope").as_ptr()), E_INVALIDARG);
        assert_eq!(ClearExtendedDataValue(wz("n0").as_ptr()), S_OK);
        assert_eq!(state().count, 14);
        let keep = [wz("s"), wz("b")];
        let ptrs = [keep[0].as_ptr(), keep[1].as_ptr()];
        ClearExtendedDataValuesExcept(ptrs.as_ptr(), 2);
        assert_eq!(state().count, 2);
        assert!(state().find_slot(&w("b")).is_some());
        ResetExtendedData();
        assert_eq!(state().count, 0);
    }
}

#[test]
fn correlation_vector_exports() {
    let _g = fx();
    unsafe {
        assert_eq!(GetDeviceInteractionVector(null_mut()), E_INVALIDARG);
        assert_eq!(ClearGtlCorrelationVector(), NOT_REG);
        assert_eq!(SetGtlCorrelationVector(null()), E_INVALIDARG);
        reg();
        let s = cv_string();
        assert_eq!(s.len(), 24);
        assert!(s.ends_with(".0"));
        let vals: [Value; 0] = [];
        let span = Span { first: vals.as_ptr(), last: vals.as_ptr() };
        TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 4, 0x80000000000, &span);
        assert!(cv_string().ends_with(".1"));
        assert_eq!(SetGtlCorrelationVector(b"rel.1\0".as_ptr()), S_OK);
        assert_eq!(state().related_cv.as_deref(), Some(&b"rel.1\0"[..]));
        assert_eq!(ClearGtlCorrelationVector(), S_OK);
        assert!(state().related_cv.is_none());
    }
}

#[test]
fn active_user_set_and_clear() {
    let _g = fx();
    unsafe {
        assert_eq!(SetGtlActiveUserId(null()), E_INVALIDARG);
        assert_eq!(SetGtlActiveUserId(wz("u").as_ptr()), NOT_REG);
        assert_eq!(ClearGtlActiveUserId(), NOT_REG);
        reg();
        assert_eq!(SetGtlActiveUserId(wz("user1").as_ptr()), S_OK);
        assert_eq!(state().user_id, Some(wz("user1")));
        assert_eq!(ClearGtlActiveUserId(), S_OK);
        assert!(state().user_id.is_none());
    }
}

#[test]
fn session_start_stop_and_error_summary() {
    let _g = fx();
    unsafe {
        assert_eq!(StartGamingTelemetrySession(null(), null(), null()), NOT_REG);
        assert_eq!(StopGamingTelemetrySession(0, 0, null()), NOT_REG);
        reg();
        assert_eq!(StartGamingTelemetrySession(wz("u").as_ptr(), b"cv.1\0".as_ptr(), wz("c").as_ptr()), S_OK);
        assert!(state().running);
        assert!(!state().session.is_zero());
        assert_eq!(TelemetryWriteError(wz("id").as_ptr(), 5, null(), 0, null(), null(), null(), 1), S_OK);
        assert_eq!(TelemetryWriteError(wz("id").as_ptr(), 5, null(), 0, null(), null(), null(), 0), S_OK);
        assert_eq!(state().errors.get(&5), Some(&2));
        assert_eq!(StartGamingTelemetrySession(null(), null(), null()), S_OK);
        assert!(state().errors.is_empty());
        assert_eq!(StopGamingTelemetrySession(-1, 10, null()), S_OK);
        assert!(!state().running);
        assert!(state().user_id.is_none());
        assert!(state().related_cv.is_none());
        assert_eq!(StopGamingTelemetrySession(0, 0, null()), S_OK);
    }
}

#[test]
fn write_error_argument_rules() {
    let _g = fx();
    unsafe {
        assert_eq!(TelemetryWriteError(null(), 0, null(), 0, null(), null(), null(), 0), E_INVALIDARG);
        assert_eq!(TelemetryWriteError(wz("i").as_ptr(), 0, null(), 3, null(), null(), null(), 0), E_INVALIDARG);
        assert_eq!(TelemetryWriteError(wz("i").as_ptr(), 0, null(), 0x300, null(), null(), null(), 0), E_INVALIDARG);
        assert_eq!(TelemetryWriteError(wz("i").as_ptr(), 0, null(), 0x101, null(), null(), null(), 0), NOT_REG);
        reg();
        assert_eq!(TelemetryWriteError(wz("i").as_ptr(), 1, wz("m").as_ptr(), 0x111, wz("cs").as_ptr(), wz("f").as_ptr(), wz("c").as_ptr(), 1), S_OK);
        assert!(state().errors.is_empty());
    }
}

#[test]
fn gaming_event_reports_listener_state() {
    let _g = fx();
    unsafe {
        let s = wz("val");
        let n = wz("k");
        let v = mk(|o| GamingTelemetryString(o, n.as_ptr(), s.as_ptr()));
        let vals = [v];
        let span = Span { first: vals.as_ptr(), last: vals.as_ptr().add(1) };
        assert_eq!(TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 4, 0, &span), NOT_REG);
        reg();
        assert_eq!(TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 4, 0, null()), E_INVALIDARG);
        assert_eq!(TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 4, 0, &span), 0xEB0001);
        enable_p1();
        assert_eq!(TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 4, 0, &span), S_OK);
        assert_eq!(TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 5, 0, &span), 0xEB0001);
        assert_eq!(TelemetryWriteGamingEventWithGroupIdType(wz("Ev").as_ptr(), 0, 4, 0, &span, 1), S_OK);
        assert_eq!(TelemetryWriteGamingEventWithGroupIdType(wz("Ev").as_ptr(), 0, 4, 0, &span, 2), 0xEB0001);
        let bad = Value { aidx: 0, ..v_prim("GTL__TlgEventTag", Prim::u64v(1)) };
        let bv = [bad];
        let bspan = Span { first: bv.as_ptr(), last: bv.as_ptr().add(1) };
        assert_eq!(TelemetryWriteGamingEvent(wz("Ev").as_ptr(), 0, 4, 0, &bspan), 0x80EB0013u32 as i32);
        value_drop(&bv[0]);
        value_drop(&vals[0]);
    }
}

#[test]
fn page_view_action_and_content_update() {
    let _g = fx();
    unsafe {
        let t0 = wz("t");
        let t1 = wz("p");
        let tags = [t0.as_ptr(), t1.as_ptr()];
        assert_eq!(TelemetryWritePageView(null(), wz("u").as_ptr(), null(), &tags, null(), null()), E_INVALIDARG);
        assert_eq!(TelemetryWritePageView(wz("p").as_ptr(), wz("u").as_ptr(), null(), &tags, null(), null()), NOT_REG);
        let content = [0u64; 7];
        assert_eq!(TelemetryWritePageAction(null(), 0, null(), content.as_ptr() as *const c_void, null(), null(), 0), E_INVALIDARG);
        assert_eq!(TelemetryWriteContentUpdate(null(), null(), 0, null(), null()), E_INVALIDARG);
        reg();
        let pt = wz("pt");
        let pid = wz("pid");
        let tags = [pt.as_ptr(), pid.as_ptr()];
        assert_eq!(TelemetryWritePageView(wz("home").as_ptr(), wz("app://home").as_ptr(), null(), &tags, null(), null()), S_OK);
        assert!(state().pages.contains_key(&w("home")));
        let pn = wz("parent");
        let mut c = [0u64; 7];
        c[0] = pn.as_ptr() as u64;
        c[1] = 3;
        assert_eq!(TelemetryWritePageAction(wz("home").as_ptr(), 1, wz("btn").as_ptr(), c.as_ptr() as *const c_void, wz("feat").as_ptr(), null(), 1), S_OK);
        assert!(state().page_action_seen);
        let before = cv_string();
        let items = [0u64; 14];
        assert_eq!(TelemetryWriteContentUpdate(wz("home").as_ptr(), items.as_ptr() as *const c_void, 2, null(), null()), S_OK);
        let after = cv_string();
        assert_ne!(before, after);
        assert!(after.starts_with(&before[..before.rfind('.').unwrap()]));
        assert_eq!(TelemetryWriteContentUpdate(wz("home").as_ptr(), null(), 1, null(), null()), E_INVALIDARG);
    }
}

#[link(name = "runtimeobject")]
extern "system" {
    fn RoInitialize(t: i32) -> i32;
    fn WindowsCreateString(s: *const u16, n: u32, h: *mut *mut c_void) -> i32;
    fn RoGetActivationFactory(c: *mut c_void, iid: *const Guid, f: *mut *mut c_void) -> i32;
}

unsafe fn parse_json(text: &str) -> *mut c_void {
    RoInitialize(1);
    let cls = w("Windows.Data.Json.JsonObject");
    let mut hc = null_mut();
    assert!(WindowsCreateString(cls.as_ptr(), cls.len() as u32, &mut hc) >= 0);
    let iid = Guid::new(0x2289f159, 0x54de, 0x45d8, [0xab, 0xcc, 0x22, 0x60, 0x3f, 0xa0, 0x66, 0xa0]);
    let mut f = null_mut();
    assert!(RoGetActivationFactory(hc, &iid, &mut f) >= 0);
    let t = w(text);
    let mut ht = null_mut();
    assert!(WindowsCreateString(t.as_ptr(), t.len() as u32, &mut ht) >= 0);
    let parse: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> i32 = std::mem::transmute(*(*(f as *const *const usize)).add(6));
    let mut o = null_mut();
    assert!(parse(f, ht, &mut o) >= 0);
    json::release(f);
    crate::sys::WindowsDeleteString(ht);
    crate::sys::WindowsDeleteString(hc);
    o
}

#[test]
fn custom_json_event_walks_windows_data_json() {
    let _g = fx();
    unsafe {
        let o = parse_json(r#"{"s":"x","n":2.5,"b":true,"z":null,"a":[{"k":1},{"k":2}],"o":{"q":1}}"#);
        let vals = json::convert(o).unwrap();
        let mut got: Vec<(String, i8, i8)> = vals.0.iter().map(|v| (String::from_utf16(v.name.wtext()).unwrap(), v.bvar().idx, if v.bvar().idx == 0 { v.bvar().prim().idx } else { v.bvar().coll().idx })).collect();
        got.sort();
        assert_eq!(got, vec![("a".into(), 1, 2), ("b".into(), 0, 5), ("n".into(), 0, 13), ("s".into(), 0, 3)]);
        drop(vals);
        assert_eq!(TelemetryWriteCustomJsonEvent(wz("J").as_ptr(), 0, 4, 0, 1, o), NOT_REG);
        reg();
        assert_eq!(TelemetryWriteCustomJsonEvent(wz("J").as_ptr(), 0, 4, 0, 1, o), S_OK);
        assert_eq!(TelemetryWriteCustomJsonEventWithGroupIdType(wz("J").as_ptr(), 0, 4, 0, 0, o, 4), S_OK);
        assert_eq!(TelemetryWriteCustomJsonEvent(wz("J").as_ptr(), 0, 4, 0, 0, null_mut()), E_INVALIDARG);
        json::release(o);
    }
}

#[test]
fn unregister_truncates_running_session() {
    let _g = fx();
    unsafe {
        reg();
        assert_eq!(StartGamingTelemetrySession(null(), null(), null()), S_OK);
        SetExtendedDataInt(wz("x").as_ptr(), 1);
        UnregisterGamingTelemetryProvider();
        let st = state();
        assert!(!st.running && !st.registered && st.cv.is_none() && st.count == 0);
    }
}
