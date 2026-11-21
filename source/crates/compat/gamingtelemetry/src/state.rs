use crate::abi::*;
use crate::cv::Cv;
use crate::sys::*;
use crate::tlg::*;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const S_OK: i32 = 0;
pub const E_OUTOFMEMORY: i32 = 0x8007000Eu32 as i32;
pub const E_ACCESSDENIED: i32 = 0x80070005u32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000FFFFu32 as i32;
pub const E_BOUNDS: i32 = 0x8000000Bu32 as i32;
pub const E_FAIL: i32 = 0x80004005u32 as i32;
pub const GTL_E_NOT_REGISTERED: i32 = 0x80EB0011u32 as i32;
pub const GTL_E_OTHER_NAME: i32 = 0x80EB0012u32 as i32;
pub const GTL_E_EXT_TYPE: i32 = 0x80EB0017u32 as i32;
pub const GTL_E_ZERO_GROUP: i32 = 0x80EB0018u32 as i32;
pub const GTL_S_ALREADY_RUNNING: i32 = 0xEB0002;
pub const KW_CV: u64 = 1 << 43;
pub const MAX_SLOTS: usize = 15;

#[derive(Default, Clone)]
pub struct Slot {
    pub name: Option<Vec<u16>>,
    pub ty: u32,
    pub data: Vec<u8>,
}

pub struct Page {
    pub id: Guid,
    pub uri: Vec<u16>,
    pub tick: u64,
}

pub struct State {
    pub registered: bool,
    pub component: Vec<u16>,
    pub provider: Vec<u16>,
    pub guid: Guid,
    pub dynp: Option<[Box<Prov>; 3]>,
    pub crit: [Option<Box<Prov>>; 4],
    pub cfg: [u32; 4],
    pub reg_tick: u64,
    pub cv: Option<Cv>,
    pub related_cv: Option<Vec<u8>>,
    pub user_id: Option<Vec<u16>>,
    pub running: bool,
    pub start: u64,
    pub session: Guid,
    pub slots: Vec<Slot>,
    pub count: usize,
    pub errors: BTreeMap<i64, u32>,
    pub features: BTreeMap<Vec<u16>, u32>,
    pub pages: BTreeMap<Vec<u16>, Page>,
    pub page_action_seen: bool,
    pub privacy: [u64; 8],
    pub keywords: [u64; 8],
    pub tags: [u32; 8],
}

unsafe impl Send for State {}

impl State {
    fn new() -> State {
        State {
            registered: false,
            component: Vec::new(),
            provider: Vec::new(),
            guid: Guid::default(),
            dynp: None,
            crit: [None, None, None, None],
            cfg: [0; 4],
            reg_tick: 0,
            cv: None,
            related_cv: None,
            user_id: None,
            running: false,
            start: 0,
            session: Guid::default(),
            slots: vec![Slot::default(); MAX_SLOTS],
            count: 0,
            errors: BTreeMap::new(),
            features: BTreeMap::new(),
            pages: BTreeMap::new(),
            page_action_seen: false,
            privacy: [0; 8],
            keywords: [0; 8],
            tags: [0; 8],
        }
    }
}

pub fn state() -> MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::new())).lock().unwrap_or_else(|e| e.into_inner())
}

pub fn copy_fmt(s: &[u16]) -> Result<Vec<u16>, i32> {
    let mut f = s.to_vec();
    f.push(0);
    let mut out = printf(s.len() + 1, &f, &[])?;
    out.push(0);
    Ok(out)
}

pub fn guid_string(g: &Guid) -> Vec<u16> {
    let mut args = vec![g.d1 as u64, g.d2 as u64, g.d3 as u64];
    args.extend(g.d4.iter().map(|b| *b as u64));
    printf(0x40, &wz("%08lX-%04hX-%04hX-%02hhX%02hhX-%02hhX%02hhX%02hhX%02hhX%02hhX%02hhX"), &args).unwrap_or_default()
}

fn wptr(v: &[u16]) -> u64 {
    v.as_ptr() as u64
}

