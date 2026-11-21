use super::fortnite::party::{capture, note};
use super::{rta, util, Request, Response};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

pub const CONTRACT_VERSION: u64 = 107;
const RTA_CONNECTIONS: &str = "https://sessiondirectory.xboxlive.com/connections";

#[derive(Clone)]
struct Doc {
    correlation_id: String,
    start_time: String,
    constants: Value,
    properties: Value,
    servers: Value,
    members: BTreeMap<u64, Value>,
    next_index: u64,
    etag: u64,
}

#[derive(Default)]
struct Store {
    sessions: HashMap<(String, String, String), Doc>,
    activities: BTreeMap<(String, u64), Value>,
    handles: BTreeMap<String, Value>,
    etag: u64,
}

fn store() -> &'static Mutex<Store> {
    static S: OnceLock<Mutex<Store>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Store::default()))
}

fn me() -> (u64, String) {
    let u = crate::gdk::user::user();
    (u.xuid, u.gamertag.clone())
}

fn err(status: u16, code: &str, description: &str) -> Response {
    Response::json_status(status, &json!({"code": code, "description": description}))
}

fn merge(target: &mut Value, patch: &Value) {
    let Some(p) = patch.as_object() else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = json!({});
    }
    let Some(t) = target.as_object_mut() else { return };
    for (k, v) in p {
        if v.is_null() {
            t.remove(k);
        } else if v.is_object() {
            merge(t.entry(k.clone()).or_insert(json!({})), v);
        } else {
            t.insert(k.clone(), v.clone());
        }
    }
}

fn section(v: &Value) -> Value {
    let mut out = json!({"system": {}, "custom": {}});
    merge(&mut out, v);
    out
}

fn member_xuid(m: &Value) -> Option<u64> {
    let x = &m["constants"]["system"]["xuid"];
    x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_u64())
}

fn render(d: &Doc) -> Value {
    let mut members = Map::new();
    for (i, m) in &d.members {
        let mut m = m.clone();
        let next = d.members.range(i + 1..).next().map(|(k, _)| *k).unwrap_or(d.next_index);
        m["next"] = json!(next);
        members.insert(i.to_string(), m);
    }
    json!({
        "contractVersion": CONTRACT_VERSION,
        "correlationId": d.correlation_id,
        "startTime": d.start_time,
        "constants": d.constants,
        "properties": d.properties,
        "servers": d.servers,
        "members": members,
        "membersInfo": {
            "first": d.members.keys().next().copied().unwrap_or(0),
            "next": d.next_index,
            "count": d.members.len(),
            "accepted": d.members.len(),
        },
    })
}

fn location(scid: &str, tpl: &str, name: &str) -> String {
    format!("/serviceconfigs/{scid}/sessionTemplates/{tpl}/sessions/{name}")
}

fn with_headers(mut r: Response, d: &Doc, loc: &str) -> Response {
    r.headers.push(("ETag".into(), format!("\"{}\"", d.etag)));
    r.headers.push(("Content-Location".into(), loc.to_string()));
    r
}

fn etag_matches(header: &str, etag: u64) -> bool {
    header.split(',').map(|e| e.trim().trim_start_matches("W/").trim_matches('"')).any(|e| e == "*" || e == etag.to_string())
}

fn apply_members(d: &mut Doc, patch: &Value, xuid: u64, gamertag: &str) {
    let Some(ms) = patch.as_object() else { return };
    for (k, v) in ms {
        let idx = if k.eq_ignore_ascii_case("me") {
            d.members.iter().find(|(_, m)| member_xuid(m) == Some(xuid)).map(|(i, _)| *i)
        } else {
            k.parse::<u64>().ok().filter(|i| d.members.contains_key(i))
        };
        if v.is_null() {
            if let Some(i) = idx {
                d.members.remove(&i);
            }
            continue;
        }
        let i = match idx {
            Some(i) => i,
            None if k.eq_ignore_ascii_case("me") => {
                let i = d.next_index;
                d.next_index += 1;
                let mut constants = section(&v["constants"]);
                constants["system"]["xuid"] = json!(xuid.to_string());
                constants["system"]["index"] = json!(i);
                d.members.insert(i, json!({
                    "constants": constants,
                    "properties": section(&Value::Null),
                    "gamertag": gamertag,
                    "joinTime": util::iso_now(),
                    "activeTitleId": crate::cfg("title_id").unwrap_or_else(|| "2D2DE1E1".into()),
                    "initialize": false,
                    "turn": false,
                }));
                i
            }
            None => continue,
        };
        if let Some(m) = d.members.get_mut(&i) {
            if let Some(p) = v.get("properties") {
                merge(&mut m["properties"], p);
            }
        }
    }
}

