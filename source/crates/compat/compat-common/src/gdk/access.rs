use super::win::{reg_dword, reg_string, wide, HKCU, HKLM};
use super::{cstr, data_root, log, setting, title, Guid, Iface, This, E_FAIL, E_INVALIDARG, E_NOT_SUFFICIENT_BUFFER, S_OK};
use core::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

pub const CLSID: Guid = Guid::parse("3e241977-6237-41e9-8559-102c2d9983f1");
pub const IID: Guid = Guid::parse("d722b373-8c4d-4692-9e51-d6ad9b37aa7d");

const E_HANDLE: i32 = 0x8007_0006u32 as i32;
const CC_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\ClosedCaptioning";
pub const PROPERTIES_SIZE: usize = 28;

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(k: usize, sub: *const u16, opt: u32, sam: u32, out: *mut usize) -> i32;
    fn RegEnumKeyExW(k: usize, i: u32, name: *mut u16, n: *mut u32, r: *mut u32, c: *mut u16, cn: *mut u32, t: *mut u64) -> i32;
    fn RegCloseKey(k: usize) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(clsid: *const Guid, outer: *mut c_void, ctx: u32, iid: *const Guid, out: *mut *mut c_void) -> i32;
}

fn state_path() -> std::path::PathBuf {
    data_root().join("accessibility.json")
}

fn load_state() -> serde_json::Value {
    std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}))
}

static STATE_LOCK: Mutex<()> = Mutex::new(());

pub fn captions_enabled() -> bool {
    if let Some(v) = setting("closed_captions") {
        return v == "1" || v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true");
    }
    let _g = STATE_LOCK.lock();
    load_state()["closedCaptionsEnabled"].as_bool().unwrap_or(false)
}

fn palette(i: u32, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    match i {
        1 => (255, 255, 255),
        2 => (0, 0, 0),
        3 => (255, 0, 0),
        4 => (0, 255, 0),
        5 => (0, 0, 255),
        6 => (255, 255, 0),
        7 => (255, 0, 255),
        8 => (0, 255, 255),
        _ => fallback,
    }
}

fn opacity(i: u32, fallback: u8) -> u8 {
    match i {
        1 => 255,
        2 => 191,
        3 => 64,
        4 => 0,
        _ => fallback,
    }
}

fn color(name: &str, default_rgb: (u8, u8, u8), default_a: u8) -> [u8; 4] {
    let c = palette(reg_dword(HKCU, CC_KEY, &format!("{name}Color")).unwrap_or(0), default_rgb);
    let a = opacity(reg_dword(HKCU, CC_KEY, &format!("{name}Opacity")).unwrap_or(0), default_a);
    [a, c.0, c.1, c.2]
}

pub fn caption_properties() -> [u8; PROPERTIES_SIZE] {
    let mut b = [0u8; PROPERTIES_SIZE];
    b[0..4].copy_from_slice(&color("Background", (0, 0, 0), 255));
    b[4..8].copy_from_slice(&color("Caption", (255, 255, 255), 255));
    b[8..12].copy_from_slice(&color("Region", (0, 0, 0), 0));
    let edge = reg_dword(HKCU, CC_KEY, "CaptionEffects").unwrap_or(0).min(5);
    let style = reg_dword(HKCU, CC_KEY, "CaptionFontStyle").unwrap_or(0).min(7);
    let scale = match reg_dword(HKCU, CC_KEY, "CaptionSize").unwrap_or(0) {
        1 => 0.5f32,
        3 => 1.5,
        4 => 2.0,
        _ => 1.0,
    };
    b[12..16].copy_from_slice(&edge.to_le_bytes());
    b[16..20].copy_from_slice(&style.to_le_bytes());
    b[20..24].copy_from_slice(&scale.to_le_bytes());
    b[24] = captions_enabled() as u8;
    b
}

unsafe extern "system" fn cc_get_properties(_t: This, out: *mut u8) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    let b = caption_properties();
    core::ptr::copy_nonoverlapping(b.as_ptr(), out, b.len());
    S_OK
}

unsafe extern "system" fn cc_set_enabled(_t: This, enabled: bool) -> i32 {
    let _g = STATE_LOCK.lock();
    let mut s = load_state();
    s["closedCaptionsEnabled"] = serde_json::json!(enabled);
    let _ = std::fs::create_dir_all(data_root());
    match std::fs::write(state_path(), s.to_string()) {
        Ok(()) => {
            log("XClosedCaptionSetEnabled", &format!("{enabled}"));
            S_OK
        }
        Err(e) => super::win::hr_from_win32(e.raw_os_error().unwrap_or(29) as u32),
    }
}

