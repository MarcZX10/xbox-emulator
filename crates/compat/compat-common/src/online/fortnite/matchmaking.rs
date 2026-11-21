use super::super::ws::{self, Outlet, WsReply, WsSession};
use super::super::{util, Request, Response};
use super::party::{capture, note};
use super::{epic_error, session, Fortnite};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const TICKET_TTL_SECS: u64 = 300;
pub const STEP_MS: u64 = 250;
pub const JOIN_DELAY_SEC: u64 = 1;
pub const DEFAULT_PLAYLIST: &str = "Playlist_DefaultSquad";
pub const DEFAULT_REGION: &str = "NAE";
const HOST_PREFIX: &str = "fortnite-matchmaking-public-service";

#[derive(Clone, Debug)]
pub struct Ticket {
    pub ticket_id: String,
    pub account_id: String,
    pub party_player_ids: Vec<String>,
    pub bucket_id: String,
    pub net_cl: String,
    pub playlist_revision: i64,
    pub region: String,
    pub playlist: String,
    pub platform: String,
    pub subregions: Vec<String>,
    pub options: Map<String, Value>,
    pub issued_at: u64,
}

impl Ticket {
    fn to_json(&self) -> Value {
        json!({
            "accountId": self.account_id,
            "partyPlayerIds": self.party_player_ids,
            "bucketId": self.bucket_id,
            "netCL": self.net_cl,
            "playlistRevision": self.playlist_revision,
            "region": self.region,
            "playlist": self.playlist,
            "platform": self.platform,
            "subregions": self.subregions,
            "options": self.options,
            "issuedAt": self.issued_at,
            "ticketId": self.ticket_id,
        })
    }
}

fn key() -> &'static [u8] {
    static K: OnceLock<Vec<u8>> = OnceLock::new();
    K.get_or_init(|| format!("{}{}", util::random_hex32(), util::random_uuid()).into_bytes())
}

fn tickets() -> &'static Mutex<HashMap<String, Ticket>> {
    static T: OnceLock<Mutex<HashMap<String, Ticket>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn sign(payload: &str) -> String {
    util::b64(&util::hmac_sha256(key(), payload.as_bytes()))
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn is_mms(req: &Request) -> bool {
    req.header("x-forwarded-host").map_or(false, |h| h.trim().to_ascii_lowercase().starts_with(HOST_PREFIX))
}

pub fn parse_bucket(bucket: &str) -> Option<(String, i64, String, String)> {
    let parts: Vec<&str> = bucket.split(':').collect();
    let [cl, rev, region, playlist] = parts.as_slice() else { return None };
    let cl = cl.trim().parse::<u32>().ok().filter(|c| *c != 0)?;
    let rev = rev.trim().parse::<i64>().ok()?;
    let playlist = playlist.trim();
    if playlist.is_empty() || playlist == "0" || !playlist.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    let region = region.trim();
    if region.is_empty() || !region.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    let region = if region.eq_ignore_ascii_case("NONE") { local_region() } else { region.to_ascii_uppercase() };
    Some((cl.to_string(), rev, region, playlist.to_string()))
}

pub fn local_region() -> String {
    std::env::var("TOMBOX_FN_REGION").ok().or_else(|| crate::cfg("fn_region")).map(|s| s.trim().to_ascii_uppercase()).filter(|s| !s.is_empty() && s != "NONE" && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).unwrap_or_else(|| DEFAULT_REGION.to_string())
}

pub fn env_playlist() -> String {
    std::env::var("TOMBOX_FN_PLAYLIST").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_PLAYLIST.to_string())
}

fn service_url(region: &str) -> String {
    format!("wss://{HOST_PREFIX}-live-{}.ol.epicgames.com:443", region.to_ascii_lowercase())
}