impl State {
    pub fn find_slot(&self, name: &[u16]) -> Option<usize> {
        self.slots.iter().position(|s| s.name.as_deref().map(|n| &n[..n.len() - 1]) == Some(name))
    }

    pub fn set_ext(&mut self, name: &[u16], ty: u32, data: &[u8]) -> i32 {
        if let Some(i) = self.find_slot(name) {
            self.slots[i].data = data.to_vec();
            return S_OK;
        }
        if self.count == MAX_SLOTS {
            return E_OUTOFMEMORY;
        }
        let Some(i) = self.slots.iter().position(|s| s.name.is_none()) else { return E_OUTOFMEMORY };
        let copy = match copy_fmt(name) {
            Ok(c) => c,
            Err(hr) => return hr,
        };
        self.slots[i] = Slot { name: Some(copy), ty, data: data.to_vec() };
        self.count += 1;
        S_OK
    }

    pub fn clear_slot(&mut self, i: usize) {
        self.slots[i] = Slot::default();
        self.count -= 1;
    }

    pub fn reset_ext(&mut self) {
        for i in 0..MAX_SLOTS {
            if self.slots[i].name.is_some() {
                self.clear_slot(i);
            }
        }
    }

    fn used_slots(&self) -> Vec<&Slot> {
        let mut out = Vec::new();
        for s in &self.slots {
            if out.len() >= self.count {
                break;
            }
            if s.name.is_some() {
                out.push(s);
            }
        }
        out
    }

    pub unsafe fn ext_map(&self) -> Result<Coll, i32> {
        if self.count > MAX_SLOTS {
            return Err(E_BOUNDS);
        }
        let mut m = RawMap::new(PRIM_NODE);
        prim_map_put(&mut m, Sp::wide(&w("sessionId")), Prim::guid(&self.session), false);
        let uid = match &self.user_id {
            Some(u) => Prim::wref(u.as_ptr()),
            None => Prim::empty_w(),
        };
        prim_map_put(&mut m, Sp::wide(&w("activeUserId")), uid, false);
        for s in self.used_slots() {
            let name = s.name.as_ref().unwrap();
            let d = &s.data;
            let q = |n: usize| -> u64 {
                let mut b = [0u8; 8];
                b[..n.min(d.len())].copy_from_slice(&d[..n.min(d.len())]);
                u64::from_le_bytes(b)
            };
            let p = match s.ty {
                1 => Prim::wref(d.as_ptr() as *const u16),
                2 => Prim::i64v(q(8) as i64),
                3 => Prim::u64v(q(8)),
                4 => Prim::f64v(f64::from_bits(q(8))),
                5 => Prim::boolv(q(4) as u32 != 0),
                _ => {
                    prim_map_destroy(&m);
                    return Err(GTL_E_EXT_TYPE);
                }
            };
            prim_map_put(&mut m, Sp::wide(&name[..name.len() - 1]), p, false);
        }
        Ok(Coll::map(m))
    }

