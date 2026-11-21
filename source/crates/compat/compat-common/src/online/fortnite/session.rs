use super::super::{util, Request, Response};
use super::matchmaking::Ticket;
use super::party::{capture, note};
use super::{epic_error, Fortnite};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const MAX_PUBLIC_PLAYERS: usize = 100;
pub const DEFAULT_GAMESERVER: (&str, u16) = ("127.0.0.1", 7777);

#[derive(Clone, Debug)]
pub struct Sess {
    pub id: String,
    pub match_id: String,
    pub owner_id: String,
    pub owner_name: String,
    pub playlist: String,
    pub region: String,
    pub subregion: String,
    pub net_cl: String,
    pub server_address: String,
    pub server_port: u16,
    pub key: String,
    pub session_key: String,
    pub players: Vec<String>,
    pub started: bool,
    pub last_updated: String,
}

fn sessions() -> &'static Mutex<HashMap<String, Sess>> {
    static S: OnceLock<Mutex<HashMap<String, Sess>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn gameserver() -> (String, u16) {
    if let Some(v) = std::env::var("TOMBOX_FN_GAMESERVER").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        if let Some((ip, port)) = v.rsplit_once(':') {
            if let Ok(p) = port.trim().parse::<u16>() {
                if !ip.trim().is_empty() {
                    return (ip.trim().to_string(), p);
                }
            }
        }
        note("session", &format!("TOMBOX_FN_GAMESERVER '{v}' is not ip:port; using cfg/default"));
    }
    let ip = crate::cfg("online_gameserver_ip").filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_GAMESERVER.0.into());
    let port = crate::cfg("online_gameserver_port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(DEFAULT_GAMESERVER.1);
    (ip, port)
}

fn random_key() -> String {
    let h = format!("{}{}", util::random_hex32(), util::random_hex32());
    let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
    util::b64(&bytes)
}

pub fn create(sid: &str, match_id: &str, t: &Ticket) -> Sess {
    let (ip, port) = gameserver();
    let owner = util::random_hex32().to_uppercase();
    let region = t.region.clone();
    let s = Sess {
        id: sid.to_string(),
        match_id: match_id.to_string(),
        owner_name: format!("[DS]fortnite-live{}-{}", region.to_ascii_lowercase(), &owner[..12].to_ascii_lowercase()),
        owner_id: owner,
        playlist: t.playlist.clone(),
        subregion: t.subregions.first().cloned().unwrap_or_else(|| region.clone()),
        region,
        net_cl: t.net_cl.clone(),
        server_address: ip,
        server_port: port,
        key: random_key(),
        session_key: util::random_hex32().to_uppercase(),
        players: t.party_player_ids.clone(),
        started: false,
        last_updated: util::iso_now(),
    };
    note("session", &format!("create {sid} match {match_id} playlist {} build {} server {}:{} players {:?}", s.playlist, s.net_cl, s.server_address, s.server_port, s.players));
    sessions().lock().unwrap_or_else(|p| p.into_inner()).insert(sid.to_string(), s.clone());
    if !s.playlist.is_empty() {
        std::env::set_var("TOMBOX_FN_PLAYLIST", &s.playlist);
    }
    s
}

pub fn get(sid: &str) -> Option<Sess> {
    sessions().lock().unwrap_or_else(|p| p.into_inner()).get(sid).cloned()
}

