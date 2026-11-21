use crate::objects::*;
use crate::transport::*;
use crate::*;
use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::null_mut;

#[repr(C)]
struct Fake {
    vtbl: *const usize,
    kind: &'static str,
    refs: u32,
    completed: u64,
}

#[derive(Clone, Debug)]
struct Rec {
    cmd: u32,
    handle: u64,
    a: usize,
    b: usize,
    data: Vec<(usize, u32)>,
    b0: u32,
}

#[derive(Default)]
struct World {
    log: Vec<String>,
    fail: Option<&'static str>,
    version: u32,
    status: u32,
    handle: u64,
    lag: bool,
    close_hr: i32,
    recs: Vec<Rec>,
    fences: Vec<usize>,
}

thread_local! {
    static W: RefCell<World> = RefCell::new(World { version: INTERFACE_VERSION, handle: 0x5150, ..Default::default() });
}

#[link(name = "kernel32")]
extern "system" {
    fn SetEvent(h: *mut c_void) -> i32;
}

fn log(s: String) {
    W.with(|w| w.borrow_mut().log.push(s));
}

fn failing(what: &str) -> bool {
    W.with(|w| w.borrow().fail == Some(what))
}

fn logs() -> Vec<String> {
    W.with(|w| w.borrow().log.clone())
}

fn recs() -> Vec<Rec> {
    W.with(|w| w.borrow().recs.clone())
}

fn reset(build: u32) {
    BUILD_OVERRIDE.with(|c| c.set(Some(build)));
    W.with(|w| *w.borrow_mut() = World { version: INTERFACE_VERSION, handle: 0x5150, ..Default::default() });
}

unsafe extern "system" fn bad(_: *mut c_void) -> i32 {
    log("bad slot".into());
    0x8000_4001u32 as i32
}

unsafe extern "system" fn add_ref(this: *mut Fake) -> u32 {
    (*this).refs += 1;
    (*this).refs
}

unsafe extern "system" fn release_fake(this: *mut Fake) -> u32 {
    (*this).refs -= 1;
    log(format!("release {}", (*this).kind));
    (*this).refs
}

unsafe extern "system" fn qi(this: *mut Fake, riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    if *riid == IID_DEVICE && !failing("qi_device") {
        add_ref(this);
        *ppv = this as *mut c_void;
        return 0;
    }
    if *riid == IID_VIDEO_DEVICE2 && (*this).kind == "device" && !failing("qi_video") {
        *ppv = fake("video") as *mut c_void;
        return 0;
    }
    *ppv = null_mut();
    0x8000_4002u32 as i32
}

unsafe extern "system" fn create_queue(_: *mut Fake, desc: *const [u32; 4], riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    assert_eq!(*riid, IID_QUEUE);
    log(format!("queue {:?}", *desc));
    *ppv = fake("queue") as *mut c_void;
    0
}

unsafe extern "system" fn create_allocator(_: *mut Fake, ty: u32, riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    assert_eq!(*riid, IID_ALLOCATOR);
    if failing("allocator") {
        return 0x8007_000Eu32 as i32;
    }
    log(format!("allocator {ty}"));
    *ppv = fake("allocator") as *mut c_void;
    0
}

unsafe extern "system" fn create_fence(_: *mut Fake, initial: u64, flags: u32, riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    assert_eq!(*riid, IID_FENCE);
    log(format!("fence {initial} {flags}"));
    *ppv = fake("fence") as *mut c_void;
    0
}

unsafe extern "system" fn create_list(_: *mut Fake, params: *const [u32; 4], alloc: *mut Fake, pso: *mut c_void, riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    assert_eq!(*riid, IID_ENCODE_LIST1);
    assert!(pso.is_null());
    log(format!("list {:?} {}", *params, (*alloc).kind));
    *ppv = fake("list") as *mut c_void;
    0
}

unsafe extern "system" fn create_ext(_: *mut Fake, desc: *const [u8; 20], params: *const u32, size: usize, session: *mut c_void, riid: *const [u8; 16], ppv: *mut *mut c_void) -> i32 {
    assert_eq!(*riid, IID_EXTENSION_COMMAND);
    assert_eq!(&(&*desc)[4..], &AVE_COMMAND_ID);
    assert!(session.is_null() && !params.is_null());
    log(format!("ext node {} size {size}", u32::from_le_bytes((&*desc)[..4].try_into().unwrap())));
    *ppv = fake("ext") as *mut c_void;
    0
}

