use super::util::{b64, sha1};
use super::Request;
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_MESSAGE: usize = 16 * 1024 * 1024;
pub const CLOSE_SOCKET: &str = "\u{0}tombox-ws-close";

#[derive(Default)]
pub struct WsReply {
    pub send: Vec<String>,
    pub close: bool,
}

impl WsReply {
    pub fn none() -> WsReply {
        WsReply::default()
    }

    pub fn one(s: String) -> WsReply {
        WsReply { send: vec![s], close: false }
    }
}

#[derive(Clone)]
pub struct Outlet {
    id: u64,
    tx: Sender<String>,
}

impl Outlet {
    pub fn detached() -> (Outlet, Receiver<String>) {
        let (tx, rx) = channel();
        (Outlet { id: next_id(), tx }, rx)
    }

    pub fn close(&self) {
        self.drop_all();
    }

    pub fn close_socket(&self) {
        let _ = self.tx.send(CLOSE_SOCKET.to_string());
    }

    pub fn subscribe(&self, ch: &str) {
        let mut g = hub().lock().unwrap_or_else(|p| p.into_inner());
        let v = g.entry(ch.to_string()).or_default();
        if !v.iter().any(|(i, _)| *i == self.id) {
            v.push((self.id, self.tx.clone()));
        }
    }

