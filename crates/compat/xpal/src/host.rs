use crate::nt::*;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const IOCTL_SET_INFORMATION: u32 = 0x0015_013C;
pub const IOCTL_QUERY_TITLE_VM_INFO: u32 = 0x0015_0114;
pub const IOCTL_WAIT_TITLE_VM_TERMINATION: u32 = 0x0015_0118;
pub const IOCTL_INITIATE_SYSTEM_SHUTDOWN: u32 = 0x0015_0120;
pub const IOCTL_INITIATE_POWER_STATE_TOGGLE: u32 = 0x0015_0140;
pub const IOCTL_SET_SYSTEM_TIME: u32 = 0x0015_0148;
pub const IOCTL_BACKGROUND_ACTIVITY_REQUEST: u32 = 0x0015_0180;
pub const IOCTL_QUERY_POWER_TOGGLE_SOURCE: u32 = 0x0015_0184;
pub const IOCTL_REPORT_UEM: u32 = 0x0015_0188;
pub const IOCTL_IS_TRANSITIONING_TO_CS: u32 = 0x0015_018C;
pub const IOCTL_IS_SILENT_BOOT_MODE: u32 = 0x0015_0194;
pub const IOCTL_INITIATE_DUMP: u32 = 0x0015_019C;
pub const IOCTL_REFRESH_POWER_CHIME_MODE: u32 = 0x0015_01BC;
pub const IOCTL_EXIT_FORCED_CS: u32 = 0x0015_01C4;
pub const IOCTL_IS_IN_FORCED_CS: u32 = 0x0015_01C8;
pub const IOCTL_NOTIFY_FORCED_CS_HEARTBEAT: u32 = 0x0015_01CC;
pub const IOCTL_IS_REBOOT_REQUESTED: u32 = 0x0015_01E0;
pub const IOCTL_REQUEST_CRASH_DUMP: u32 = 0x0015_0284;
pub const IOCTL_SET_WAKEUP_TIMER: u32 = 0x0015_0288;

pub const TITLE_VM_INFO_RAW: usize = 0x214;
pub const PACKAGE_NAME_CHARS: usize = 0x80;
pub const AUMID_CHARS: usize = 0x83;
pub const FLAG_SUSPENDED: u32 = 1;
pub const FLAG_CONSTRAINED: u32 = 1 << 5;
pub const MINIDUMP_VALID_TYPE_MASK: u64 = 0x001F_FFFF;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const CRASH_DUMP_TRIAGE: u64 = 1;
pub const CRASH_DUMP_HEAP: u64 = 2;
pub const CRASH_DUMP_MINI: u64 = 4;
pub const MINIDUMP_MINI: u32 = 0x1000 | 0x4;
pub const MINIDUMP_HEAP: u32 = 0x200 | 0x800 | 0x1;
pub const MINIDUMP_TRIAGE: u32 = 0x10_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    On,
    ConnectedStandby,
}

#[derive(Clone, Debug)]
pub struct TitleVm {
    pub id: u32,
    pub package_full_name: String,
    pub aumid: String,
    pub flags: u32,
}

#[derive(Clone, Debug)]
pub struct Dump {
    pub path: PathBuf,
    pub kind: u32,
    pub reason: u64,
    pub correlation: [u8; 16],
}

pub struct State {
    pub title: Option<TitleVm>,
    pub title_running: bool,
    pub power: Power,
    pub transitioning_to_cs: bool,
    pub toggle_source: u32,
    pub background_requests: u32,
    pub reboot_requested: bool,
    pub silent_boot: bool,
    pub forced_cs: bool,
    pub heartbeats: u64,
    pub last_heartbeat: Option<(u32, Instant)>,
    pub uem: Vec<[u32; 3]>,
    pub wakeup_timer: Option<u32>,
    pub wake_at: Option<Instant>,
    pub woken_by_timer: u64,
    pub time_offset: i64,
    pub shutdown: Option<(u32, bool)>,
    pub chime_mode: u32,
    pub information: Vec<u8>,
    pub dumps: Vec<Dump>,
}

pub struct Host {
    pub dump_dir: PathBuf,
    state: Mutex<State>,
    terminated: Condvar,
}

