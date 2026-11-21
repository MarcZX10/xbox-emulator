pub mod xa;

use std::ffi::c_void;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}

pub const S_OK: i32 = 0;

pub fn dll_dir() -> PathBuf {
    match std::env::var_os("AUDIO_TITLES_DLL_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(r"..\..\..\target-xaudio\release"),
    }
}

pub struct Dll(*mut c_void, String);
unsafe impl Send for Dll {}
unsafe impl Sync for Dll {}

impl Dll {
    pub fn load(name: &str) -> Dll {
        let p = dll_dir().join(name);
        assert!(p.exists(), "built DLL {} missing: build it with CARGO_TARGET_DIR=target-xaudio cargo build --release -p acphal -p chad -p xapu", p.display());
        let w: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let m = unsafe { LoadLibraryW(w.as_ptr()) };
        assert!(!m.is_null(), "LoadLibraryW({}) failed", p.display());
        Dll(m, name.to_string())
    }

    pub fn sym<T: Copy>(&self, name: &str) -> T {
        assert_eq!(core::mem::size_of::<T>(), core::mem::size_of::<usize>());
        let c: Vec<u8> = name.bytes().chain(Some(0)).collect();
        let f = unsafe { GetProcAddress(self.0, c.as_ptr()) };
        assert!(!f.is_null(), "{} does not export {name}", self.1);
        unsafe { core::mem::transmute_copy(&f) }
    }
}

type CreateHeapFn = unsafe extern "system" fn(u32, u32) -> i32;
type AllocFn = unsafe extern "system" fn(*mut *mut c_void, *mut u32, u32, u32, u32) -> i32;
type FreeFn = unsafe extern "system" fn(*mut c_void) -> i32;
type ShapeFn = unsafe extern "system" fn(*mut u8) -> i32;
type ReleaseShapeFn = unsafe extern "system" fn() -> i32;
type CreateFn = unsafe extern "system" fn(*mut *mut c_void) -> i32;
type ConnectFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> i32;
type DisconnectFn = unsafe extern "system" fn(*mut c_void) -> i32;
type SubmitFn = unsafe extern "system" fn(*mut c_void, u32, u64, u32, *const u8, u32) -> i32;
type PopFn = unsafe extern "system" fn(*mut c_void, *mut u8) -> i32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
type HeapStateFn = unsafe extern "system" fn(*mut u32, u32) -> i32;
type MapApuFn = unsafe extern "system" fn(u32) -> *mut c_void;
type MapVaFn = unsafe extern "system" fn(*const c_void) -> u32;

pub struct AcpHal {
    pub create_heap: CreateHeapFn,
    pub alloc: AllocFn,
    pub free: FreeFn,
    pub allocate_shape: ShapeFn,
    pub release_shape: ReleaseShapeFn,
    pub create: CreateFn,
    pub heap_state: HeapStateFn,
    pub map_apu: MapApuFn,
    pub map_va: MapVaFn,
}

pub fn acphal() -> &'static AcpHal {
    static A: OnceLock<(Dll, AcpHal)> = OnceLock::new();
    &A.get_or_init(|| {
        let d = Dll::load("acphal.dll");
        let a = AcpHal {
            create_heap: d.sym("ApuCreateHeap"),
            alloc: d.sym("ApuAlloc"),
            free: d.sym("ApuFree"),
            allocate_shape: d.sym("AcpHalAllocateShapeContexts"),
            release_shape: d.sym("AcpHalReleaseShapeContexts"),
            create: d.sym("AcpHalCreate"),
            heap_state: d.sym("ApuHeapGetState"),
            map_apu: d.sym("ApuMapApuAddress"),
            map_va: d.sym("ApuMapVirtualAddress"),
        };
        (d, a)
    })
    .1
}

pub fn serial() -> MutexGuard<'static, ()> {
    static M: Mutex<()> = Mutex::new(());
    M.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn heap(size: u32) {
    static H: OnceLock<i32> = OnceLock::new();
    let hr = *H.get_or_init(|| unsafe { (acphal().create_heap)(size, 0) });
    assert_eq!(hr, S_OK, "ApuCreateHeap({size:#x}, 0)");
}

pub struct ApuMem {
    pub va: usize,
    pub apu: u32,
    pub size: u32,
}

impl ApuMem {
    pub fn new(size: u32, align: u32, flags: u32) -> ApuMem {
        let mut p: *mut c_void = core::ptr::null_mut();
        let mut a = 0u32;
        let hr = unsafe { (acphal().alloc)(&mut p, &mut a, size, align, flags) };
        assert_eq!(hr, S_OK, "ApuAlloc(size {size:#x}, align {align:#x}, flags {flags})");
        unsafe { core::ptr::write_bytes(p as *mut u8, 0, size as usize) };
        ApuMem { va: p as usize, apu: a, size }
    }

    pub fn rd(&self, off: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.va + off) as *const u32) }
    }

    pub fn wr(&self, off: usize, v: u32) {
        unsafe { core::ptr::write_volatile((self.va + off) as *mut u32, v) }
    }

    pub fn bytes(&self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u8, self.size as usize) }
    }
}

impl Drop for ApuMem {
    fn drop(&mut self) {
        unsafe { (acphal().free)(self.va as *mut c_void) };
    }
}

pub struct ShapeContexts {
    pub desc: [u8; 0x60],
}

