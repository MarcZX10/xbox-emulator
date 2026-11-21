use crate::livemerge::*;
use crate::ntpath::NtPathMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::Mutex;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> usize;
    fn GetCurrentProcessId() -> u32;
    fn QueryDosDeviceW(name: *const u16, buf: *mut u16, n: u32) -> u32;
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
}

#[derive(Clone, Debug)]
pub struct Captured {
    pub header: EventHeader,
    pub user_data_length: u16,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub base: u64,
    pub size: u64,
    pub pid: u32,
    pub name: String,
    pub hr: i32,
}

#[repr(C)]
pub struct TestCb {
    pub base: ICallback,
    pub refs: AtomicU32,
    pub events: Mutex<Vec<Captured>>,
    pub failures: Mutex<Vec<Failure>>,
    pub event_hr: AtomicI32,
    pub failure_hr: AtomicI32,
}

unsafe extern "system" fn t_qi(_: *mut ICallback, _: *const Guid, ppv: *mut *mut c_void) -> i32 {
    *ppv = core::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn t_add_ref(c: *mut ICallback) -> u32 {
    (*(c as *mut TestCb)).refs.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn t_release(c: *mut ICallback) -> u32 {
    (*(c as *mut TestCb)).refs.fetch_sub(1, Ordering::SeqCst) - 1
}

unsafe extern "system" fn t_on_event(c: *mut ICallback, r: *const EventRecord) -> i32 {
    let t = &*(c as *mut TestCb);
    let r = &*r;
    let payload = if r.user_data.is_null() { Vec::new() } else { core::slice::from_raw_parts(r.user_data as *const u8, r.user_data_length as usize).to_vec() };
    t.events.lock().unwrap().push(Captured { header: r.header, user_data_length: r.user_data_length, payload });
    t.event_hr.load(Ordering::SeqCst)
}

unsafe extern "system" fn t_on_failed(c: *mut ICallback, base: u64, size: u64, pid: u32, name: *const u16, hr: i32) -> i32 {
    let t = &*(c as *mut TestCb);
    let mut n = 0;
    while *name.add(n) != 0 {
        n += 1;
    }
    let name = String::from_utf16(core::slice::from_raw_parts(name, n)).unwrap();
    t.failures.lock().unwrap().push(Failure { base, size, pid, name, hr });
    t.failure_hr.load(Ordering::SeqCst)
}

static TEST_VTBL: ICallbackVtbl = ICallbackVtbl { query_interface: t_qi, add_ref: t_add_ref, release: t_release, on_event: t_on_event, on_image_lookup_failed: t_on_failed };

pub fn new_cb() -> *mut TestCb {
    Box::into_raw(Box::new(TestCb {
        base: ICallback { vtbl: &TEST_VTBL },
        refs: AtomicU32::new(1),
        events: Mutex::new(Vec::new()),
        failures: Mutex::new(Vec::new()),
        event_hr: AtomicI32::new(0),
        failure_hr: AtomicI32::new(0),
    }))
}

pub unsafe fn events(cb: *mut TestCb) -> Vec<Captured> {
    (*cb).events.lock().unwrap().clone()
}

pub unsafe fn free_cb(cb: *mut TestCb) {
    assert_eq!((*cb).refs.load(Ordering::SeqCst), 0);
    drop(Box::from_raw(cb));
}

pub fn cfg(include_kernel: u8, all: u8, pid: u32) -> LiveMergerConfig {
    LiveMergerConfig { include_kernel, all_processes: all, pad: [0; 2], process_id: pid }
}

pub unsafe fn create(c: LiveMergerConfig, cb: *mut TestCb) -> *mut LiveMerger {
    let mut p: *mut c_void = core::ptr::null_mut();
    assert_eq!(CreateLiveMerger(cb as *mut ICallback, &c, &IID_ILIVEMERGER, &mut p), S_OK);
    p as *mut LiveMerger
}

pub unsafe fn process(m: *mut LiveMerger, r: &EventRecord) -> i32 {
    ((*(*m).vtbl).process_event)(m, r)
}

pub unsafe fn release(m: *mut LiveMerger) -> u32 {
    ((*(*m).vtbl).release)(m)
}

pub unsafe fn stats(m: *mut LiveMerger) -> [u32; 3] {
    let mut s = [0u32; 3];
    assert_eq!(((*(*m).vtbl).get_stats)(m, s.as_mut_ptr()), S_OK);
    s
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn image_payload(base: u64, size: u64, pid: u32, tds: u32, name: &str) -> Vec<u8> {
    let mut p = vec![0u8; 0x38];
    p[0..8].copy_from_slice(&base.to_le_bytes());
    p[8..16].copy_from_slice(&size.to_le_bytes());
    p[0x10..0x14].copy_from_slice(&pid.to_le_bytes());
    p[0x18..0x1c].copy_from_slice(&tds.to_le_bytes());
    for c in w(name) {
        p.extend_from_slice(&c.to_le_bytes());
    }
    p.extend_from_slice(&[0, 0]);
    p
}

pub fn record(provider: Guid, opcode: u8, version: u8, payload: &mut Vec<u8>) -> EventRecord {
    let mut r = EventRecord::zeroed();
    r.header.provider_id = provider;
    r.header.descriptor.opcode = opcode;
    r.header.descriptor.version = version;
    r.header.thread_id = 0x1234;
    r.header.process_id = 0x5678;
    r.header.time_stamp = 0x0123_4567_89ab_cdef;
    r.header.kernel_time = 7;
    r.header.user_time = 9;
    r.user_data_length = payload.len() as u16;
    r.user_data = payload.as_mut_ptr() as *mut c_void;
    r
}

#[derive(Debug)]
pub struct PeFacts {
    pub size_of_image: u32,
    pub tds: u32,
    pub guid: Guid,
    pub age: u32,
    pub pdb: Vec<u8>,
}

fn u32at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u16at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

pub fn pe_facts(b: &[u8]) -> PeFacts {
    let e = u32at(b, 0x3c) as usize;
    let nsec = u16at(b, e + 6) as usize;
    let optsz = u16at(b, e + 20) as usize;
    let opt = e + 24;
    let mut f = PeFacts { size_of_image: u32at(b, opt + 56), tds: u32at(b, e + 8), guid: [0; 16], age: 0, pdb: Vec::new() };
    let dsize = u32at(b, opt + 112 + 6 * 8 + 4) as usize;
    for i in 0..dsize / 28 {
        let rva = u32at(b, opt + 112 + 6 * 8) as usize + i * 28;
        let mut off = None;
        for s in 0..nsec {
            let sh = opt + optsz + s * 40;
            let va = u32at(b, sh + 12) as usize;
            let vs = u32at(b, sh + 8).max(u32at(b, sh + 16)) as usize;
            if va <= rva && rva < va + vs {
                off = Some(u32at(b, sh + 20) as usize + rva - va);
            }
        }
        let d = off.unwrap();
        if u32at(b, d + 12) != 2 {
            continue;
        }
        let p = u32at(b, d + 24) as usize;
        let n = u32at(b, d + 16) as usize;
        let data = &b[p..p + n];
        if &data[0..4] != b"RSDS" {
            continue;
        }
        f.guid.copy_from_slice(&data[4..20]);
        f.age = u32at(data, 20);
        f.pdb = data[24..].iter().take_while(|c| **c != 0).copied().collect();
        return f;
    }
    panic!("no RSDS");
}

pub fn system_dir() -> String {
    let mut b = [0u16; 300];
    let n = unsafe { GetSystemDirectoryW(b.as_mut_ptr(), 300) } as usize;
    String::from_utf16(&b[..n]).unwrap()
}

pub fn nt_path_of(dos: &str) -> String {
    let drive: Vec<u16> = w(&dos[..2]).into_iter().chain([0]).collect();
    let mut b = [0u16; 300];
    assert!(unsafe { QueryDosDeviceW(drive.as_ptr(), b.as_mut_ptr(), 300) } != 0);
    let n = b.iter().position(|c| *c == 0).unwrap();
    format!("{}{}", String::from_utf16(&b[..n]).unwrap(), &dos[2..])
}

pub fn check_image_pair(evs: &[Captured], src: &EventRecord, base: u64, size: u64, pid: u32, name: &str, f: &PeFacts) {
    assert_eq!(evs.len(), 2);
    let a = &evs[0];
    assert_eq!(a.header.provider_id, IMAGE_ID_GUID);
    assert_eq!(a.header.descriptor.opcode, OPCODE_IMAGE_ID);
    assert_eq!(a.header.descriptor.version, 2);
    assert_eq!(a.header.time_stamp, src.header.time_stamp);
    assert_eq!(a.header.thread_id, src.header.thread_id);
    assert_eq!(a.header.process_id, src.header.process_id);
    assert_eq!(a.header.kernel_time, src.header.kernel_time);
    assert_eq!(a.header.user_time, src.header.user_time);
    let wn = w(name);
    assert_eq!(a.user_data_length as usize, wn.len() * 2 + 0x1a);
    assert_eq!(a.header.size, a.user_data_length + 0x70);
    assert_eq!(u64::from_le_bytes(a.payload[0..8].try_into().unwrap()), base);
    assert_eq!(u64::from_le_bytes(a.payload[8..16].try_into().unwrap()), size);
    assert_eq!(u32at(&a.payload, 0x10), pid);
    assert_eq!(u32at(&a.payload, 0x14), f.tds);
    let got: Vec<u16> = a.payload[0x18..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    assert_eq!(&got[..wn.len()], &wn[..]);
    assert_eq!(got[wn.len()], 0);
    let d = &evs[1];
    assert_eq!(d.header.provider_id, IMAGE_ID_GUID);
    assert_eq!(d.header.descriptor.opcode, OPCODE_DBGID_RSDS);
    assert_eq!(d.header.descriptor.version, 2);
    assert_eq!(d.user_data_length as usize, f.pdb.len() + 0x21);
    assert_eq!(u64::from_le_bytes(d.payload[0..8].try_into().unwrap()), base);
    assert_eq!(u32at(&d.payload, 8), pid);
    assert_eq!(&d.payload[0xc..0x1c], &f.guid);
    assert_eq!(u32at(&d.payload, 0x1c), f.age);
    assert_eq!(&d.payload[0x20..0x20 + f.pdb.len()], &f.pdb[..]);
    assert_eq!(d.payload[0x20 + f.pdb.len()], 0);
}

#[test]
fn create_validates_arguments_like_console() {
    unsafe {
        let cb = new_cb();
        let c = cfg(0, 0, 5);
        let mut sentinel = 0x1111usize as *mut c_void;
        assert_eq!(CreateLiveMerger(cb as *mut ICallback, core::ptr::null(), &IID_ILIVEMERGER, &mut sentinel), E_POINTER);
        assert_eq!(sentinel as usize, 0x1111);
        assert_eq!(CreateLiveMerger(cb as *mut ICallback, &c, &IID_ILIVEMERGER, core::ptr::null_mut()), E_POINTER);
        let bad = cfg(1, 0, u32::MAX);
        assert_eq!(CreateLiveMerger(cb as *mut ICallback, &bad, &IID_ILIVEMERGER, &mut sentinel), E_INVALIDARG);
        assert_eq!(sentinel as usize, 0x1111);
        assert_eq!(CreateLiveMerger(core::ptr::null_mut(), &c, &IID_ILIVEMERGER, &mut sentinel), E_POINTER);
        assert_eq!((*cb).refs.load(Ordering::SeqCst), 1);
        let other = guid(1, 2, 3, [4; 8]);
        assert_eq!(CreateLiveMerger(cb as *mut ICallback, &c, &other, &mut sentinel), E_NOINTERFACE);
        assert!(sentinel.is_null());
        free_cb(cb);
    }
}

#[test]
fn merger_consumes_callback_reference_and_supports_iunknown() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, u32::MAX), cb);
        assert_eq!((*cb).refs.load(Ordering::SeqCst), 3);
        let mut u: *mut c_void = core::ptr::null_mut();
        assert_eq!(((*(*m).vtbl).query_interface)(m, &IID_IUNKNOWN, &mut u), S_OK);
        assert_eq!(u as usize, m as usize);
        assert_eq!(release(m), 1);
        assert_eq!(release(m), 0);
        free_cb(cb);
    }
}