    pub fn json(&self) -> Result<Vec<u16>, i32> {
        if self.count > MAX_SLOTS {
            return Err(E_BOUNDS);
        }
        let mut acc: Option<Vec<u16>> = None;
        let join = |acc: Option<Vec<u16>>, e: Vec<u16>| -> Result<Vec<u16>, i32> {
            match acc {
                None => Ok(e),
                Some(mut a) => {
                    a.push(0);
                    let mut e = e;
                    e.push(0);
                    printf(0x3c00, &wz("%s, %s"), &[wptr(&a), wptr(&e)])
                }
            }
        };
        for s in self.used_slots() {
            let name = s.name.as_ref().unwrap();
            let d = &s.data;
            let q = {
                let mut b = [0u8; 8];
                b[..8.min(d.len())].copy_from_slice(&d[..8.min(d.len())]);
                u64::from_le_bytes(b)
            };
            let tf_true = wz("true");
            let tf_false = wz("false");
            let e = match s.ty {
                1 => printf(0x400, &wz("\"%s\": \"%s\""), &[wptr(name), d.as_ptr() as u64])?,
                2 | 3 => printf(0x400, &wz("\"%s\": %d"), &[wptr(name), q])?,
                4 => printf(0x400, &wz("\"%s\": %f"), &[wptr(name), q])?,
                5 => printf(0x400, &wz("\"%s\": %s"), &[wptr(name), if q as u32 == 1 { wptr(&tf_true) } else { wptr(&tf_false) }])?,
                _ => return Err(GTL_E_EXT_TYPE),
            };
            acc = Some(join(acc, e)?);
        }
        let mut g = guid_string(&self.session);
        g.push(0);
        let sid = printf(0x400, &wz("\"sessionId\": \"%s\""), &[wptr(&g)])?;
        let acc2 = join(acc, sid)?;
        let empty = vec![0u16];
        let u = self.user_id.as_deref().unwrap_or(&empty);
        let uid = printf(0x400, &wz("\"activeUserId\": \"%s\""), &[wptr(u)])?;
        let mut all = join(Some(acc2), uid)?;
        all.push(0);
        printf(0x3c01, &wz("{ %s }"), &[wptr(&all)])
    }

    fn cv_ansi(&self) -> Vec<u8> {
        let mut v = self.related_cv.clone().unwrap_or_else(|| vec![0]);
        v.pop();
        v
    }

    fn comp(&self) -> &[u16] {
        &self.component
    }

    pub fn crit_write(&self, i: usize, meta: Vec<u8>, pay: Pay) {
        if let Some(p) = &self.crit[i] {
            if p.crit_enabled() {
                p.write_static(&meta, &pay.0);
            }
        }
    }

    pub fn crit_on(&self, i: usize) -> bool {
        self.crit[i].as_ref().map(|p| p.crit_enabled()).unwrap_or(false)
    }

    unsafe fn rotate_cv(&mut self) -> i32 {
        let Some(cv) = self.cv.as_ref() else { return E_UNEXPECTED };
        let Some(s) = cv.format(cv.packed) else { return E_OUTOFMEMORY };
        self.cv = None;
        match Cv::extend(&s) {
            Some(n) => {
                self.cv = Some(n);
                S_OK
            }
            None => {
                self.cv = Some(Cv::new_v2(&uuid().bytes()));
                let old: Vec<u16> = s.iter().map(|c| *c as u16).chain(std::iter::once(0)).collect();
                let vals = Vals(vec![v_string("relatedCv", old.as_ptr())]);
                let hr = self.write_event(&w("RelatedCv"), 0x2000000, 4, 0x480000000000, &vals.0, 1, true);
                if hr < 0 {
                    hr
                } else {
                    S_OK
                }
            }
        }
    }