pub fn high_contrast_mode() -> u32 {
    let (on, scheme) = super::win::high_contrast();
    if !on {
        return 0;
    }
    let s = scheme.to_ascii_lowercase();
    if s.contains("white") || s.contains("desert") {
        2
    } else if s.contains("black") || s.contains("#1") || s.contains("#2") || s.contains("aquatic") || s.contains("night") || s.contains("dusk") {
        1
    } else {
        3
    }
}

unsafe extern "system" fn hc_get_mode(_t: This, out: *mut u32) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    *out = high_contrast_mode();
    S_OK
}

static STT_POSITION: AtomicU32 = AtomicU32::new(0);

pub fn speech_to_text_position() -> u32 {
    STT_POSITION.load(Ordering::Relaxed)
}

unsafe extern "system" fn stt_set_position(_t: This, pos: u32) -> i32 {
    if pos > 7 {
        return E_INVALIDARG;
    }
    STT_POSITION.store(pos, Ordering::Relaxed);
    S_OK
}

fn transcript(event: &str, id: Option<u32>, who: &str, text: &str, kind: u32) {
    let line = serde_json::json!({"t": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0), "event": event, "id": id, "speaker": who, "text": text, "type": if kind == 0 { "voice" } else { "text" }, "position": speech_to_text_position()});
    let dir = data_root().join("speech_to_text");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{}.jsonl", super::sanitize(&title().key)))) {
        let _ = writeln!(f, "{line}");
    }
}

fn show(who: &str, text: &str, kind: u32) {
    let tag = if kind == 0 { "(voice)" } else { "(text)" };
    super::win::spawn_toast(format!("{who} {tag}"), text.to_string(), 4000);
}

unsafe extern "system" fn stt_send_string(_t: This, speaker: *const u8, content: *const u8, kind: u32) -> i32 {
    let (Some(who), Some(text)) = (cstr(speaker), cstr(content)) else { return E_INVALIDARG };
    if kind > 1 {
        return E_INVALIDARG;
    }
    transcript("send", None, &who, &text, kind);
    show(&who, &text, kind);
    S_OK
}

#[derive(Clone, Debug)]
pub struct Hypothesis {
    pub speaker: String,
    pub content: String,
    pub kind: u32,
}

fn hypotheses() -> &'static Mutex<(u32, std::collections::BTreeMap<u32, Hypothesis>)> {
    static H: std::sync::OnceLock<Mutex<(u32, std::collections::BTreeMap<u32, Hypothesis>)>> = std::sync::OnceLock::new();
    H.get_or_init(|| Mutex::new((0, std::collections::BTreeMap::new())))
}

pub fn hypothesis(id: u32) -> Option<Hypothesis> {
    hypotheses().lock().ok()?.1.get(&id).cloned()
}

unsafe extern "system" fn stt_begin_hypothesis(_t: This, speaker: *const u8, content: *const u8, kind: u32, id: *mut u32) -> i32 {
    let (Some(who), Some(text)) = (cstr(speaker), cstr(content)) else { return E_INVALIDARG };
    if id.is_null() || kind > 1 {
        return E_INVALIDARG;
    }
    let mut g = hypotheses().lock().unwrap_or_else(|e| e.into_inner());
    let n = g.0;
    g.0 = g.0.wrapping_add(1);
    g.1.insert(n, Hypothesis { speaker: who.clone(), content: text.clone(), kind });
    drop(g);
    *id = n;
    transcript("begin", Some(n), &who, &text, kind);
    S_OK
}

unsafe extern "system" fn stt_update_hypothesis(_t: This, id: u32, content: *const u8) -> i32 {
    let Some(text) = cstr(content) else { return E_INVALIDARG };
    let mut g = hypotheses().lock().unwrap_or_else(|e| e.into_inner());
    let Some(h) = g.1.get_mut(&id) else { return E_INVALIDARG };
    h.content = text.clone();
    let h = h.clone();
    drop(g);
    transcript("update", Some(id), &h.speaker, &text, h.kind);
    S_OK
}

unsafe extern "system" fn stt_finalize_hypothesis(_t: This, id: u32, content: *const u8) -> i32 {
    let Some(text) = cstr(content) else { return E_INVALIDARG };
    let Some(h) = hypotheses().lock().unwrap_or_else(|e| e.into_inner()).1.remove(&id) else { return E_INVALIDARG };
    transcript("finalize", Some(id), &h.speaker, &text, h.kind);
    show(&h.speaker, &text, h.kind);
    S_OK
}

