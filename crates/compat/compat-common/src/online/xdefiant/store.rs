use super::super::util::{iso_now, now_millis, random_uuid};
use super::super::Request;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const PRIVILEGES: &[&str] = &["multiplayer", "crossPlay", "communications", "userGeneratedContent", "purchase", "voiceChat", "textChat"];

const LIST_CAP: usize = 20000;

#[derive(Default)]
pub struct State {
    pub profile_id: String,
    pub session: Option<Value>,
    pub profiles: HashMap<String, Value>,
    pub files: HashMap<String, Vec<u8>>,
    lists: HashMap<String, Vec<Value>>,
    docs: HashMap<String, HashMap<String, Value>>,
    entities: Vec<Value>,
    stats: HashMap<String, Map<String, Value>>,
    parties: HashMap<String, Value>,
    games: HashMap<String, Vec<Value>>,
    requests: HashMap<String, u64>,
}

fn now_secs() -> u64 {
    now_millis() / 1000
}

fn parse_iso_secs(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |a: usize, l: usize| s.get(a..a + l)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let t = days * 86400 + h * 3600 + mi * 60 + se;
    (t >= 0).then_some(t as u64)
}

impl State {
    pub fn new(profile_id: &str) -> State {
        let mut s = State { profile_id: profile_id.to_string(), ..Default::default() };
        let default = crate::gdk::data_root().join("xdefiant").join("catalog.json");
        match crate::cfg("xdefiant_catalog") {
            Some(path) => s.load_catalog(&path),
            None if default.is_file() => s.load_catalog(&default.to_string_lossy()),
            None => crate::record_raw("online", "xdefiant", &format!("no catalog: set xdefiant_catalog or place {}", default.display())),
        }
        s
    }

    pub fn load_catalog(&mut self, path: &str) {
        let Ok(text) = std::fs::read_to_string(path) else {
            crate::record_raw("online", "xdefiant", &format!("catalog '{path}' could not be read"));
            return;
        };
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        for k in ["seasons", "news", "policies", "populations", "primarystoreMappings", "inventory"] {
            for item in v[k].as_array().cloned().unwrap_or_default() {
                self.push(k, item);
            }
        }
    }

    pub fn push(&mut self, col: &str, v: Value) {
        let l = self.lists.entry(col.to_string()).or_default();
        l.push(v);
        if l.len() > LIST_CAP {
            l.remove(0);
        }
    }

    pub fn list(&self, col: &str) -> Vec<Value> {
        self.lists.get(col).cloned().unwrap_or_default()
    }

    pub fn upsert(&mut self, col: &str, key: &str, v: Value) {
        let l = self.lists.entry(col.to_string()).or_default();
        let id = v[key].clone();
        match l.iter_mut().find(|x| x[key] == id) {
            Some(x) => *x = v,
            None => l.push(v),
        }
    }

    pub fn remove(&mut self, col: &str, key: &str, id: &str) -> bool {
        let Some(l) = self.lists.get_mut(col) else { return false };
        let n = l.len();
        l.retain(|x| x[key].as_str().map_or(true, |s| !s.eq_ignore_ascii_case(id)));
        l.len() != n
    }

    pub fn set_doc(&mut self, col: &str, key: &str, v: Value) {
        self.docs.entry(col.to_string()).or_default().insert(key.to_ascii_lowercase(), v);
    }

    pub fn doc(&self, col: &str, key: &str) -> Option<Value> {
        self.docs.get(col)?.get(&key.to_ascii_lowercase()).cloned()
    }

    pub fn count_request(&mut self, k: &str) {
        *self.requests.entry(k.to_string()).or_default() += 1;
    }

    pub fn entities(&self, owner: Option<&str>, space: Option<&str>, req: &Request) -> Vec<Value> {
        let want_space = req.query("spaceId").map(|s| s.to_ascii_lowercase());
        let want_type = req.query("type");
        let want_name = req.query("name");
        let ids: Vec<String> = req.query("entityIds").map(|s| s.split(',').map(|x| x.trim().to_ascii_lowercase()).collect()).unwrap_or_default();
        self.entities.iter().filter(|e| {
            owner.map_or(true, |o| e["profileId"].as_str().map_or(false, |p| p.eq_ignore_ascii_case(o)))
                && space.map_or(true, |s| e["spaceId"].as_str().map_or(false, |p| p.eq_ignore_ascii_case(s)) && e["profileId"].is_null())
                && want_space.as_deref().map_or(true, |s| e["spaceId"].as_str().map_or(false, |p| p.eq_ignore_ascii_case(s)))
                && want_type.map_or(true, |t| e["type"] == json!(t))
                && want_name.map_or(true, |n| e["name"] == json!(n))
                && (ids.is_empty() || e["entityId"].as_str().map_or(false, |i| ids.contains(&i.to_ascii_lowercase())))
        }).cloned().collect()
    }