unsafe extern "system" fn list_close(_: *mut Fake) -> i32 {
    log("close".into());
    W.with(|w| w.borrow().close_hr)
}

unsafe extern "system" fn list_reset(_: *mut Fake, alloc: *mut Fake) -> i32 {
    log(format!("list reset {}", (*alloc).kind));
    0
}

unsafe extern "system" fn list_init_ext(_: *mut Fake, ext: *mut Fake, params: *const [u8; 16], size: usize) {
    assert_eq!(*params, [0u8; 16]);
    log(format!("init_ext {} {size}", (*ext).kind));
}

unsafe extern "system" fn list_exec_ext(_: *mut Fake, ext: *mut Fake, params: *const Params, size: usize) {
    assert_eq!((*ext).kind, "ext");
    assert_eq!(size, 0x50);
    let p = &*params;
    assert_eq!(p.tag, PARAMS_TAG);
    let pkg = &mut *p.package;
    let data = (0..MAX_RESOURCES).filter(|&i| !p.data[i].is_null()).map(|i| (p.data[i] as usize, p.sizes[i])).collect();
    log(format!("exec_ext {:#x}", pkg.cmd));
    W.with(|w| {
        let mut w = w.borrow_mut();
        let b0 = if pkg.cmd == 6 && !pkg.b.is_null() { *(pkg.b as *const u32) } else { 0 };
        w.recs.push(Rec { cmd: pkg.cmd, handle: pkg.handle, a: pkg.a as usize, b: pkg.b as usize, data, b0 });
        if pkg.cmd == 1 {
            *((pkg.b as *mut u8).add(8) as *mut u32) = w.version;
        }
        if pkg.cmd == 5 {
            pkg.b = w.handle as *mut c_void;
        }
        pkg.status = w.status;
    });
}

unsafe extern "system" fn queue_submit(_: *mut Fake, n: u32, lists: *const *mut Fake) {
    log(format!("submit {n} {}", (**lists).kind));
}

unsafe extern "system" fn queue_signal(_: *mut Fake, fence: *mut Fake, value: u64) -> i32 {
    log(format!("signal {} {value}", (*fence).kind));
    if !W.with(|w| w.borrow().lag) {
        (*fence).completed = value;
    }
    if failing("signal") { E_FAIL } else { 0 }
}

unsafe extern "system" fn queue_wait(_: *mut Fake, fence: *mut Fake, value: u64) -> i32 {
    log(format!("wait {} {value}", (*fence).kind));
    0
}

unsafe extern "system" fn fence_completed(this: *mut Fake) -> u64 {
    (*this).completed
}

unsafe extern "system" fn fence_on_completion(this: *mut Fake, value: u64, event: *mut c_void) -> i32 {
    log(format!("on_completion {value}"));
    if failing("on_completion") {
        return E_FAIL;
    }
    (*this).completed = value;
    SetEvent(event);
    0
}

unsafe extern "system" fn allocator_reset(_: *mut Fake) -> i32 {
    log("allocator reset".into());
    0
}

fn fake(kind: &'static str) -> *mut Fake {
    let mut v = vec![bad as *const () as usize; 80];
    v[0] = qi as *const () as usize;
    v[1] = add_ref as *const () as usize;
    v[2] = release_fake as *const () as usize;
    match kind {
        "device" => {
            v[8] = create_queue as *const () as usize;
            v[9] = create_allocator as *const () as usize;
            v[36] = create_fence as *const () as usize;
            v[73] = create_list as *const () as usize;
        }
        "video" => v[12] = create_ext as *const () as usize,
        "list" => {
            v[9] = list_close as *const () as usize;
            v[10] = list_reset as *const () as usize;
            v[25] = list_init_ext as *const () as usize;
            v[26] = list_exec_ext as *const () as usize;
        }
        "queue" => {
            v[10] = queue_submit as *const () as usize;
            v[14] = queue_signal as *const () as usize;
            v[15] = queue_wait as *const () as usize;
        }
        "fence" | "user fence" => {
            v[8] = fence_completed as *const () as usize;
            v[9] = fence_on_completion as *const () as usize;
        }
        "allocator" => v[8] = allocator_reset as *const () as usize,
        _ => {}
    }
    let vtbl = Box::leak(v.into_boxed_slice()).as_ptr();
    let f = Box::into_raw(Box::new(Fake { vtbl, kind, refs: 1, completed: 0 }));
    W.with(|w| w.borrow_mut().fences.push(f as usize));
    f
}

