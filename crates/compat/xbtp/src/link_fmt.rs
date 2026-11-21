use crate::sys::*;
use core::ffi::c_void;

pub type LineCallback = unsafe extern "C" fn(*const u8, *mut c_void) -> i32;

fn clip(s: &[u8], size: usize) -> Vec<u8> {
    let n = s.len().min(size - 1);
    let mut v = s[..n].to_vec();
    v.push(0);
    v
}

fn line(s: String, size: usize) -> Vec<u8> {
    clip(s.as_bytes(), size)
}

unsafe fn emit(cb: LineCallback, ctx: *mut c_void, l: &[u8]) -> bool {
    cb(l.as_ptr(), ctx) != 0
}

unsafe fn rd<T: Copy>(p: *const u8, off: usize) -> T {
    core::ptr::read_unaligned(p.add(off) as *const T)
}

unsafe fn be_words(p: *const u8) -> [u16; 8] {
    let mut w = [0u16; 8];
    for (i, x) in w.iter_mut().enumerate() {
        *x = u16::from_be_bytes([*p.add(i * 2), *p.add(i * 2 + 1)]);
    }
    w
}

fn words_hex(w: &[u16; 8], upper: bool) -> String {
    w.iter().map(|x| if upper { format!("{:X}", x) } else { format!("{:x}", x) }).collect::<Vec<_>>().join(":")
}

unsafe fn cstr_bounded(p: *const u8, max: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..max {
        let c = *p.add(i);
        if c == 0 {
            break;
        }
        v.push(c);
    }
    v
}

unsafe fn wide_text(p: *const u8, count: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..count {
        let c: u16 = rd(p, i * 2);
        if c == 0 || c > 0xff {
            break;
        }
        v.push(c as u8);
    }
    v
}

fn packet_name(code: u8, int_kind: u32, sub: &mut u32) -> Vec<u8> {
    let s: &str = match code {
        1 => "DATA",
        2 => "CREQ",
        3 => "CRES",
        4 => "DISC",
        5 => "END ",
        0x80 => match int_kind {
            0 => "INT0",
            1 => "INT1",
            _ => {
                *sub = int_kind;
                "INT?"
            }
        },
        0x81 => "GW_I",
        0x82 => "RCON",
        0x40 => "GW_C",
        0x41 => "GW_O",
        _ => return line(format!("0x{:02X}", code), 0xc),
    };
    clip(s.as_bytes(), 0xc)
}