pub fn exe_name() -> String {
    std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap().to_string()
}

pub fn exe_facts() -> PeFacts {
    pe_facts(&std::fs::read(std::env::current_exe().unwrap()).unwrap())
}

#[test]
fn own_process_image_is_read_from_loaded_module_and_cached() {
    let _g = crate::test_lock();
    unsafe {
        let cb = new_cb();
        let pid = GetCurrentProcessId();
        let m = create(cfg(0, 0, pid), cb);
        let base = GetModuleHandleW(core::ptr::null()) as u64;
        let f = exe_facts();
        let name = exe_name();
        let fake = format!("\\Device\\HarddiskVolume77\\nowhere\\{}", name);
        let size = f.size_of_image as u64;
        let mut p = image_payload(base, size, pid, 0xdead, &fake);
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        check_image_pair(&events(cb), &r, base, size, pid, &name, &f);
        assert_eq!(stats(m), [0, 1, 0]);
        (*cb).events.lock().unwrap().clear();
        let mut p3 = image_payload(base, size, pid, 0, &fake);
        let r3 = record(IMAGE_LOAD_GUID, 3, 3, &mut p3);
        assert_eq!(process(m, &r3), S_OK);
        check_image_pair(&events(cb), &r3, base, size, pid, &name, &f);
        assert_eq!(stats(m), [1, 1, 0]);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn cache_flag_off_forces_lookup_every_time() {
    let _g = crate::test_lock();
    unsafe {
        crate::SetImageFileNameCacheUsage(false);
        let cb = new_cb();
        let pid = GetCurrentProcessId();
        let m = create(cfg(0, 0, pid), cb);
        let base = GetModuleHandleW(core::ptr::null()) as u64;
        let f = exe_facts();
        let name = exe_name();
        let fake = format!("\\Device\\HarddiskVolume77\\nowhere\\{}", name);
        let size = f.size_of_image as u64;
        for _ in 0..3 {
            (*cb).events.lock().unwrap().clear();
            let mut p = image_payload(base, size, pid, 0, &fake);
            let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
            assert_eq!(process(m, &r), S_OK);
            check_image_pair(&events(cb), &r, base, size, pid, &name, &f);
        }
        assert_eq!(stats(m), [0, 3, 0]);
        crate::SetImageFileNameCacheUsage(true);
        (*cb).events.lock().unwrap().clear();
        let mut p = image_payload(base, size, pid, 0, &fake);
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        assert_eq!(stats(m), [1, 3, 0]);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn own_process_module_mismatch_falls_back_to_file() {
    let _g = crate::test_lock();
    unsafe {
        let cb = new_cb();
        let pid = GetCurrentProcessId();
        let m = create(cfg(0, 0, pid), cb);
        let base = GetModuleHandleW(core::ptr::null()) as u64;
        let f = exe_facts();
        let name = exe_name();
        let exe = std::env::current_exe().unwrap();
        let nt = nt_path_of(exe.to_str().unwrap());
        let size = f.size_of_image as u64;
        let cases = [(base, size + 0x1000, size + 0x1000), (base + 0x10000, size, size), (base, 0, size)];
        for (i, (b, s, want)) in cases.iter().enumerate() {
            let cbi = new_cb();
            let mi = create(cfg(0, 0, pid), cbi);
            let mut p = image_payload(*b, *s, pid, i as u32, &nt);
            let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
            assert_eq!(process(mi, &r), S_OK);
            check_image_pair(&events(cbi), &r, *b, *want, pid, &name, &f);
            assert_eq!(stats(mi), [0, 0, 1]);
            release(mi);
            free_cb(cbi);
        }
        (*cb).events.lock().unwrap().clear();
        let fake = format!("\\Device\\HarddiskVolume77\\nowhere\\{}", name);
        let mut p = image_payload(base, size + 0x2000, pid, 0, &fake);
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        assert!(events(cb).is_empty());
        let fl = (*cb).failures.lock().unwrap().clone();
        assert_eq!(fl.len(), 1);
        assert_eq!(fl[0].size, size + 0x2000);
        assert!(fl[0].hr < 0);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn cache_hit_reuses_cached_size_and_stamp_keyed_by_name() {
    let _g = crate::test_lock();
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 1), cb);
        let dos = format!("{}\\kernel32.dll", system_dir());
        let f = pe_facts(&std::fs::read(&dos).unwrap());
        let nt = nt_path_of(&dos);
        let mut p = image_payload(0x7ff8_0000_0000, 0x5000, 7, 0, &nt);
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        check_image_pair(&events(cb), &r, 0x7ff8_0000_0000, 0x5000, 7, "kernel32.dll", &f);
        (*cb).events.lock().unwrap().clear();
        let mut p2 = image_payload(0x7ff9_0000_0000, 0x9000, 8, 0x1234, &nt);
        let r2 = record(IMAGE_LOAD_GUID, 10, 2, &mut p2);
        assert_eq!(process(m, &r2), S_OK);
        check_image_pair(&events(cb), &r2, 0x7ff9_0000_0000, 0x5000, 8, "kernel32.dll", &f);
        assert_eq!(stats(m), [1, 0, 1]);
        crate::SetImageFileNameCacheUsage(false);
        (*cb).events.lock().unwrap().clear();
        assert_eq!(process(m, &r2), S_OK);
        check_image_pair(&events(cb), &r2, 0x7ff9_0000_0000, 0x9000, 8, "kernel32.dll", &f);
        assert_eq!(stats(m), [1, 0, 2]);
        crate::SetImageFileNameCacheUsage(true);
        (*cb).events.lock().unwrap().clear();
        assert_eq!(process(m, &r2), S_OK);
        check_image_pair(&events(cb), &r2, 0x7ff9_0000_0000, 0x5000, 8, "kernel32.dll", &f);
        assert_eq!(stats(m), [2, 0, 2]);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn open_loaded_module_checks_base_and_size() {
    unsafe {
        let base = GetModuleHandleW(core::ptr::null()) as u64;
        let f = exe_facts();
        let name = w(&format!("x\\y\\{}", exe_name()));
        assert!(open_loaded_module(&name, base, f.size_of_image as u64).is_ok());
        assert_eq!(open_loaded_module(&name, base, f.size_of_image as u64 + 1).err(), Some(HR_MOD_NOT_FOUND));
        assert_eq!(open_loaded_module(&name, base + 1, f.size_of_image as u64).err(), Some(HR_MOD_NOT_FOUND));
        assert_eq!(open_loaded_module(&w("C:\\z\\plm_not_loaded_9917.dll"), base, 1).err(), Some(HR_MOD_NOT_FOUND));
        let k32 = w("kernel32.dll");
        let kb = GetModuleHandleW(w("kernel32.dll\0").as_ptr()) as u64;
        let kf = pe_facts(&std::fs::read(format!("{}\\kernel32.dll", system_dir())).unwrap());
        let mut pe = open_loaded_module(&k32, kb, kf.size_of_image as u64).unwrap();
        let mut d = DebugInfo::default();
        assert_eq!(pe.debug_info(&mut d), 0);
        assert_eq!(d.guid, kf.guid);
        assert_eq!(d.age, kf.age);
    }
}

#[test]
fn foreign_process_image_is_read_from_nt_device_path() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 1), cb);
        let dos = format!("{}\\kernel32.dll", system_dir());
        let f = pe_facts(&std::fs::read(&dos).unwrap());
        let nt = nt_path_of(&dos);
        let mut p = image_payload(0x7ff8_0000_0000, 0x1_0000_5000, 4242, 0, &nt);
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        check_image_pair(&events(cb), &r, 0x7ff8_0000_0000, 0x5000, 4242, "kernel32.dll", &f);
        assert_eq!(stats(m), [0, 0, 1]);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn systemroot_and_dos_device_prefixes_open_the_same_file() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 1), cb);
        let dos = format!("{}\\ntdll.dll", system_dir());
        let f = pe_facts(&std::fs::read(&dos).unwrap());
        for (i, path) in [format!("\\SystemRoot\\System32\\ntdll.dll"), format!("\\??\\{}", dos)].iter().enumerate() {
            (*cb).events.lock().unwrap().clear();
            let mut p = image_payload(0x1000 * (i as u64 + 1), 0, 99, 0, path);
            let r = record(IMAGE_LOAD_GUID, 4, 2, &mut p);
            assert_eq!(process(m, &r), S_OK, "{path}");
            check_image_pair(&events(cb), &r, 0x1000 * (i as u64 + 1), f.size_of_image as u64, 99, "ntdll.dll", &f);
        }
        assert_eq!(stats(m), [0, 0, 2]);
        (*cb).events.lock().unwrap().clear();
        let mut pg = image_payload(0x9000, 0, 99, 0, &format!("\\GLOBAL??\\{}", dos));
        let rg = record(IMAGE_LOAD_GUID, 4, 2, &mut pg);
        assert_eq!(process(m, &rg), S_OK);
        assert!(events(cb).is_empty());
        let fl = (*cb).failures.lock().unwrap().clone();
        assert_eq!(fl.len(), 1);
        assert!(fl[0].hr < 0);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn process_filter_and_opcode_filter() {
    let _g = crate::test_lock();
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 0, 4242), cb);
        let dos = format!("{}\\kernel32.dll", system_dir());
        for (pid, op, ver) in [(0u32, 10u8, 2u8), (17, 10, 2), (4242, 5, 2), (4242, 0x24, 2), (4242, 0x26, 2)] {
            let mut p = image_payload(0x2000, 0, pid, 0, &dos);
            let r = record(IMAGE_LOAD_GUID, op, ver, &mut p);
            assert_eq!(process(m, &r), S_OK);
        }
        assert!(events(cb).is_empty());
        let mut pv = image_payload(0x2000, 0, 4242, 0, &dos);
        let rv = record(IMAGE_LOAD_GUID, 2, 1, &mut pv);
        assert_eq!(process(m, &rv), E_UNEXPECTED);
        let mut pz = image_payload(0x2000, 0, 0, 0, &dos);
        let rz = record(IMAGE_LOAD_GUID, 2, 1, &mut pz);
        assert_eq!(process(m, &rz), E_UNEXPECTED);
        for op in [1u8, 2, 3, 4, 10, 37] {
            let mut p = image_payload(0x2000, 0, 4242, 0, &dos);
            let r = record(IMAGE_LOAD_GUID, op, 2, &mut p);
            assert_eq!(process(m, &r), S_OK);
        }
        assert_eq!(events(cb).len(), 12);
        assert_eq!(stats(m), [5, 0, 1]);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn kernel_images_require_include_kernel_and_fall_back_to_file() {
    let _g = crate::test_lock();
    unsafe {
        let cb = new_cb();
        let m = create(cfg(1, 0, 4242), cb);
        let dos = format!("{}\\ntdll.dll", system_dir());
        let f = pe_facts(&std::fs::read(&dos).unwrap());
        crate::ResetVbiRecordsSearched();
        let mut p = image_payload(0xfffff800_00000000, 0, 0, 0, &dos);
        let r = record(IMAGE_LOAD_GUID, 3, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        assert!(crate::GetVbiRecordsSearched());
        check_image_pair(&events(cb), &r, 0xfffff800_00000000, f.size_of_image as u64, 0, "ntdll.dll", &f);
        assert_eq!(stats(m), [0, 0, 1]);
        release(m);
        let cb2 = new_cb();
        let m2 = create(cfg(0, 0, 4242), cb2);
        let mut p2 = image_payload(0xfffff800_00000000, 0, 0, 0, &dos);
        let r2 = record(IMAGE_LOAD_GUID, 3, 2, &mut p2);
        assert_eq!(process(m2, &r2), S_OK);
        assert!(events(cb2).is_empty());
        assert_eq!(stats(m2), [0, 0, 0]);
        release(m2);
        free_cb(cb);
        free_cb(cb2);
    }
}

#[test]
fn kernel_image_resolved_from_vbi_record() {
    let _g = crate::test_lock();
    unsafe {
        let mut probe = [0u8; 0x38];
        let _ = crate::PdbInfoFromVBI(w("probe\0").as_ptr(), probe.as_mut_ptr());
        assert!(crate::GetVbiRecordsSearched());
        let guid: Guid = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xf0, 0x01];
        let pdb = b"plm_vbi_unique_7731.pdb".to_vec();
        let before = crate::global_records(|v| {
            let n = v.len();
            v.push(crate::msvc::Record { name: crate::msvc::WString::from_slice(&w("plm_vbi_unique_7731.sys")), guid, age: 5, pad: 0, pdb: crate::msvc::AString::copy_of(&pdb) });
            n
        });
        let cb = new_cb();
        let m = create(cfg(1, 0, 4242), cb);
        let mut p = image_payload(0xfffff806_1234_0000, 0x1_0002_3000, 0, 0x5eed_1234, "\\SystemRoot\\System32\\drivers\\plm_vbi_unique_7731.sys");
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        let f = PeFacts { size_of_image: 0, tds: 0x5eed_1234, guid, age: 5, pdb };
        check_image_pair(&events(cb), &r, 0xfffff806_1234_0000, 0x2_3000, 0, "plm_vbi_unique_7731.sys", &f);
        assert_eq!(stats(m), [0, 0, 0]);
        release(m);
        free_cb(cb);
        crate::global_records(|v| {
            for rec in &mut v.as_mut_slice()[before..] {
                rec.name.drop_in_place();
                rec.pdb.drop_in_place();
            }
            v.last = v.first.add(before);
        });
    }
}