    pub unsafe fn write_event(&mut self, name: &[u16], privacy: u64, level: u8, kw: u64, user: &[Value], group: u32, flag: bool) -> i32 {
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        let ext = match self.ext_map() {
            Ok(c) => c,
            Err(hr) => return hr,
        };
        let mut cvbuf: Option<Vec<u8>> = None;
        if kw & KW_CV != 0 {
            if !flag && self.page_action_seen {
                let hr = self.rotate_cv();
                if hr < 0 {
                    coll_drop(&ext);
                    return hr;
                }
            }
            let Some(cv) = self.cv.as_mut() else {
                coll_drop(&ext);
                return E_UNEXPECTED;
            };
            let nv = cv.increment();
            let Some(mut s) = cv.format(nv) else {
                coll_drop(&ext);
                return E_UNEXPECTED;
            };
            s.push(0);
            cvbuf = Some(s);
        }
        let mut front: Vec<Value> = Vec::new();
        if kw & KW_CV != 0 {
            if let Some(r) = &self.related_cv {
                front.push(v_prim("relatedCv", Prim::nref(r.as_ptr())));
            }
            front.push(v_prim("__TlgCV__", Prim::nref(cvbuf.as_ref().unwrap().as_ptr())));
        } else if let Some(r) = &self.related_cv {
            front.push(v_prim("__TlgCV__", Prim::nref(r.as_ptr())));
        }
        front.push(v_coll("extendedData", ext));
        for v in user.iter().rev() {
            front.push(value_clone(v));
        }
        let vals = Vals(front);
        let mut tags = 0u32;
        for v in &vals.0 {
            if is_tag_name(v) {
                if v.aidx != 0 || v.bvar().idx != 0 || v.bvar().prim().idx != 9 {
                    return GTL_E_BAD_VARIANT;
                }
                tags |= v.bvar().prim().d[0] as u32;
            }
        }
        let kw2 = if self.cfg[1] != 0 { CRIT_KW } else { kw };
        if self.cfg[0] != 0 {
            tags |= 0x200000;
        }
        if self.cfg[2] != 0 {
            tags |= 0x800000;
        }
        if self.cfg[3] != 0 {
            tags |= 0x80000;
        }
        let Some(dp) = self.dynp.as_ref() else { return GTL_E_NOT_REGISTERED };
        let p = if tags & 0x80000 != 0 || group == 2 {
            &dp[1]
        } else if group == 4 {
            &dp[2]
        } else {
            &dp[0]
        };
        let hr = p.write(name, level, kw2, tags, privacy, &vals.0);
        drop(vals);
        if hr < 0 && self.crit_on(0) {
            let pay = Pay::default()
                .u64(0x1000000)
                .wide(&self.provider)
                .wide(&w("{}"))
                .wide(&w("GamingTelemetryLite::TelemetryWriteGamingEvent()"))
                .u32(hr as u32)
                .wide(&[])
                .u32(0)
                .wide(&[])
                .wide(&[]);
            self.crit_write(0, meta_error_short(), pay);
        }
        hr
    }

    pub unsafe fn stop_write(&mut self, privacy: u64, duration: u64, st: &str, end: i32, custom: *const u16) -> i32 {
        let mut em = RawMap::new(PRIM_NODE);
        let mut total = 0u32;
        for (k, c) in &self.errors {
            prim_map_put(&mut em, Sp::wide(&w(&k.to_string())), Prim::i32v(*c as i32), false);
            total = total.wrapping_add(*c);
        }
        let mut fm = RawMap::new(PRIM_NODE);
        for (k, c) in &self.features {
            prim_map_put(&mut fm, Sp::wide(k), Prim::i32v(*c as i32), false);
        }
        let stw = wz(st);
        let vals = Vals(vec![
            v_tag(self.tags[1]),
            v_prim("duration", Prim::u64v(duration)),
            v_prim("state", Prim::wref(stw.as_ptr())),
            v_prim("endStatus", Prim::i32v(end)),
            v_prim("totalErrorCount", Prim::u32v(total)),
            v_coll("errorSummary", Coll::map(em)),
            v_coll("featureUsageSummary", Coll::map(fm)),
            v_string("customAttributes", custom),
        ]);
        let hr = self.write_event(&w("SessionStopped"), privacy, if end < 0 { 2 } else { 4 }, self.keywords[1], &vals.0, 1, false);
        drop(vals);
        if hr < 0 {
            return hr;
        }
        let json = match self.json() {
            Ok(j) => j,
            Err(hr) => return hr,
        };
        let pay = Pay::default()
            .u64(0x1000000)
            .ansi(&self.cv_ansi())
            .wide(self.comp())
            .wide(&json)
            .u64(duration)
            .wide(&w(st))
            .u32(end as u32)
            .u32(total)
            .wide(wstr(custom));
        self.crit_write(3, meta_session_stopped(), pay);
        self.errors.clear();
        self.features.clear();
        S_OK
    }

    pub fn reset_privacy(&mut self, i: usize) {
        self.privacy[i] = if i == 2 { 0x1000000 } else { 0x2000000 };
    }