struct Setup {
    device: *mut Fake,
    out: Box<[u64; 4]>,
    args: Box<[u64; 4]>,
    reply: Box<[u64; 4]>,
}

impl Setup {
    fn factory(&self) -> *mut Factory {
        self.out[1] as *mut Factory
    }
}

unsafe fn make_factory() -> (Setup, i32) {
    let device = fake("device");
    let mut desc = Box::new([0u64, 3, device as u64, 0]);
    let mut s = Setup { device, out: Box::new([0; 4]), args: Box::new([0, 0x77, 0, 0]), reply: Box::new([0; 4]) };
    let hr = Ordinal6958(desc.as_mut_ptr() as P, s.args.as_mut_ptr() as P, s.out.as_mut_ptr() as P, s.reply.as_mut_ptr() as P);
    (s, hr)
}

unsafe fn make_encoder(s: &Setup) -> (*mut Encoder, Box<[u64; 4]>) {
    let mut a = Box::new([0u64, 0, 0xabc, 0]);
    let mut holder = Box::new([0u64; 4]);
    assert_eq!(Ordinal6953(s.factory(), a.as_mut_ptr() as P, holder.as_mut_ptr() as P), 0);
    (holder[1] as *mut Encoder, holder)
}

#[test]
fn ordinal6958_builds_transport_and_negotiates_version() {
    reset(22621);
    unsafe {
        let (s, hr) = make_factory();
        assert_eq!(hr, 0);
        assert_eq!(s.out[2] as u32, INTERFACE_VERSION);
        let f = &*s.factory();
        assert_eq!(f.init, 1);
        assert_eq!(f.version, 0x77);
        let t = &*f.transport;
        assert_eq!(t.init, 1);
        assert_eq!(t.device, s.device as P);
        assert_eq!(t.index, 2);
        assert_eq!(t.value, 2);
        assert_eq!(t.fence_values, [1, 2, 0]);
        assert!(!t.event.is_null());
        let l = logs();
        let expect = [
            "allocator 6", "allocator 6", "allocator 6", "list [6, 0, 0, 0] allocator", "queue [6, 0, 0, 0]", "ext node 0 size 4", "fence 0 0",
            "init_ext ext 16", "close", "submit 1 list", "signal fence 1",
            "allocator reset", "list reset allocator", "exec_ext 0x1", "close", "submit 1 list", "signal fence 2",
        ];
        assert_eq!(l, expect);
        let r = &recs()[0];
        assert_eq!((r.cmd, r.handle, r.a, r.b), (1, 0, s.args.as_ptr() as usize, s.reply.as_ptr() as usize));
    }
}