    pub fn create_entity(&mut self, owner: Option<&str>, space: &str, body: Value) -> Value {
        let e = json!({
            "entityId": random_uuid(),
            "type": body["type"].clone(),
            "spaceId": body["spaceId"].as_str().unwrap_or(space),
            "profileId": owner,
            "name": body["name"].clone(),
            "tags": if body["tags"].is_array() { body["tags"].clone() } else { json!([]) },
            "obj": if body["obj"].is_null() { json!({}) } else { body["obj"].clone() },
            "revision": 1,
            "lastModified": iso_now(),
        });
        self.entities.push(e.clone());
        e
    }

    pub fn entity(&self, id: &str) -> Option<Value> {
        self.entities.iter().find(|e| e["entityId"].as_str().map_or(false, |i| i.eq_ignore_ascii_case(id))).cloned()
    }

    pub fn update_entity(&mut self, id: &str, body: Value) -> Option<Value> {
        let e = self.entities.iter_mut().find(|e| e["entityId"].as_str().map_or(false, |i| i.eq_ignore_ascii_case(id)))?;
        for k in ["type", "name", "tags", "obj", "spaceId"] {
            if !body[k].is_null() {
                e[k] = body[k].clone();
            }
        }
        let rev = e["revision"].as_u64().unwrap_or(0) + 1;
        e["revision"] = json!(rev);
        e["lastModified"] = json!(iso_now());
        Some(e.clone())
    }

    pub fn delete_entity(&mut self, id: &str) -> bool {
        let n = self.entities.len();
        self.entities.retain(|e| e["entityId"].as_str().map_or(true, |i| !i.eq_ignore_ascii_case(id)));
        n != self.entities.len()
    }

    pub fn stats(&self, owner: &str, names: &[String]) -> Value {
        let m = self.stats.get(&owner.to_ascii_lowercase()).cloned().unwrap_or_default();
        if names.is_empty() {
            return Value::Object(m);
        }
        Value::Object(m.into_iter().filter(|(k, _)| names.iter().any(|n| n == k)).collect())
    }

    pub fn put_stats(&mut self, owner: &str, stats: &Value) {
        let m = self.stats.entry(owner.to_ascii_lowercase()).or_default();
        if let Some(o) = stats.as_object() {
            for (k, v) in o {
                let value = if v.is_object() && !v["value"].is_null() { v["value"].clone() } else { v.clone() };
                m.insert(k.clone(), json!({ "value": value, "lastModified": iso_now() }));
            }
        }
    }

    pub fn apply_transaction(&mut self, body: Value) -> Value {
        let tid = random_uuid();
        let mut changes = Vec::new();
        let add = body["addItems"].as_array().or_else(|| body["items"].as_array()).cloned().unwrap_or_default();
        let consume = body["consumeItems"].as_array().cloned().unwrap_or_default();
        for (list, sign) in [(add, 1i64), (consume, -1i64)] {
            for it in list {
                let id = it["itemId"].as_str().unwrap_or("").to_string();
                if id.is_empty() {
                    continue;
                }
                let q = it["quantity"].as_i64().unwrap_or(1) * sign;
                let inv = self.lists.entry("inventory".into()).or_default();
                match inv.iter_mut().find(|x| x["itemId"] == json!(id)) {
                    Some(x) => {
                        let nq = (x["quantity"].as_i64().unwrap_or(0) + q).max(0);
                        x["quantity"] = json!(nq);
                        x["lastModified"] = json!(iso_now());
                    }
                    None if q > 0 => inv.push(json!({
                        "itemId": id, "quantity": q, "type": it["type"].as_str().unwrap_or("Consumable"), "state": "ReadyToUse",
                        "name": it["name"].clone(), "obtainedDate": iso_now(), "lastModified": iso_now(), "expirationDate": it["expirationDate"].clone(),
                    })),
                    None => {}
                }
                changes.push(json!({ "itemId": id, "quantity": q }));
            }
        }
        if let Some(inv) = self.lists.get_mut("inventory") {
            inv.retain(|x| x["quantity"].as_i64().unwrap_or(1) > 0);
        }
        let t = json!({ "transactionId": tid, "date": iso_now(), "changes": changes, "request": body });
        self.push("transactions", t.clone());
        t
    }