    pub unsafe fn register(&mut self, name: *const u16, guid: *const Guid, cfg: *const u8) -> i32 {
        if name.is_null() || *name == 0 {
            return E_INVALIDARG;
        }
        let n = wstr(name);
        if n.len() > 0x34 {
            return E_INVALIDARG;
        }
        if self.registered {
            return if n == self.component.as_slice() { S_OK } else { GTL_E_OTHER_NAME };
        }
        if cfg.is_null() {
            return E_INVALIDARG;
        }
        let mut nz = n.to_vec();
        nz.push(0);
        let comp = match printf(0x34, &wz("%s"), &[wptr(&nz)]) {
            Ok(c) => c,
            Err(hr) => return hr,
        };
        let mut cz = comp.clone();
        cz.push(0);
        let prov = match printf(0x200, &wz("Microsoft.Gaming.%s"), &[wptr(&cz)]) {
            Ok(p) => p,
            Err(hr) => return hr,
        };
        let g = if guid.is_null() { name_guid(&prov) } else { *guid };
        let rd = |o: usize| u32::from_le_bytes(std::slice::from_raw_parts(cfg.add(o), 4).try_into().unwrap());
        let c = [rd(0), rd(4), rd(8), rd(0xc)];
        let use_group = rd(0x10) != 0;
        let cg = Guid::from_bytes(std::slice::from_raw_parts(cfg.add(0x14), 16).try_into().unwrap());
        if use_group && cg.is_zero() {
            return GTL_E_ZERO_GROUP;
        }
        let group = if use_group { cg } else { GROUP_4F50 };
        self.component = comp;
        self.provider = prov;
        self.guid = g;
        self.cv = Some(Cv::new_v2(&uuid().bytes()));
        if self.dynp.is_none() {
            let mut p1 = Prov::new(traits_blob(&self.provider, Some(&group)));
            let mut p2 = Prov::new(traits_blob(&self.provider, Some(&GROUP_C7DE)));
            let mut p3 = Prov::new(traits_blob(&self.provider, Some(&GROUP_5ECB)));
            p1.register(&g);
            p2.register(&g);
            p3.register(&g);
            self.dynp = Some([p1, p2, p3]);
            for i in [1usize, 0] {
                if self.crit[i].is_none() {
                    let mut p = Prov::new(crit_traits(i));
                    p.register(&CRIT_GUID);
                    self.crit[i] = Some(p);
                }
            }
            self.reg_tick = tick32();
            self.cfg = c;
            let pay = Pay::default().u64(0x1000000).wide(&self.provider);
            self.crit_write(0, meta_provider_registered(), pay);
        }
        for i in [3usize, 2] {
            let mut p = Prov::new(crit_traits(i));
            p.register(&CRIT_GUID);
            self.crit[i] = Some(p);
        }
        self.registered = true;
        for i in 0..8 {
            self.reset_privacy(i);
            self.keywords[i] = if i < 7 { 0x400000000000 } else { 0 };
            self.tags[i] = 0;
        }
        S_OK
    }

    pub unsafe fn unregister(&mut self) {
        if self.running {
            let d = tick64().wrapping_sub(self.start);
            self.stop_write(0x2000000, d, "SessionTruncatedProviderUnregistered", E_FAIL, std::ptr::null());
            self.running = false;
            self.session = Guid::default();
        }
        if let Some(dp) = self.dynp.take() {
            if self.crit_on(1) {
                let up = tick32().wrapping_sub(self.reg_tick);
                let pay = Pay::default().u64(0x1000000).wide(&self.provider).u64(up);
                self.crit_write(1, meta_provider_unregistered(), pay);
            }
            for p in dp.iter() {
                p.unregister();
            }
        }
        self.cv = None;
        self.reset_ext();
        for i in [3usize, 2] {
            if let Some(p) = self.crit[i].take() {
                p.unregister();
            }
        }
        self.user_id = None;
        self.related_cv = None;
        self.registered = false;
    }