unsafe extern "system" fn stt_cancel_hypothesis(_t: This, id: u32) -> i32 {
    let Some(h) = hypotheses().lock().unwrap_or_else(|e| e.into_inner()).1.remove(&id) else { return E_INVALIDARG };
    transcript("cancel", Some(id), &h.speaker, &h.content, h.kind);
    S_OK
}

#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub token: String,
    pub name: String,
    pub gender: u8,
    pub language: String,
}

const VOICE_ROOTS: &[&str] = &["SOFTWARE\\Microsoft\\Speech\\Voices\\Tokens", "SOFTWARE\\Microsoft\\Speech_OneCore\\Voices\\Tokens"];

fn subkeys(root: usize, path: &str) -> Vec<String> {
    let mut v = Vec::new();
    unsafe {
        let mut k = 0usize;
        if RegOpenKeyExW(root, wide(path).as_ptr(), 0, 0x2_0019, &mut k) != 0 {
            return v;
        }
        let mut i = 0;
        loop {
            let mut buf = [0u16; 256];
            let mut n = buf.len() as u32;
            if RegEnumKeyExW(k, i, buf.as_mut_ptr(), &mut n, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) != 0 {
                break;
            }
            v.push(String::from_utf16_lossy(&buf[..n as usize]));
            i += 1;
        }
        RegCloseKey(k);
    }
    v
}

fn lcid_to_tag(hex: &str) -> String {
    let id = u32::from_str_radix(hex.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0x409);
    let mut buf = [0u16; 85];
    #[link(name = "kernel32")]
    extern "system" {
        fn LCIDToLocaleName(l: u32, name: *mut u16, n: i32, f: u32) -> i32;
    }
    let n = unsafe { LCIDToLocaleName(id, buf.as_mut_ptr(), buf.len() as i32, 0) };
    if n > 1 { String::from_utf16_lossy(&buf[..n as usize - 1]) } else { "en-US".into() }
}

pub fn voices() -> Vec<Voice> {
    let mut v: Vec<Voice> = Vec::new();
    for root in VOICE_ROOTS {
        for t in subkeys(HKLM, root) {
            let key = format!("{root}\\{t}");
            let attrs = format!("{key}\\Attributes");
            let name = reg_string(HKLM, &attrs, "Name").or_else(|| reg_string(HKLM, &key, "")).unwrap_or_else(|| t.clone());
            if v.iter().any(|x| x.name == name) {
                continue;
            }
            let gender = if reg_string(HKLM, &attrs, "Gender").map(|g| g.eq_ignore_ascii_case("male")).unwrap_or(false) { 1 } else { 0 };
            let language = lcid_to_tag(&reg_string(HKLM, &attrs, "Language").unwrap_or_default());
            v.push(Voice { token: format!("HKEY_LOCAL_MACHINE\\{key}"), name, gender, language });
        }
    }
    v
}

struct Synth {
    magic: u64,
    refs: AtomicUsize,
    voice: Mutex<Option<String>>,
}

struct Stream {
    magic: u64,
    refs: AtomicUsize,
    data: Vec<u8>,
}

const SMAGIC: u64 = 0x5354_5453_594E_5448;
const TMAGIC: u64 = 0x5354_5453_5452_4541;

unsafe fn synth<'a>(h: usize) -> Option<&'a Synth> {
    (h as *const Synth).as_ref().filter(|s| s.magic == SMAGIC)
}

unsafe fn stream<'a>(h: usize) -> Option<&'a Stream> {
    (h as *const Stream).as_ref().filter(|s| s.magic == TMAGIC)
}

unsafe extern "system" fn synth_create(_t: This, out: *mut usize) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    *out = Box::into_raw(Box::new(Synth { magic: SMAGIC, refs: AtomicUsize::new(1), voice: Mutex::new(None) })) as usize;
    S_OK
}

unsafe extern "system" fn synth_duplicate(_t: This, h: usize, out: *mut usize) -> i32 {
    let (Some(s), false) = (synth(h), out.is_null()) else { return E_INVALIDARG };
    s.refs.fetch_add(1, Ordering::AcqRel);
    *out = h;
    S_OK
}

unsafe extern "system" fn synth_close(_t: This, h: usize) -> i32 {
    let Some(s) = synth(h) else { return E_HANDLE };
    if s.refs.fetch_sub(1, Ordering::AcqRel) == 1 {
        let mut b = Box::from_raw(h as *mut Synth);
        b.magic = 0;
    }
    S_OK
}

