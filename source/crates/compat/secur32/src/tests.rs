use super::*;
use std::sync::Mutex as TestMutex;

static SERIAL: TestMutex<()> = TestMutex::new(());

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileMappingW(file: isize, sa: *mut c_void, protect: u32, hi: u32, lo: u32, name: *const u16) -> *mut c_void;
    fn GetHandleInformation(h: *mut c_void, flags: *mut u32) -> i32;
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn view(cpus: usize) -> Vec<u8> {
    let mut v = vec![0u8; cpus * PER_CPU_STRIDE];
    for cpu in 0..cpus {
        let b = cpu * PER_CPU_STRIDE;
        for i in 0..LSA_COUNTERS {
            v[b + 4 * i..b + 4 * i + 4].copy_from_slice(&((i as u32 + 1) * (cpu as u32 + 1)).to_le_bytes());
        }
    }
    let put = |v: &mut Vec<u8>, off: usize, x: u32| v[off..off + 4].copy_from_slice(&x.to_le_bytes());
    put(&mut v, INSTANCE_BASE + INSTANCE_ENTRY * 3, 1234);
    for cpu in 0..cpus {
        put(&mut v, cpu * PER_CPU_STRIDE + INSTANCE_BASE + INSTANCE_ENTRY * 3 + 4, 10 + cpu as u32);
        put(&mut v, cpu * PER_CPU_STRIDE + INSTANCE_BASE + INSTANCE_ENTRY * 3 + 8, 100 + cpu as u32);
    }
    v
}

fn dword(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

#[test]
fn open_lsa_performance_data_matches_host_section() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        {
            let mut st = STATE.lock().unwrap();
            st.opened = false;
            st.refs = 0;
            st.view = 0;
            st.section = 0;
        }
        let r = OpenLsaPerformanceData(core::ptr::null());
        let st = STATE.lock().unwrap();
        if r == 0 {
            assert!(st.opened && st.refs == 1 && st.view != 0 && st.section != 0);
        } else {
            assert_eq!(r, 2);
            assert!(!st.opened && st.refs == 0 && st.view == 0 && st.section == 0);
        }
        drop(st);
        if r == 0 {
            assert_eq!(OpenLsaPerformanceData(core::ptr::null()), 0);
            assert_eq!(STATE.lock().unwrap().refs, 2);
            CloseLsaPerformanceData();
            CloseLsaPerformanceData();
        }
    }
}

#[test]
fn close_lsa_performance_data_refcounts_and_unmaps() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let section = CreateFileMappingW(-1, core::ptr::null_mut(), 4, 0, 0x1000, core::ptr::null());
        assert!(!section.is_null());
        let mut base: *mut c_void = core::ptr::null_mut();
        let mut size = 0usize;
        assert!(NtMapViewOfSection(section, -1, &mut base, 0, 0, core::ptr::null_mut(), &mut size, VIEW_SHARE, 0, PAGE_READONLY) >= 0);
        {
            let mut st = STATE.lock().unwrap();
            st.opened = true;
            st.refs = 2;
            st.view = base as usize;
            st.section = section as usize;
        }
        assert_eq!(CloseLsaPerformanceData(), 0);
        assert_eq!(STATE.lock().unwrap().refs, 1);
        assert_ne!(STATE.lock().unwrap().view, 0);
        assert_eq!(CloseLsaPerformanceData(), 0);
        let st = STATE.lock().unwrap();
        assert!(st.opened && st.refs == 0 && st.view == 0 && st.section == 0);
        drop(st);
        let mut flags = 0u32;
        assert_eq!(GetHandleInformation(section, &mut flags), 0);
        let mut st = STATE.lock().unwrap();
        st.opened = false;
        drop(st);
        assert_eq!(CloseLsaPerformanceData(), 0);
        assert_eq!(STATE.lock().unwrap().refs, 0);
    }
}

#[test]
fn collect_lsa_performance_data_layout() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let v = view(2);
        let mut buf = vec![0xccu8; 0x1000];
        let mut p = buf.as_mut_ptr();
        let mut total = 0x1000u32;
        let mut n = 7u32;
        assert_eq!(collect_from(v.as_ptr() as usize, 2, core::ptr::null(), &mut p, &mut total, &mut n), 0);
        assert_eq!(n, 2);
        let inst_size = 0x18 + 0x10 + 0x10;
        assert_eq!(total, 0x3b8 + 0x90 + inst_size);
        assert_eq!(p as usize - buf.as_ptr() as usize, total as usize);
        assert_eq!(&buf[..0x360], &LSA_OBJECT_TEMPLATE[..]);
        assert_eq!(dword(&buf, 0x360), 0x58);
        assert_eq!(dword(&buf, 0x364), 0xcccccccc);
        for i in 0..LSA_COUNTERS {
            assert_eq!(dword(&buf, 0x368 + 4 * i), (i as u32 + 1) * 3);
        }
        let o = 0x3b8;
        assert_eq!(dword(&buf, o), 0x90 + inst_size);
        assert_eq!(dword(&buf, o + 0x28), 1);
        let i = o + 0x90;
        assert_eq!(dword(&buf, i), 0x10 + 0x18);
        assert_eq!(dword(&buf, i + 0xc), 0xffff_ffff);
        assert_eq!(dword(&buf, i + 0x10), 0x18);
        assert_eq!(dword(&buf, i + 0x14), 10);
        let name: Vec<u16> = buf[i + 0x18..i + 0x22].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(name, w("1234"));
        assert_eq!(dword(&buf, i + 0x10 + 0x18), 0x10);
        assert_eq!(dword(&buf, i + 0x10 + 0x1c), 21);
        assert_eq!(dword(&buf, i + 0x10 + 0x20), 201);

        let mut p = buf.as_mut_ptr();
        let mut total = 0x3b8u32 + 0x10;
        let mut n = 5u32;
        assert_eq!(collect_from(v.as_ptr() as usize, 2, core::ptr::null(), &mut p, &mut total, &mut n), ERROR_MORE_DATA);
        assert_eq!((total, n), (0, 0));
        assert_eq!(p, buf.as_mut_ptr());

        for q in ["Foreign", "Costly", "1 2 3"] {
            let s = w(q);
            let mut p = buf.as_mut_ptr();
            let mut total = 0x1000u32;
            let mut n = 5u32;
            assert_eq!(collect_from(v.as_ptr() as usize, 2, s.as_ptr(), &mut p, &mut total, &mut n), 0);
            assert_eq!((total, n), (0, 0));
        }
        let s = w(" 1666 1570");
        let mut p = buf.as_mut_ptr();
        let mut total = 0x1000u32;
        let mut n = 0u32;
        assert_eq!(collect_from(v.as_ptr() as usize, 2, s.as_ptr(), &mut p, &mut total, &mut n), 0);
        assert_eq!((total, n), (0x3b8, 1));
        assert!(is_number_in_list(1570, s.as_ptr()));
        assert!(!is_number_in_list(1570, w("1666,1570").as_ptr()));

        {
            let mut st = STATE.lock().unwrap();
            st.opened = false;
        }
        let mut p = buf.as_mut_ptr();
        let mut total = 0x1000u32;
        let mut n = 9u32;
        assert_eq!(CollectLsaPerformanceData(core::ptr::null(), &mut p, &mut total, &mut n), 0);
        assert_eq!((total, n), (0, 0));
    }
}
