pub mod host;
pub mod wire;
pub mod xcrd;

use host::{Fault, GameHost, HR_NO_CONNECTION};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use wire::*;

pub const FIRST_HOST_PROTOCOL_BUILD: u32 = 22000;

static HOST: Mutex<Option<Result<GameHost, i32>>> = Mutex::new(None);
static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);
static WER_CLIENT: Mutex<Option<u64>> = Mutex::new(None);

pub fn os_build() -> u32 {
    compat_common::os_host::routes::current_build()
}

pub fn capability_from(dll: &str, n: i32) -> Result<bool, i32> {
    let m = xcrd::load(dll)?;
    let f = xcrd::proc(m, "XIsCapabilityEnabled")?;
    let f: extern "system" fn(i32) -> u8 = unsafe { core::mem::transmute(f) };
    Ok(f(n) != 0)
}

pub fn start_host(os_build: u32) -> Result<GameHost, i32> {
    let cap = if os_build >= 22621 { capability_from("xpal.dll", host::CAPABILITY_LOCAL_DUMPS)? } else { false };
    Ok(GameHost::new(os_build, cap, Box::new(xcrd::DllXcrd::new("XCRDAPI.dll"))))
}

fn guard() -> MutexGuard<'static, Option<Result<GameHost, i32>>> {
    HOST.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn install(h: GameHost) {
    *guard() = Some(Ok(h));
}

pub fn reset() {
    *guard() = None;
    *WER_CLIENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn with_host<R>(f: impl FnOnce(&mut GameHost) -> R) -> Result<R, i32> {
    let mut g = guard();
    if g.is_none() {
        *g = Some(start_host(os_build()));
    }
    match g.as_mut().unwrap() {
        Ok(h) => Ok(f(h)),
        Err(_) => Err(HR_NO_CONNECTION),
    }
}

pub fn connect() -> Result<u64, i32> {
    with_host(|_| NEXT_CLIENT.fetch_add(1, Ordering::Relaxed))
}

pub fn send(client: u64, msg: &Message) -> Result<Result<Option<Message>, Fault>, i32> {
    with_host(|h| h.dispatch(client, msg))
}

pub fn disconnect(client: u64) {
    let _ = with_host(|h| h.disconnect(client));
}

pub fn fail_fast() -> ! {
    unsafe { core::arch::asm!("int 0x29", in("ecx") 7u32, options(noreturn)) }
}

pub fn validate_crash_dump_reply(m: &Message) -> Result<CrashDumpInfo, Fault> {
    if m.id != MSG_CRASH_DUMP_INFO {
        return Err(Fault::FailFast { file: "gamehostserviceproxy.cpp", line: 0x30 });
    }
    if m.payload.len() != CRASH_DUMP_INFO_SIZE {
        return Err(Fault::FailFast { file: "gamehostserviceproxy.cpp", line: 0x27 });
    }
    Ok(CrashDumpInfo::decode(&m.payload))
}

pub mod wer {
    use super::*;

    pub fn uses_host(os_build: u32) -> bool {
        os_build >= FIRST_HOST_PROTOCOL_BUILD
    }

    pub fn sends_progress(os_build: u32) -> bool {
        os_build >= 22621
    }

    fn client() -> Result<u64, i32> {
        let mut c = WER_CLIENT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(id) = *c {
            return Ok(id);
        }
        let id = connect()?;
        *c = Some(id);
        Ok(id)
    }

    pub fn dump_notify_start() -> Result<Option<(u64, String)>, i32> {
        if !uses_host(os_build()) {
            return Ok(None);
        }
        let id = client()?;
        let reply = match send(id, &Message::new(MSG_CRASH_DUMP_INFO, Vec::new()))? {
            Ok(Some(r)) => r,
            Ok(None) | Err(_) => fail_fast(),
        };
        match validate_crash_dump_reply(&reply) {
            Ok(info) => Ok(Some((info.flags(), String::from_utf16_lossy(&info.file)))),
            Err(_) => fail_fast(),
        }
    }

    pub fn transport_event(kind: u64) -> Option<i32> {
        if !sends_progress(os_build()) {
            return None;
        }
        let Some(id) = *WER_CLIENT.lock().unwrap_or_else(|e| e.into_inner()) else { return Some(HR_NO_CONNECTION) };
        Some(match send(id, &Message::new(MSG_CRASH_DUMP_PROGRESS, kind.to_le_bytes().to_vec())) {
            Ok(_) => 0,
            Err(e) => e,
        })
    }

    pub fn dump_notify_complete() {
        let id = WER_CLIENT.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(id) = id {
            disconnect(id);
        }
    }
}