fn shoulder_tap(scid: &str, tpl: &str, name: &str, d: &Doc) -> usize {
    let subscribed = d.members.values().any(|m| m["properties"]["system"].get("subscription").map_or(false, |s| !s.is_null()));
    if !subscribed {
        return 0;
    }
    rta::publish(RTA_CONNECTIONS, &json!({"shoulderTaps": [{"resource": format!("{scid}~{tpl}~{name}"), "changeNumber": d.etag, "branch": d.correlation_id}]}))
}

fn put_session(req: &Request, scid: &str, tpl: &str, name: &str) -> Response {
    capture("mpsd", req);
    let body = req.json();
    if !body.is_object() {
        return err(400, "BadRequest", "the session document must be a JSON object");
    }
    let key = (scid.to_ascii_lowercase(), tpl.to_ascii_lowercase(), name.to_ascii_lowercase());
    let loc = location(scid, tpl, name);
    let (xuid, tag) = me();
    let mut st = store().lock().unwrap_or_else(|p| p.into_inner());
    let existing = st.sessions.get(&key).cloned();
    if let Some(h) = req.header("if-match") {
        if !existing.as_ref().map_or(false, |d| etag_matches(h, d.etag)) {
            return err(412, "PreconditionFailed", "If-Match does not match the current session etag");
        }
    }
    if let Some(h) = req.header("if-none-match") {
        if existing.as_ref().map_or(false, |d| etag_matches(h, d.etag)) {
            return err(412, "PreconditionFailed", "the session already exists");
        }
    }
    let created = existing.is_none();
    let mut d = existing.unwrap_or_else(|| Doc {
        correlation_id: util::random_uuid(),
        start_time: util::iso_now(),
        constants: section(&body["constants"]),
        properties: section(&Value::Null),
        servers: json!({}),
        members: BTreeMap::new(),
        next_index: 0,
        etag: 0,
    });
    if let Some(p) = body.get("properties") {
        merge(&mut d.properties, p);
    }
    if let Some(s) = body.get("servers") {
        merge(&mut d.servers, s);
    }
    apply_members(&mut d, &body["members"], xuid, &tag);
    if d.members.is_empty() {
        st.sessions.remove(&key);
        drop(st);
        note("mpsd", &format!("PUT {loc} -> no members left, deleted"));
        return Response::empty(204);
    }
    st.etag += 1;
    d.etag = st.etag;
    st.sessions.insert(key, d.clone());
    drop(st);
    let taps = shoulder_tap(scid, tpl, name, &d);
    note("mpsd", &format!("PUT {loc} -> {} etag {} members {} rta {taps}", if created { 201 } else { 200 }, d.etag, d.members.len()));
    with_headers(Response::json_status(if created { 201 } else { 200 }, &render(&d)), &d, &loc)
}

fn get_session(scid: &str, tpl: &str, name: &str) -> Response {
    let key = (scid.to_ascii_lowercase(), tpl.to_ascii_lowercase(), name.to_ascii_lowercase());
    match store().lock().unwrap_or_else(|p| p.into_inner()).sessions.get(&key).cloned() {
        Some(d) => with_headers(Response::json(&render(&d)), &d, &location(scid, tpl, name)),
        None => err(404, "NotFound", "session not found"),
    }
}