#[test]
fn lookup_failures_go_to_callback_and_its_result_is_returned() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 0), cb);
        let mut p = image_payload(0x5000, 0x77, 31, 0, "C:\\definitely\\not\\here\\missing.dll");
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), S_OK);
        let fl = (*cb).failures.lock().unwrap().clone();
        assert_eq!(fl, vec![Failure { base: 0x5000, size: 0x77, pid: 31, name: "C:\\definitely\\not\\here\\missing.dll".into(), hr: 0x8007_0003u32 as i32 }]);
        (*cb).failure_hr.store(0x8000_4005u32 as i32, Ordering::SeqCst);
        assert_eq!(process(m, &r), 0x8000_4005u32 as i32);
        assert!(events(cb).is_empty());
        let tmp = std::env::temp_dir().join(format!("plm_notpe_{}.dll", GetCurrentProcessId()));
        let mut notpe = vec![b'M'; 0x200];
        notpe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        std::fs::write(&tmp, &notpe).unwrap();
        (*cb).failure_hr.store(0, Ordering::SeqCst);
        let mut p2 = image_payload(0x6000, 0, 31, 0, tmp.to_str().unwrap());
        let r2 = record(IMAGE_LOAD_GUID, 10, 2, &mut p2);
        assert_eq!(process(m, &r2), S_OK);
        let mut nodbg = vec![0u8; 0x400];
        nodbg[0] = b'M';
        nodbg[1] = b'Z';
        nodbg[0x3c] = 0x80;
        nodbg[0x80..0x84].copy_from_slice(b"PE\0\0");
        nodbg[0x80 + 4 + 0x10] = 0xf0;
        let tmp2 = std::env::temp_dir().join(format!("plm_nodbg_{}.dll", GetCurrentProcessId()));
        std::fs::write(&tmp2, &nodbg).unwrap();
        let mut p3 = image_payload(0x7000, 0, 31, 0, tmp2.to_str().unwrap());
        let r3 = record(IMAGE_LOAD_GUID, 10, 2, &mut p3);
        assert_eq!(process(m, &r3), S_OK);
        let fl = (*cb).failures.lock().unwrap().clone();
        assert_eq!(fl[2].hr, E_INVALIDARG);
        assert_eq!(fl[3].hr, HR_NOT_FOUND);
        assert_eq!(stats(m), [0, 0, 0]);
        std::fs::remove_file(tmp).unwrap();
        std::fs::remove_file(tmp2).unwrap();
        release(m);
        free_cb(cb);
    }
}