fn issue(fnt: &Fortnite, req: &Request, acct: &str) -> Response {
    let _ = fnt;
    let bucket = req.query("bucketId").unwrap_or("").to_string();
    let Some((net_cl, rev, region, bucket_playlist)) = parse_bucket(&bucket) else {
        note("mms", &format!("ticket for {acct} rejected: bucketId '{bucket}'"));
        return epic_error(400, "errors.com.epicgames.fortnite.invalid_bucket_id", &format!("blank or invalid bucketId: '{bucket}'"), 16102, &[bucket.as_str()]);
    };
    let mut ids: Vec<String> = req.query_all("partyPlayerIds").iter().flat_map(|v| v.split(',')).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if ids.is_empty() {
        ids.push(acct.to_string());
    }
    if !ids.iter().any(|i| i.eq_ignore_ascii_case(acct)) {
        return epic_error(403, "errors.com.epicgames.fortnite.player_not_in_party", &format!("{acct} is not one of partyPlayerIds"), 16103, &[acct]);
    }
    let mut options = Map::new();
    for (k, v) in &req.query {
        if k.starts_with("player.option.") || k.starts_with("input.") || k == "player.input" || k == "player.playerGroups" {
            options.insert(k.clone(), json!(v));
        }
    }
    let t = Ticket {
        ticket_id: util::random_hex32(),
        account_id: acct.to_string(),
        party_player_ids: ids,
        bucket_id: bucket.clone(),
        net_cl,
        playlist_revision: rev,
        region: region.clone(),
        playlist: bucket_playlist,
        platform: req.query("player.platform").unwrap_or("XSX").to_string(),
        subregions: req.query("player.subregions").unwrap_or("").split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
        options,
        issued_at: util::now_secs(),
    };
    let payload = util::b64(t.to_json().to_string().as_bytes());
    let signature = sign(&payload);
    {
        let mut g = tickets().lock().unwrap_or_else(|p| p.into_inner());
        let now = util::now_secs();
        g.retain(|_, v| v.issued_at + TICKET_TTL_SECS > now);
        g.insert(t.ticket_id.clone(), t.clone());
    }
    note("mms", &format!("ticket {} for {acct} bucket {bucket} playlist {} players {}", t.ticket_id, t.playlist, t.party_player_ids.len()));
    Response::json(&json!({
        "serviceUrl": service_url(&region),
        "ticketType": "mms-player",
        "payload": payload,
        "signature": signature,
    }))
}

#[derive(Debug, PartialEq)]
pub enum TicketError {
    Malformed,
    BadSignature,
    Unknown,
    Expired,
}

pub fn verify(payload: &str, signature: &str) -> Result<Ticket, TicketError> {
    if payload.is_empty() || signature.is_empty() {
        return Err(TicketError::Malformed);
    }
    if !same(&util::b64_decode(&sign(payload)), &util::b64_decode(signature)) {
        return Err(TicketError::BadSignature);
    }
    let body: Value = serde_json::from_slice(&util::b64_decode(payload)).map_err(|_| TicketError::Malformed)?;
    let id = body.get("ticketId").and_then(Value::as_str).ok_or(TicketError::Malformed)?;
    let issued = body.get("issuedAt").and_then(Value::as_u64).ok_or(TicketError::Malformed)?;
    if issued + TICKET_TTL_SECS <= util::now_secs() {
        return Err(TicketError::Expired);
    }
    let t = tickets().lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned().ok_or(TicketError::Unknown)?;
    if t.issued_at + TICKET_TTL_SECS <= util::now_secs() {
        return Err(TicketError::Expired);
    }
    Ok(t)
}

pub fn authorize(req: &Request) -> Result<Ticket, TicketError> {
    let auth = req.header("authorization").unwrap_or("");
    let f: Vec<&str> = auth.split_whitespace().collect();
    if f.len() < 4 || f.len() > 5 || !f[0].eq_ignore_ascii_case("Epic-Signed") || !f[1].eq_ignore_ascii_case("mms-player") {
        return Err(TicketError::Malformed);
    }
    verify(f[2], f[3])
}

pub fn websocket(req: &Request) -> Option<(Box<dyn WsSession>, Option<String>)> {
    capture("mms", req);
    match authorize(req) {
        Ok(t) => {
            note("mms", &format!("websocket ticket {} accepted", t.ticket_id));
            Some((Box::new(MmsSession::new(t)), None))
        }
        Err(e) => {
            note("mms", &format!("websocket rejected: {e:?}"));
            None
        }
    }
}

pub fn status(state: &str, extra: Value) -> String {
    let mut p = json!({"state": state});
    if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            o.insert(k.clone(), v.clone());
        }
    }
    json!({"name": "StatusUpdate", "payload": p}).to_string()
}

pub struct MmsSession {
    ticket: Ticket,
    match_id: String,
    session_id: String,
    outlet: Option<Outlet>,
}

impl MmsSession {
    pub fn new(ticket: Ticket) -> MmsSession {
        MmsSession { ticket, match_id: util::random_hex32(), session_id: util::random_hex32(), outlet: None }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn channel(&self) -> String {
        format!("mms:{}", self.ticket.ticket_id)
    }
}

impl WsSession for MmsSession {
    fn name(&self) -> &'static str {
        "mms"
    }