pub fn session_json(s: &Sess) -> Value {
    let open = MAX_PUBLIC_PLAYERS.saturating_sub(s.players.len());
    json!({
        "id": s.id,
        "ownerId": s.owner_id,
        "ownerName": s.owner_name,
        "serverName": s.owner_name,
        "serverAddress": s.server_address,
        "serverPort": s.server_port,
        "maxPublicPlayers": MAX_PUBLIC_PLAYERS,
        "openPublicPlayers": open,
        "maxPrivatePlayers": 0,
        "openPrivatePlayers": 0,
        "attributes": {
            "REGION_s": s.region,
            "GAMEMODE_s": "FORTATHENA",
            "ALLOWBROADCASTING_b": true,
            "SUBREGION_s": s.subregion,
            "DCID_s": format!("FORTNITE-LIVE{}-{}", s.region, s.subregion).to_ascii_uppercase(),
            "tenant_s": "Fortnite",
            "MATCHMAKINGPOOL_s": "Any",
            "STORMSHIELDDEFENSETYPE_i": 0,
            "HOTFIXVERSION_i": 0,
            "PLAYLISTNAME_s": s.playlist,
            "SESSIONKEY_s": s.session_key,
            "TENANT_s": "Fortnite",
            "BEACONPORT_i": 15009,
        },
        "publicPlayers": [],
        "privatePlayers": [],
        "totalPlayers": s.players.len(),
        "allowJoinInProgress": false,
        "shouldAdvertise": false,
        "isDedicated": false,
        "usesStats": false,
        "allowInvites": false,
        "usesPresence": false,
        "allowJoinViaPresence": true,
        "allowJoinViaPresenceFriendsOnly": false,
        "buildUniqueId": s.net_cl,
        "lastUpdated": s.last_updated,
        "started": s.started,
    })
}

fn not_found(id: &str) -> Response {
    epic_error(404, "errors.com.epicgames.fortnite.session_not_found", &format!("Sorry, we couldn't find a session for id {id}"), 16101, &[id])
}

