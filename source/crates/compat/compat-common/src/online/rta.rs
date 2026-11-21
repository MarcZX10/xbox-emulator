use super::ws::{Outlet, WsReply, WsSession};
use super::Request;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

pub const PROTOCOL: &str = "rta.xboxlive.com.V2";

pub const SUBSCRIBE: u64 = 1;
pub const UNSUBSCRIBE: u64 = 2;
pub const EVENT: u64 = 3;
pub const RESYNC: u64 = 4;

pub const SUCCESS: u64 = 0;
pub const UNKNOWN_RESOURCE: u64 = 1;
pub const SUBSCRIPTION_LIMIT_REACHED: u64 = 2;
pub const NO_RESOURCE_DATA: u64 = 3;

pub const MAX_SUBSCRIPTIONS: usize = 1100;

struct Sub {
    channel: String,
    outlet: u64,
    id: u64,
}

fn registry() -> &'static Mutex<BTreeMap<String, Vec<Sub>>> {
    static R: OnceLock<Mutex<BTreeMap<String, Vec<Sub>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn is_rta(req: &Request) -> bool {
    req.header("x-forwarded-host").map_or(false, |h| h.split(':').next().unwrap_or("").eq_ignore_ascii_case("rta.xboxlive.com"))
}

pub fn normalize(uri: &str) -> String {
    uri.trim().trim_end_matches('/').to_ascii_lowercase()
}

pub fn publish(uri: &str, data: &Value) -> usize {
    let key = normalize(uri);
    let g = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut n = 0;
    if let Some(v) = g.get(&key) {
        for s in v {
            n += super::ws::publish(&s.channel, &json!([EVENT, s.id, data]).to_string());
        }
    }
    n
}

pub fn subscribers(uri: &str) -> usize {
    registry().lock().unwrap_or_else(|e| e.into_inner()).get(&normalize(uri)).map_or(0, |v| v.len())
}

fn guid(seed: u64) -> String {
    let a = seed ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let b = a.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29) ^ 0xd1b5_4a32_d192_ed03;
    format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}", (a >> 32) as u32, (a >> 16) as u16, a as u16 & 0xfff, (b >> 48) as u16 & 0x3fff | 0x8000, b & 0xffff_ffff_ffff)
}

pub struct RtaSession {
    outlet: Option<Outlet>,
    connection_id: String,
    next_sub: u64,
    subs: BTreeMap<u64, String>,
}

impl RtaSession {
    pub fn new() -> RtaSession {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RtaSession { outlet: None, connection_id: guid(n), next_sub: 1, subs: BTreeMap::new() }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    fn outlet_id(&self) -> u64 {
        self.outlet.as_ref().map_or(0, |o| o.id())
    }

    fn initial_data(&self, uri: &str) -> Result<Value, u64> {
        let rest = uri.split_once("://").map_or(uri, |(_, r)| r);
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = host.to_ascii_lowercase();
        if host != "xboxlive.com" && !host.ends_with(".xboxlive.com") {
            return Err(UNKNOWN_RESOURCE);
        }
        let path = path.trim_end_matches('/').to_ascii_lowercase();
        let seg: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        match (host.split('.').next().unwrap_or(""), seg.as_slice()) {
            ("sessiondirectory", ["connections"]) => Ok(json!({"ConnectionId": self.connection_id})),
            ("privacy", ["users", owner, list @ ("mutelist" | "neverlist")]) => match super::xboxlive::privacy_list(owner, if *list == "mutelist" { "mute" } else { "avoid" }) {
                Some(x) => Ok(json!({"Xuids": x.iter().map(|v| v.to_string()).collect::<Vec<_>>()})),
                None => Err(UNKNOWN_RESOURCE),
            },
            _ => Err(NO_RESOURCE_DATA),
        }
    }

    fn subscribe(&mut self, seq: u64, uri: &str) -> String {
        if self.subs.len() >= MAX_SUBSCRIPTIONS {
            return json!([SUBSCRIBE, seq, SUBSCRIPTION_LIMIT_REACHED]).to_string();
        }
        let data = match self.initial_data(uri) {
            Ok(d) => Some(d),
            Err(NO_RESOURCE_DATA) => None,
            Err(code) => {
                crate::record_raw("online", "rta", &format!("subscribe {seq} {uri} -> status {code}"));
                return json!([SUBSCRIBE, seq, code]).to_string();
            }
        };
        let id = self.next_sub;
        self.next_sub += 1;
        let channel = format!("rta#{}#{}", self.outlet_id(), id);
        if let Some(o) = &self.outlet {
            o.subscribe(&channel);
        }
        registry().lock().unwrap_or_else(|e| e.into_inner()).entry(normalize(uri)).or_default().push(Sub { channel, outlet: self.outlet_id(), id });
        self.subs.insert(id, normalize(uri));
        crate::record_raw("online", "rta", &format!("subscribe {seq} {uri} -> subscription {id}{}", if data.is_some() { " with data" } else { "" }));
        match data {
            Some(d) => json!([SUBSCRIBE, seq, SUCCESS, id, d]).to_string(),
            None => json!([SUBSCRIBE, seq, SUCCESS, id]).to_string(),
        }
    }

    fn unsubscribe(&mut self, seq: u64, id: u64) -> String {
        let Some(uri) = self.subs.remove(&id) else {
            return json!([UNSUBSCRIBE, seq, UNKNOWN_RESOURCE]).to_string();
        };
        let me = self.outlet_id();
        let mut g = registry().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = g.get_mut(&uri) {
            v.retain(|s| {
                let keep = !(s.outlet == me && s.id == id);
                if !keep {
                    if let Some(o) = &self.outlet {
                        o.unsubscribe(&s.channel);
                    }
                }
                keep
            });
            if v.is_empty() {
                g.remove(&uri);
            }
        }
        crate::record_raw("online", "rta", &format!("unsubscribe {seq} subscription {id}"));
        json!([UNSUBSCRIBE, seq, SUCCESS]).to_string()
    }
}

impl Default for RtaSession {
    fn default() -> Self {
        RtaSession::new()
    }
}

impl WsSession for RtaSession {
    fn name(&self) -> &'static str {
        "rta"
    }