pub fn crockford13(h8: [u8; 8]) -> String {
    const ALPHA: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let v = (u64::from_be_bytes(h8) as u128) << 1;
    (0..13).map(|i| ALPHA[((v >> (60 - 5 * i)) & 31) as usize] as char).collect()
}

pub fn package_publisher_id(publisher: &str) -> Option<String> {
    let w: Vec<u8> = publisher.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    sha256(&w).map(|h| crockford13(h[..8].try_into().unwrap()))
}

fn app_id(dir: &Option<PathBuf>) -> String {
    let text = dir.as_ref().and_then(|d| d.ancestors().take(5).find_map(|a| std::fs::read_to_string(a.join("MicrosoftGame.config")).ok()));
    text.and_then(|t| {
        let s = t.find("<Executable")?;
        let rest = &t[s..t[s..].find('>').map(|e| s + e)?];
        let i = rest.find(" Id=\"")? + 5;
        rest[i..].split('"').next().map(|x| x.to_string())
    })
    .filter(|x| !x.is_empty())
    .unwrap_or_else(|| "Game".into())
}

pub fn title_vm_from_identity() -> TitleVm {
    let t = compat_common::gdk::title();
    let v = t.version;
    let pubid = package_publisher_id(&t.publisher).unwrap_or_default();
    let full = format!("{}_{}.{}.{}.{}_x64__{}", t.identity, v[0], v[1], v[2], v[3], pubid);
    let aumid = format!("{}_{}!{}", t.identity, pubid, app_id(&t.dir));
    TitleVm { id: std::process::id(), package_full_name: full, aumid, flags: 0 }
}

fn put_wide(dst: &mut [u8], s: &str, chars: usize) {
    let w: Vec<u16> = s.encode_utf16().take(chars - 1).collect();
    for (i, c) in w.iter().enumerate() {
        dst[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()))
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|x| u64::from_le_bytes(x.try_into().unwrap()))
}

fn put_out(out: &mut [u8], v: &[u8]) -> Result<usize, i32> {
    if out.len() < v.len() {
        return Err(STATUS_BUFFER_TOO_SMALL);
    }
    out[..v.len()].copy_from_slice(v);
    Ok(v.len())
}

impl Host {
    pub fn new(dump_dir: PathBuf) -> Host {
        Host {
            dump_dir,
            state: Mutex::new(State {
                title: None,
                title_running: true,
                power: Power::On,
                transitioning_to_cs: false,
                toggle_source: 0,
                background_requests: 0,
                reboot_requested: false,
                silent_boot: false,
                forced_cs: false,
                heartbeats: 0,
                last_heartbeat: None,
                uem: Vec::new(),
                wakeup_timer: None,
                wake_at: None,
                woken_by_timer: 0,
                time_offset: 0,
                shutdown: None,
                chime_mode: 0,
                information: Vec::new(),
                dumps: Vec::new(),
            }),
            terminated: Condvar::new(),
        }
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = s.wake_at {
            if Instant::now() >= t {
                s.wake_at = None;
                s.wakeup_timer = None;
                if s.power == Power::ConnectedStandby && !s.forced_cs {
                    s.power = Power::On;
                    s.transitioning_to_cs = false;
                    s.woken_by_timer += 1;
                }
            }
        }
        s
    }