fn text(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

unsafe fn option_line(prefix: &str, ty: u16, len: u16, d: *const u8) -> Option<String> {
    let xtd = |name: &str| -> Option<String> {
        if len >= 0xa {
            return None;
        }
        let n = ((len >> 1) as usize).min(0x3f);
        Some(format!("{}:O:{} {}", prefix, name, text(&wide_text(d, n))))
    };
    match ty {
        1 if len >= 0x10 => {
            let mut s = format!("{}:O:HOST_ID ", prefix);
            for i in 0..16 {
                s.push_str(&format!("{:02X}", *d.add(i)));
            }
            Some(s)
        }
        2 => xtd("XTD_REQ"),
        3 if len >= 0x10 => Some(format!("{}:O:REDIRECT {}", prefix, words_hex(&be_words(d), true))),
        4 => xtd("XTD_ACK"),
        5 => xtd("XTD_FAIL"),
        6 => xtd("XTD_PASS"),
        7 => xtd("XTD_RESET"),
        9 if len >= 4 => Some(format!("{}:O:PROT_VER {:08X}", prefix, rd::<u32>(d, 0))),
        0xa if len >= 8 => Some(format!("{}:O:LINK_INDEX {}", prefix, rd::<i64>(d, 0))),
        0xb if len >= 4 => Some(format!("{}:O:CONN_FLAGS {:08X}", prefix, rd::<u32>(d, 0))),
        0xc if len >= 2 => Some(format!("{}:O:PATH_INFO TCP:{} XTD:{}", prefix, *d, *d.add(1))),
        0xe => xtd("XTD_KILL"),
        0xf if len >= 2 => Some(format!("{}:O:XTD_PENDING_RECV: {}", prefix, rd::<u16>(d, 0))),
        _ => None,
    }
}

#[no_mangle]
pub unsafe extern "C" fn XbtpLinkDecodePacketRecord(rec: *const u8, size: u32, cb: LineCallback, ctx: *mut c_void) -> HRESULT {
    if size < 0xa0 {
        return E_INVALIDARG;
    }
    let head: u32 = rd(rec, 0);
    let prefix = text(&line(format!("{:04X}:{:016}", rd::<u16>(rec, 4), rd::<i64>(rec, 0x10)), 0x20));
    let msg = rec.add(0x20);
    let kind = head & 0xf000_0000;
    if kind == 0 {
        let dir = if head & 2 != 0 { "O" } else if head & 1 != 0 { "I" } else { "????" };
        let mut sub = *rec.add(0x21) as u32;
        let name = packet_name(*msg, rd(rec, 0x58), &mut sub);
        let src = be_words(rec.add(0x34));
        let dst = be_words(rec.add(0x48));
        let mut cur = line(
            format!(
                "{}:P:{}:{}:{:02X} {}:{:X} - {}:{:X} {}[{}]",
                prefix,
                dir,
                text(&name),
                sub,
                words_hex(&src, true),
                rd::<u32>(rec, 0x30),
                words_hex(&dst, true),
                rd::<u32>(rec, 0x44),
                rd::<i64>(rec, 0x28),
                rd::<i32>(rec, 0x24)
            ),
            0x200,
        );
        if !emit(cb, ctx, &cur) {
            return E_ABORT;
        }
        let mut opts = [(core::ptr::null::<u8>(), 0u16, 0u16); 0x20];
        let n = super::parse_options(msg, &mut opts);
        for o in &opts[..n as usize] {
            if let Some(s) = option_line(&prefix, o.1, o.2, o.0) {
                cur = line(s, 0x200);
            }
            if !emit(cb, ctx, &cur) {
                return E_ABORT;
            }
        }
        return S_OK;
    }
    if kind == 0x2000_0000 {
        if cstr_bounded(msg, 0x80).len() < 0x80 && cb(msg, ctx) == 0 {
            return E_ABORT;
        }
        return S_OK;
    }
    let raw = cstr_bounded(msg, size as usize - 0x20);
    let mut s = format!("{}:L:", prefix).into_bytes();
    s.extend_from_slice(&raw);
    if emit(cb, ctx, &clip(&s, 0x200)) {
        S_OK
    } else {
        E_ABORT
    }
}

pub unsafe fn format_link_address(a: *const u8) -> String {
    let w = be_words(a.add(4));
    let s = if w[..5].iter().all(|&x| x == 0) && w[5] == 0xffff {
        format!("{}.{}.{}.{}", *a.add(0x10), *a.add(0x11), *a.add(0x12), *a.add(0x13))
    } else if w[..7].iter().all(|&x| x == 0) && w[7] == 1 {
        "[::1]".to_string()
    } else if w.iter().all(|&x| x == 0) {
        "[::]".to_string()
    } else {
        format!("[{}]", words_hex(&w, false))
    };
    text(&line(s, 0x40))
}

#[no_mangle]
pub unsafe extern "C" fn XbtpLinkDecodeStatisticRecord(rec: *const u8, size: u32, cb: LineCallback, ctx: *mut c_void) -> HRESULT {
    if size < 0x98 {
        return E_INVALIDARG;
    }
    let t = *rec.add(4);
    let (a, b) = if t != 0 { (format_link_address(rec.add(0x68)), format_link_address(rec.add(0x7c))) } else { (String::new(), String::new()) };
    let state = if rd::<u32>(rec, 4) & 0x20000 != 0 { "CLOSED" } else { "ACTIVE" };
    let q = |o: usize| rd::<i64>(rec, o);
    let mbs = |o: usize| rd::<u64>(rec, o) as f64 / 1_000_000.0;
    let mut lines: Vec<String> = Vec::new();
    match t {
        0 => {
            if rd::<i64>(rec, 8) == -1 {
                lines.push("Default Gateway".to_string());
            } else {
                lines.push("Gateway".to_string());
                lines.push(format!(" TCP: Recv: {} bytes ({:.3} MB/s) Send: {} bytes ({:.3} MB/s)", q(0x38), mbs(0x40), q(0x18), mbs(0x20)));
                lines.push(format!(" XTD: Recv: {} bytes ({:.3} MB/s) Send: {} bytes ({:.3} MB/s)", q(0x48), mbs(0x50), q(0x28), mbs(0x30)));
            }
        }
        1 => lines.push(format!(" Address {}", a)),
        3 => lines.push(format!(" Listening [::1]:{}", rd::<i32>(rec, 0x68))),
        4 | 5 => {
            let p90 = rd::<i32>(rec, 0x90);
            let p68 = rd::<u32>(rec, 0x68);
            let p7c = rd::<u32>(rec, 0x7c);
            if t == 4 {
                lines.push(format!(" Port [::1]:{}-{:08X} {}:{}-{:X} IN {}", p90, p68, b, 0, p7c, state));
            } else {
                lines.push(format!(" Port [::1]:{}-{:08X} {}:{}-{:X} OUT {}", 0, p68, b, p90, p7c, state));
            }
            lines.push(format!(
                "  Recv: {}/{} Send: {}/{} Stall: {}.{:03} Lifetime: {}.{:03}",
                q(0x38),
                q(0x40),
                q(0x18),
                q(0x20),
                q(0x58) / 1000,
                q(0x58) % 1000,
                q(0x60) / 1000,
                q(0x60) % 1000
            ));
        }
        _ => {}
    }
    for l in lines {
        if !emit(cb, ctx, &line(l, 0x100)) {
            return E_ABORT;
        }
    }
    S_OK
}
