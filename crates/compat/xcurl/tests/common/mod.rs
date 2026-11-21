#![allow(dead_code)]

use core::ffi::c_void;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

pub struct Sink {
    pub body: Vec<u8>,
    pub headers: Vec<Vec<u8>>,
    pub threads: Vec<std::thread::ThreadId>,
}

impl Sink {
    pub fn new() -> Box<Sink> {
        Box::new(Sink { body: Vec::new(), headers: Vec::new(), threads: Vec::new() })
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.iter().find_map(|h| {
            let s = String::from_utf8_lossy(h);
            let (k, v) = s.split_once(':')?;
            if k.trim().eq_ignore_ascii_case(name) {
                Some(v.trim().to_string())
            } else {
                None
            }
        })
    }

    pub fn ptr(&mut self) -> usize {
        self as *mut Sink as usize
    }
}

pub unsafe extern "C" fn write_cb(p: *mut u8, size: usize, n: usize, ud: *mut c_void) -> usize {
    let s = &mut *(ud as *mut Sink);
    s.threads.push(std::thread::current().id());
    s.body.extend_from_slice(core::slice::from_raw_parts(p, size * n));
    size * n
}

pub unsafe extern "C" fn header_cb(p: *mut u8, size: usize, n: usize, ud: *mut c_void) -> usize {
    let s = &mut *(ud as *mut Sink);
    s.threads.push(std::thread::current().id());
    s.headers.push(core::slice::from_raw_parts(p, size * n).to_vec());
    size * n
}

pub struct Source {
    pub data: Vec<u8>,
    pub pos: usize,
    pub threads: Vec<std::thread::ThreadId>,
}

pub unsafe extern "C" fn read_cb(p: *mut u8, size: usize, n: usize, ud: *mut c_void) -> usize {
    let s = &mut *(ud as *mut Source);
    s.threads.push(std::thread::current().id());
    let k = (size * n).min(s.data.len() - s.pos);
    core::ptr::copy_nonoverlapping(s.data.as_ptr().add(s.pos), p, k);
    s.pos += k;
    k
}

fn respond(w: &mut TcpStream, status: &str, headers: &[(&str, String)], body: &[u8]) {
    let mut h = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (k, v) in headers {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    let _ = w.write_all(h.as_bytes());
    let _ = w.write_all(body);
    let _ = w.flush();
}

fn handle(stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut w = stream.try_clone().unwrap();
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    if r.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut l = String::new();
        if r.read_line(&mut l).unwrap_or(0) == 0 {
            break;
        }
        let l = l.trim_end().to_string();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let hv = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let len: usize = hv("content-length").parse().unwrap_or(0);
    let mut body = vec![0u8; len];
    let _ = r.read_exact(&mut body);
    let path = target.split('?').next().unwrap_or("").to_string();
    let echo = vec![
        ("X-Method", method.clone()),
        ("X-Target", target.clone()),
        ("X-Got-Content-Type", hv("content-type")),
        ("X-Got-Custom", hv("x-custom")),
        ("X-Got-Accept", hv("accept")),
        ("X-Got-Authorization", hv("authorization")),
        ("X-Got-Empty", if headers.iter().any(|(k, _)| k == "x-empty") { "present".to_string() } else { "absent".to_string() }),
    ];
    match path.as_str() {
        "/hello" => respond(&mut w, "200 OK", &[("Content-Type", "text/plain".into()), ("X-Test", "1".into())], b"hello world"),
        "/echo" => {
            let mut h = echo.clone();
            h.push(("Content-Type", "application/octet-stream".into()));
            respond(&mut w, "200 OK", &h, &body)
        }
        "/redirect" => respond(&mut w, "302 Found", &[("Location", "/hello".into())], b"moved"),
        "/redirect-post" => respond(&mut w, "302 Found", &[("Location", "echo".into())], b""),
        "/loop" => respond(&mut w, "302 Found", &[("Location", "/loop".into())], b""),
        "/slow" => {
            std::thread::sleep(Duration::from_millis(2500));
            respond(&mut w, "200 OK", &[], b"late")
        }
        "/missing" => respond(&mut w, "404 Not Found", &[("Content-Type", "text/plain".into())], b"nope"),
        "/binary" => respond(&mut w, "200 OK", &[], &[0, 1, 2, 0, 255, 0, 7]),
        "/big" => {
            let b: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
            respond(&mut w, "200 OK", &[], &b)
        }
        "/chunked" => {
            let _ = w.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n");
        }
        _ => respond(&mut w, "404 Not Found", &echo, b""),
    }
}

pub fn server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming() {
            if let Ok(s) = c {
                std::thread::spawn(move || handle(s));
            }
        }
    });
    port
}