fn handles(req: &Request, m: &str, rest: &[&str]) -> Option<Response> {
    let (xuid, _) = me();
    Some(match (m, rest) {
        ("POST", []) => {
            capture("mpsd", req);
            let b = req.json();
            let ty = b.get("type").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
            if !matches!(ty.as_str(), "activity" | "invite") || !b["sessionRef"].is_object() {
                return Some(err(400, "BadRequest", "type must be activity or invite and sessionRef is required"));
            }
            let id = util::random_uuid();
            let mut h = b.clone();
            h["id"] = json!(id);
            h["ownerXuid"] = json!(xuid.to_string());
            h["createTime"] = json!(util::iso_now());
            let mut st = store().lock().unwrap_or_else(|p| p.into_inner());
            if ty == "activity" {
                st.handles.retain(|_, v| !(v["type"].as_str().map_or(false, |t| t.eq_ignore_ascii_case("activity")) && v["ownerXuid"] == json!(xuid.to_string())));
            }
            st.handles.insert(id, h.clone());
            Response::json_status(201, &h)
        }
        ("POST", ["query"]) => {
            let b = req.json();
            let owners: Vec<String> = b["owners"]["xuids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string()))).collect()).unwrap_or_default();
            let ty = b.get("type").and_then(Value::as_str).unwrap_or("activity").to_ascii_lowercase();
            let st = store().lock().unwrap_or_else(|p| p.into_inner());
            let results: Vec<Value> = st.handles.values().filter(|h| h["type"].as_str().map_or(false, |t| t.eq_ignore_ascii_case(&ty)) && (owners.is_empty() || owners.iter().any(|o| h["ownerXuid"] == json!(o)))).cloned().collect();
            Response::json(&json!({"results": results}))
        }
        ("DELETE", [id]) => match store().lock().unwrap_or_else(|p| p.into_inner()).handles.remove(*id) {
            Some(_) => Response::empty(204),
            None => err(404, "NotFound", "handle not found"),
        },
        ("GET", [id]) => match store().lock().unwrap_or_else(|p| p.into_inner()).handles.get(*id).cloned() {
            Some(h) => Response::json(&h),
            None => err(404, "NotFound", "handle not found"),
        },
        _ => return None,
    })
}

pub fn route(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    match (m, seg) {
        ("PUT", ["serviceconfigs", scid, "sessiontemplates" | "sessionTemplates", tpl, "sessions", name]) => Some(put_session(req, scid, tpl, name)),
        ("GET", ["serviceconfigs", scid, "sessiontemplates" | "sessionTemplates", tpl, "sessions", name]) => Some(get_session(scid, tpl, name)),
        (_, ["handles", rest @ ..]) => handles(req, m, rest),
        _ => {
            capture("mpsd", req);
            Some(err(404, "NotFound", "unknown session directory resource"))
        }
    }
}

fn parse_xuid(s: &str) -> Option<u64> {
    let s = s.trim();
    s.strip_prefix("xuid(").and_then(|r| r.strip_suffix(')')).unwrap_or(s).parse().ok()
}

pub fn activity(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let (mine, _) = me();
    Some(match (m, seg) {
        ("PUT" | "DELETE", ["titles", title, "users" | "xuids", x, "activities" | "activity"]) => {
            capture("mpsd", req);
            let Some(xuid) = parse_xuid(x) else { return Some(err(400, "BadRequest", "xuid must be numeric")) };
            if xuid != mine {
                return Some(err(403, "Forbidden", "activity can only be changed for the signed-in user"));
            }
            let key = (title.to_ascii_lowercase(), xuid);
            let mut st = store().lock().unwrap_or_else(|p| p.into_inner());
            if m == "DELETE" {
                st.activities.remove(&key);
                return Some(Response::empty(204));
            }
            let b = req.json();
            if !b.is_object() || b.get("connectionString").and_then(Value::as_str).map_or(true, str::is_empty) {
                return Some(err(400, "BadRequest", "connectionString is required"));
            }
            let mut a = b.clone();
            a["titleId"] = json!(title);
            a["xuid"] = json!(xuid.to_string());
            st.activities.insert(key, a.clone());
            Response::json(&a)
        }
        ("POST", ["titles", title, "activities", "query"]) => {
            let b = req.json();
            let users: Vec<u64> = b["users"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().and_then(parse_xuid).or_else(|| v.as_u64())).collect()).unwrap_or_default();
            let st = store().lock().unwrap_or_else(|p| p.into_inner());
            let t = title.to_ascii_lowercase();
            let list: Vec<Value> = st.activities.iter().filter(|((ti, x), _)| *ti == t && (users.is_empty() || users.contains(x))).map(|(_, v)| v.clone()).collect();
            Response::json(&Value::Array(list))
        }
        _ => {
            capture("mpsd", req);
            err(404, "NotFound", "unknown multiplayer activity resource")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, path: &str, headers: Vec<(&str, String)>, body: Value) -> Request {
        Request { method: method.into(), path: path.into(), query: Vec::new(), headers: headers.into_iter().map(|(k, v)| (k.to_string(), v)).collect(), body: if body.is_null() { Vec::new() } else { body.to_string().into_bytes() } }
    }

    fn put(name: &str, headers: Vec<(&str, String)>, body: Value) -> Response {
        let r = req("PUT", &format!("/serviceconfigs/00000000-0000-0000-0000-00006d2de1e1/sessionTemplates/PartySession/sessions/{name}"), headers, body);
        route(&r, "PUT", &r.segments()).unwrap()
    }

    fn etag(r: &Response) -> String {
        r.headers.iter().find(|(k, _)| k == "ETag").unwrap().1.clone()
    }

    fn join_body() -> Value {
        json!({"constants": {"system": {"visibility": "open", "maxMembersCount": 16}}, "members": {"me": {"constants": {"system": {"initialize": true}}, "properties": {"system": {"active": true}}}}, "properties": {"system": {"joinRestriction": "followed"}}})
    }

    #[test]
    fn put_creates_201_with_etag() {
        let r = put("mpsd-a", vec![], join_body());
        assert_eq!(r.status, 201);
        assert!(etag(&r).starts_with('"'));
        assert!(r.headers.iter().any(|(k, v)| k == "Content-Location" && v.ends_with("/sessions/mpsd-a")));
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["contractVersion"], 107);
        assert_eq!(b["members"]["0"]["constants"]["system"]["xuid"], json!(me().0.to_string()));
        assert_eq!(b["membersInfo"], json!({"first": 0, "next": 1, "count": 1, "accepted": 1}));
        assert_eq!(b["constants"]["system"]["maxMembersCount"], 16);
    }

    #[test]
    fn put_merge_updates_properties() {
        put("mpsd-b", vec![], join_body());
        let r = put("mpsd-b", vec![], json!({"properties": {"custom": {"partyId": "abc"}, "system": {"joinRestriction": "local"}}, "constants": {"system": {"maxMembersCount": 2}}}));
        assert_eq!(r.status, 200);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["properties"]["custom"]["partyId"], "abc");
        assert_eq!(b["properties"]["system"]["joinRestriction"], "local");
        assert_eq!(b["constants"]["system"]["maxMembersCount"], 16);
        assert_eq!(b["membersInfo"]["count"], 1);
    }

    #[test]
    fn if_match_mismatch_412() {
        let r = put("mpsd-c", vec![], join_body());
        let e = etag(&r);
        assert_eq!(put("mpsd-c", vec![("If-Match", "\"999999\"".into())], json!({"properties": {"custom": {"x": 1}}})).status, 412);
        assert_eq!(put("mpsd-c", vec![("If-Match", e)], json!({"properties": {"custom": {"x": 1}}})).status, 200);
        assert_eq!(put("mpsd-c", vec![("If-None-Match", "*".into())], join_body()).status, 412);
    }

    #[test]
    fn member_null_leaves_and_deletes() {
        put("mpsd-d", vec![], join_body());
        let r = put("mpsd-d", vec![], json!({"members": {"me": null}}));
        assert_eq!(r.status, 204);
        let g = req("GET", "/serviceconfigs/00000000-0000-0000-0000-00006d2de1e1/sessionTemplates/PartySession/sessions/mpsd-d", vec![], Value::Null);
        assert_eq!(route(&g, "GET", &g.segments()).unwrap().status, 404);
    }

    #[test]
    fn activity_put_query_delete() {
        let x = me().0;
        let p = req("PUT", &format!("/titles/2D2DE1E1/users/{x}/activities"), vec![], json!({"connectionString": "party:abc", "joinRestriction": "Followed", "maxPlayers": 16, "currentPlayers": 1, "platform": "Scarlett"}));
        assert_eq!(activity(&p, "PUT", &p.segments()).unwrap().status, 200);
        let q = req("POST", "/titles/2D2DE1E1/activities/query", vec![], json!({"users": [x.to_string()]}));
        let r = activity(&q, "POST", &q.segments()).unwrap();
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b[0]["connectionString"], "party:abc");
        let d = req("DELETE", &format!("/titles/2D2DE1E1/users/{x}/activities"), vec![], Value::Null);
        assert_eq!(activity(&d, "DELETE", &d.segments()).unwrap().status, 204);
    }
}
