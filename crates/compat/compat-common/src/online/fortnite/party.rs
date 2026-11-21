use super::super::{util, ws, xmpp, Request, Response};
use super::Fortnite;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const NOTIFY_DELAY_MS: u64 = 25;
const CAPTURE_LIMIT: usize = 4096;
const NS: &str = "Fortnite";
const DOMAIN: &str = "prod.ol.epicgames.com";

#[derive(Clone)]
pub struct Member {
    pub account_id: String,
    pub meta: Map<String, Value>,
    pub connections: Vec<Value>,
    pub revision: u64,
    pub updated_at: String,
    pub joined_at: String,
    pub role: String,
}

#[derive(Clone)]
pub struct Party {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub config: Map<String, Value>,
    pub meta: Map<String, Value>,
    pub members: Vec<Member>,
    pub applicants: Vec<Value>,
    pub invites: Vec<Value>,
    pub intentions: Vec<Value>,
    pub revision: u64,
}

fn parties() -> &'static Mutex<HashMap<String, Party>> {
    static P: OnceLock<Mutex<HashMap<String, Party>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captures() -> &'static Mutex<VecDeque<String>> {
    static C: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn capture(tag: &str, req: &Request) {
    let body = req.body_str();
    let cut = if body.len() > CAPTURE_LIMIT {
        let mut n = CAPTURE_LIMIT;
        while !body.is_char_boundary(n) {
            n -= 1;
        }
        format!("{}...(+{} bytes)", &body[..n], body.len() - n)
    } else {
        body
    };
    let line = format!("{} {} body={}", req.method, req.path, cut.replace(['\r', '\n'], " "));
    crate::record_raw("online", &format!("[{tag}]"), &line);
    let mut g = captures().lock().unwrap_or_else(|p| p.into_inner());
    g.push_back(format!("[{tag}] {line}"));
    while g.len() > 256 {
        g.pop_front();
    }
}

pub fn captured() -> Vec<String> {
    captures().lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect()
}

pub fn note(tag: &str, line: &str) {
    crate::record_raw("online", &format!("[{tag}]"), line);
    let mut g = captures().lock().unwrap_or_else(|p| p.into_inner());
    g.push_back(format!("[{tag}] {line}"));
    while g.len() > 256 {
        g.pop_front();
    }
}

pub fn party_error(status: u16, code: &str, message: &str, numeric: u32, vars: &[&str]) -> Response {
    Response::json_status(status, &json!({
        "errorCode": code,
        "errorMessage": message,
        "messageVars": vars,
        "numericErrorCode": numeric,
        "originatingService": "party",
        "intent": "prod",
    }))
}

fn party_not_found(id: &str) -> Response {
    party_error(404, "errors.com.epicgames.social.party.party_not_found", &format!("Sorry, we couldn't find a party by id {id}"), 51002, &[id])
}

fn member_not_found(acct: &str) -> Response {
    party_error(404, "errors.com.epicgames.social.party.member_not_found", &format!("Sorry, we couldn't find a member by id {acct}"), 51004, &[acct])
}

fn stale(sent: u64, current: u64) -> Response {
    let (s, c) = (sent.to_string(), current.to_string());
    party_error(409, "errors.com.epicgames.social.party.stale_revision", &format!("Sorry, the revision you sent {s} is stale, current revision is {c}"), 51012, &[&s, &c])
}

fn not_found(path: &str) -> Response {
    party_error(404, "errors.com.epicgames.social.party.not_found", "Sorry the resource you were trying to find could not be found", 51000, &[path])
}

pub fn channel_for(acct: &str) -> String {
    format!("xmpp:{}@{DOMAIN}", acct.to_ascii_lowercase())
}

pub fn stanza(acct: &str, body: &Value) -> String {
    format!(
        "<message from=\"xmpp-admin@{DOMAIN}\" to=\"{acct}@{DOMAIN}\" xmlns=\"jabber:client\"><body>{}</body></message>",
        xmpp::escape(&body.to_string())
    )
}

fn kind(t: &str) -> String {
    format!("com.epicgames.social.party.notification.v0.{t}")
}