    fn attach(&mut self, outlet: Outlet) {
        self.outlet = Some(outlet);
    }

    fn on_open(&mut self) -> WsReply {
        crate::record_raw("online", "rta", &format!("connected, connection id {}", self.connection_id));
        WsReply::none()
    }

    fn on_text(&mut self, text: &str) -> WsReply {
        let v: Value = serde_json::from_str(text.trim_end_matches(['\0', ' ', '\r', '\n', '\t'])).unwrap_or(Value::Null);
        let a = v.as_array().cloned().unwrap_or_default();
        let kind = a.first().and_then(Value::as_u64);
        let seq = a.get(1).and_then(Value::as_u64);
        match (kind, seq) {
            (Some(SUBSCRIBE), Some(seq)) => match a.get(2).and_then(Value::as_str) {
                Some(uri) => WsReply::one(self.subscribe(seq, uri)),
                None => WsReply::one(json!([SUBSCRIBE, seq, UNKNOWN_RESOURCE]).to_string()),
            },
            (Some(UNSUBSCRIBE), Some(seq)) => match a.get(2).and_then(Value::as_u64) {
                Some(id) => WsReply::one(self.unsubscribe(seq, id)),
                None => WsReply::one(json!([UNSUBSCRIBE, seq, UNKNOWN_RESOURCE]).to_string()),
            },
            _ => {
                crate::record_raw("online", "rta", &format!("malformed message {text:?}; closing"));
                WsReply { send: Vec::new(), close: true }
            }
        }
    }

    fn on_close(&mut self) {
        let me = self.outlet_id();
        let mut g = registry().lock().unwrap_or_else(|e| e.into_inner());
        for uri in self.subs.values() {
            if let Some(v) = g.get_mut(uri) {
                v.retain(|s| s.outlet != me);
            }
        }
        g.retain(|_, v| !v.is_empty());
        drop(g);
        self.subs.clear();
        if let Some(o) = &self.outlet {
            o.close();
        }
        crate::record_raw("online", "rta", &format!("disconnected, connection id {}", self.connection_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (RtaSession, std::sync::mpsc::Receiver<String>) {
        let (o, rx) = Outlet::detached();
        let mut s = RtaSession::new();
        s.attach(o);
        (s, rx)
    }

    #[test]
    fn connections_subscription_acks_with_connection_id() {
        let (mut s, _rx) = session();
        let r = s.on_text(r#"[1,1,"https://sessiondirectory.xboxlive.com/connections/"]"#);
        let v: Value = serde_json::from_str(&r.send[0]).unwrap();
        assert_eq!(v[0], 1);
        assert_eq!(v[1], 1);
        assert_eq!(v[2], 0);
        assert_eq!(v[4]["ConnectionId"], s.connection_id());
        let id = v[3].as_u64().unwrap();
        let r = s.on_text(&format!("[2,2,{id}]"));
        assert_eq!(r.send[0], "[2,2,0]");
        assert_eq!(s.on_text("[2,3,999]").send[0], "[2,3,1]");
        assert_eq!(s.on_text(r#"[1,4,"https://example.com/x"]"#).send[0], "[1,4,1]");
        let r = s.on_text("[1, 5, \"https://sessiondirectory.xboxlive.com/connections/\"]\0");
        assert!(r.send[0].starts_with("[1,5,0,"));
        assert!(s.on_text("nonsense").close);
    }

    #[test]
    fn privacy_list_events_reach_subscribers() {
        let (mut s, rx) = session();
        let x = crate::gdk::user::user().xuid;
        let uri = format!("https://privacy.xboxlive.com/users/xuid({x})/mutelist");
        let r = s.on_text(&json!([1, 7, uri]).to_string());
        let v: Value = serde_json::from_str(&r.send[0]).unwrap();
        assert_eq!(v[2], 0);
        assert!(v[4]["Xuids"].is_array());
        let id = v[3].as_u64().unwrap();
        assert!(publish(&uri, &json!({"NotificationType": "Added", "Xuids": ["42"]})) >= 1);
        let ev: Value = serde_json::from_str(&rx.recv().unwrap()).unwrap();
        assert_eq!(ev[0], 3);
        assert_eq!(ev[1], id);
        assert_eq!(ev[2]["Xuids"][0], "42");
        s.on_close();
        assert_eq!(subscribers(&uri), 0);
    }
}