    fn title<'a>(&self, s: &'a mut State) -> &'a TitleVm {
        s.title.get_or_insert_with(title_vm_from_identity)
    }

    pub fn set_title(&self, t: TitleVm) {
        self.state().title = Some(t);
    }

    pub fn terminate_title(&self) {
        self.state().title_running = false;
        self.terminated.notify_all();
    }

    pub fn enter_forced_cs(&self) {
        let mut s = self.state();
        s.forced_cs = true;
        s.power = Power::ConnectedStandby;
    }

    pub fn set_silent_boot(&self, v: bool) {
        self.state().silent_boot = v;
    }

    pub fn complete_transition(&self) {
        self.state().transitioning_to_cs = false;
    }

    pub fn emulated_system_time(&self) -> u64 {
        (now_filetime() as i64).wrapping_add(self.state().time_offset) as u64
    }

    fn write_dump(&self, kind: u32, reason: u64, correlation: [u8; 16]) -> Result<usize, i32> {
        let write = minidump_writer().ok_or(STATUS_UNSUCCESSFUL)?;
        std::fs::create_dir_all(&self.dump_dir).map_err(|_| STATUS_UNSUCCESSFUL)?;
        let n = self.state().dumps.len();
        let path = self.dump_dir.join(format!("title-{}-{}-{:x}-{}.dmp", std::process::id(), n, reason, now_filetime()));
        let f = std::fs::File::create(&path).map_err(|_| STATUS_UNSUCCESSFUL)?;
        let ok = unsafe { write(GetCurrentProcess(), std::process::id(), f.as_raw_handle() as *mut _, kind, core::ptr::null(), core::ptr::null(), core::ptr::null()) };
        drop(f);
        if ok == 0 {
            let _ = std::fs::remove_file(&path);
            return Err(STATUS_UNSUCCESSFUL);
        }
        self.state().dumps.push(Dump { path, kind, reason, correlation });
        Ok(0)
    }

    fn wait_title(&self, input: &[u8]) -> Result<usize, i32> {
        let len = u32_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)? as usize;
        let timeout = u32_at(input, 0x10).ok_or(STATUS_INVALID_PARAMETER)?;
        let name = input.get(0x18..0x18 + len).ok_or(STATUS_INVALID_PARAMETER)?;
        let w: Vec<u16> = name.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
        let name = String::from_utf16_lossy(&w);
        let mut s = self.state();
        let t = self.title(&mut s).clone();
        if !name.eq_ignore_ascii_case(&t.package_full_name) && !name.eq_ignore_ascii_case(&t.aumid) {
            return Err(STATUS_NOT_FOUND);
        }
        let deadline = (timeout != INFINITE).then(|| Instant::now() + Duration::from_millis(timeout as u64));
        while s.title_running {
            match deadline {
                None => s = self.terminated.wait(s).unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return Err(STATUS_IO_TIMEOUT);
                    }
                    s = self.terminated.wait_timeout(s, d - now).unwrap_or_else(|e| e.into_inner()).0;
                }
            }
        }
        Ok(0)
    }

    pub fn ioctl(&self, code: u32, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        match code {
            IOCTL_QUERY_TITLE_VM_INFO => {
                let mut s = self.state();
                let running = s.title_running;
                let t = self.title(&mut s).clone();
                let mut b = vec![0u8; TITLE_VM_INFO_RAW];
                b[0..4].copy_from_slice(&(running as u32).to_le_bytes());
                b[4..8].copy_from_slice(&t.id.to_le_bytes());
                b[8..12].copy_from_slice(&t.flags.to_le_bytes());
                put_wide(&mut b[0xC..0x10C], &t.package_full_name, PACKAGE_NAME_CHARS);
                put_wide(&mut b[0x10C..0x212], &t.aumid, AUMID_CHARS);
                put_out(out, &b)
            }
            IOCTL_WAIT_TITLE_VM_TERMINATION => self.wait_title(input),
            IOCTL_INITIATE_SYSTEM_SHUTDOWN => {
                let reason = u32_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)?;
                let reboot = u32_at(input, 4).ok_or(STATUS_INVALID_PARAMETER)? != 0;
                {
                    let mut s = self.state();
                    s.shutdown = Some((reason, reboot));
                    s.reboot_requested |= reboot;
                    s.title_running = false;
                }
                self.terminated.notify_all();
                Ok(0)
            }
            IOCTL_INITIATE_POWER_STATE_TOGGLE => {
                let src = u32_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)?;
                let mut s = self.state();
                s.toggle_source = src;
                match s.power {
                    Power::On => {
                        s.power = Power::ConnectedStandby;
                        s.transitioning_to_cs = true;
                    }
                    Power::ConnectedStandby => {
                        s.power = Power::On;
                        s.transitioning_to_cs = false;
                        s.forced_cs = false;
                    }
                }
                Ok(0)
            }
            IOCTL_QUERY_POWER_TOGGLE_SOURCE => {
                let v = self.state().toggle_source;
                put_out(out, &v.to_le_bytes())
            }
            IOCTL_BACKGROUND_ACTIVITY_REQUEST => {
                let on = u32_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)? != 0;
                let mut s = self.state();
                if on {
                    s.background_requests += 1;
                } else if s.background_requests == 0 {
                    return Err(STATUS_INVALID_DEVICE_STATE);
                } else {
                    s.background_requests -= 1;
                }
                Ok(0)
            }
            IOCTL_REFRESH_POWER_CHIME_MODE => {
                let v = compat_common::gdk::setting("xpal_power_chime").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                self.state().chime_mode = v;
                Ok(0)
            }
            IOCTL_IS_REBOOT_REQUESTED => {
                let v = self.state().reboot_requested as u8;
                put_out(out, &[v])
            }
            IOCTL_IS_SILENT_BOOT_MODE => {
                let v = self.state().silent_boot as u32;
                put_out(out, &v.to_le_bytes())
            }
            IOCTL_IS_TRANSITIONING_TO_CS => {
                let v = self.state().transitioning_to_cs as u32;
                put_out(out, &v.to_le_bytes())
            }
            IOCTL_EXIT_FORCED_CS => {
                let mut s = self.state();
                if !s.forced_cs {
                    return Err(STATUS_INVALID_DEVICE_STATE);
                }
                s.forced_cs = false;
                s.power = Power::On;
                s.transitioning_to_cs = false;
                Ok(0)
            }
            IOCTL_IS_IN_FORCED_CS => {
                let v = self.state().forced_cs as u32;
                put_out(out, &v.to_le_bytes())
            }
            IOCTL_NOTIFY_FORCED_CS_HEARTBEAT => {
                let v = u32_at(input, 0).unwrap_or(0);
                let mut s = self.state();
                s.heartbeats += 1;
                s.last_heartbeat = Some((v, Instant::now()));
                Ok(0)
            }
            IOCTL_REPORT_UEM => {
                let a = [u32_at(input, 0), u32_at(input, 4), u32_at(input, 8)];
                let [Some(x), Some(y), Some(z)] = a else { return Err(STATUS_INVALID_PARAMETER) };
                self.state().uem.push([x, y, z]);
                compat_common::record("xpal", "XpalReportUem", &format!("{x:#x} {y:#x} {z:#x}"));
                Ok(0)
            }
            IOCTL_INITIATE_DUMP => {
                let kind = u64_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)?;
                if kind & !MINIDUMP_VALID_TYPE_MASK != 0 {
                    return Err(STATUS_INVALID_PARAMETER);
                }
                self.write_dump(kind as u32, 0, [0; 16])
            }
            IOCTL_REQUEST_CRASH_DUMP => {
                let flags = u64_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)?;
                let kind = u64_at(input, 8).ok_or(STATUS_INVALID_PARAMETER)?;
                if input.len() < 0x28 || flags >= 8 || kind as i64 >= 9 {
                    return Err(STATUS_INVALID_PARAMETER);
                }
                let mut mk = 0u32;
                if flags & CRASH_DUMP_MINI != 0 {
                    mk |= MINIDUMP_MINI;
                }
                if flags & CRASH_DUMP_HEAP != 0 {
                    mk |= MINIDUMP_HEAP;
                }
                if flags & CRASH_DUMP_TRIAGE != 0 {
                    mk |= MINIDUMP_TRIAGE;
                }
                let mut correlation = [0u8; 16];
                correlation.copy_from_slice(&input[0x10..0x20]);
                self.write_dump(mk, kind, correlation)
            }
            IOCTL_SET_WAKEUP_TIMER => {
                let v = u32_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)?;
                let mut s = self.state();
                s.wakeup_timer = (v != 0).then_some(v);
                s.wake_at = (v != 0).then(|| Instant::now() + Duration::from_secs(v as u64));
                Ok(0)
            }
            IOCTL_SET_SYSTEM_TIME => {
                let t = u64_at(input, 0).ok_or(STATUS_INVALID_PARAMETER)?;
                if t > i64::MAX as u64 {
                    return Err(STATUS_INVALID_PARAMETER);
                }
                self.state().time_offset = (t as i64).wrapping_sub(now_filetime() as i64);
                Ok(0)
            }
            IOCTL_SET_INFORMATION => {
                self.state().information = input.to_vec();
                Ok(0)
            }
            _ => Err(STATUS_INVALID_DEVICE_REQUEST),
        }
    }
}
