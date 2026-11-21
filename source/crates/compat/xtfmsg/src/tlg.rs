use crate::sys::*;
use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Once;

pub const IN_UNICODESTRING: u8 = 1;
pub const IN_ANSISTRING: u8 = 2;
pub const IN_INT32: u8 = 7;
pub const IN_UINT32: u8 = 8;
pub const IN_UINT64: u8 = 10;
pub const IN_GUID: u8 = 15;
pub const PRIV_TAGS: u64 = 0x200_0000;
pub const MICROSOFT_TELEMETRY_GROUP: GUID = GUID::new(0x5ecb0bac, 0xb930, 0x47f5, [0xa8, 0xa4, 0xe8, 0x25, 0x35, 0x29, 0xed, 0xb7]);

pub struct Provider {
    pub guid: GUID,
    pub name: &'static str,
    pub level_plus1: AtomicU32,
    pub any: AtomicU64,
    pub all: AtomicU64,
    pub handle: AtomicU64,
    pub once: Once,
}

impl Provider {
    pub const fn new(guid: GUID, name: &'static str) -> Provider {
        Provider { guid, name, level_plus1: AtomicU32::new(0), any: AtomicU64::new(0), all: AtomicU64::new(0), handle: AtomicU64::new(0), once: Once::new() }
    }

    pub fn traits(&self) -> Vec<u8> {
        let mut v = vec![0u8, 0u8];
        v.extend_from_slice(self.name.as_bytes());
        v.push(0);
        v.extend_from_slice(&0x13u16.to_le_bytes());
        v.push(1);
        v.extend_from_slice(guid_bytes(&MICROSOFT_TELEMETRY_GROUP).as_slice());
        let n = v.len() as u16;
        v[0..2].copy_from_slice(&n.to_le_bytes());
        v
    }

    pub fn register(&'static self) {
        self.once.call_once(|| unsafe {
            let meta: &'static [u8] = Box::leak(self.traits().into_boxed_slice());
            let mut h = 0u64;
            if EventRegister(&self.guid, Some(enable_cb), self as *const Provider as *mut c_void, &mut h) == 0 {
                EventSetInformation(h, 2, meta.as_ptr() as *const c_void, meta.len() as u32);
                self.handle.store(h, Ordering::SeqCst);
            }
            META.with_provider(self, meta);
        });
    }

    pub fn enabled(&'static self, level: u8, keyword: u64) -> bool {
        self.register();
        if self.level_plus1.load(Ordering::SeqCst) <= level as u32 {
            return false;
        }
        if keyword == 0 {
            return true;
        }
        let any = self.any.load(Ordering::SeqCst);
        let all = self.all.load(Ordering::SeqCst);
        keyword & any != 0 && keyword & all == all
    }

    pub fn write(&'static self, ev: &Event, activity: *const GUID, related: *const GUID, data: &[EventDataDescriptor]) -> u32 {
        self.register();
        let pm = META.get(self);
        let em = ev.metadata();
        let desc = EventDescriptor { id: 0, version: 0, channel: 11, level: ev.level, opcode: ev.opcode, task: 0, keyword: ev.keyword };
        let mut d = Vec::with_capacity(data.len() + 2);
        d.push(EventDataDescriptor { ptr: pm.as_ptr() as u64, size: pm.len() as u32, reserved: 2 });
        d.push(EventDataDescriptor { ptr: em.as_ptr() as u64, size: em.len() as u32, reserved: 1 });
        d.extend_from_slice(data);
        unsafe { EventWriteTransfer(self.handle.load(Ordering::SeqCst), &desc, activity, related, d.len() as u32, d.as_ptr()) }
    }
}

struct MetaTable(std::sync::Mutex<Vec<(usize, &'static [u8])>>);

impl MetaTable {
    fn with_provider(&self, p: &'static Provider, m: &'static [u8]) {
        self.0.lock().unwrap().push((p as *const Provider as usize, m));
    }
    fn get(&self, p: &'static Provider) -> &'static [u8] {
        let k = p as *const Provider as usize;
        self.0.lock().unwrap().iter().find(|e| e.0 == k).map(|e| e.1).unwrap_or(&[])
    }
}

static META: MetaTable = MetaTable(std::sync::Mutex::new(Vec::new()));

unsafe extern "system" fn enable_cb(_src: *const GUID, is_enabled: u32, level: u8, any: u64, all: u64, _f: *const c_void, ctx: *mut c_void) {
    let p = &*(ctx as *const Provider);
    match is_enabled {
        0 => p.level_plus1.store(0, Ordering::SeqCst),
        1 => {
            p.level_plus1.store(if level != 0 { level as u32 + 1 } else { 256 }, Ordering::SeqCst);
            p.any.store(any, Ordering::SeqCst);
            p.all.store(all, Ordering::SeqCst);
        }
        _ => {}
    }
}

pub fn guid_bytes(g: &GUID) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&g.data1.to_le_bytes());
    b[4..6].copy_from_slice(&g.data2.to_le_bytes());
    b[6..8].copy_from_slice(&g.data3.to_le_bytes());
    b[8..16].copy_from_slice(&g.data4);
    b
}

pub struct Event {
    pub name: &'static str,
    pub level: u8,
    pub opcode: u8,
    pub keyword: u64,
    pub fields: &'static [(&'static str, u8)],
}