    pub unsafe fn set_user(&mut self, u: *const u16) -> i32 {
        if u.is_null() {
            return E_INVALIDARG;
        }
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        match copy_fmt(wstr(u)) {
            Ok(c) => self.user_id = Some(c),
            Err(hr) => return hr,
        }
        let vals = Vals(vec![v_tag(self.tags[6])]);
        let hr = self.write_event(&w("ActiveUserUpdated"), self.privacy[6], 4, self.keywords[6], &vals.0, 1, false);
        drop(vals);
        if hr < 0 {
            return hr;
        }
        let json = match self.json() {
            Ok(j) => j,
            Err(hr) => return hr,
        };
        let pay = Pay::default().u64(0x2000000).ansi(&self.cv_ansi()).wide(self.comp()).wide(&json);
        self.crit_write(3, meta_active_user(), pay);
        S_OK
    }

    pub unsafe fn start(&mut self, user: *const u16, cv: *const u8, custom: *const u16) -> i32 {
        let t = tick64();
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        if self.running {
            let hr = self.stop_write(self.privacy[0], t.wrapping_sub(self.start), "SessionTruncatedAlreadyRunning", GTL_S_ALREADY_RUNNING, std::ptr::null());
            if hr < 0 {
                return hr;
            }
        }
        self.start = t;
        if !user.is_null() {
            match copy_fmt(wstr(user)) {
                Ok(c) => self.user_id = Some(c),
                Err(hr) => return hr,
            }
        }
        if !cv.is_null() {
            let mut v = cstr(cv).to_vec();
            v.push(0);
            self.related_cv = Some(v);
        }
        self.session = co_guid();
        self.running = true;
        let vals = Vals(vec![v_tag(self.tags[0]), v_string("customAttributes", custom)]);
        let hr = self.write_event(&w("SessionStarted"), self.privacy[0], 4, self.keywords[0], &vals.0, 1, false);
        drop(vals);
        if hr < 0 {
            return hr;
        }
        let json = match self.json() {
            Ok(j) => j,
            Err(hr) => return hr,
        };
        let pay = Pay::default().u64(0x2000000).ansi(&self.cv_ansi()).wide(self.comp()).wide(&json).wide(wstr(custom));
        self.crit_write(3, meta_session_started(), pay);
        S_OK
    }

    pub unsafe fn stop(&mut self, hr_in: i32, duration: u64, custom: *const u16) -> i32 {
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        let d = if duration == 0 { tick64().wrapping_sub(self.start) } else { duration };
        let st = if self.running {
            if hr_in < 0 {
                "CompletedFailure"
            } else {
                "CompletedSuccess"
            }
        } else {
            "SessionNotRunning"
        };
        let hr = self.stop_write(self.privacy[1], d, st, hr_in, custom);
        if hr < 0 {
            return hr;
        }
        self.running = false;
        self.session = Guid::default();
        self.user_id = None;
        self.related_cv = None;
        S_OK
    }

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn write_error(&mut self, id: *const u16, code: i64, msg: *const u16, props: u64, stack: *const u16, feature: *const u16, custom: *const u16, critical: u32) -> i32 {
        if id.is_null() {
            return E_INVALIDARG;
        }
        if (props & 0xf).count_ones() > 1 || (props & 0xf00).count_ones() > 1 {
            return E_INVALIDARG;
        }
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        if self.running {
            *self.errors.entry(code).or_insert(0) += 1;
        }
        let stack_p = if stack.is_null() { Prim::wref(std::ptr::null()) } else { Prim::wref(stack) };
        let vals = Vals(vec![
            v_tag(self.tags[2]),
            v_string("identifier", id),
            v_prim("errorCode", Prim::i64v(code)),
            v_string("message", msg),
            v_prim("properties", Prim::u64v(props)),
            v_prim("callStack", stack_p),
            v_string("featureName", feature),
            v_string("customAttributes", custom),
        ]);
        let kw = self.keywords[2] | if props & 0x30 != 0 { KW_CV } else { 0 };
        let hr = self.write_event(&w("Error"), self.privacy[2], 2, kw, &vals.0, 1, false);
        drop(vals);
        if hr < 0 {
            return hr;
        }
        if critical != 0 {
            let json = match self.json() {
                Ok(j) => j,
                Err(hr) => return hr,
            };
            let pay = Pay::default()
                .u64(0x1000000)
                .ansi(&self.cv_ansi())
                .wide(self.comp())
                .wide(&json)
                .wide(wstr(id))
                .u64(code as u64)
                .wide(wstr(msg))
                .u64(props)
                .wide(wstr(stack))
                .wide(wstr(feature));
            self.crit_write(2, meta_error_full(), pay);
        }
        S_OK
    }

