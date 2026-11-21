#![allow(non_snake_case, clippy::missing_safety_doc)]

pub mod abi;
pub mod cv;
pub mod json;
pub mod state;
pub mod sys;
pub mod tlg;

#[cfg(test)]
mod tests;

use abi::*;
use state::*;
use std::ffi::c_void;
use sys::Guid;
use tlg::E_INVALIDARG;

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryValue(out: *mut Value, name: *const u16, b: *mut BVar) -> *mut Value {
    let v = Value::with_bvar(Sp::name(name), bvar_norm(&*b));
    bvar_drop(&*b);
    out.write(v);
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryString(out: *mut Value, name: *const u16, v: *const u16) -> *mut Value {
    out.write(Value::with_bvar(Sp::name(name), BVar::from_prim(string_prim(v))));
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryPrimitiveVector(out: *mut Value, name: *const u16, span: *const Span<Prim>) -> *mut Value {
    let v: Vec<Prim> = span_items(span).iter().map(|p| normalize(p)).collect();
    out.write(Value::with_bvar(Sp::name(name), BVar::from_coll(Coll::new(1, raw_vec(&v)))));
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryPrimitiveMapValue(out: *mut PrimMapValue, name: *const u16, p: *mut Prim) -> *mut PrimMapValue {
    let v = PrimMapValue { name: Sp::name(name), p: normalize(&*p) };
    (*p).drop_ref();
    out.write(v);
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryPrimitiveMap(out: *mut Value, name: *const u16, span: *const Span<PrimMapValue>) -> *mut Value {
    let mut m = RawMap::new(PRIM_NODE);
    for e in span_items(span) {
        prim_map_put(&mut m, e.name.clone_ref(), normalize(&e.p), true);
    }
    out.write(Value::with_bvar(Sp::name(name), BVar::from_coll(Coll::map(m))));
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryMapValue(out: *mut MapValue, name: *const u16, b: *mut BVar) -> *mut MapValue {
    let v = MapValue { name: Sp::name(name), b: bvar_norm(&*b) };
    bvar_drop(&*b);
    out.write(v);
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryMap(out: *mut Value, name: *const u16, span: *const Span<MapValue>) -> *mut Value {
    let mut m = RawMap::new(BVAR_NODE);
    for e in span_items(span) {
        bvar_map_put(&mut m, e.name.clone_ref(), bvar_norm(&e.b));
    }
    out.write(Value::with_map(Sp::name(name), m));
    out
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryBaseType(out: *mut Value, t: *const u16) -> *mut Value {
    let n = wz("baseType");
    let mut b = BVar::from_prim(Prim::wref(t));
    GamingTelemetryValue(out, n.as_ptr(), &mut b)
}

#[no_mangle]
pub unsafe extern "system" fn GamingTelemetryBaseData(out: *mut Value, span: *const Span<MapValue>) -> *mut Value {
    let n = wz("baseData");
    GamingTelemetryMap(out, n.as_ptr(), span)
}

unsafe fn set_ext(name: *const u16, ty: u32, data: &[u8]) -> i32 {
    if name.is_null() {
        return E_INVALIDARG;
    }
    state().set_ext(wstr(name), ty, data)
}

#[no_mangle]
pub unsafe extern "system" fn SetExtendedDataString(name: *const u16, v: *const u16) -> i32 {
    if name.is_null() || v.is_null() {
        return E_INVALIDARG;
    }
    let mut d: Vec<u8> = wstr(v).iter().flat_map(|c| c.to_le_bytes()).collect();
    d.extend_from_slice(&[0, 0]);
    set_ext(name, 1, &d)
}

#[no_mangle]
pub unsafe extern "system" fn SetExtendedDataInt(name: *const u16, v: i64) -> i32 {
    set_ext(name, 2, &v.to_le_bytes())
}

#[no_mangle]
pub unsafe extern "system" fn SetExtendedDataUInt(name: *const u16, v: u64) -> i32 {
    set_ext(name, 3, &v.to_le_bytes())
}

#[no_mangle]
pub unsafe extern "system" fn SetExtendedDataDouble(name: *const u16, v: f64) -> i32 {
    set_ext(name, 4, &v.to_le_bytes())
}

#[no_mangle]
pub unsafe extern "system" fn SetExtendedDataBoolean(name: *const u16, v: u32) -> i32 {
    set_ext(name, 5, &v.to_le_bytes())
}

#[no_mangle]
pub unsafe extern "system" fn ClearExtendedDataValue(name: *const u16) -> i32 {
    if name.is_null() {
        return E_INVALIDARG;
    }
    let mut st = state();
    match st.find_slot(wstr(name)) {
        Some(i) => {
            st.clear_slot(i);
            S_OK
        }
        None => E_INVALIDARG,
    }
}

#[no_mangle]
pub unsafe extern "system" fn ClearExtendedDataValuesExcept(names: *const *const u16, count: usize) {
    if names.is_null() && count != 0 {
        return;
    }
    let keep: Vec<&[u16]> = (0..count).map(|i| wstr(*names.add(i))).collect();
    let mut st = state();
    for i in 0..MAX_SLOTS {
        let Some(n) = st.slots[i].name.clone() else { continue };
        let n = &n[..n.len() - 1];
        if !keep.iter().any(|k| *k == n) {
            st.clear_slot(i);
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn ResetExtendedData() {
    state().reset_ext()
}

fn kind(i: i32) -> Option<usize> {
    if (0..8).contains(&i) {
        Some(i as usize)
    } else {
        None
    }
}

#[no_mangle]
pub extern "system" fn ResetPrivacyFlagsForEvent(i: i32) -> i32 {
    let Some(k) = kind(i) else { return E_INVALIDARG };
    let mut st = state();
    if !st.registered {
        return GTL_E_NOT_REGISTERED;
    }
    st.reset_privacy(k);
    S_OK
}

#[no_mangle]
pub extern "system" fn OverridePrivacyFlagsForEvent(i: i32, v: u64) -> i32 {
    let Some(k) = kind(i) else { return E_INVALIDARG };
    let mut st = state();
    if !st.registered {
        return GTL_E_NOT_REGISTERED;
    }
    st.privacy[k] = v;
    S_OK
}

#[no_mangle]
pub extern "system" fn OverrideTagsForEvent(i: i32, v: u32) -> i32 {
    let Some(k) = kind(i) else { return E_INVALIDARG };
    let mut st = state();
    if !st.registered {
        return GTL_E_NOT_REGISTERED;
    }
    st.tags[k] = v;
    S_OK
}

#[no_mangle]
pub extern "system" fn OverrideKeywordsForEvent(i: i32, v: u64) -> i32 {
    let Some(k) = kind(i) else { return E_INVALIDARG };
    if v & 0xe00000000000 == 0 {
        return E_INVALIDARG;
    }
    let mut st = state();
    if !st.registered {
        return GTL_E_NOT_REGISTERED;
    }
    st.keywords[k] = v;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn RegisterGamingTelemetryProvider(name: *const u16, guid: *const Guid, config: *const c_void) -> i32 {
    state().register(name, guid, config as *const u8)
}

#[no_mangle]
pub unsafe extern "system" fn UnregisterGamingTelemetryProvider() {
    state().unregister()
}

#[no_mangle]
pub unsafe extern "system" fn SetGtlCorrelationVector(cv: *const u8) -> i32 {
    if cv.is_null() {
        return E_INVALIDARG;
    }
    let mut v = cstr(cv).to_vec();
    v.push(0);
    state().related_cv = Some(v);
    S_OK
}

#[no_mangle]
pub extern "system" fn ClearGtlCorrelationVector() -> i32 {
    let mut st = state();
    if !st.registered {
        return GTL_E_NOT_REGISTERED;
    }
    st.related_cv = None;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn GetDeviceInteractionVector(out: *mut u8) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    let st = state();
    let Some(cv) = st.cv.as_ref() else { return E_UNEXPECTED };
    let Some(s) = cv.format(cv.packed) else { return E_UNEXPECTED };
    std::ptr::copy_nonoverlapping(s.as_ptr(), out, s.len());
    *out.add(s.len()) = 0;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn SetGtlActiveUserId(u: *const u16) -> i32 {
    state().set_user(u)
}

#[no_mangle]
pub extern "system" fn ClearGtlActiveUserId() -> i32 {
    let mut st = state();
    if !st.registered {
        return GTL_E_NOT_REGISTERED;
    }
    st.user_id = None;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn StartGamingTelemetrySession(user: *const u16, cv: *const u8, custom: *const u16) -> i32 {
    state().start(user, cv, custom)
}

#[no_mangle]
pub unsafe extern "system" fn StopGamingTelemetrySession(hr: i32, duration: u64, custom: *const u16) -> i32 {
    state().stop(hr, duration, custom)
}

#[no_mangle]
pub unsafe extern "system" fn TelemetryWriteGamingEvent(name: *const u16, privacy: u64, level: u8, kw: u64, span: *const Span<Value>) -> i32 {
    TelemetryWriteGamingEventWithGroupIdType(name, privacy, level, kw, span, 1)
}

#[no_mangle]
pub unsafe extern "system" fn TelemetryWriteGamingEventWithGroupIdType(name: *const u16, privacy: u64, level: u8, kw: u64, span: *const Span<Value>, group: u32) -> i32 {
    if span.is_null() {
        return E_INVALIDARG;
    }
    state().write_event(wstr(name), privacy, level, kw, span_items(span), group, false)
}

#[no_mangle]
pub unsafe extern "system" fn TelemetryWriteCustomJsonEvent(name: *const u16, privacy: u64, level: u8, kw: u64, tag: u32, json: *mut c_void) -> i32 {
    TelemetryWriteCustomJsonEventWithGroupIdType(name, privacy, level, kw, tag, json, 1)
}

#[no_mangle]
pub unsafe extern "system" fn TelemetryWriteCustomJsonEventWithGroupIdType(name: *const u16, privacy: u64, level: u8, kw: u64, tag: u32, json: *mut c_void, group: u32) -> i32 {
    let mut vals = match json::convert(json) {
        Ok(v) => v,
        Err(hr) => return hr,
    };
    if tag != 0 {
        vals.0.insert(0, v_tag(tag));
    }
    let hr = state().write_event(wstr(name), privacy, level, kw, &vals.0, group, false);
    if hr < 0 {
        hr
    } else {
        S_OK
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "system" fn TelemetryWriteError(id: *const u16, code: i64, msg: *const u16, props: u64, stack: *const u16, feature: *const u16, custom: *const u16, critical: u32) -> i32 {
    state().write_error(id, code, msg, props, stack, feature, custom, critical)
}

#[no_mangle]
pub unsafe extern "system" fn TelemetryWritePageView(name: *const u16, uri: *const u16, refuri: *const u16, tags: *const [*const u16; 2], feature: *const u16, custom: *const u16) -> i32 {
    state().page_view(name, uri, refuri, tags, feature, custom)
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "system" fn TelemetryWritePageAction(page: *const u16, behavior: u32, target: *const u16, content: *const c_void, feature: *const u16, custom: *const u16, counts: u32) -> i32 {
    state().page_action(page, behavior, target, content as *const u8, feature, custom, counts)
}

#[no_mangle]
pub unsafe extern "system" fn TelemetryWriteContentUpdate(page: *const u16, items: *const c_void, count: u64, feature: *const u16, custom: *const u16) -> i32 {
    state().content_update(page, items as *const u8, count, feature, custom)
}