impl Event {
    pub fn metadata(&self) -> Vec<u8> {
        let mut v = vec![0u8, 0u8, 0u8];
        v.extend_from_slice(self.name.as_bytes());
        v.push(0);
        for (n, t) in self.fields {
            v.extend_from_slice(n.as_bytes());
            v.push(0);
            v.push(*t);
        }
        let n = v.len() as u16;
        v[0..2].copy_from_slice(&n.to_le_bytes());
        v
    }
}

pub fn d_u64(v: &u64) -> EventDataDescriptor {
    EventDataDescriptor { ptr: v as *const u64 as u64, size: 8, reserved: 0 }
}

pub fn d_u32(v: &u32) -> EventDataDescriptor {
    EventDataDescriptor { ptr: v as *const u32 as u64, size: 4, reserved: 0 }
}

pub fn d_guid(v: &GUID) -> EventDataDescriptor {
    EventDataDescriptor { ptr: v as *const GUID as u64, size: 16, reserved: 0 }
}

static EMPTY_A: u8 = 0;
static EMPTY_W: u16 = 0;

pub unsafe fn d_astr(p: *const u8) -> EventDataDescriptor {
    if p.is_null() {
        return EventDataDescriptor { ptr: &EMPTY_A as *const u8 as u64, size: 1, reserved: 0 };
    }
    EventDataDescriptor { ptr: p as u64, size: strlen(p) as u32 + 1, reserved: 0 }
}

pub unsafe fn d_wstr(p: *const u16) -> EventDataDescriptor {
    if p.is_null() {
        return EventDataDescriptor { ptr: &EMPTY_W as *const u16 as u64, size: 2, reserved: 0 };
    }
    EventDataDescriptor { ptr: p as u64, size: (wcslen(p) as u32 + 1) * 2, reserved: 0 }
}

pub static CORE_DEV_TOOLS_TLG: Provider = Provider::new(GUID::new(0x9e917965, 0xc241, 0x446c, [0x85, 0x21, 0x18, 0x97, 0x0e, 0x1b, 0xf3, 0x0c]), "Xbox.CoreDevTools");
pub static CORE_DEV_TOOLS: Provider = Provider::new(GUID::new(0x16a78bae, 0xc464, 0x432f, [0xba, 0xb2, 0x7b, 0x18, 0x62, 0xc5, 0xfa, 0x7a]), "CoreDevTools");

pub static HRESULT_FAIL: Event = Event { name: "HResult", level: 0, opcode: 0, keyword: 0, fields: &[("PartA_PrivTags", IN_UINT64), ("traceInfo", IN_ANSISTRING), ("name", IN_ANSISTRING), ("hResult", IN_UINT32), ("message", IN_ANSISTRING)] };
pub static HRESULT_OK: Event = Event { name: "HResult", level: 5, opcode: 0, keyword: 0, fields: &[("PartA_PrivTags", IN_UINT64), ("traceInfo", IN_ANSISTRING), ("name", IN_ANSISTRING)] };
pub static ACTIVITY_START: Event = Event { name: "ActivityStart", level: 0, opcode: 1, keyword: 0, fields: &[("PartA_PrivTags", IN_UINT64), ("toolsCorrelation", IN_GUID), ("traceInfo", IN_ANSISTRING)] };
pub static ACTIVITY_END: Event = Event { name: "ActivityEnd", level: 0, opcode: 2, keyword: 0, fields: &[("PartA_PrivTags", IN_UINT64), ("toolsCorrelation", IN_GUID)] };
pub static OPERATION_RESULT: Event = Event { name: "XtfOperationResult", level: 0, opcode: 0, keyword: 0x0000_8000_0000_0000, fields: &[("PartA_PrivTags", IN_UINT64), ("ExtensionVersion", IN_UNICODESTRING), ("ExtensionId", IN_GUID), ("SessionId", IN_GUID), ("OperationId", IN_UINT32), ("TaskName", IN_UNICODESTRING), ("TaskValue", IN_UNICODESTRING), ("HRESULT", IN_INT32)] };