#[test]
fn ordinal6958_rejects_bad_arguments_and_versions() {
    reset(22621);
    unsafe {
        let mut out = [0u64; 4];
        let mut args = [0u64; 4];
        let mut desc = [0u64, 3, 0, 0];
        assert_eq!(Ordinal6958(desc.as_mut_ptr() as P, args.as_mut_ptr() as P, out.as_mut_ptr() as P, args.as_mut_ptr() as P), E_INVALIDARG);
        assert_eq!(Ordinal6958(null_mut(), args.as_mut_ptr() as P, out.as_mut_ptr() as P, args.as_mut_ptr() as P), E_INVALIDARG);
        let device = fake("device");
        let mut desc = [0u64, 2, device as u64, 0];
        let mut reply = [0u64; 4];
        assert_eq!(Ordinal6958(desc.as_mut_ptr() as P, args.as_mut_ptr() as P, out.as_mut_ptr() as P, reply.as_mut_ptr() as P), E_INVALIDARG);
        assert_eq!((out[1], out[2] as u32), (0, INTERFACE_VERSION));
        desc[1] = 3;
        W.with(|w| w.borrow_mut().version = 0x30016);
        assert_eq!(Ordinal6958(desc.as_mut_ptr() as P, args.as_mut_ptr() as P, out.as_mut_ptr() as P, reply.as_mut_ptr() as P), E_INVALIDARG);
        assert_eq!(out[1], 0);
        assert!(logs().contains(&"release device".to_string()));
        BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
        assert_eq!(Ordinal6958(desc.as_mut_ptr() as P, args.as_mut_ptr() as P, out.as_mut_ptr() as P, reply.as_mut_ptr() as P), E_FAIL);
        W.with(|w| w.borrow_mut().fail = Some("allocator"));
        assert_eq!(Ordinal6958(desc.as_mut_ptr() as P, args.as_mut_ptr() as P, out.as_mut_ptr() as P, reply.as_mut_ptr() as P), E_OUTOFMEMORY);
        BUILD_OVERRIDE.with(|c| c.set(Some(22621)));
        W.with(|w| w.borrow_mut().fail = Some("qi_device"));
        assert_eq!(Ordinal6958(desc.as_mut_ptr() as P, args.as_mut_ptr() as P, out.as_mut_ptr() as P, reply.as_mut_ptr() as P), E_OUTOFMEMORY);
        assert_eq!(out[1], 0);
    }
}

#[test]
fn ordinal6957_deletes_factory_and_releases_d3d12_objects() {
    reset(22621);
    unsafe {
        let (mut s, hr) = make_factory();
        assert_eq!(hr, 0);
        let mut status = [u32::MAX; 4];
        assert_eq!(Ordinal6957(null_mut(), status.as_mut_ptr() as P), E_INVALIDARG);
        assert_eq!(Ordinal6957(s.out.as_mut_ptr() as P, null_mut()), E_INVALIDARG);
        W.with(|w| w.borrow_mut().log.clear());
        assert_eq!(Ordinal6957(s.out.as_mut_ptr() as P, status.as_mut_ptr() as P), 0);
        assert_eq!(status[1], 0);
        let l = logs();
        let expect = ["release device", "release queue", "release list", "release ext", "release video", "release allocator", "release allocator", "release allocator", "release fence"];
        assert_eq!(l, expect);
        let mut empty = [0u64; 2];
        assert_eq!(Ordinal6957(empty.as_mut_ptr() as P, status.as_mut_ptr() as P), E_INVALIDARG);
    }
}

