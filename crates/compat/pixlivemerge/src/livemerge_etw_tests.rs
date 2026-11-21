use crate::livemerge::*;
use crate::livemerge_tests::*;
use std::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> usize;
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
}

#[repr(C)]
struct DataDesc {
    ptr: u64,
    size: u32,
    reserved: u32,
}

#[link(name = "advapi32")]
extern "system" {
    fn EventRegister(id: *const Guid, cb: *const c_void, ctx: *const c_void, h: *mut u64) -> u32;
    fn EventUnregister(h: u64) -> u32;
    fn EventWrite(h: u64, d: *const EventDescriptor, n: u32, data: *const DataDesc) -> u32;
    fn StartTraceW(h: *mut u64, name: *const u16, props: *mut u8) -> u32;
    fn EnableTraceEx2(h: u64, id: *const Guid, code: u32, level: u8, any: u64, all: u64, timeout: u32, params: *const c_void) -> u32;
    fn ControlTraceW(h: u64, name: *const u16, props: *mut u8, code: u32) -> u32;
    fn OpenTraceW(logfile: *mut u8) -> u64;
    fn ProcessTrace(h: *const u64, n: u32, start: *const c_void, end: *const c_void) -> u32;
    fn CloseTrace(h: u64) -> u32;
}

fn wz(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn props(provider: &Guid, file: &[u16]) -> Vec<u64> {
    let mut v = vec![0u64; (0x78 + 0x1000) / 8];
    let b = v.as_mut_ptr() as *mut u8;
    unsafe {
        (b as *mut u32).write((0x78 + 0x1000) as u32);
        core::ptr::copy_nonoverlapping(provider.as_ptr(), b.add(0x18), 16);
        (b.add(0x28) as *mut u32).write(1);
        (b.add(0x2c) as *mut u32).write(0x20000);
        (b.add(0x40) as *mut u32).write(0x800 | 0x20000 | 1);
        (b.add(0x74) as *mut u32).write(0x78);
        (b.add(0x70) as *mut u32).write(0x78 + 0x800);
        core::ptr::copy_nonoverlapping(file.as_ptr(), b.add(0x78 + 0x800) as *mut u16, file.len());
    }
    v
}

unsafe extern "system" fn on_record(r: *mut EventRecord) {
    let m = (*r).user_context as *mut LiveMerger;
    assert_eq!(process(m, &*r), S_OK);
}

unsafe fn write(h: u64, opcode: u8, version: u8, payload: &[u8]) {
    let d = EventDescriptor { id: 1, version, channel: 0, level: 4, opcode, task: 0, keyword: 0 };
    let dd = DataDesc { ptr: payload.as_ptr() as u64, size: payload.len() as u32, reserved: 0 };
    assert_eq!(EventWrite(h, &d, 1, &dd), 0);
}

#[test]
fn private_etw_session_feeds_merger_and_output_is_merged() {
    let _g = crate::test_lock();
    unsafe {
        let pid = GetCurrentProcessId();
        let tid = GetCurrentThreadId();
        let mut img = 0u64;
        let mut fio = 0u64;
        assert_eq!(EventRegister(&IMAGE_LOAD_GUID, core::ptr::null(), core::ptr::null(), &mut img), 0);
        assert_eq!(EventRegister(&FILE_IO_GUID, core::ptr::null(), core::ptr::null(), &mut fio), 0);
        let etl = std::env::temp_dir().join(format!("plm_live_{}.etl", pid));
        let _ = std::fs::remove_file(&etl);
        let file = wz(etl.to_str().unwrap());
        let name = wz(&format!("PixLiveMergeTest{}", pid));
        let mut p = props(&IMAGE_LOAD_GUID, &file);
        let mut th = 0u64;
        assert_eq!(StartTraceW(&mut th, name.as_ptr(), p.as_mut_ptr() as *mut u8), 0);
        assert_eq!(EnableTraceEx2(th, &IMAGE_LOAD_GUID, 1, 5, 0, 0, 0, core::ptr::null()), 0);
        assert_eq!(EnableTraceEx2(th, &FILE_IO_GUID, 1, 5, 0, 0, 0, core::ptr::null()), 0);
        let base = GetModuleHandleW(core::ptr::null()) as u64;
        let own_f = exe_facts();
        let own_name = exe_name();
        let own = image_payload(base, own_f.size_of_image as u64, pid, 0, &format!("\\Device\\HarddiskVolume1\\t\\{}", own_name));
        write(img, 10, 2, &own);
        let dos = format!("{}\\kernel32.dll", system_dir());
        let k32 = image_payload(0x7ffa_0000_0000, 0, pid + 4, 0, &nt_path_of(&dos));
        write(img, 3, 3, &k32);
        write(fio, 64, 2, &[0u8; 0x18]);
        write(fio, 65, 2, &[0u8; 0x18]);
        assert_eq!(ControlTraceW(th, core::ptr::null(), p.as_mut_ptr() as *mut u8, 1), 0);
        EventUnregister(img);
        EventUnregister(fio);
        let cb = new_cb();
        let m = create(cfg(0, 1, pid), cb);
        let mut lf = vec![0u64; 0x200 / 8];
        let l = lf.as_mut_ptr() as *mut u8;
        (l as *mut *const u16).write(file.as_ptr());
        (l.add(0x1c) as *mut u32).write(0x1000_0000);
        (l.add(0x1a8) as *mut usize).write(on_record as *const () as usize);
        (l.add(0x1b8) as *mut *mut LiveMerger).write(m);
        let h = OpenTraceW(l);
        assert_ne!(h, u64::MAX);
        assert_eq!(ProcessTrace(&h, 1, core::ptr::null(), core::ptr::null()), 0);
        CloseTrace(h);
        let evs = events(cb);
        let k32_f = pe_facts(&std::fs::read(&dos).unwrap());
        assert!(evs.len() > 4);
        for e in &evs[..4] {
            assert_eq!(e.header.thread_id, tid);
            assert_eq!(e.header.process_id, pid);
            assert_ne!(e.header.time_stamp, 0);
        }
        let src = |e: &Captured| {
            let mut r = EventRecord::zeroed();
            r.header = e.header;
            r
        };
        check_image_pair(&evs[0..2], &src(&evs[0]), base, own_f.size_of_image as u64, pid, &own_name, &own_f);
        check_image_pair(&evs[2..4], &src(&evs[2]), 0x7ffa_0000_0000, k32_f.size_of_image as u64, pid + 4, "kernel32.dll", &k32_f);
        assert_eq!(evs[1].header.time_stamp, evs[0].header.time_stamp);
        assert!(evs[2].header.time_stamp >= evs[0].header.time_stamp);
        let ts = evs[4].header.time_stamp;
        assert!(ts >= evs[2].header.time_stamp);
        check_device_map(&evs[4..], ts);
        assert_eq!(stats(m), [0, 1, 1]);
        release(m);
        free_cb(cb);
        std::fs::remove_file(&etl).unwrap();
    }
}
