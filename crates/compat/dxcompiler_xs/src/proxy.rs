use crate::com::*;
use crate::guids::*;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Summary {
    pub profile: Option<String>,
    pub entry: Option<String>,
    pub xbox: Vec<String>,
}

pub fn looks_xbox(arg: &str) -> bool {
    let l = arg.to_ascii_lowercase();
    l.contains("xbox") || l.contains("__xb_") || l.contains("xdxr") || l.contains("scarlett")
}

pub fn summarize(entry: Option<String>, profile: Option<String>, args: &[String]) -> Summary {
    let mut s = Summary { profile, entry, xbox: Vec::new() };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if looks_xbox(a) {
            s.xbox.push(a.to_string());
        }
        let flag = a.strip_prefix('-').or_else(|| a.strip_prefix('/'));
        if let Some(f) = flag {
            let slot = match f.as_bytes().first() {
                Some(b'T') => Some(&mut s.profile),
                Some(b'E') => Some(&mut s.entry),
                _ => None,
            };
            if let Some(slot) = slot {
                if f.len() == 1 {
                    if let Some(v) = args.get(i + 1) {
                        *slot = Some(v.clone());
                        i += 1;
                    }
                } else {
                    *slot = Some(f[1..].to_string());
                }
            }
        }
        i += 1;
    }
    s
}

#[repr(C)]
struct DxcDefine {
    name: *const u16,
    value: *const u16,
}

unsafe fn wide_list(p: *const *const u16, n: u32) -> Vec<String> {
    if p.is_null() {
        return Vec::new();
    }
    (0..n as usize).filter_map(|i| wide_to_string(*p.add(i))).collect()
}

unsafe fn define_list(p: *const DxcDefine, n: u32) -> Vec<String> {
    if p.is_null() {
        return Vec::new();
    }
    (0..n as usize)
        .map(|i| {
            let d = &*p.add(i);
            let name = wide_to_string(d.name).unwrap_or_default();
            match wide_to_string(d.value) {
                Some(v) => format!("-D{name}={v}"),
                None => format!("-D{name}"),
            }
        })
        .collect()
}

fn clip(s: String) -> String {
    if s.len() > 600 {
        let mut e = 600;
        while !s.is_char_boundary(e) {
            e -= 1;
        }
        format!("{}...", &s[..e])
    } else {
        s
    }
}

unsafe fn result_status(res: *mut c_void) -> Option<Hr> {
    if res.is_null() {
        return None;
    }
    #[repr(C)]
    struct OpResultVtbl {
        unk: UnknownVtbl,
        get_status: unsafe extern "system" fn(*mut c_void, *mut Hr) -> Hr,
    }
    let mut st = 0;
    let hr = (vtbl::<OpResultVtbl>(res).get_status)(res, &mut st);
    (hr >= 0).then_some(st)
}

fn report(method: &str, name: Option<String>, sum: &Summary, all: &[String], hr: Hr, status: Option<Hr>) {
    let st = status.map(|s| format!(" status={:#010x}", s as u32)).unwrap_or_default();
    let xb = if sum.xbox.is_empty() { String::new() } else { format!(" xbox-flavoured={:?}", sum.xbox) };
    crate::note(
        method,
        &clip(format!(
            "profile={} entry={} source={} args={:?}{xb} -> hr={:#010x}{st} (host DXC emits plain DXIL, not Xbox ISA)",
            sum.profile.as_deref().unwrap_or("?"),
            sum.entry.as_deref().unwrap_or("?"),
            name.as_deref().unwrap_or("?"),
            all,
            hr as u32
        )),
    );
}

#[repr(C)]
struct Proxy {
    vtbl: *const c_void,
    inner: *mut c_void,
    refs: AtomicU32,
}

#[repr(C)]
struct LegacyVtbl {
    unk: UnknownVtbl,
    compile: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u16, *const u16, *const u16, *const *const u16, u32, *const DxcDefine, u32, *mut c_void, *mut *mut c_void) -> Hr,
    preprocess: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u16, *const *const u16, u32, *const DxcDefine, u32, *mut c_void, *mut *mut c_void) -> Hr,
    disassemble: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> Hr,
    compile_with_debug: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u16, *const u16, *const u16, *const *const u16, u32, *const DxcDefine, u32, *mut c_void, *mut *mut c_void, *mut *mut u16, *mut *mut c_void) -> Hr,
}

#[repr(C)]
struct V3Vtbl {
    unk: UnknownVtbl,
    compile: unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const *const u16, u32, *mut c_void, *const Guid, *mut *mut c_void) -> Hr,
    disassemble: unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const Guid, *mut *mut c_void) -> Hr,
}

unsafe fn me(this: *mut c_void) -> &'static Proxy {
    &*(this as *const Proxy)
}

unsafe extern "system" fn p_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> Hr {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let Some(g) = Guid::read(iid as *const c_void) else { return E_POINTER };
    if g == IID_IUNKNOWN {
        p_add_ref(this);
        *out = this;
        return S_OK;
    }
    let p = me(this);
    let hr = (vtbl::<UnknownVtbl>(p.inner).query_interface)(p.inner, iid, out);
    if hr >= 0 && !(*out).is_null() {
        *out = wrap_if_compiler(&g, *out);
    }
    hr
}