unsafe fn factory_export_case(f: unsafe extern "system" fn(*mut Factory, P, P) -> i32, cmd: u32, field: Option<(bool, usize)>) {
    reset(22621);
    let (s, hr) = make_factory();
    assert_eq!(hr, 0);
    let mut a = [1u64; 4];
    let mut b = [1u64; 4];
    W.with(|w| w.borrow_mut().status = 0x8899);
    assert_eq!(f(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), 0x8899);
    let r = recs().last().unwrap().clone();
    assert_eq!((r.cmd, r.handle, r.a, r.b), (cmd, 0, a.as_ptr() as usize, b.as_ptr() as usize));
    assert_eq!(f(null_mut(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_INVALIDARG);
    assert_eq!(f(s.factory(), null_mut(), b.as_mut_ptr() as P), E_POINTER);
    if let Some((in_b, off)) = field {
        let v = if in_b { &mut b } else { &mut a };
        v[off / 8] = 0;
        assert_eq!(f(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_POINTER);
        BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
        assert_eq!(f(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
        BUILD_OVERRIDE.with(|c| c.set(Some(22621)));
    }
    (*s.factory()).init = 0;
    assert_eq!(f(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
}

#[test]
fn ordinal6956_sends_command_2() {
    unsafe { factory_export_case(Ordinal6956, 2, None) }
}

#[test]
fn ordinal6955_sends_command_3() {
    unsafe { factory_export_case(Ordinal6955, 3, Some((true, 0x10))) }
}

#[test]
fn ordinal6954_sends_command_4() {
    unsafe { factory_export_case(Ordinal6954, 4, Some((true, 8))) }
}

#[test]
fn ordinal6953_creates_encoder_from_driver_handle() {
    reset(22621);
    unsafe {
        let (s, _) = make_factory();
        let (e, _holder) = make_encoder(&s);
        let r = recs().last().unwrap().clone();
        assert_eq!((r.cmd, r.b), (5, 0));
        assert_eq!(((*e).handle, (*e).init, (*e).closed), (0x5150, 1, 0));
        assert_eq!((*e).transport, (*s.factory()).transport);
        let mut a = [0u64, 0, 0, 0];
        let mut holder = [u64::MAX; 2];
        assert_eq!(Ordinal6953(s.factory(), a.as_mut_ptr() as P, holder.as_mut_ptr() as P), E_POINTER);
        BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
        assert_eq!(Ordinal6953(s.factory(), a.as_mut_ptr() as P, holder.as_mut_ptr() as P), E_FAIL);
        BUILD_OVERRIDE.with(|c| c.set(Some(22621)));
        a[2] = 1;
        W.with(|w| w.borrow_mut().status = 0x8765);
        assert_eq!(Ordinal6953(s.factory(), a.as_mut_ptr() as P, holder.as_mut_ptr() as P), 0x8765);
        assert_eq!(holder, [u64::MAX & 0xffff_ffff, 0]);
        W.with(|w| {
            let mut w = w.borrow_mut();
            w.status = 0;
            w.handle = 0;
        });
        assert_eq!(Ordinal6953(s.factory(), a.as_mut_ptr() as P, holder.as_mut_ptr() as P), E_INVALIDARG);
        let rs = recs();
        let r = &rs[rs.len() - 2];
        assert_eq!((r.cmd, r.a, r.b0), (6, 0, 8));
        assert_eq!(rs[rs.len() - 1].cmd, 0x23);
        assert_eq!(holder[1], 0);
        assert_eq!(Ordinal6953(null_mut(), a.as_mut_ptr() as P, holder.as_mut_ptr() as P), E_INVALIDARG);
    }
}

#[test]
fn ordinal6952_destroys_encoder_through_driver() {
    reset(22621);
    unsafe {
        let (s, _) = make_factory();
        let (_, mut holder) = make_encoder(&s);
        let mut b = [0u64; 2];
        assert_eq!(Ordinal6952(s.factory(), holder.as_mut_ptr() as P, null_mut()), E_POINTER);
        BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
        assert_eq!(Ordinal6952(s.factory(), holder.as_mut_ptr() as P, null_mut()), E_INVALIDARG);
        BUILD_OVERRIDE.with(|c| c.set(Some(22621)));
        W.with(|w| w.borrow_mut().status = 0x42);
        assert_eq!(Ordinal6952(s.factory(), holder.as_mut_ptr() as P, b.as_mut_ptr() as P), 0x42);
        let l = logs();
        let pos = l.iter().rposition(|x| x == "exec_ext 0x6").unwrap();
        assert_eq!(l[pos + 1..], ["close", "submit 1 list", "signal fence 4", "allocator reset", "list reset allocator", "exec_ext 0x23", "close"]);
        let rs = recs();
        let destroy = &rs[rs.len() - 2];
        assert_eq!((destroy.cmd, destroy.a, destroy.b), (6, 0x5150, b.as_ptr() as usize));
        let follow = &rs[rs.len() - 1];
        assert_eq!((follow.cmd, follow.a, follow.b), (0x23, 0x5150, 0));
        assert_eq!(Ordinal6952(null_mut(), holder.as_mut_ptr() as P, b.as_mut_ptr() as P), E_INVALIDARG);
        (*s.factory()).init = 0;
        assert_eq!(Ordinal6952(s.factory(), holder.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
    }
}

type EncFn = unsafe extern "system" fn(*mut Encoder, P, P) -> i32;

unsafe fn encoder_export_case(f: EncFn, cmd: u32, check: Check) {
    reset(22621);
    let (s, _) = make_factory();
    let (e, _holder) = make_encoder(&s);
    let mut a = [1u64; 4];
    let mut b = [1u64; 4];
    W.with(|w| w.borrow_mut().status = 0x1234);
    assert_eq!(f(e, a.as_mut_ptr() as P, b.as_mut_ptr() as P), 0x1234);
    let r = recs().last().unwrap().clone();
    assert_eq!((r.cmd, r.handle, r.a, r.b), (cmd, 0x5150, a.as_ptr() as usize, b.as_ptr() as usize));
    assert_eq!(f(null_mut(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_INVALIDARG);
    assert_eq!(f(e, a.as_mut_ptr() as P, null_mut()), E_POINTER);
    match check {
        Check::A(off) => a[off / 8] = 0,
        Check::B(off) | Check::BThenClosed(off) => b[off / 8] = 0,
        _ => {}
    }
    if matches!(check, Check::A(_) | Check::B(_) | Check::BThenClosed(_)) {
        assert_eq!(f(e, a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_POINTER);
        BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
        assert_eq!(f(e, a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
        BUILD_OVERRIDE.with(|c| c.set(Some(22621)));
        a = [1; 4];
        b = [1; 4];
    }
    (*e).closed = 1;
    let closed = f(e, a.as_mut_ptr() as P, b.as_mut_ptr() as P);
    let closed_null = f(e, a.as_mut_ptr() as P, null_mut());
    match check {
        Check::ClosedAfter | Check::BThenClosed(_) => assert_eq!((closed, closed_null), (E_UNEXPECTED, E_POINTER)),
        Check::ClosedFirst => assert_eq!((closed, closed_null), (E_UNEXPECTED, E_UNEXPECTED)),
        _ => assert_eq!((closed, closed_null), (0x1234, E_POINTER)),
    }
    BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
    if !matches!(check, Check::None | Check::A(_) | Check::B(_)) {
        assert_eq!(f(e, a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
    }
    BUILD_OVERRIDE.with(|c| c.set(Some(22621)));
    (*e).init = 0;
    assert_eq!(f(e, a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
    (*e).init = 1;
}

macro_rules! enc_test {
    ($t:ident, $f:ident, $cmd:expr, $check:expr) => {
        #[test]
        fn $t() {
            unsafe { encoder_export_case($f, $cmd, $check) }
        }
    };
}

enc_test!(ordinal6950_command_8, Ordinal6950, 0x8, Check::B(8));
enc_test!(ordinal6949_command_9, Ordinal6949, 0x9, Check::B(8));
enc_test!(ordinal6948_command_a, Ordinal6948, 0xa, Check::B(8));
enc_test!(ordinal6947_command_b, Ordinal6947, 0xb, Check::B(8));
enc_test!(ordinal6946_command_c, Ordinal6946, 0xc, Check::A(0x10));
enc_test!(ordinal6944_command_e, Ordinal6944, 0xe, Check::ClosedFirst);
enc_test!(ordinal6943_command_f, Ordinal6943, 0xf, Check::BThenClosed(0x10));
enc_test!(ordinal6942_command_10, Ordinal6942, 0x10, Check::ClosedAfter);
enc_test!(ordinal6941_command_11, Ordinal6941, 0x11, Check::ClosedAfter);
enc_test!(ordinal6940_command_12, Ordinal6940, 0x12, Check::B(8));
enc_test!(ordinal6939_command_13, Ordinal6939, 0x13, Check::None);
enc_test!(ordinal6938_command_14, Ordinal6938, 0x14, Check::None);
enc_test!(ordinal6937_command_15, Ordinal6937, 0x15, Check::None);
enc_test!(ordinal6936_command_16, Ordinal6936, 0x16, Check::None);
enc_test!(ordinal6935_command_17, Ordinal6935, 0x17, Check::B(8));
enc_test!(ordinal6934_command_22, Ordinal6934, 0x22, Check::B(0x10));

#[test]
fn ordinal6945_encodes_with_resources_and_fences() {
    reset(22621);
    unsafe {
        let (s, _) = make_factory();
        let (e, _holder) = make_encoder(&s);
        let mut r1 = Resource { data: 0x1000 as P, size: 64 };
        let mut r2 = Resource { data: 0x2000 as P, size: 128 };
        let user = fake("user fence");
        let mut sync = FenceSync { signal_fence: user as P, signal_value: 9, wait_fence: user as P, wait_value: 7 };
        let items = [
            Input { kind: 2, pad: 0, data: &mut r1 as *mut Resource as P },
            Input { kind: 3, pad: 0, data: 0x3000 as P },
            Input { kind: 6, pad: 0, data: &mut sync as *mut FenceSync as P },
            Input { kind: 5, pad: 0, data: &mut r2 as *mut Resource as P },
        ];
        let mut list = InputList { reserved: 0, count: 4, pad: 0, items: items.as_ptr() };
        let mut b = [0u64; 4];
        W.with(|w| w.borrow_mut().log.clear());
        assert_eq!(Ordinal6945(e, &mut list as *mut InputList as P, b.as_mut_ptr() as P), 0);
        let r = recs().last().unwrap().clone();
        assert_eq!((r.cmd, r.handle), (0xd, 0x5150));
        assert_eq!(r.data, vec![(0x1000, 64), (0x2000, 128)]);
        assert_eq!(logs(), ["allocator reset", "list reset allocator", "exec_ext 0xd", "close", "wait user fence 7", "submit 1 list", "signal fence 4", "signal user fence 9"]);
        assert_eq!(Ordinal6945(null_mut(), &mut list as *mut InputList as P, b.as_mut_ptr() as P), E_INVALIDARG);
        assert_eq!(Ordinal6945(e, &mut list as *mut InputList as P, null_mut()), E_INVALIDARG);
        let six: Vec<Input> = (0..6).map(|_| Input { kind: 2, pad: 0, data: &mut r1 as *mut Resource as P }).collect();
        let mut many = InputList { reserved: 0, count: 6, pad: 0, items: six.as_ptr() };
        assert_eq!(Ordinal6945(e, &mut many as *mut InputList as P, b.as_mut_ptr() as P), E_INVALIDARG);
        (*e).closed = 1;
        assert_eq!(Ordinal6945(e, &mut list as *mut InputList as P, b.as_mut_ptr() as P), E_UNEXPECTED);
        BUILD_OVERRIDE.with(|c| c.set(Some(19041)));
        assert_eq!(Ordinal6945(e, &mut list as *mut InputList as P, b.as_mut_ptr() as P), E_FAIL);
        (*e).closed = 0;
        (*e).init = 0;
        assert_eq!(Ordinal6945(e, &mut list as *mut InputList as P, b.as_mut_ptr() as P), E_FAIL);
    }
}

#[test]
fn transport_waits_on_its_fence_before_reusing_an_allocator() {
    reset(22621);
    unsafe {
        let (s, _) = make_factory();
        W.with(|w| w.borrow_mut().lag = true);
        let mut a = [1u64; 4];
        let mut b = [1u64; 4];
        for _ in 0..4 {
            assert_eq!(Ordinal6956(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), 0);
        }
        assert!(logs().contains(&"on_completion 3".to_string()));
        W.with(|w| w.borrow_mut().fail = Some("on_completion"));
        assert_eq!(Ordinal6956(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), E_FAIL);
        W.with(|w| {
            let mut w = w.borrow_mut();
            w.fail = None;
            w.lag = false;
            w.close_hr = 0x8007_0005u32 as i32;
        });
        let fence = (*(*s.factory()).transport).fence as *mut Fake;
        (*fence).completed = 100;
        assert_eq!(Ordinal6956(s.factory(), a.as_mut_ptr() as P, b.as_mut_ptr() as P), 0x8007_0005u32 as i32);
        W.with(|w| w.borrow_mut().close_hr = 0);
        let t = (*s.factory()).transport;
        let mut pkg = Package::new(2, 0, null_mut(), null_mut());
        let rd = r_dummy();
        let res = [&rd as *const Resource];
        assert_eq!(transport_execute(t, &mut pkg, 1, null_mut()), E_INVALIDARG);
        assert_eq!(transport_execute(t, null_mut(), 0, res.as_ptr()), E_INVALIDARG);
        (*t).init = 0;
        assert_eq!(transport_execute(t, &mut pkg, 0, null_mut()), E_FAIL);
    }
}

fn r_dummy() -> Resource {
    Resource { data: null_mut(), size: 0 }
}