struct Pending {
    due: Instant,
    recipients: Vec<String>,
    body: Value,
    log: String,
}

fn deliver(p: &Pending) {
    let mut n = 0;
    for a in &p.recipients {
        n += ws::publish(&channel_for(a), &stanza(a, &p.body));
    }
    note("party", &format!("{} published {n}", p.log));
}

fn notifier() -> Option<&'static Mutex<Sender<Pending>>> {
    static Q: OnceLock<Option<Mutex<Sender<Pending>>>> = OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = channel::<Pending>();
        std::thread::Builder::new()
            .name("tombox-party-notify".into())
            .spawn(move || {
                while let Ok(p) = rx.recv() {
                    let now = Instant::now();
                    if p.due > now {
                        std::thread::sleep(p.due - now);
                    }
                    deliver(&p);
                }
            })
            .ok()
            .map(|_| Mutex::new(tx))
    })
    .as_ref()
}

fn notify(recipients: Vec<String>, body: Value, log: String) {
    let p = Pending { due: Instant::now() + Duration::from_millis(NOTIFY_DELAY_MS), recipients, body, log };
    match notifier() {
        Some(q) => {
            if let Err(e) = q.lock().unwrap_or_else(|x| x.into_inner()).send(p) {
                deliver(&e.0);
            }
        }
        None => deliver(&p),
    }
}