    fn page_fields(&self, page: &[u16]) -> (Prim, Prim, u64) {
        match self.pages.get(page) {
            Some(p) => (Prim::guid(&p.id), Prim::wref(p.uri.as_ptr()), p.tick),
            None => (Prim::empty_w(), Prim::new(3, 0, 0), u64::MAX),
        }
    }

    pub unsafe fn page_view(&mut self, name: *const u16, uri: *const u16, refuri: *const u16, tags: *const [*const u16; 2], feature: *const u16, custom: *const u16) -> i32 {
        if name.is_null() || uri.is_null() {
            return E_INVALIDARG;
        }
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        if tags.is_null() {
            return E_INVALIDARG;
        }
        let id = co_guid();
        let t = tick64();
        let mut u = wstr(uri).to_vec();
        u.push(0);
        self.pages.insert(wstr(name).to_vec(), Page { id, uri: u, tick: t });
        let none = wz("None");
        let ver = wz("gtl.pageview.1");
        let nm = if *name == 0 { none.as_ptr() } else { name };
        let tg = &*tags;
        let mut keep = Vec::new();
        let vals = Vals(vec![
            v_tag(self.tags[3]),
            v_base_type("PageViewData", &mut keep),
            v_bvar_map(
                "baseData",
                vec![
                    ("id", BVar::from_prim(Prim::guid(&id))),
                    ("name", BVar::from_prim(normalize(&Prim::wref(nm)))),
                    ("uri", BVar::from_prim(normalize(&Prim::wref(uri)))),
                    ("refUri", BVar::from_prim(normalize(&Prim::wref(refuri)))),
                    ("pageType", BVar::from_prim(normalize(&Prim::wref(tg[0])))),
                    ("productId", BVar::from_prim(normalize(&Prim::wref(tg[1])))),
                    ("ver", BVar::from_prim(normalize(&Prim::wref(ver.as_ptr())))),
                ],
            ),
            v_string("featureName", feature),
            v_string("customAttributes", custom),
        ]);
        let hr = self.write_event(&w("PageView"), self.privacy[3], 4, self.keywords[3] | KW_CV, &vals.0, 1, false);
        if hr < 0 {
            hr
        } else {
            S_OK
        }
    }