fn matches(s: &Sess, criteria: &Value) -> bool {
    let attrs = session_json(s)["attributes"].clone();
    criteria.as_array().map_or(true, |list| list.iter().all(|c| {
        let key = c.get("key").and_then(Value::as_str).unwrap_or("");
        let ty = c.get("type").and_then(Value::as_str).unwrap_or("EQUAL");
        let want = c.get("value").cloned().unwrap_or(Value::Null);
        let have = attrs.get(key).cloned().unwrap_or(Value::Null);
        let eq = match (&have, &want) {
            (Value::String(a), Value::String(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a == b,
        };
        match ty {
            "NOT_EQUAL" => !eq,
            _ => eq,
        }
    }))
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let _ = fnt;
    Some(match (m, seg) {
        ("GET", ["fortnite", "api", "matchmaking", "session", "findPlayer", acct]) => {
            let g = sessions().lock().unwrap_or_else(|p| p.into_inner());
            Response::json(&Value::Array(g.values().filter(|s| s.players.iter().any(|p| p.eq_ignore_ascii_case(acct))).map(session_json).collect()))
        }
        ("POST", ["fortnite", "api", "matchmaking", "session", "matchMakingRequest"]) => {
            capture("session", req);
            let body = req.json();
            let criteria = body.get("criteria").cloned().unwrap_or(Value::Null);
            let max = body.get("maxResults").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
            let g = sessions().lock().unwrap_or_else(|p| p.into_inner());
            let mut v: Vec<&Sess> = g.values().filter(|s| !s.started && matches(s, &criteria)).collect();
            v.sort_by(|a, b| a.last_updated.cmp(&b.last_updated));
            Response::json(&Value::Array(v.into_iter().take(max).map(session_json).collect()))
        }
        ("POST", ["fortnite", "api", "matchmaking", "session", id, "join"]) => {
            capture("session", req);
            let mut g = sessions().lock().unwrap_or_else(|p| p.into_inner());
            let Some(s) = g.get_mut(*id) else { return Some(not_found(id)) };
            let Some(acct) = req.query("accountId").map(str::to_string).filter(|a| !a.is_empty()) else {
                return Some(epic_error(400, "errors.com.epicgames.common.missing_parameter", "accountId is required", 1040, &["accountId"]));
            };
            if !s.players.iter().any(|p| p.eq_ignore_ascii_case(&acct)) {
                if s.players.len() >= MAX_PUBLIC_PLAYERS {
                    return Some(epic_error(409, "errors.com.epicgames.fortnite.session_full", &format!("session {id} is full"), 16104, &[id]));
                }
                s.players.push(acct.clone());
            }
            s.last_updated = util::iso_now();
            note("session", &format!("join {id} {acct} -> {} players", s.players.len()));
            Response::empty(204)
        }
        ("GET", ["fortnite", "api", "matchmaking", "session", id]) => match get(id) {
            Some(s) => Response::json(&session_json(&s)),
            None => not_found(id),
        },
        ("GET", ["fortnite", "api", "game", "v2", "matchmaking", "account", acct, "session", sid]) => match get(sid) {
            Some(s) if s.players.iter().any(|p| p.eq_ignore_ascii_case(acct)) => Response::json(&json!({"accountId": acct, "sessionId": sid, "key": s.key})),
            Some(_) => epic_error(404, "errors.com.epicgames.fortnite.player_not_in_session", &format!("{acct} is not a member of session {sid}"), 16105, &[acct, sid]),
            None => not_found(sid),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn ticket(acct: &str, playlist: &str) -> Ticket {
        Ticket {
            ticket_id: util::random_hex32(),
            account_id: acct.into(),
            party_player_ids: vec![acct.into()],
            bucket_id: format!("14457890:1:NAE:{playlist}"),
            net_cl: "14457890".into(),
            playlist_revision: 1,
            region: "NAE".into(),
            playlist: playlist.into(),
            platform: "XSX".into(),
            subregions: vec!["VA".into()],
            options: Map::new(),
            issued_at: util::now_secs(),
        }
    }

    fn call(method: &str, path: &str) -> Response {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        let r = Request { method: method.into(), path: p.into(), query: util::parse_pairs(q), headers: Vec::new(), body: Vec::new() };
        let f = Fortnite::with_account("x", "x");
        route(&f, &r, method, &r.segments()).unwrap()
    }

    #[test]
    fn session_from_ticket_carries_playlist_and_build() {
        let sid = util::random_hex32();
        create(&sid, "m1", &ticket("SeA", "playlist_defaultsquad"));
        let r = call("GET", &format!("/fortnite/api/matchmaking/session/{sid}"));
        assert_eq!(r.status, 200);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["attributes"]["PLAYLISTNAME_s"], "playlist_defaultsquad");
        assert_eq!(b["buildUniqueId"], "14457890");
        assert_eq!(b["attributes"]["REGION_s"], "NAE");
        assert_eq!(b["attributes"]["SUBREGION_s"], "VA");
        assert_eq!(b["maxPublicPlayers"], 100);
        assert_eq!(b["openPublicPlayers"], 99);
        assert!(b["serverPort"].as_u64().unwrap() > 0);
        assert_eq!(call("GET", "/fortnite/api/matchmaking/session/doesnotexist").status, 404);
    }

    #[test]
    fn key_route_requires_member() {
        let sid = util::random_hex32();
        let s = create(&sid, "m2", &ticket("SeB", "playlist_defaultsolo"));
        let r = call("GET", &format!("/fortnite/api/game/v2/matchmaking/account/SeB/session/{sid}"));
        assert_eq!(r.status, 200);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["key"], json!(s.key));
        assert_eq!(util::b64_decode(&s.key).len(), 32);
        assert_eq!(call("GET", &format!("/fortnite/api/game/v2/matchmaking/account/Stranger/session/{sid}")).status, 404);
        assert_eq!(call("GET", "/fortnite/api/game/v2/matchmaking/account/SeB/session/none").status, 404);
    }

    #[test]
    fn join_adds_player() {
        let sid = util::random_hex32();
        create(&sid, "m3", &ticket("SeC", "playlist_defaultduo"));
        assert_eq!(call("POST", &format!("/fortnite/api/matchmaking/session/{sid}/join?accountId=SeD")).status, 204);
        assert!(get(&sid).unwrap().players.contains(&"SeD".to_string()));
        assert_eq!(call("POST", "/fortnite/api/matchmaking/session/missing/join?accountId=SeD").status, 404);
        let r = call("GET", "/fortnite/api/matchmaking/session/findPlayer/SeD");
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert!(b.as_array().unwrap().iter().any(|s| s["id"] == json!(sid)));
    }
}