unsafe extern "system" fn p_add_ref(this: *mut c_void) -> u32 {
    me(this).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn p_release(this: *mut c_void) -> u32 {
    let n = me(this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        let b = Box::from_raw(this as *mut Proxy);
        release(b.inner);
    }
    n
}

unsafe extern "system" fn l_compile(
    this: *mut c_void,
    src: *mut c_void,
    name: *const u16,
    entry: *const u16,
    profile: *const u16,
    args: *const *const u16,
    argc: u32,
    defs: *const DxcDefine,
    defc: u32,
    inc: *mut c_void,
    out: *mut *mut c_void,
) -> Hr {
    let inner = me(this).inner;
    let hr = (vtbl::<LegacyVtbl>(inner).compile)(inner, src, name, entry, profile, args, argc, defs, defc, inc, out);
    let mut all = wide_list(args, argc);
    all.extend(define_list(defs, defc));
    let sum = summarize(wide_to_string(entry), wide_to_string(profile), &all);
    report("IDxcCompiler::Compile", wide_to_string(name), &sum, &all, hr, if out.is_null() { None } else { result_status(*out) });
    hr
}

unsafe extern "system" fn l_preprocess(
    this: *mut c_void,
    src: *mut c_void,
    name: *const u16,
    args: *const *const u16,
    argc: u32,
    defs: *const DxcDefine,
    defc: u32,
    inc: *mut c_void,
    out: *mut *mut c_void,
) -> Hr {
    let inner = me(this).inner;
    let hr = (vtbl::<LegacyVtbl>(inner).preprocess)(inner, src, name, args, argc, defs, defc, inc, out);
    let mut all = wide_list(args, argc);
    all.extend(define_list(defs, defc));
    let sum = summarize(None, None, &all);
    report("IDxcCompiler::Preprocess", wide_to_string(name), &sum, &all, hr, if out.is_null() { None } else { result_status(*out) });
    hr
}

unsafe extern "system" fn l_disassemble(this: *mut c_void, src: *mut c_void, out: *mut *mut c_void) -> Hr {
    let inner = me(this).inner;
    (vtbl::<LegacyVtbl>(inner).disassemble)(inner, src, out)
}

unsafe extern "system" fn l_compile_dbg(
    this: *mut c_void,
    src: *mut c_void,
    name: *const u16,
    entry: *const u16,
    profile: *const u16,
    args: *const *const u16,
    argc: u32,
    defs: *const DxcDefine,
    defc: u32,
    inc: *mut c_void,
    out: *mut *mut c_void,
    dbg_name: *mut *mut u16,
    dbg: *mut *mut c_void,
) -> Hr {
    let inner = me(this).inner;
    let hr = (vtbl::<LegacyVtbl>(inner).compile_with_debug)(inner, src, name, entry, profile, args, argc, defs, defc, inc, out, dbg_name, dbg);
    let mut all = wide_list(args, argc);
    all.extend(define_list(defs, defc));
    let sum = summarize(wide_to_string(entry), wide_to_string(profile), &all);
    report("IDxcCompiler2::CompileWithDebug", wide_to_string(name), &sum, &all, hr, if out.is_null() { None } else { result_status(*out) });
    hr
}

unsafe extern "system" fn v3_compile(
    this: *mut c_void,
    buf: *const DxcBuffer,
    args: *const *const u16,
    argc: u32,
    inc: *mut c_void,
    riid: *const Guid,
    out: *mut *mut c_void,
) -> Hr {
    let inner = me(this).inner;
    let hr = (vtbl::<V3Vtbl>(inner).compile)(inner, buf, args, argc, inc, riid, out);
    let all = wide_list(args, argc);
    let sum = summarize(None, None, &all);
    let st = if out.is_null() || Guid::read(riid as *const c_void).map(|g| interface(&g).is_none()).unwrap_or(true) {
        None
    } else {
        result_status(*out)
    };
    report("IDxcCompiler3::Compile", None, &sum, &all, hr, st);
    hr
}

unsafe extern "system" fn v3_disassemble(this: *mut c_void, buf: *const DxcBuffer, riid: *const Guid, out: *mut *mut c_void) -> Hr {
    let inner = me(this).inner;
    (vtbl::<V3Vtbl>(inner).disassemble)(inner, buf, riid, out)
}

static LEGACY: LegacyVtbl = LegacyVtbl {
    unk: UnknownVtbl { query_interface: p_qi, add_ref: p_add_ref, release: p_release },
    compile: l_compile,
    preprocess: l_preprocess,
    disassemble: l_disassemble,
    compile_with_debug: l_compile_dbg,
};

static V3: V3Vtbl = V3Vtbl {
    unk: UnknownVtbl { query_interface: p_qi, add_ref: p_add_ref, release: p_release },
    compile: v3_compile,
    disassemble: v3_disassemble,
};

pub unsafe fn is_proxy(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let v = *(p as *const *const c_void);
    v == &LEGACY as *const _ as *const c_void || v == &V3 as *const _ as *const c_void
}

pub unsafe fn inner_of(p: *mut c_void) -> Option<*mut c_void> {
    is_proxy(p).then(|| me(p).inner)
}

pub unsafe fn wrap_if_compiler(iid: &Guid, obj: *mut c_void) -> *mut c_void {
    let vt = if *iid == IID_IDXC_COMPILER || *iid == IID_IDXC_COMPILER2 {
        &LEGACY as *const _ as *const c_void
    } else if *iid == IID_IDXC_COMPILER3 {
        &V3 as *const _ as *const c_void
    } else {
        return obj;
    };
    if obj.is_null() || is_proxy(obj) {
        return obj;
    }
    Box::into_raw(Box::new(Proxy { vtbl: vt, inner: obj, refs: AtomicU32::new(1) })) as *mut c_void
}