unsafe extern "system" fn synth_default_voice(_t: This, h: usize) -> i32 {
    let Some(s) = synth(h) else { return E_HANDLE };
    *s.voice.lock().unwrap_or_else(|e| e.into_inner()) = None;
    S_OK
}

unsafe extern "system" fn synth_custom_voice(_t: This, h: usize, id: *const u8) -> i32 {
    let (Some(s), Some(id)) = (synth(h), cstr(id)) else { return E_INVALIDARG };
    let Some(v) = voices().into_iter().find(|v| v.token.eq_ignore_ascii_case(&id) || v.token.rsplit('\\').next().map(|t| t.eq_ignore_ascii_case(&id)).unwrap_or(false) || v.name.eq_ignore_ascii_case(&id)) else {
        return E_INVALIDARG;
    };
    *s.voice.lock().unwrap_or_else(|e| e.into_inner()) = Some(v.token);
    S_OK
}

const CLSID_SPVOICE: Guid = Guid::parse("96749377-3391-11d2-9ee3-00c04f797396");
const IID_ISPVOICE: Guid = Guid::parse("6c44df74-72b9-4992-a1ec-ef996e0422d4");
const CLSID_SPSTREAM: Guid = Guid::parse("715d9c59-4442-11d2-9605-00c04f8ee628");
const IID_ISPSTREAM: Guid = Guid::parse("12e3cca9-7518-44c5-a5e7-ba5a79cb929e");
const SPDFID_WAVEFORMATEX: Guid = Guid::parse("c31adbae-527f-4ff5-a230-f62bb61ff70c");
const CLSID_SPOBJECTTOKEN: Guid = Guid::parse("ef411752-3736-4cb4-9c8c-8ef4ccb58efe");
const IID_ISPOBJECTTOKEN: Guid = Guid::parse("14056589-e16c-11d2-bb90-00c04f8ee6c0");

#[repr(C)]
struct WaveFormat {
    tag: u16,
    channels: u16,
    rate: u32,
    bytes_per_sec: u32,
    align: u16,
    bits: u16,
    extra: u16,
}

unsafe fn vcall(obj: *mut c_void, slot: usize) -> usize {
    *(*(obj as *const *const usize)).add(slot)
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(vcall(obj, 2));
        f(obj);
    }
}

fn synthesize(text: String, ssml: bool, voice: Option<String>) -> Result<Vec<u8>, i32> {
    let path = std::env::temp_dir().join(format!("tombox_tts_{}_{}.wav", std::process::id(), super::next_token()));
    let p = path.clone();
    let r = std::thread::spawn(move || unsafe {
        CoInitializeEx(core::ptr::null_mut(), 0);
        let out = (|| {
            let mut v: *mut c_void = core::ptr::null_mut();
            let hr = CoCreateInstance(&CLSID_SPVOICE, core::ptr::null_mut(), 1, &IID_ISPVOICE, &mut v);
            if hr < 0 {
                return Err(hr);
            }
            let mut st: *mut c_void = core::ptr::null_mut();
            let hr = CoCreateInstance(&CLSID_SPSTREAM, core::ptr::null_mut(), 1, &IID_ISPSTREAM, &mut st);
            if hr < 0 {
                release(v);
                return Err(hr);
            }
            let wf = WaveFormat { tag: 1, channels: 1, rate: 24000, bytes_per_sec: 48000, align: 2, bits: 16, extra: 0 };
            let wp = wide(&p.to_string_lossy());
            let bind: unsafe extern "system" fn(*mut c_void, *const u16, u32, *const Guid, *const WaveFormat, u64) -> i32 = core::mem::transmute(vcall(st, 17));
            let mut hr = bind(st, wp.as_ptr(), 3, &SPDFID_WAVEFORMATEX, &wf, 0);
            if hr >= 0 {
                if let Some(tok) = voice {
                    let mut t: *mut c_void = core::ptr::null_mut();
                    if CoCreateInstance(&CLSID_SPOBJECTTOKEN, core::ptr::null_mut(), 1, &IID_ISPOBJECTTOKEN, &mut t) >= 0 {
                        let set_id: unsafe extern "system" fn(*mut c_void, *const u16, *const u16, i32) -> i32 = core::mem::transmute(vcall(t, 15));
                        let tw = wide(&tok);
                        if set_id(t, core::ptr::null(), tw.as_ptr(), 0) >= 0 {
                            let set_voice: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 = core::mem::transmute(vcall(v, 18));
                            set_voice(v, t);
                        }
                        release(t);
                    }
                }
                let set_output: unsafe extern "system" fn(*mut c_void, *mut c_void, i32) -> i32 = core::mem::transmute(vcall(v, 13));
                hr = set_output(v, st, 1);
                if hr >= 0 {
                    let speak: unsafe extern "system" fn(*mut c_void, *const u16, u32, *mut u32) -> i32 = core::mem::transmute(vcall(v, 20));
                    let tw = wide(&text);
                    hr = speak(v, tw.as_ptr(), if ssml { 0x8 | 0x80 } else { 0x10 }, core::ptr::null_mut());
                }
                let close: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(vcall(st, 18));
                close(st);
            }
            release(st);
            release(v);
            if hr < 0 { Err(hr) } else { Ok(()) }
        })();
        CoUninitialize();
        out
    })
    .join()
    .unwrap_or(Err(E_FAIL));
    let data = r.and_then(|_| std::fs::read(&path).map_err(|e| super::win::hr_from_win32(e.raw_os_error().unwrap_or(2) as u32)));
    let _ = std::fs::remove_file(&path);
    data
}