    pub fn expired_items(&self) -> Vec<Value> {
        let now = now_secs();
        self.list("inventory").into_iter().filter(|x| x["expirationDate"].as_str().and_then(parse_iso_secs).map_or(false, |t| t < now)).collect()
    }

    pub fn seasons(&self, space: &str) -> Vec<Value> {
        self.list("seasons").into_iter().filter(|s| s["spaceId"].as_str().map_or(true, |x| x.eq_ignore_ascii_case(space))).collect()
    }

    pub fn season_progress(&self, space: &str) -> Vec<Value> {
        self.seasons(space).into_iter().map(|s| {
            let id = s["seasonId"].as_str().unwrap_or("").to_string();
            self.doc("seasonProgress", &id).unwrap_or_else(|| json!({ "seasonId": id, "spaceId": space, "profileId": self.profile_id, "xp": 0, "tier": 0, "premium": false, "tiersBanked": [] }))
        }).collect()
    }

    pub fn preferences(&self, owner: &str, space: &str) -> Value {
        let key = format!("{owner}:{space}");
        let mut base = json!({ "profileId": owner, "spaceId": space, "crossPlayEnabled": true, "lastModified": Value::Null });
        if let Some(Value::Object(o)) = self.doc("preferences", &key) {
            for (k, v) in o {
                base[k] = v;
            }
        }
        base
    }

    pub fn put_preferences(&mut self, owner: &str, space: &str, body: Value) -> Value {
        let mut cur = self.preferences(owner, space);
        if let Some(o) = body.as_object() {
            for (k, v) in o {
                if k != "profileId" {
                    cur[k] = v.clone();
                }
            }
        }
        cur["lastModified"] = json!(iso_now());
        self.set_doc("preferences", &format!("{owner}:{space}"), cur.clone());
        cur
    }

    pub fn create_party(&mut self, space: &str, leader: &str, body: Value) -> Value {
        let id = random_uuid();
        let p = json!({
            "partyId": id, "spaceId": space, "leaderProfileId": leader,
            "members": [{ "profileId": leader, "joinedAt": iso_now() }],
            "maxMembers": body["maxMembers"].as_u64().unwrap_or(4),
            "properties": if body["properties"].is_null() { body.clone() } else { body["properties"].clone() },
            "createdAt": iso_now(), "xboxSync": Value::Null,
        });
        self.parties.insert(id, p.clone());
        p
    }

    pub fn parties(&self, space: &str) -> Vec<Value> {
        self.parties.values().filter(|p| p["spaceId"].as_str().map_or(false, |s| s.eq_ignore_ascii_case(space))).cloned().collect()
    }

    pub fn parties_of(&self, member: &str) -> Vec<Value> {
        self.parties.values().filter(|p| p["members"].as_array().map_or(false, |m| m.iter().any(|x| x["profileId"].as_str().map_or(false, |i| i.eq_ignore_ascii_case(member))))).cloned().collect()
    }

    pub fn party(&self, id: &str) -> Option<Value> {
        self.parties.get(&id.to_ascii_lowercase()).or_else(|| self.parties.get(id)).cloned()
    }

    fn party_mut(&mut self, id: &str) -> Option<&mut Value> {
        let key = self.parties.keys().find(|k| k.eq_ignore_ascii_case(id))?.clone();
        self.parties.get_mut(&key)
    }

    pub fn join_party(&mut self, id: &str, member: &str) -> Option<Value> {
        let p = self.party_mut(id)?;
        let max = p["maxMembers"].as_u64().unwrap_or(4) as usize;
        let members = p["members"].as_array_mut()?;
        if !members.iter().any(|x| x["profileId"] == json!(member)) && members.len() < max {
            members.push(json!({ "profileId": member, "joinedAt": iso_now() }));
        }
        Some(p.clone())
    }

    pub fn leave_party(&mut self, id: &str, member: &str) -> Option<Value> {
        let p = self.party_mut(id)?;
        if let Some(m) = p["members"].as_array_mut() {
            m.retain(|x| x["profileId"] != json!(member));
        }
        let first = p["members"].as_array().and_then(|m| m.first()).map(|x| x["profileId"].clone());
        if p["leaderProfileId"] == json!(member) {
            p["leaderProfileId"] = first.unwrap_or(Value::Null);
        }
        Some(p.clone())
    }