fn obj(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn display_name(fnt: &Fortnite, acct: &str) -> String {
    if acct.eq_ignore_ascii_case(&fnt.account_id) { fnt.display_name.clone() } else { acct.to_string() }
}

fn account_of(conn_id: &str) -> String {
    conn_id.split('@').next().unwrap_or("").to_string()
}

fn member_json(m: &Member) -> Value {
    json!({
        "account_id": m.account_id,
        "meta": m.meta,
        "connections": m.connections,
        "revision": m.revision,
        "updated_at": m.updated_at,
        "joined_at": m.joined_at,
        "role": m.role,
    })
}

pub fn party_json(p: &Party) -> Value {
    json!({
        "id": p.id,
        "created_at": p.created_at,
        "updated_at": p.updated_at,
        "config": p.config,
        "members": p.members.iter().map(member_json).collect::<Vec<_>>(),
        "applicants": p.applicants,
        "meta": p.meta,
        "invites": p.invites,
        "revision": p.revision,
        "intentions": p.intentions,
    })
}

fn captain(p: &Party) -> String {
    p.members.iter().find(|m| m.role == "CAPTAIN").map(|m| m.account_id.clone()).unwrap_or_default()
}

fn recipients(p: &Party) -> Vec<String> {
    p.members.iter().map(|m| m.account_id.clone()).collect()
}

fn connection(conn: &Value, now: &str) -> Value {
    json!({
        "id": conn.get("id").and_then(Value::as_str).unwrap_or(""),
        "connected_at": now,
        "updated_at": now,
        "yield_leadership": conn.get("yield_leadership").and_then(Value::as_bool).unwrap_or(false),
        "meta": obj(conn.get("meta")),
    })
}

fn member_joined(fnt: &Fortnite, p: &Party, m: &Member) -> Value {
    json!({
        "type": kind("MEMBER_JOINED"),
        "sent": util::iso_now(),
        "ns": NS,
        "party_id": p.id,
        "account_id": m.account_id,
        "account_dn": display_name(fnt, &m.account_id),
        "member_state_updated": m.meta,
        "connection": m.connections.first().cloned().unwrap_or(json!({})),
        "revision": m.revision,
        "joined_at": m.joined_at,
        "updated_at": m.updated_at,
    })
}

fn create(fnt: &Fortnite, req: &Request) -> Response {
    let body = req.json();
    let Some(conn) = body.get("join_info").and_then(|j| j.get("connection")).filter(|c| c.get("id").and_then(Value::as_str).map_or(false, |s| !s.is_empty())) else {
        return party_error(400, "errors.com.epicgames.common.json_mapping_error", "join_info.connection.id is required", 1019, &["join_info.connection.id"]);
    };
    let now = util::iso_now();
    let mut config = Map::new();
    config.insert("type".into(), json!("DEFAULT"));
    config.insert("joinability".into(), json!("OPEN"));
    config.insert("discoverability".into(), json!("ALL"));
    config.insert("sub_type".into(), json!("default"));
    config.insert("max_size".into(), json!(16));
    config.insert("invite_ttl".into(), json!(14400));
    config.insert("join_confirmation".into(), json!(false));
    for (k, v) in obj(body.get("config")) {
        config.insert(k, v);
    }
    config.insert("intention_ttl".into(), json!(60));
    let conn_id = conn.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let acct = account_of(&conn_id);
    let member = Member {
        account_id: acct.clone(),
        meta: obj(body["join_info"].get("meta")),
        connections: vec![connection(conn, &now)],
        revision: 0,
        updated_at: now.clone(),
        joined_at: now.clone(),
        role: "CAPTAIN".into(),
    };
    let party = Party {
        id: util::random_hex32(),
        created_at: now.clone(),
        updated_at: now,
        config,
        meta: obj(body.get("meta")),
        members: vec![member.clone()],
        applicants: Vec::new(),
        invites: Vec::new(),
        intentions: Vec::new(),
        revision: 0,
    };
    let out = party_json(&party);
    let ev = member_joined(fnt, &party, &member);
    let log = format!("POST {} create party {} captain {acct}", req.path, party.id);
    let to = recipients(&party);
    parties().lock().unwrap_or_else(|p| p.into_inner()).insert(party.id.clone(), party);
    notify(to, ev, log);
    Response::json(&out)
}

fn apply_meta(target: &mut Map<String, Value>, delete: &[Value], update: &Map<String, Value>) {
    for k in delete.iter().filter_map(Value::as_str) {
        target.remove(k);
    }
    for (k, v) in update {
        target.insert(k.clone(), v.clone());
    }
}

fn sent_revision(body: &Value) -> Option<u64> {
    body.get("revision").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

fn patch_party(req: &Request, id: &str) -> Response {
    let body = req.json();
    let Some(sent) = sent_revision(&body) else {
        return party_error(400, "errors.com.epicgames.common.json_mapping_error", "revision is required", 1019, &["revision"]);
    };
    let mut g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let Some(p) = g.get_mut(id) else { return party_not_found(id) };
    if sent != p.revision {
        let r = stale(sent, p.revision);
        note("party", &format!("PATCH {} stale rev sent {sent} current {}", req.path, p.revision));
        return r;
    }
    for (k, v) in obj(body.get("config")) {
        p.config.insert(k, v);
    }
    let meta = body.get("meta").cloned().unwrap_or(json!({}));
    let delete = meta.get("delete").and_then(Value::as_array).cloned().unwrap_or_default();
    let update = obj(meta.get("update"));
    apply_meta(&mut p.meta, &delete, &update);
    let pick = |body_key: &str, cfg_key: &str, dflt: Value, cfg: &mut Map<String, Value>| -> Value {
        match body.get(body_key).filter(|v| !v.is_null()) {
            Some(v) => {
                cfg.insert(cfg_key.into(), v.clone());
                v.clone()
            }
            None => cfg.get(cfg_key).cloned().unwrap_or(dflt),
        }
    };
    let privacy = match body.get("party_privacy_type").filter(|v| !v.is_null()) {
        Some(v) => v.clone(),
        None => p.config.get("joinability").cloned().unwrap_or(json!("OPEN")),
    };
    let party_type = pick("party_type", "type", json!("DEFAULT"), &mut p.config);
    let sub_type = pick("party_sub_type", "sub_type", json!("default"), &mut p.config);
    let max = pick("max_number_of_members", "max_size", json!(16), &mut p.config);
    let ttl = pick("invite_ttl_seconds", "invite_ttl", json!(14400), &mut p.config);
    let old = p.revision;
    p.revision += 1;
    p.updated_at = util::iso_now();
    let ev = json!({
        "type": kind("PARTY_UPDATED"),
        "sent": util::iso_now(),
        "ns": NS,
        "party_id": p.id,
        "captain_id": captain(p),
        "party_state_updated": update,
        "party_state_removed": delete,
        "party_state_overrode": obj(body.get("party_state_overridden")),
        "party_privacy_type": privacy,
        "party_type": party_type,
        "party_sub_type": sub_type,
        "max_number_of_members": max,
        "invite_ttl_seconds": ttl,
        "created_at": p.created_at,
        "updated_at": p.updated_at,
        "revision": p.revision,
    });
    let to = recipients(p);
    let log = format!("PATCH {} rev {old}->{}", req.path, p.revision);
    drop(g);
    notify(to, ev, log);
    Response::empty(204)
}

fn patch_member_meta(fnt: &Fortnite, req: &Request, id: &str, acct: &str) -> Response {
    let body = req.json();
    let Some(sent) = sent_revision(&body) else {
        return party_error(400, "errors.com.epicgames.common.json_mapping_error", "revision is required", 1019, &["revision"]);
    };
    let mut g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let Some(p) = g.get_mut(id) else { return party_not_found(id) };
    let to = recipients(p);
    let pid = p.id.clone();
    let Some(m) = p.members.iter_mut().find(|m| m.account_id.eq_ignore_ascii_case(acct)) else { return member_not_found(acct) };
    if sent != m.revision {
        note("party", &format!("PATCH {} stale rev sent {sent} current {}", req.path, m.revision));
        return stale(sent, m.revision);
    }
    let delete = body.get("delete").and_then(Value::as_array).cloned().unwrap_or_default();
    let update = obj(body.get("update"));
    apply_meta(&mut m.meta, &delete, &update);
    let old = m.revision;
    m.revision += 1;
    m.updated_at = util::iso_now();
    let ev = json!({
        "type": kind("MEMBER_STATE_UPDATED"),
        "sent": util::iso_now(),
        "ns": NS,
        "party_id": pid,
        "account_id": m.account_id,
        "account_dn": display_name(fnt, &m.account_id),
        "member_state_updated": update,
        "member_state_removed": delete,
        "joined_at": m.joined_at,
        "updated_at": m.updated_at,
        "revision": m.revision,
    });
    let log = format!("PATCH {} rev {old}->{}", req.path, m.revision);
    p.updated_at = util::iso_now();
    drop(g);
    notify(to, ev, log);
    Response::empty(204)
}

fn join(fnt: &Fortnite, req: &Request, id: &str, acct: &str) -> Response {
    let body = req.json();
    let mut g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let Some(p) = g.get_mut(id) else { return party_not_found(id) };
    let max = p.config.get("max_size").and_then(Value::as_u64).unwrap_or(16) as usize;
    let now = util::iso_now();
    let conn = body.get("connection").cloned().unwrap_or(json!({}));
    if let Some(m) = p.members.iter_mut().find(|m| m.account_id.eq_ignore_ascii_case(acct)) {
        m.connections = vec![connection(&conn, &now)];
        m.updated_at = now;
        return Response::json(&json!({"status": "JOINED", "party_id": id}));
    }
    if p.members.len() >= max {
        return party_error(409, "errors.com.epicgames.social.party.party_is_full", &format!("Party {id} is full"), 51025, &[id]);
    }
    let m = Member {
        account_id: acct.to_string(),
        meta: obj(body.get("meta")),
        connections: vec![connection(&conn, &now)],
        revision: 0,
        updated_at: now.clone(),
        joined_at: now,
        role: if p.members.is_empty() { "CAPTAIN".into() } else { "MEMBER".into() },
    };
    p.members.push(m.clone());
    p.updated_at = util::iso_now();
    let ev = member_joined(fnt, p, &m);
    let to = recipients(p);
    drop(g);
    notify(to, ev, format!("POST {} join {acct}", req.path));
    Response::json(&json!({"status": "JOINED", "party_id": id}))
}

fn leave(fnt: &Fortnite, req: &Request, id: &str, acct: &str) -> Response {
    let mut g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let Some(p) = g.get_mut(id) else { return party_not_found(id) };
    let Some(i) = p.members.iter().position(|m| m.account_id.eq_ignore_ascii_case(acct)) else { return member_not_found(acct) };
    let to = recipients(p);
    let gone = p.members.remove(i);
    p.updated_at = util::iso_now();
    let left = json!({
        "type": kind("MEMBER_LEFT"),
        "sent": util::iso_now(),
        "ns": NS,
        "party_id": p.id,
        "account_id": gone.account_id,
        "member_state_updated": {},
    });
    let promoted = if gone.role == "CAPTAIN" && !p.members.is_empty() {
        p.members[0].role = "CAPTAIN".into();
        Some(json!({
            "type": kind("MEMBER_NEW_CAPTAIN"),
            "sent": util::iso_now(),
            "ns": NS,
            "party_id": p.id,
            "account_id": p.members[0].account_id,
            "account_dn": display_name(fnt, &p.members[0].account_id),
            "member_state_updated": {},
            "joined_at": p.members[0].joined_at,
            "updated_at": p.members[0].updated_at,
            "revision": p.members[0].revision,
        }))
    } else {
        None
    };
    let rest = recipients(p);
    if p.members.is_empty() {
        g.remove(id);
    }
    drop(g);
    notify(to, left, format!("DELETE {} member {acct} left", req.path));
    if let Some(ev) = promoted {
        notify(rest, ev, format!("DELETE {} new captain", req.path));
    }
    Response::empty(204)
}

fn promote(fnt: &Fortnite, req: &Request, id: &str, acct: &str) -> Response {
    let mut g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let Some(p) = g.get_mut(id) else { return party_not_found(id) };
    if !p.members.iter().any(|m| m.account_id.eq_ignore_ascii_case(acct)) {
        return member_not_found(acct);
    }
    for m in p.members.iter_mut() {
        m.role = if m.account_id.eq_ignore_ascii_case(acct) { "CAPTAIN".into() } else { "MEMBER".into() };
    }
    p.updated_at = util::iso_now();
    let Some(m) = p.members.iter().find(|m| m.account_id.eq_ignore_ascii_case(acct)).cloned() else { return member_not_found(acct) };
    let ev = json!({
        "type": kind("MEMBER_NEW_CAPTAIN"),
        "sent": util::iso_now(),
        "ns": NS,
        "party_id": p.id,
        "account_id": m.account_id,
        "account_dn": display_name(fnt, &m.account_id),
        "member_state_updated": {},
        "joined_at": m.joined_at,
        "updated_at": m.updated_at,
        "revision": m.revision,
    });
    let to = recipients(p);
    drop(g);
    notify(to, ev, format!("POST {} promote {acct}", req.path));
    Response::empty(204)
}

fn conference(req: &Request, id: &str, acct: &str) -> Response {
    let g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let Some(p) = g.get(id) else { return party_not_found(id) };
    if !p.members.iter().any(|m| m.account_id.eq_ignore_ascii_case(acct)) {
        return member_not_found(acct);
    }
    let asked: Vec<String> = req.json().get("providers").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default();
    note("party", &format!("POST {} conference providers {asked:?} -> none available", req.path));
    Response::json(&json!({"providers": {}}))
}

fn disband(req: &Request, id: &str) -> Response {
    let Some(p) = parties().lock().unwrap_or_else(|p| p.into_inner()).remove(id) else { return party_not_found(id) };
    let ev = json!({
        "type": kind("PARTY_DISBANDED"),
        "sent": util::iso_now(),
        "ns": NS,
        "party_id": p.id,
        "captain_id": captain(&p),
        "party_state_removed": p.meta.keys().cloned().collect::<Vec<_>>(),
    });
    notify(recipients(&p), ev, format!("DELETE {} disband", req.path));
    Response::empty(204)
}

pub fn user_parties(acct: &str) -> Vec<Value> {
    let g = parties().lock().unwrap_or_else(|p| p.into_inner());
    let mut v: Vec<&Party> = g.values().filter(|p| p.members.iter().any(|m| m.account_id.eq_ignore_ascii_case(acct))).collect();
    v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    v.into_iter().map(party_json).collect()
}

pub fn party(id: &str) -> Option<Party> {
    parties().lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let ["party", "api", "v1", ns, rest @ ..] = seg else { return None };
    if !ns.eq_ignore_ascii_case(NS) {
        return None;
    }
    match rest {
        ["user", _, "notifications", ..] => return None,
        ["user", ..] | ["parties", ..] => {}
        _ => return None,
    }
    if m != "GET" {
        capture("party", req);
    }
    Some(match (m, rest) {
        ("GET", ["user", acct]) => Response::json(&json!({"current": user_parties(acct), "pending": [], "invites": [], "pings": []})),
        ("GET", ["user", _, "pings", ..]) => Response::json(&json!([])),
        ("POST", ["parties"]) => create(fnt, req),
        ("GET", ["parties", id]) => match party(id) {
            Some(p) => Response::json(&party_json(&p)),
            None => party_not_found(id),
        },
        ("PATCH", ["parties", id]) => patch_party(req, id),
        ("DELETE", ["parties", id]) => disband(req, id),
        ("PATCH", ["parties", id, "members", acct, "meta"]) => patch_member_meta(fnt, req, id, acct),
        ("POST", ["parties", id, "members", acct, "join"]) => join(fnt, req, id, acct),
        ("DELETE", ["parties", id, "members", acct]) => leave(fnt, req, id, acct),
        ("POST", ["parties", id, "members", acct, "promote"]) => promote(fnt, req, id, acct),
        ("POST", ["parties", id, "members", acct, "conferences", "connection"]) => conference(req, id, acct),
        _ => not_found(&req.path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    fn req(method: &str, path: &str, body: Value) -> Request {
        Request { method: method.into(), path: path.into(), query: Vec::new(), headers: Vec::new(), body: if body.is_null() { Vec::new() } else { body.to_string().into_bytes() } }
    }

    fn fnt(acct: &str) -> Fortnite {
        Fortnite::with_account(acct, acct)
    }

    fn create_for(acct: &str) -> String {
        let f = fnt(acct);
        let r = route(&f, &req("POST", "/party/api/v1/Fortnite/parties", json!({
            "config": {"join_confirmation": false, "joinability": "OPEN", "max_size": 16},
            "join_info": {"connection": {"id": format!("{acct}@prod.ol.epicgames.com/V2:Fortnite:XBX::ABC"), "meta": {"urn:epic:conn:platform_s": "XBX"}}, "meta": {"urn:epic:member:dn_s": acct}},
            "meta": {"urn:epic:cfg:party-type-id_s": "default"},
        })), "POST", &["party", "api", "v1", "Fortnite", "parties"]).unwrap();
        assert_eq!(r.status, 200);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        b["id"].as_str().unwrap().to_string()
    }

    fn call(acct: &str, method: &str, path: &str, body: Value) -> Response {
        let f = fnt(acct);
        let r = req(method, path, body);
        let seg = r.segments();
        route(&f, &r, method, &seg).unwrap()
    }

    fn listen(acct: &str) -> (ws::Outlet, Receiver<String>) {
        let (o, rx) = ws::Outlet::detached();
        o.subscribe(&channel_for(acct));
        (o, rx)
    }

    fn next_of(rx: &Receiver<String>, ty: &str, party: &str) -> Value {
        loop {
            let s = rx.recv_timeout(Duration::from_secs(3)).expect("stanza");
            let start = s.find("<body>").unwrap() + 6;
            let end = s.find("</body>").unwrap();
            let v: Value = serde_json::from_str(&xmpp::unescape(&s[start..end])).unwrap();
            if v["type"].as_str().map_or(false, |t| t.ends_with(ty)) && v["party_id"] == party {
                assert!(s.starts_with("<message from=\"xmpp-admin@prod.ol.epicgames.com\""));
                return v;
            }
        }
    }

    #[test]
    fn create_then_get_roundtrip() {
        let (_o, rx) = listen("PtA");
        let id = create_for("PtA");
        let joined = next_of(&rx, "MEMBER_JOINED", &id);
        assert_eq!(joined["account_id"], "PtA");
        assert_eq!(joined["connection"]["yield_leadership"], false);
        let r = call("PtA", "GET", &format!("/party/api/v1/Fortnite/parties/{id}"), Value::Null);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["id"], json!(id));
        assert_eq!(b["members"][0]["role"], "CAPTAIN");
        assert_eq!(b["revision"], 0);
        assert_eq!(b["meta"]["urn:epic:cfg:party-type-id_s"], "default");
        assert_eq!(call("PtA", "GET", "/party/api/v1/Fortnite/parties/nope", Value::Null).status, 404);
    }

    #[test]
    fn patch_increments_revision() {
        let id = create_for("PtB");
        let r = call("PtB", "PATCH", &format!("/party/api/v1/Fortnite/parties/{id}"), json!({"meta": {"delete": [], "update": {"Default:PrivacySettings_j": "{}"}}, "revision": 0}));
        assert_eq!(r.status, 204);
        let p = party(&id).unwrap();
        assert_eq!(p.revision, 1);
        assert_eq!(p.meta["Default:PrivacySettings_j"], "{}");
        let r = call("PtB", "PATCH", &format!("/party/api/v1/Fortnite/parties/{id}"), json!({"meta": {"delete": ["Default:PrivacySettings_j"], "update": {}}, "revision": 1}));
        assert_eq!(r.status, 204);
        let p = party(&id).unwrap();
        assert_eq!(p.revision, 2);
        assert!(p.meta.get("Default:PrivacySettings_j").is_none());
    }

    #[test]
    fn stale_revision_409() {
        let id = create_for("PtC");
        let r = call("PtC", "PATCH", &format!("/party/api/v1/Fortnite/parties/{id}"), json!({"meta": {"delete": [], "update": {}}, "revision": 5}));
        assert_eq!(r.status, 409);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["errorCode"], "errors.com.epicgames.social.party.stale_revision");
        assert_eq!(b["messageVars"], json!(["5", "0"]));
        let r = call("PtC", "PATCH", &format!("/party/api/v1/Fortnite/parties/{id}/members/PtC/meta"), json!({"delete": [], "update": {}, "revision": 3}));
        assert_eq!(r.status, 409);
    }

    #[test]
    fn member_meta_patch_publishes_member_state_updated() {
        let (_o, rx) = listen("PtD");
        let id = create_for("PtD");
        let r = call("PtD", "PATCH", &format!("/party/api/v1/Fortnite/parties/{id}/members/PtD/meta"), json!({"delete": ["Old_s"], "update": {"Default:LobbyState_j": "{\"LobbyState\":{\"gameReadiness\":\"Ready\"}}"}, "revision": 0}));
        assert_eq!(r.status, 204);
        let v = next_of(&rx, "MEMBER_STATE_UPDATED", &id);
        assert_eq!(v["type"], "com.epicgames.social.party.notification.v0.MEMBER_STATE_UPDATED");
        assert_eq!(v["ns"], "Fortnite");
        assert_eq!(v["account_id"], "PtD");
        assert_eq!(v["account_dn"], "PtD");
        assert_eq!(v["revision"], 1);
        assert_eq!(v["member_state_removed"], json!(["Old_s"]));
        assert!(v["member_state_updated"]["Default:LobbyState_j"].as_str().unwrap().contains("Ready"));
        for k in ["sent", "joined_at", "updated_at"] {
            assert!(v[k].is_string(), "{k}");
        }
    }

    #[test]
    fn rapid_member_patches_publish_in_revision_order() {
        let (_o, rx) = listen("PtO");
        let id = create_for("PtO");
        let path = format!("/party/api/v1/Fortnite/parties/{id}/members/PtO/meta");
        for rev in 0..6u64 {
            let r = call("PtO", "PATCH", &path, json!({"delete": [], "update": {"Default:LobbyState_j": format!("{{\"n\":{rev}}}")}, "revision": rev}));
            assert_eq!(r.status, 204);
        }
        for want in 1..=6u64 {
            assert_eq!(next_of(&rx, "MEMBER_STATE_UPDATED", &id)["revision"], want);
        }
    }

    #[test]
    fn party_patch_publishes_party_updated() {
        let (_o, rx) = listen("PtE");
        let id = create_for("PtE");
        let r = call("PtE", "PATCH", &format!("/party/api/v1/Fortnite/parties/{id}"), json!({
            "config": {"joinability": "OPEN"},
            "meta": {"delete": ["X_s"], "update": {"Default:ActivityName_s": "Lobby"}},
            "party_state_overridden": {}, "party_privacy_type": "PUBLIC", "party_type": "DEFAULT", "party_sub_type": "default",
            "max_number_of_members": 16, "invite_ttl_seconds": 14400, "revision": 0,
        }));
        assert_eq!(r.status, 204);
        let v = next_of(&rx, "PARTY_UPDATED", &id);
        assert_eq!(v["captain_id"], "PtE");
        assert_eq!(v["revision"], 1);
        assert_eq!(v["party_state_updated"]["Default:ActivityName_s"], "Lobby");
        assert_eq!(v["party_state_removed"], json!(["X_s"]));
        assert_eq!(v["party_state_overrode"], json!({}));
        assert_eq!(v["party_privacy_type"], "PUBLIC");
        assert_eq!(v["max_number_of_members"], 16);
        assert_eq!(v["invite_ttl_seconds"], 14400);
        let want = format!("[party] PATCH /party/api/v1/Fortnite/parties/{id} rev 0->1 published 1");
        let mut seen = false;
        for _ in 0..50 {
            if captured().iter().any(|l| *l == want) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen, "{want}");
    }

    #[test]
    fn user_current_lists_party() {
        let id = create_for("PtF");
        let r = call("PtF", "GET", "/party/api/v1/Fortnite/user/PtF", Value::Null);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["current"][0]["id"], json!(id));
        assert_eq!(b["pending"], json!([]));
        let r = call("Nobody", "GET", "/party/api/v1/Fortnite/user/Nobody", Value::Null);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["current"], json!([]));
        let f = fnt("PtF");
        let rq = req("GET", "/party/api/v1/Fortnite/user/PtF/notifications/undelivered/count", Value::Null);
        assert!(route(&f, &rq, "GET", &rq.segments()).is_none());
    }

    #[test]
    fn unknown_party_path_404() {
        let id = create_for("PtG");
        let r = call("PtG", "POST", &format!("/party/api/v1/Fortnite/parties/{id}/members/PtG/whatever"), json!({}));
        assert_eq!(r.status, 404);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["errorCode"], "errors.com.epicgames.social.party.not_found");
        assert_eq!(call("PtG", "PUT", "/party/api/v1/Fortnite/parties", json!({})).status, 404);
    }

    #[test]
    fn leave_last_member_deletes_party() {
        let (_o, rx) = listen("PtH");
        let id = create_for("PtH");
        assert_eq!(call("PtH", "DELETE", &format!("/party/api/v1/Fortnite/parties/{id}/members/PtH"), Value::Null).status, 204);
        assert_eq!(next_of(&rx, "MEMBER_LEFT", &id)["account_id"], "PtH");
        assert!(party(&id).is_none());
    }

    #[test]
    fn logs_body_on_patch() {
        let id = create_for("PtI");
        let f = super::super::Fortnite::with_account("PtI", "PtI");
        let r = req("PATCH", &format!("/party/api/v1/Fortnite/parties/{id}"), json!({"meta": {"delete": [], "update": {"Marker_s": "logs_body_on_patch"}}, "revision": 0}));
        let resp = <super::super::Fortnite as super::super::super::TitleService>::route(&f, &r).unwrap();
        assert_eq!(resp.status, 204);
        assert!(captured().iter().any(|l| l.starts_with(&format!("[party] PATCH /party/api/v1/Fortnite/parties/{id} body=")) && l.contains("logs_body_on_patch")));
    }
}