pub fn log_file(name: &str) -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    p.push(name);
    p
}

pub const CURLOPT_WRITEDATA: u32 = 10001;
pub const CURLOPT_URL: u32 = 10002;
pub const CURLOPT_READDATA: u32 = 10009;
pub const CURLOPT_ERRORBUFFER: u32 = 10010;
pub const CURLOPT_WRITEFUNCTION: u32 = 20011;
pub const CURLOPT_READFUNCTION: u32 = 20012;
pub const CURLOPT_TIMEOUT: u32 = 13;
pub const CURLOPT_INFILESIZE: u32 = 14;
pub const CURLOPT_POSTFIELDS: u32 = 10015;
pub const CURLOPT_USERAGENT: u32 = 10018;
pub const CURLOPT_HTTPHEADER: u32 = 10023;
pub const CURLOPT_HEADERDATA: u32 = 10029;
pub const CURLOPT_CUSTOMREQUEST: u32 = 10036;
pub const CURLOPT_VERBOSE: u32 = 41;
pub const CURLOPT_HEADER: u32 = 42;
pub const CURLOPT_NOBODY: u32 = 44;
pub const CURLOPT_FAILONERROR: u32 = 45;
pub const CURLOPT_UPLOAD: u32 = 46;
pub const CURLOPT_POST: u32 = 47;
pub const CURLOPT_FOLLOWLOCATION: u32 = 52;
pub const CURLOPT_POSTFIELDSIZE: u32 = 60;
pub const CURLOPT_SSL_VERIFYPEER: u32 = 64;
pub const CURLOPT_MAXREDIRS: u32 = 68;
pub const CURLOPT_HEADERFUNCTION: u32 = 20079;
pub const CURLOPT_SSL_VERIFYHOST: u32 = 81;
pub const CURLOPT_DEBUGFUNCTION: u32 = 20094;
pub const CURLOPT_USERPWD: u32 = 10005;
pub const CURLOPT_PRIVATE: u32 = 10103;
pub const CURLOPT_TIMEOUT_MS: u32 = 155;
pub const CURLOPT_SSLCERT: u32 = 10025;
pub const CURLOPT_SSLKEY: u32 = 10087;
pub const CURLOPT_SSL_CIPHER_LIST: u32 = 10083;
pub const CURLOPT_CONNECT_TO: u32 = 10243;

pub const CURLINFO_EFFECTIVE_URL: u32 = 0x100001;
pub const CURLINFO_RESPONSE_CODE: u32 = 0x200002;
pub const CURLINFO_TOTAL_TIME: u32 = 0x300003;
pub const CURLINFO_SIZE_DOWNLOAD: u32 = 0x300008;
pub const CURLINFO_SIZE_DOWNLOAD_T: u32 = 0x600008;
pub const CURLINFO_CONTENT_TYPE: u32 = 0x100012;
pub const CURLINFO_PRIVATE: u32 = 0x100015;
pub const CURLINFO_REDIRECT_COUNT: u32 = 0x200014;
pub const CURLINFO_OS_ERRNO: u32 = 0x200019;
pub const CURLINFO_REDIRECT_URL: u32 = 0x10001f;
pub const CURLINFO_HTTP_VERSION: u32 = 0x20002e;
pub const CURLINFO_CONTENT_LENGTH_DOWNLOAD_T: u32 = 0x60000f;