#[test]
fn send_failure_from_callback_is_propagated() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 0), cb);
        (*cb).event_hr.store(0x8007_000eu32 as i32, Ordering::SeqCst);
        let dos = format!("{}\\kernel32.dll", system_dir());
        let mut p = image_payload(0x5000, 0, 3, 0, &dos);
        let r = record(IMAGE_LOAD_GUID, 10, 2, &mut p);
        assert_eq!(process(m, &r), 0x8007_000eu32 as i32);
        assert_eq!(events(cb).len(), 1);
        release(m);
        free_cb(cb);
    }
}

pub fn wstrings(b: &[u8]) -> Vec<String> {
    let v: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for c in v {
        if c == 0 {
            out.push(String::from_utf16(&cur).unwrap());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    assert!(cur.is_empty());
    out
}

pub fn check_device_map(evs: &[Captured], ts: i64) {
    let map = NtPathMap::build().unwrap();
    let s = |v: &Vec<u16>| String::from_utf16(v).unwrap();
    assert_eq!(evs.len(), 1 + map.unmounted.len() + map.entries.len());
    for e in evs {
        assert_eq!(e.header.provider_id, VOLUME_MAP_GUID);
        assert_eq!(e.header.time_stamp, ts);
        assert_eq!(e.header.descriptor.version, 0);
        assert_eq!(e.header.size, e.user_data_length + 0x70);
        assert_eq!(e.header.thread_id, 0);
    }
    assert_eq!(evs[0].header.descriptor.opcode, OPCODE_SYSTEM_PATHS);
    assert_eq!(wstrings(&evs[0].payload), vec![s(&map.sysdir), s(&map.windir)]);
    let mut i = 1;
    for u in &map.unmounted {
        assert_eq!(evs[i].header.descriptor.opcode, OPCODE_UNMOUNTED_VOLUME);
        assert_eq!(wstrings(&evs[i].payload), vec![s(u)]);
        i += 1;
    }
    for (p, r) in &map.entries {
        assert_eq!(evs[i].header.descriptor.opcode, OPCODE_VOLUME_MAPPING);
        assert_eq!(wstrings(&evs[i].payload), vec![s(p), s(r)]);
        i += 1;
    }
    assert_eq!(wstrings(&evs[1 + map.unmounted.len()].payload), vec!["\\??\\".to_string(), String::new()]);
}

#[test]
fn first_file_io_event_emits_device_map_once() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 0), cb);
        let mut empty = Vec::new();
        let other = record(guid(9, 9, 9, [9; 8]), 0, 0, &mut empty);
        assert_eq!(process(m, &other), S_OK);
        let ip = record(IMAGE_LOAD_GUID, 7, 2, &mut empty);
        assert_eq!(process(m, &ip), S_OK);
        assert!(events(cb).is_empty());
        let mut fio = record(FILE_IO_GUID, 64, 3, &mut empty);
        fio.header.time_stamp = 0x55aa;
        assert_eq!(process(m, &fio), S_OK);
        check_device_map(&events(cb), 0x55aa);
        let n = events(cb).len();
        fio.header.time_stamp = 0x66bb;
        assert_eq!(process(m, &fio), S_OK);
        assert_eq!(events(cb).len(), n);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn fixed_and_string_events_and_null_checks() {
    unsafe {
        let cb = new_cb();
        let m = create(cfg(0, 1, 0), cb);
        let mut h = EventHeader::default();
        h.thread_id = 11;
        h.process_id = 22;
        h.time_stamp = 33;
        h.kernel_time = 44;
        h.user_time = 55;
        h.descriptor.version = 9;
        let data = [0xabu8; 0x20];
        assert_eq!(((*(*m).vtbl).send_fixed_event)(m, &h, data.as_ptr() as *const c_void), S_OK);
        let s = w("hello\0");
        assert_eq!(((*(*m).vtbl).send_string_event)(m, &h, s.as_ptr()), S_OK);
        let evs = events(cb);
        assert_eq!(evs.len(), 2);
        for (e, op, len) in [(&evs[0], OPCODE_FIXED_EVENT, 0x20u16), (&evs[1], OPCODE_STRING_EVENT, 12)] {
            assert_eq!(e.header.provider_id, MERGER_EVENT_GUID);
            assert_eq!(e.header.size, 0x90);
            assert_eq!(e.header.descriptor.opcode, op);
            assert_eq!(e.header.descriptor.version, 0);
            assert_eq!((e.header.thread_id, e.header.process_id, e.header.time_stamp, e.header.kernel_time, e.header.user_time), (11, 22, 33, 44, 55));
            assert_eq!(e.user_data_length, len);
        }
        assert_eq!(evs[0].payload, data.to_vec());
        assert_eq!(wstrings(&evs[1].payload), vec!["hello".to_string()]);
        assert_eq!(((*(*m).vtbl).send_fixed_event)(m, core::ptr::null(), data.as_ptr() as *const c_void), E_POINTER);
        assert_eq!(((*(*m).vtbl).send_string_event)(m, &h, core::ptr::null()), E_POINTER);
        assert_eq!(((*(*m).vtbl).send_string_event)(m, core::ptr::null(), s.as_ptr()), E_POINTER);
        assert_eq!(((*(*m).vtbl).get_stats)(m, core::ptr::null_mut()), E_POINTER);
        assert_eq!(events(cb).len(), 2);
        release(m);
        free_cb(cb);
    }
}