unsafe fn create_stream(h: usize, text: *const u8, ssml: bool, out: *mut usize) -> i32 {
    let (Some(s), Some(t)) = (synth(h), cstr(text)) else { return E_INVALIDARG };
    if out.is_null() {
        return E_INVALIDARG;
    }
    let voice = s.voice.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match synthesize(t, ssml, voice) {
        Ok(data) => {
            log(if ssml { "XSpeechSynthesizerCreateStreamFromSsml" } else { "XSpeechSynthesizerCreateStreamFromText" }, &format!("{} bytes of WAV", data.len()));
            *out = Box::into_raw(Box::new(Stream { magic: TMAGIC, refs: AtomicUsize::new(1), data })) as usize;
            S_OK
        }
        Err(hr) => hr,
    }
}

unsafe extern "system" fn stream_from_text(_t: This, h: usize, text: *const u8, out: *mut usize) -> i32 {
    create_stream(h, text, false, out)
}

unsafe extern "system" fn stream_from_ssml(_t: This, h: usize, ssml: *const u8, out: *mut usize) -> i32 {
    create_stream(h, ssml, true, out)
}

unsafe extern "system" fn stream_close(_t: This, h: usize) -> i32 {
    let Some(s) = stream(h) else { return E_HANDLE };
    if s.refs.fetch_sub(1, Ordering::AcqRel) == 1 {
        let mut b = Box::from_raw(h as *mut Stream);
        b.magic = 0;
    }
    S_OK
}

unsafe extern "system" fn stream_size(_t: This, h: usize, out: *mut usize) -> i32 {
    let (Some(s), false) = (stream(h), out.is_null()) else { return E_INVALIDARG };
    *out = s.data.len();
    S_OK
}

unsafe extern "system" fn stream_data(_t: This, h: usize, size: usize, buf: *mut u8, used: *mut usize) -> i32 {
    let Some(s) = stream(h) else { return E_INVALIDARG };
    if buf.is_null() || size < s.data.len() {
        return E_NOT_SUFFICIENT_BUFFER;
    }
    core::ptr::copy_nonoverlapping(s.data.as_ptr(), buf, s.data.len());
    if !used.is_null() {
        *used = s.data.len();
    }
    S_OK
}

pub fn interfaces() -> Vec<Iface> {
    vec![Iface {
        clsid: CLSID,
        iid: IID,
        name: "XAccessibility+XSpeechSynthesizer(d722b373)",
        slots: vec![
            (3, cc_get_properties as usize),
            (4, cc_set_enabled as usize),
            (5, hc_get_mode as usize),
            (6, stt_set_position as usize),
            (7, stt_send_string as usize),
            (8, synth_duplicate as usize),
            (9, synth_create as usize),
            (10, synth_close as usize),
            (11, synth_default_voice as usize),
            (12, synth_custom_voice as usize),
            (13, stream_from_text as usize),
            (14, stream_close as usize),
            (15, stream_size as usize),
            (16, stream_data as usize),
            (17, stt_begin_hypothesis as usize),
            (18, stt_update_hypothesis as usize),
            (19, stt_finalize_hypothesis as usize),
            (20, stt_cancel_hypothesis as usize),
            (21, stream_from_ssml as usize),
        ],
    }]
}