    pub fn sync_party(&mut self, id: &str, body: Value) -> Option<Value> {
        let p = self.party_mut(id)?;
        p["xboxSync"] = json!({ "data": body, "syncedAt": iso_now() });
        Some(p.clone())
    }

    pub fn games_played(&self, owner: &str) -> Vec<Value> {
        self.games.get(&owner.to_ascii_lowercase()).cloned().unwrap_or_default()
    }

    pub fn record_game_played(&mut self, owner: &str, space: &str) -> Value {
        let l = self.games.entry(owner.to_ascii_lowercase()).or_default();
        match l.iter_mut().find(|g| g["spaceId"] == json!(space)) {
            Some(g) => {
                g["lastPlayedDate"] = json!(iso_now());
                let n = g["sessionCount"].as_u64().unwrap_or(0) + 1;
                g["sessionCount"] = json!(n);
                g.clone()
            }
            None => {
                let g = json!({ "spaceId": space, "firstPlayedDate": iso_now(), "lastPlayedDate": iso_now(), "sessionCount": 1 });
                l.push(g.clone());
                g
            }
        }
    }

    pub fn dump(&self) -> Value {
        json!({
            "profileId": self.profile_id,
            "session": self.session,
            "lists": self.lists,
            "docs": self.docs,
            "entities": self.entities,
            "stats": self.stats,
            "parties": self.parties,
            "gamesPlayed": self.games,
            "files": self.files.iter().map(|(k, v)| (k.clone(), json!(v.len()))).collect::<Map<String, Value>>(),
            "requests": self.requests,
            "connections": all_connections(),
        })
    }
}

fn conns() -> &'static Mutex<HashMap<String, Value>> {
    static C: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn open_connection(profile: &str, cid: &str) {
    conns().lock().unwrap_or_else(|p| p.into_inner()).insert(cid.to_string(), json!({
        "connectionId": cid, "profileId": profile, "notificationTypes": [], "spaceIds": [], "objectMetadata": Value::Null, "openedAt": iso_now(),
    }));
}

pub fn close_connection(cid: &str) {
    conns().lock().unwrap_or_else(|p| p.into_inner()).remove(cid);
}

pub fn all_connections() -> Vec<Value> {
    conns().lock().unwrap_or_else(|p| p.into_inner()).values().cloned().collect()
}

pub fn connections_for(ids: &[String]) -> Vec<Value> {
    all_connections().into_iter().filter(|c| c["profileId"].as_str().map_or(false, |p| ids.iter().any(|i| i.eq_ignore_ascii_case(p)))).collect()
}

pub fn update_connection(profile: &str, cid: Option<&str>, body: Value) -> Option<Value> {
    let mut g = conns().lock().unwrap_or_else(|p| p.into_inner());
    let key = match cid.or_else(|| body["connectionId"].as_str()) {
        Some(c) => g.keys().find(|k| k.eq_ignore_ascii_case(c))?.clone(),
        None => g.iter().find(|(_, v)| v["profileId"].as_str().map_or(false, |p| p.eq_ignore_ascii_case(profile)))?.0.clone(),
    };
    let c = g.get_mut(&key)?;
    for k in ["notificationTypes", "spaceIds", "objectMetadata"] {
        if !body[k].is_null() {
            c[k] = body[k].clone();
        }
    }
    c["updatedAt"] = json!(iso_now());
    Some(c.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse_roundtrip() {
        let t = parse_iso_secs("2026-11-21T00:00:00.000Z").unwrap();
        assert_eq!(super::super::super::util::iso_millis(t * 1000).get(..19).unwrap(), "2026-11-21T00:00:00");
    }

    #[test]
    fn inventory_transactions_add_consume() {
        let mut s = State::new("p");
        s.apply_transaction(json!({ "addItems": [{ "itemId": "a", "quantity": 3 }] }));
        s.apply_transaction(json!({ "consumeItems": [{ "itemId": "a", "quantity": 1 }] }));
        assert_eq!(s.list("inventory")[0]["quantity"], 2);
        s.apply_transaction(json!({ "consumeItems": [{ "itemId": "a", "quantity": 5 }] }));
        assert!(s.list("inventory").is_empty());
        assert_eq!(s.list("transactions").len(), 3);
    }
}