    pub fn unsubscribe(&self, ch: &str) {
        let mut g = hub().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(v) = g.get_mut(ch) {
            v.retain(|(i, _)| *i != self.id);
            if v.is_empty() {
                g.remove(ch);
            }
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    fn drop_all(&self) {
        let mut g = hub().lock().unwrap_or_else(|p| p.into_inner());
        for v in g.values_mut() {
            v.retain(|(i, _)| *i != self.id);
        }
        g.retain(|_, v| !v.is_empty());
    }
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

type Hub = Mutex<HashMap<String, Vec<(u64, Sender<String>)>>>;

fn hub() -> &'static Hub {
    static H: OnceLock<Hub> = OnceLock::new();
    H.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn publish(ch: &str, text: &str) -> usize {
    publish_except(ch, text, 0)
}

pub fn publish_except(ch: &str, text: &str, except: u64) -> usize {
    let g = hub().lock().unwrap_or_else(|p| p.into_inner());
    let mut n = 0;
    if let Some(v) = g.get(ch) {
        for (id, tx) in v {
            if *id != except && tx.send(text.to_string()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

pub fn subscribers(ch: &str) -> usize {
    hub().lock().unwrap_or_else(|p| p.into_inner()).get(ch).map_or(0, |v| v.len())
}

pub trait WsSession: Send {
    fn name(&self) -> &'static str;
    fn attach(&mut self, _outlet: Outlet) {}
    fn on_open(&mut self) -> WsReply {
        WsReply::none()
    }
    fn on_text(&mut self, text: &str) -> WsReply;
    fn on_binary(&mut self, _data: &[u8]) -> WsReply {
        WsReply::none()
    }
    fn on_close(&mut self) {}
}

pub fn is_upgrade(req: &Request) -> bool {
    req.method == "GET"
        && req.header("upgrade").map_or(false, |u| u.eq_ignore_ascii_case("websocket"))
        && req.header("sec-websocket-key").is_some()
}

pub fn accept_key(key: &str) -> String {
    b64(&sha1(format!("{}{GUID}", key.trim()).as_bytes()))
}

pub fn requested_protocols(req: &Request) -> Vec<String> {
    req.header("sec-websocket-protocol").map(|p| p.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

pub fn encode_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 10);
    f.push(0x80 | (opcode & 0x0f));
    let n = payload.len();
    if n < 126 {
        f.push(n as u8);
    } else if n <= 0xffff {
        f.push(126);
        f.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        f.push(127);
        f.extend_from_slice(&(n as u64).to_be_bytes());
    }
    f.extend_from_slice(payload);
    f
}

pub fn read_frame<R: Read>(r: &mut R) -> std::io::Result<(bool, u8, Vec<u8>)> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    let fin = h[0] & 0x80 != 0;
    let opcode = h[0] & 0x0f;
    let masked = h[1] & 0x80 != 0;
    let mut len = (h[1] & 0x7f) as u64;
    if len == 126 {
        let mut b = [0u8; 2];
        r.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as u64;
    } else if len == 127 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        len = u64::from_be_bytes(b);
    }
    if len as usize > MAX_MESSAGE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "websocket frame too large"));
    }
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    let mut data = vec![0u8; len as usize];
    r.read_exact(&mut data)?;
    if masked {
        for (i, b) in data.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok((fin, opcode, data))
}

type Shared = Arc<Mutex<TcpStream>>;

fn write_frame(w: &Shared, op: u8, data: &[u8]) -> std::io::Result<()> {
    let mut g = w.lock().unwrap_or_else(|p| p.into_inner());
    g.write_all(&encode_frame(op, data))?;
    g.flush()
}

fn send_all(w: &Shared, reply: &WsReply) -> std::io::Result<()> {
    for m in &reply.send {
        write_frame(w, 1, m.as_bytes())?;
    }
    if reply.close {
        write_frame(w, 8, &1000u16.to_be_bytes())?;
    }
    Ok(())
}

pub fn run(mut reader: BufReader<TcpStream>, mut writer: TcpStream, req: &Request, mut session: Box<dyn WsSession>, protocol: Option<String>) {
    let key = req.header("sec-websocket-key").unwrap_or("");
    let mut head = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n", accept_key(key));
    if let Some(p) = &protocol {
        head.push_str(&format!("Sec-WebSocket-Protocol: {p}\r\n"));
    }
    head.push_str("\r\n");
    if writer.write_all(head.as_bytes()).is_err() {
        return;
    }
    let _ = writer.set_read_timeout(None);
    let _ = reader.get_ref().set_read_timeout(None);
    let writer: Shared = Arc::new(Mutex::new(writer));
    let (tx, rx) = channel::<String>();
    let outlet = Outlet { id: next_id(), tx };
    let pusher = {
        let w = writer.clone();
        std::thread::Builder::new().name("tombox-ws-push".into()).spawn(move || {
            while let Ok(m) = rx.recv() {
                if m == CLOSE_SOCKET {
                    let _ = write_frame(&w, 8, &1000u16.to_be_bytes());
                    let _ = w.lock().map(|g| g.shutdown(std::net::Shutdown::Write));
                    break;
                }
                if write_frame(&w, 1, m.as_bytes()).is_err() {
                    break;
                }
            }
        })
    };
    let name = session.name();
    crate::record_raw("online", "websocket", &format!("[{name}] {} opened{}", req.path, protocol.as_deref().map(|p| format!(" protocol={p}")).unwrap_or_default()));
    session.attach(outlet.clone());
    if send_all(&writer, &session.on_open()).is_ok() {
        pump(&mut reader, &writer, session.as_mut());
    }
    session.on_close();
    drop(session);
    outlet.drop_all();
    drop(outlet);
    if let Ok(g) = writer.lock() {
        let _ = g.shutdown(std::net::Shutdown::Both);
    }
    if let Ok(t) = pusher {
        let _ = t.join();
    }
    crate::record_raw("online", "websocket", &format!("[{name}] {} closed", req.path));
}

fn pump(reader: &mut BufReader<TcpStream>, writer: &Shared, session: &mut dyn WsSession) {
    let mut partial: Option<(u8, Vec<u8>)> = None;
    loop {
        let (fin, op, data) = match read_frame(reader) {
            Ok(f) => f,
            Err(_) => return,
        };
        let (op, data) = match op {
            0 => match partial.as_mut() {
                Some((_, buf)) => {
                    if buf.len() + data.len() > MAX_MESSAGE {
                        return;
                    }
                    buf.extend_from_slice(&data);
                    if !fin {
                        continue;
                    }
                    match partial.take() {
                        Some(p) => p,
                        None => return,
                    }
                }
                None => return,
            },
            1 | 2 if !fin => {
                partial = Some((op, data));
                continue;
            }
            o => (o, data),
        };
        let reply = match op {
            1 => {
                let text = String::from_utf8_lossy(&data).into_owned();
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.on_text(&text))) {
                    Ok(r) => r,
                    Err(_) => {
                        crate::record_raw("online", "panic", &format!("websocket [{}] text handler", session.name()));
                        WsReply { send: Vec::new(), close: true }
                    }
                }
            }
            2 => session.on_binary(&data),
            8 => {
                let _ = write_frame(writer, 8, &data[..data.len().min(2)]);
                return;
            }
            9 => {
                if write_frame(writer, 10, &data).is_err() {
                    return;
                }
                continue;
            }
            _ => continue,
        };
        if send_all(writer, &reply).is_err() || reply.close {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_key_matches_rfc6455_example() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn frame_roundtrip_with_mask() {
        let payload = vec![7u8; 300];
        let mut f = vec![0x81, 0x80 | 126];
        f.extend_from_slice(&300u16.to_be_bytes());
        let mask = [1u8, 2, 3, 4];
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        let (fin, op, data) = read_frame(&mut &f[..]).unwrap();
        assert!(fin);
        assert_eq!(op, 1);
        assert_eq!(data, payload);
        let e = encode_frame(1, &payload);
        let (_, _, d2) = read_frame(&mut &e[..]).unwrap();
        assert_eq!(d2, payload);
    }

    #[test]
    fn hub_routes_to_subscribers_only() {
        let (tx, rx) = channel();
        let o = Outlet { id: next_id(), tx };
        o.subscribe("test:hub-a");
        assert_eq!(publish("test:hub-a", "x"), 1);
        assert_eq!(publish("test:hub-b", "y"), 0);
        assert_eq!(publish_except("test:hub-a", "z", o.id()), 0);
        assert_eq!(rx.recv().unwrap(), "x");
        o.drop_all();
        assert_eq!(subscribers("test:hub-a"), 0);
    }
}
