use crate::abi::*;
use crate::sys::*;
use std::ffi::c_void;
use std::ptr::null_mut;

pub type Raw = *mut c_void;

pub const E_NOTIMPL: i32 = 0x80004001u32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000FFFFu32 as i32;

pub const IID_IMAP: Guid = Guid::new(0xc9d9a725, 0x786b, 0x5113, [0xb4, 0xb7, 0x9b, 0x61, 0x76, 0x4c, 0x22, 0x0b]);
pub const IID_ITER_KVP: Guid = Guid::new(0xdfabb6e1, 0x0411, 0x5a8f, [0xaa, 0x87, 0x35, 0x4e, 0x71, 0x10, 0xf0, 0x99]);
pub const IID_IVECTOR: Guid = Guid::new(0xd44662bc, 0xdce3, 0x59a8, [0x92, 0x72, 0x4b, 0x21, 0x0f, 0x33, 0x90, 0x8b]);
pub const IID_ITER_VAL: Guid = Guid::new(0xcb0492b6, 0x4113, 0x55cf, [0xb2, 0xc5, 0x99, 0xeb, 0x42, 0x8b, 0xa4, 0x93]);

unsafe fn slot(o: Raw, off: usize) -> usize {
    *(*(o as *const *const usize)).add(off / 8)
}

pub unsafe fn add_ref(o: Raw) {
    let f: unsafe extern "system" fn(Raw) -> u32 = std::mem::transmute(slot(o, 8));
    f(o);
}

pub unsafe fn release(o: Raw) {
    let f: unsafe extern "system" fn(Raw) -> u32 = std::mem::transmute(slot(o, 0x10));
    f(o);
}

unsafe fn qi(o: Raw, iid: &Guid) -> Result<Raw, i32> {
    let f: unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> i32 = std::mem::transmute(slot(o, 0));
    let mut out = null_mut();
    let hr = f(o, iid, &mut out);
    if hr < 0 {
        Err(hr)
    } else {
        Ok(out)
    }
}

unsafe fn call<T: Default>(o: Raw, off: usize) -> Result<T, i32> {
    let f: unsafe extern "system" fn(Raw, *mut T) -> i32 = std::mem::transmute(slot(o, off));
    let mut out = T::default();
    let hr = f(o, &mut out);
    if hr < 0 {
        Err(hr)
    } else {
        Ok(out)
    }
}

unsafe fn obj(o: Raw, off: usize) -> Result<Raw, i32> {
    let f: unsafe extern "system" fn(Raw, *mut Raw) -> i32 = std::mem::transmute(slot(o, off));
    let mut out = null_mut();
    let hr = f(o, &mut out);
    if hr < 0 {
        Err(hr)
    } else {
        Ok(out)
    }
}

struct Rel(Vec<Raw>, Vec<Raw>);

impl Rel {
    fn keep(&mut self, o: Raw) -> Raw {
        if !o.is_null() {
            self.0.push(o);
        }
        o
    }
    fn hs(&mut self, h: Raw) -> Raw {
        if !h.is_null() {
            self.1.push(h);
        }
        h
    }
}

impl Drop for Rel {
    fn drop(&mut self) {
        for o in self.0.iter().rev() {
            unsafe { release(*o) };
        }
        for h in &self.1 {
            unsafe { WindowsDeleteString(*h) };
        }
    }
}

unsafe fn hstr<'a>(h: Raw) -> &'a [u16] {
    if h.is_null() {
        return &[];
    }
    let mut n = 0u32;
    let p = WindowsGetStringRawBuffer(h, &mut n);
    if p.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(p, n as usize)
    }
}

unsafe fn prim_of(val: Raw, ty: i32) -> Result<Prim, i32> {
    match ty {
        0 => Err(E_UNEXPECTED),
        1 => Ok(Prim::u8v(call::<u8>(val, 0x50)?)),
        2 => Ok(Prim::f64v(call::<f64>(val, 0x48)?)),
        3 => {
            let h = obj(val, 0x40)?;
            let mut r = Rel(Vec::new(), Vec::new());
            r.hs(h);
            Ok(Prim::sp(3, Sp::wide(hstr(h))))
        }
        _ => Err(E_NOTIMPL),
    }
}

unsafe fn pairs(o: Raw, r: &mut Rel, mut each: impl FnMut(&[u16], Raw, &mut Rel) -> Result<(), i32>) -> Result<(), i32> {
    let map = r.keep(qi(o, &IID_IMAP)?);
    let view = r.keep(obj(map, 0x48)?);
    let it = r.keep(qi(view, &IID_ITER_KVP)?);
    let cur = r.keep(obj(it, 0x30)?);
    let mut has = call::<u8>(cur, 0x38)?;
    while has != 0 {
        let mut pr = Rel(Vec::new(), Vec::new());
        let pair = pr.keep(obj(cur, 0x30)?);
        let key = pr.hs(obj(pair, 0x30)?);
        let val = pr.keep(obj(pair, 0x38)?);
        each(hstr(key), val, &mut pr)?;
        drop(pr);
        has = call::<u8>(cur, 0x40)?;
    }
    Ok(())
}

unsafe fn array_maps(arr: Raw) -> Result<Vec<RawMap>, i32> {
    let mut r = Rel(Vec::new(), Vec::new());
    let mut maps: Vec<RawMap> = Vec::new();
    let res = (|| -> Result<(), i32> {
        let vec = r.keep(qi(arr, &IID_IVECTOR)?);
        let view = r.keep(obj(vec, 0x40)?);
        let it = r.keep(qi(view, &IID_ITER_VAL)?);
        let cur = r.keep(obj(it, 0x30)?);
        let mut has = call::<u8>(cur, 0x38)?;
        while has != 0 {
            let mut er = Rel(Vec::new(), Vec::new());
            let el = er.keep(obj(cur, 0x30)?);
            let o = er.keep(obj(el, 0x60)?);
            let mut m = RawMap::new(PRIM_NODE);
            let pr = pairs(o, &mut er, |k, v, _| {
                let ty = call::<i32>(v, 0x30)?;
                if let Ok(p) = prim_of(v, ty) {
                    prim_map_put(&mut m, Sp::wide(k), p, false);
                }
                Ok(())
            });
            if let Err(hr) = pr {
                prim_map_destroy(&m);
                return Err(hr);
            }
            if m.size != 0 {
                maps.push(m);
            } else {
                prim_map_destroy(&m);
            }
            drop(er);
            has = call::<u8>(cur, 0x40)?;
        }
        Ok(())
    })();
    match res {
        Ok(()) => Ok(maps),
        Err(hr) => {
            for m in &maps {
                prim_map_destroy(m);
            }
            Err(hr)
        }
    }
}

pub unsafe fn convert(json: Raw) -> Result<Vals, i32> {
    if json.is_null() {
        return Err(crate::tlg::E_INVALIDARG);
    }
    add_ref(json);
    let mut r = Rel(vec![json], Vec::new());
    let mut out = Vals(Vec::new());
    pairs(json, &mut r, |k, v, _| {
        let ty = call::<i32>(v, 0x30)?;
        if ty == 4 {
            let arr = obj(v, 0x58)?;
            let maps = array_maps(arr);
            if !arr.is_null() {
                release(arr);
            }
            let maps = maps?;
            out.0.insert(0, Value::with_bvar(Sp::wide(k), BVar::from_coll(Coll::new(2, raw_vec(&maps)))));
        } else if let Ok(p) = prim_of(v, ty) {
            out.0.insert(0, Value::with_bvar(Sp::wide(k), BVar::from_prim(p)));
        }
        Ok(())
    })?;
    Ok(out)
}