    unsafe fn content_map(target: *const u16, parent: *const u16, sn: u32, id: *const u16, cn: *const u16, cs: *const u16, ctags: *const u16) -> RawMap {
        prim_map_of(vec![
            ("parentName", normalize(&Prim::wref(parent))),
            ("sN", Prim::u32v(sn)),
            ("id", normalize(&Prim::wref(id))),
            ("cN", normalize(&Prim::wref(cn))),
            ("cS", normalize(&Prim::wref(cs))),
            ("targetItemId", normalize(&Prim::wref(target))),
            ("contentTags", normalize(&Prim::wref(ctags))),
        ])
    }

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn page_action(&mut self, page: *const u16, behavior: u32, target: *const u16, content: *const u8, feature: *const u16, custom: *const u16, counts: u32) -> i32 {
        let t = tick64();
        if page.is_null() {
            return E_INVALIDARG;
        }
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        if content.is_null() {
            return E_INVALIDARG;
        }
        if counts != 0 && self.running {
            if feature.is_null() {
                return E_INVALIDARG;
            }
            *self.features.entry(wstr(feature).to_vec()).or_insert(0) += 1;
        }
        let q = |o: usize| *(content.add(o) as *const u64);
        let cm = Self::content_map(target, q(0) as *const u16, q(8) as u32, q(0x10) as *const u16, q(0x18) as *const u16, q(0x20) as *const u16, q(0x30) as *const u16);
        let target_uri = q(0x28) as *const u16;
        let (id, uri, ptick) = self.page_fields(wstr(page));
        let tta = if ptick == u64::MAX { 0 } else { t.wrapping_sub(ptick) };
        let uri = normalize(&uri);
        self.page_action_seen = true;
        let none = wz("None");
        let ver = wz("gtl.pageaction.2");
        let nm = if *page == 0 { none.as_ptr() } else { page };
        let mut keep = Vec::new();
        let vals = Vals(vec![
            v_tag(self.tags[4]),
            v_base_type("PageActionData", &mut keep),
            v_bvar_map(
                "baseData",
                vec![
                    ("id", BVar::from_prim(normalize(&id))),
                    ("name", BVar::from_prim(normalize(&Prim::wref(nm)))),
                    ("uri", BVar::from_prim(uri)),
                    ("behavior", BVar::from_prim(Prim::i32v(behavior as i32))),
                    ("ver", BVar::from_prim(normalize(&Prim::wref(ver.as_ptr())))),
                    ("targetUri", BVar::from_prim(normalize(&Prim::wref(target_uri)))),
                    ("content", BVar::from_coll(Coll::new(2, raw_vec(&[cm])))),
                ],
            ),
            v_prim("timeToActionMs", Prim::u64v(tta)),
            v_string("featureName", feature),
            v_prim("countsAsFeatureUsage", Prim::i32v(counts as i32)),
            v_string("customAttributes", custom),
        ]);
        let hr = self.write_event(&w("PageAction"), self.privacy[4], 4, self.keywords[4] | KW_CV, &vals.0, 1, true);
        if hr < 0 {
            hr
        } else {
            S_OK
        }
    }

    pub unsafe fn content_update(&mut self, page: *const u16, items: *const u8, count: u64, feature: *const u16, custom: *const u16) -> i32 {
        if page.is_null() {
            return E_INVALIDARG;
        }
        if !self.registered {
            return GTL_E_NOT_REGISTERED;
        }
        if items.is_null() && count != 0 {
            return E_INVALIDARG;
        }
        let mut maps = Vec::new();
        for i in 0..count as usize {
            let b = items.add(i * 0x38);
            let q = |o: usize| *(b.add(o) as *const u64);
            maps.push(Self::content_map(q(0) as *const u16, q(8) as *const u16, q(0x10) as u32, q(0x18) as *const u16, q(0x20) as *const u16, q(0x28) as *const u16, q(0x30) as *const u16));
        }
        let (id, uri, _) = self.page_fields(wstr(page));
        let uri = normalize(&uri);
        let none = wz("None");
        let ver = wz("gtl.contentupdate.1");
        let nm = if *page == 0 { none.as_ptr() } else { page };
        let mut keep = Vec::new();
        let vals = Vals(vec![
            v_tag(self.tags[5]),
            v_base_type("ContentUpdateData", &mut keep),
            v_bvar_map(
                "baseData",
                vec![
                    ("id", BVar::from_prim(normalize(&id))),
                    ("name", BVar::from_prim(normalize(&Prim::wref(nm)))),
                    ("uri", BVar::from_prim(uri)),
                    ("content", BVar::from_coll(Coll::new(2, raw_vec(&maps)))),
                    ("ver", BVar::from_prim(normalize(&Prim::wref(ver.as_ptr())))),
                ],
            ),
            v_string("featureName", feature),
            v_string("customAttributes", custom),
        ]);
        let hr = self.write_event(&w("ContentUpdate"), self.privacy[5], 4, self.keywords[5] | KW_CV, &vals.0, 1, false);
        if hr < 0 {
            hr
        } else {
            S_OK
        }
    }
}
