use std::path::{Path, PathBuf};

pub const ENTRY_VAR: &str = "TOMBOX_GAMEHOST_ENTRY";
pub const OBSERVER_VAR: &str = "TOMBOX_TITLE_OBSERVER";

pub const OP_CONNECT: u32 = 1;
pub const OP_SEND: u32 = 2;
pub const OP_DISCONNECT: u32 = 3;
pub const OP_QUIESCE: u32 = 4;

pub const REPLY_NONE: i32 = 0;
pub const REPLY_MESSAGE: i32 = 1;
pub const FAULT_FAIL_FAST: i32 = 2;
pub const FAULT_MALFORMED: i32 = 3;
pub const FAULT_OUT_OF_TABLE: i32 = 4;

pub const MSG_LAUNCH_NEW_GAME: u32 = 0x67;

pub type EntryFn = unsafe extern "system" fn(op: u32, input: *const u8, input_len: usize, out: *mut u8, out_cap: usize, out_len: *mut usize) -> i32;

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
}

pub fn pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

pub fn entry_value(f: usize) -> String {
    format!("{}:{:x}", pid(), f)
}

pub fn publish_entry(f: usize) {
    std::env::set_var(ENTRY_VAR, entry_value(f));
}

pub fn parse_entry(v: &str, my_pid: u32) -> Option<usize> {
    let (p, f) = v.split_once(':')?;
    if p.parse::<u32>().ok()? != my_pid {
        return None;
    }
    usize::from_str_radix(f.trim_start_matches("0x"), 16).ok().filter(|&f| f != 0)
}

pub fn entry() -> Option<usize> {
    parse_entry(&std::env::var(ENTRY_VAR).ok()?, pid())
}

pub const OUT_CAP: usize = 0x10000;
pub const HR_INSUFFICIENT_BUFFER: i32 = 0x8007007a_u32 as i32;

pub fn call(f: usize, op: u32, input: &[u8]) -> (i32, Vec<u8>) {
    let e: EntryFn = unsafe { core::mem::transmute(f) };
    let mut out = vec![0u8; OUT_CAP];
    let mut len = 0usize;
    let r = unsafe { e(op, input.as_ptr(), input.len(), out.as_mut_ptr(), out.len(), &mut len) };
    if len > out.len() {
        return (HR_INSUFFICIENT_BUFFER, Vec::new());
    }
    out.truncate(len);
    (r, out)
}

pub fn put_wstr(v: &mut Vec<u8>, s: &[u16]) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
    v.extend_from_slice(&(s.len() as u32).to_le_bytes());
    while v.len() % 2 != 0 {
        v.push(0);
    }
    for c in s {
        v.extend_from_slice(&c.to_le_bytes());
    }
}

pub fn send_input(client: u64, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + payload.len());
    v.extend_from_slice(&client.to_le_bytes());
    v.extend_from_slice(&id.to_le_bytes());
    v.extend_from_slice(payload);
    v
}

pub fn quiesce_input(hr: i32, time: [u16; 8], module: &[u16]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20 + module.len() * 2);
    v.extend_from_slice(&hr.to_le_bytes());
    for t in time {
        v.extend_from_slice(&t.to_le_bytes());
    }
    for c in module {
        v.extend_from_slice(&c.to_le_bytes());
    }
    v
}

pub fn host_quiesce(hr: i32, time: [u16; 8], module: &[u16]) -> Option<i32> {
    let f = entry()?;
    Some(call(f, OP_QUIESCE, &quiesce_input(hr, time, module)).0)
}

pub fn host_launch_new_game(exe: &str, args: &str) -> Option<i32> {
    let f = entry()?;
    let (r, out) = call(f, OP_CONNECT, &[]);
    if r < 0 {
        return Some(r);
    }
    let client = u64::from_le_bytes(out.get(..8)?.try_into().ok()?);
    let mut payload = Vec::new();
    put_wstr(&mut payload, &exe.encode_utf16().collect::<Vec<_>>());
    put_wstr(&mut payload, &args.encode_utf16().collect::<Vec<_>>());
    let (r, _) = call(f, OP_SEND, &send_input(client, MSG_LAUNCH_NEW_GAME, &payload));
    call(f, OP_DISCONNECT, &client.to_le_bytes());
    Some(if r < 0 { r } else { 0 })
}

pub fn observer_dir() -> Option<PathBuf> {
    std::env::var_os(OBSERVER_VAR).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn plan_path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.launch"))
}

pub fn encode_plan(mode: u32, exe: &Path, args: &str) -> String {
    format!("{mode}\n{}\n{}\n", exe.display(), args.replace(['\r', '\n'], " "))
}

pub fn decode_plan(s: &str) -> Option<(u32, PathBuf, String)> {
    let mut l = s.splitn(3, '\n');
    let mode = l.next()?.trim().parse().ok()?;
    let exe = PathBuf::from(l.next()?.trim_end_matches('\r'));
    let args = l.next().unwrap_or("").trim_end_matches(['\r', '\n']).to_string();
    if exe.as_os_str().is_empty() {
        return None;
    }
    Some((mode, exe, args))
}

pub fn resolve(exe: &str) -> PathBuf {
    let mut path = PathBuf::from(super::fs::translate(exe).unwrap_or_else(|| exe.to_string()));
    if path.is_relative() {
        if let Some(d) = super::title().dir.as_ref() {
            path = d.join(&path);
        }
    }
    path
}

pub fn spawn(path: &Path, args: &str) -> Result<u32, i32> {
    let mut cmd = std::process::Command::new(path);
    cmd.args(super::system::split_args(args));
    cmd.env_remove(ENTRY_VAR);
    if let Some(d) = path.parent() {
        cmd.current_dir(d);
    }
    match cmd.spawn() {
        Ok(c) => Ok(c.id()),
        Err(e) => Err(super::win::hr_from_win32(e.raw_os_error().unwrap_or(2) as u32)),
    }
}