    fn attach(&mut self, outlet: Outlet) {
        let ch = self.channel();
        outlet.subscribe(&ch);
        self.outlet = Some(outlet.clone());
        let t = self.ticket.clone();
        let (m, s) = (self.match_id.clone(), self.session_id.clone());
        let _ = std::thread::Builder::new().name("tombox-mms-pace".into()).spawn(move || {
            let n = t.party_player_ids.len();
            let steps = [
                status("Waiting", json!({"totalPlayers": n, "connectedPlayers": n})),
                status("Queued", json!({"ticketId": t.ticket_id, "queuedPlayers": n, "estimatedWaitSec": 0, "status": {}})),
                status("SessionAssignment", json!({"matchId": m})),
            ];
            for f in steps {
                std::thread::sleep(Duration::from_millis(STEP_MS));
                if ws::publish(&ch, &f) == 0 {
                    note("mms", &format!("ticket {} client gone before {f}", t.ticket_id));
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(STEP_MS));
            session::create(&s, &m, &t);
            let play = json!({"name": "Play", "payload": {"matchId": m, "sessionId": s, "joinDelaySec": JOIN_DELAY_SEC}}).to_string();
            if ws::publish(&ch, &play) == 0 {
                note("mms", &format!("ticket {} client gone before Play", t.ticket_id));
                return;
            }
            note("mms", &format!("ticket {} -> match {m} session {s}", t.ticket_id));
            outlet.close();
            outlet.close_socket();
        });
    }

    fn on_open(&mut self) -> WsReply {
        WsReply::one(status("Connecting", json!({})))
    }

    fn on_text(&mut self, text: &str) -> WsReply {
        let t = text.trim();
        if t.eq_ignore_ascii_case("ping") {
            return WsReply::one("pong".into());
        }
        note("mms", &format!("ticket {} client sent {}", self.ticket.ticket_id, t.chars().take(4096).collect::<String>()));
        WsReply::none()
    }

    fn on_close(&mut self) {
        if let Some(o) = &self.outlet {
            o.unsubscribe(&self.channel());
        }
    }
}

fn reject(req: &Request) -> Response {
    let why = match authorize(req) {
        Ok(_) => return epic_error(426, "errors.com.epicgames.common.upgrade_required", "websocket upgrade required", 1000, &[]),
        Err(e) => format!("{e:?}"),
    };
    let mut r = epic_error(401, "errors.com.epicgames.common.authentication.authentication_failed", &format!("mms-player ticket rejected: {why}"), 1032, &[why.as_str()]);
    r.headers.push(("WWW-Authenticate".into(), "Epic-Signed".into()));
    r
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    if is_mms(req) {
        return Some(reject(req));
    }
    match (m, seg) {
        ("GET", ["fortnite", "api", "game", "v2", "matchmakingservice", "ticket", "player", acct]) => {
            capture("mms", req);
            Some(issue(fnt, req, acct))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    fn ticket_req(acct: &str, bucket: &str) -> Request {
        Request {
            method: "GET".into(),
            path: format!("/fortnite/api/game/v2/matchmakingservice/ticket/player/{acct}"),
            query: util::parse_pairs(&format!("partyPlayerIds={acct}&bucketId={bucket}&player.platform=XSX&player.subregions=VA,OH&player.option.crossplayOptOut=false&player.input=GAMEPAD&input.KBM=false")),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn get(acct: &str, bucket: &str) -> Response {
        let f = Fortnite::with_account(acct, acct);
        let r = ticket_req(acct, bucket);
        route(&f, &r, "GET", &r.segments()).unwrap()
    }

    fn body(r: &Response) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    #[test]
    fn ticket_bucket_validation() {
        let r = get("MmA", "garbage");
        assert_eq!(r.status, 400);
        assert_eq!(body(&r)["errorCode"], "errors.com.epicgames.fortnite.invalid_bucket_id");
        assert_eq!(get("MmA", "0:0:NONE:0").status, 400);
        assert_eq!(get("MmA", "14457890:0:NONE:0").status, 400);
        assert_eq!(get("MmA", "14457890:0:NONE:").status, 400);
        assert_eq!(get("MmA", "14457890:0:NO/NE:playlist_defaultsquad").status, 400);
    }

    #[test]
    fn region_none_is_hosted_locally() {
        let r = get("MmN", "14457890:0:NONE:playlist_defaultsquad");
        assert_eq!(r.status, 200);
        let b = body(&r);
        let t = verify(b["payload"].as_str().unwrap(), b["signature"].as_str().unwrap()).unwrap();
        assert_eq!(t.region, local_region());
        assert_eq!(t.playlist, "playlist_defaultsquad");
        assert_eq!(b["serviceUrl"], json!(format!("wss://fortnite-matchmaking-public-service-live-{}.ol.epicgames.com:443", local_region().to_ascii_lowercase())));
    }

    #[test]
    fn ticket_signature_roundtrip() {
        let r = get("MmB", "14457890:1:NAE:playlist_defaultsquad");
        assert_eq!(r.status, 200);
        let b = body(&r);
        assert_eq!(b["ticketType"], "mms-player");
        assert_eq!(b["serviceUrl"], "wss://fortnite-matchmaking-public-service-live-nae.ol.epicgames.com:443");
        let t = verify(b["payload"].as_str().unwrap(), b["signature"].as_str().unwrap()).unwrap();
        assert_eq!(t.playlist, "playlist_defaultsquad");
        assert_eq!(t.net_cl, "14457890");
        assert_eq!(t.region, "NAE");
        assert_eq!(t.subregions, vec!["VA", "OH"]);
        assert_eq!(t.options["player.input"], "GAMEPAD");
        assert_eq!(t.ticket_id.len(), 32);
        let auth = Request { method: "GET".into(), path: "/".into(), query: Vec::new(), headers: vec![("Authorization".into(), format!("Epic-Signed mms-player {} {} abcdef", b["payload"].as_str().unwrap(), b["signature"].as_str().unwrap()))], body: Vec::new() };
        assert_eq!(authorize(&auth).unwrap().ticket_id, t.ticket_id);
    }

    #[test]
    fn tampered_ticket_rejected() {
        let b = body(&get("MmC", "14457890:1:NAE:playlist_defaultsolo"));
        let p = b["payload"].as_str().unwrap();
        let mut json: Value = serde_json::from_slice(&util::b64_decode(p)).unwrap();
        json["playlist"] = json!("playlist_playground");
        let forged = util::b64(json.to_string().as_bytes());
        assert_eq!(verify(&forged, b["signature"].as_str().unwrap()).unwrap_err(), TicketError::BadSignature);
        assert_eq!(verify(p, "AAAA").unwrap_err(), TicketError::BadSignature);
        let bad = Request { method: "GET".into(), path: "/".into(), query: Vec::new(), headers: vec![("X-Forwarded-Host".into(), "fortnite-matchmaking-public-service-live-nae.ol.epicgames.com".into()), ("Authorization".into(), format!("Epic-Signed mms-player {forged} {}", b["signature"].as_str().unwrap()))], body: Vec::new() };
        assert!(websocket(&bad).is_none());
        let f = Fortnite::with_account("MmC", "MmC");
        assert_eq!(route(&f, &bad, "GET", &[]).unwrap().status, 401);
    }

    #[test]
    fn expired_ticket_rejected() {
        let b = body(&get("MmD", "14457890:1:NAE:playlist_defaultduo"));
        let p = b["payload"].as_str().unwrap();
        let mut json: Value = serde_json::from_slice(&util::b64_decode(p)).unwrap();
        json["issuedAt"] = json!(json["issuedAt"].as_u64().unwrap() - TICKET_TTL_SECS - 1);
        let old = util::b64(json.to_string().as_bytes());
        assert_eq!(verify(&old, &sign(&old)).unwrap_err(), TicketError::Expired);
        assert!(verify(p, b["signature"].as_str().unwrap()).is_ok());
    }

    fn frame(rx: &Receiver<String>) -> String {
        rx.recv_timeout(Duration::from_secs(5)).expect("frame")
    }

    #[test]
    fn mms_sequence_order_and_fields() {
        let b = body(&get("MmE", "14457890:1:NAE:playlist_defaultsquad"));
        let t = verify(b["payload"].as_str().unwrap(), b["signature"].as_str().unwrap()).unwrap();
        let mut s = MmsSession::new(t.clone());
        let sid = s.session_id().to_string();
        let (o, rx) = Outlet::detached();
        s.attach(o);
        let first: Value = serde_json::from_str(&s.on_open().send[0]).unwrap();
        assert_eq!(first, json!({"name": "StatusUpdate", "payload": {"state": "Connecting"}}));
        let w: Value = serde_json::from_str(&frame(&rx)).unwrap();
        assert_eq!(w["payload"]["state"], "Waiting");
        assert_eq!(w["payload"]["totalPlayers"], 1);
        assert_eq!(w["payload"]["connectedPlayers"], 1);
        let q: Value = serde_json::from_str(&frame(&rx)).unwrap();
        assert_eq!(q["payload"]["state"], "Queued");
        assert_eq!(q["payload"]["ticketId"], json!(t.ticket_id));
        assert_eq!(q["payload"]["estimatedWaitSec"], 0);
        let a: Value = serde_json::from_str(&frame(&rx)).unwrap();
        assert_eq!(a["payload"]["state"], "SessionAssignment");
        let mid = a["payload"]["matchId"].clone();
        let p: Value = serde_json::from_str(&frame(&rx)).unwrap();
        assert_eq!(p["name"], "Play");
        assert_eq!(p["payload"]["matchId"], mid);
        assert_eq!(p["payload"]["sessionId"], json!(sid));
        assert_eq!(p["payload"]["joinDelaySec"], 1);
        assert_eq!(frame(&rx), ws::CLOSE_SOCKET);
        assert!(session::get(&sid).is_some());
        assert_eq!(s.on_text("ping").send, vec!["pong".to_string()]);
        assert!(s.on_text("{\"name\":\"x\"}").send.is_empty());
    }
}