impl ShapeContexts {
    pub fn allocate(counts: [u32; 6]) -> ShapeContexts {
        let mut desc = [0u8; 0x60];
        for (k, c) in counts.iter().enumerate() {
            desc[k * 4..k * 4 + 4].copy_from_slice(&c.to_le_bytes());
        }
        let hr = unsafe { (acphal().allocate_shape)(desc.as_mut_ptr()) };
        assert_eq!(hr, S_OK, "AcpHalAllocateShapeContexts({counts:?})");
        ShapeContexts { desc }
    }

    pub fn va(&self, kind: usize) -> usize {
        usize::from_le_bytes(self.desc[0x18 + kind * 8..0x20 + kind * 8].try_into().unwrap())
    }

    pub fn apu(&self, kind: usize) -> u32 {
        u32::from_le_bytes(self.desc[0x48 + kind * 4..0x4c + kind * 4].try_into().unwrap())
    }
}

impl Drop for ShapeContexts {
    fn drop(&mut self) {
        unsafe { (acphal().release_shape)() };
    }
}

pub const SRC: usize = 0;
pub const DMA: usize = 3;
pub const XMA: usize = 4;

pub struct Hal {
    pub obj: *mut c_void,
    connected: bool,
}
unsafe impl Send for Hal {}
unsafe impl Sync for Hal {}

impl Hal {
    pub fn create() -> Hal {
        let mut o: *mut c_void = core::ptr::null_mut();
        assert_eq!(unsafe { (acphal().create)(&mut o) }, S_OK, "AcpHalCreate");
        Hal { obj: o, connected: false }
    }

    fn slot<T: Copy>(&self, i: usize) -> T {
        unsafe {
            let vt = *(self.obj as *const *const usize);
            core::mem::transmute_copy(&*vt.add(i))
        }
    }

    pub fn connect(&mut self, commands: u32, messages: u32) {
        let f: ConnectFn = self.slot(0);
        assert_eq!(unsafe { f(self.obj, commands, messages) }, S_OK, "IAcpHal::Connect({commands:#x}, {messages:#x})");
        self.connected = true;
    }

    pub fn disconnect(&mut self) -> i32 {
        let f: DisconnectFn = self.slot(1);
        self.connected = false;
        unsafe { f(self.obj) }
    }

    pub fn submit(&self, kind: u32, id: u64, frame: u32, payload: &[u8], fence: u32) -> i32 {
        let f: SubmitFn = self.slot(2);
        unsafe { f(self.obj, kind, id, frame, payload.as_ptr(), fence) }
    }

    pub fn pop(&self) -> Option<[u8; 28]> {
        let f: PopFn = self.slot(3);
        let mut m = [0u8; 28];
        (unsafe { f(self.obj, m.as_mut_ptr()) } != 0).then_some(m)
    }
}

impl Drop for Hal {
    fn drop(&mut self) {
        if self.connected {
            self.disconnect();
        }
        let f: ReleaseFn = self.slot(5);
        unsafe { f(self.obj) };
    }
}

pub fn msg_kind(m: &[u8; 28]) -> u32 {
    u32::from_le_bytes(m[0..4].try_into().unwrap())
}

pub fn msg_id(m: &[u8; 28]) -> u64 {
    u64::from_le_bytes(m[16..24].try_into().unwrap())
}

pub const XDEFIANT_DATA: &str = r"D:\xbox-pc-port\games\XDefiant\extracted\Mount\hunter\sdf\scarlett\data";

pub struct Xma2 {
    pub channels: u32,
    pub rate: u32,
    pub samples_encoded: u32,
    pub play_length: u32,
    pub data: Vec<u8>,
}

pub fn load_xma2(file: &str, off: u64, size: usize) -> Xma2 {
    let path = format!(r"{XDEFIANT_DATA}\{file}");
    let mut f = std::fs::File::open(&path).unwrap_or_else(|e| panic!("real XMA2 source {path}: {e}"));
    f.seek(SeekFrom::Start(off)).unwrap();
    let mut b = vec![0u8; size];
    f.read_exact(&mut b).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let mut q = 12;
    let mut fmt = None;
    let mut data = None;
    while q + 8 <= b.len() {
        let id = &b[q..q + 4];
        let len = u32::from_le_bytes(b[q + 4..q + 8].try_into().unwrap()) as usize;
        let body = &b[q + 8..(q + 8 + len).min(b.len())];
        if id == b"fmt " {
            fmt = Some(body.to_vec());
        } else if id == b"data" {
            data = Some(body.to_vec());
        }
        q += 8 + len + (len & 1);
    }
    let fmt = fmt.expect("fmt chunk");
    let u16at = |o: usize| u16::from_le_bytes(fmt[o..o + 2].try_into().unwrap()) as u32;
    let u32at = |o: usize| u32::from_le_bytes(fmt[o..o + 4].try_into().unwrap());
    assert_eq!(u16at(0), 0x166, "WAVE_FORMAT_XMA2");
    assert_eq!(u16at(18), (u16at(2) + 1) / 2, "NumStreams");
    Xma2 { channels: u16at(2), rate: u32at(4), samples_encoded: u32at(24), play_length: u32at(36), data: data.expect("data chunk") }
}

pub const MONO: (&str, u64, usize) = ("sdf-A-0293.sdfdata", 0x1651e91, 16392);
pub const STEREO: (&str, u64, usize) = ("sdf-A-0288.sdfdata", 0x3959878, 45064);

pub fn rms(samples: &[i16], channels: usize, ch: usize) -> f64 {
    let n = samples.len() / channels;
    if n == 0 {
        return 0.0;
    }
    let s: f64 = (0..n).map(|k| (samples[k * channels + ch] as f64).powi(2)).sum();
    (s / n as f64).sqrt()
}