#[test]
fn split_file_name_matches_wsplitpath() {
    assert_eq!(split_file_name(&w("\\Device\\X\\a\\b.c.dll")), w("b.c.dll"));
    assert_eq!(split_file_name(&w("C:noslash.exe")), w("noslash.exe"));
    assert_eq!(split_file_name(&w("a/b/c")), w("c"));
    assert_eq!(split_file_name(&w("dir.x\\file")), w("file"));
}

#[test]
fn resolve_image_path_branches() {
    let sys = system_dir();
    let win = &sys[..sys.rfind('\\').unwrap()];
    let r = |p: &str| String::from_utf16(&resolve_image_path(&w(p)).unwrap()).unwrap();
    assert_eq!(r("C:\\x\\y.dll"), "C:\\x\\y.dll");
    assert_eq!(r("\\\\srv\\s\\y.dll"), "\\\\srv\\s\\y.dll");
    assert_eq!(r("\\systemroot\\System32\\a.dll"), format!("{}\\System32\\a.dll", win));
    assert_eq!(r("\\Device\\HarddiskVolume3\\a.dll"), "\\\\?\\GLOBALROOT\\Device\\HarddiskVolume3\\a.dll");
    assert_eq!(r("\\GLOBAL??\\D\\a.dll"), "\\??\\D\\a.dll");
    assert_eq!(r("\\Program Files\\a.dll"), format!("{}\\Program Files\\a.dll", &win[..2]));
    assert_eq!(r("rel.dll"), format!("{}\\rel.dll", sys));
    assert_eq!(r("%SystemRoot%\\b.dll").to_lowercase(), format!("{}\\b.dll", win).to_lowercase());
}
