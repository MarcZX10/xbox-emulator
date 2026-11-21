use super::super::{util, Request, Response};
use super::{data, Fortnite};
use serde_json::{json, Value};

fn ingest_telemetry(req: &Request) -> Response {
    use std::io::Write;
    let body = req.json();
    if !body.is_object() || body.get("Events").map_or(false, |e| !e.is_array()) {
        return Response::json_status(400, &json!({"errorCode": "errors.com.epicgames.datarouter.invalid_payload", "errorMessage": "payload must be a JSON object with an Events array"}));
    }
    let events = body.get("Events").and_then(Value::as_array).cloned().unwrap_or_default();
    if events.is_empty() {
        return Response::empty(204);
    }
    let dir = crate::gdk::data_root().join("telemetry");
    let path = dir.join("datarouter.jsonl");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::metadata(&path).map_or(false, |m| m.len() > 64 * 1024 * 1024) {
        let _ = std::fs::rename(&path, dir.join("datarouter.1.jsonl"));
    }
    let meta = json!({"SessionID": req.query("SessionID"), "AppID": req.query("AppID"), "AppVersion": req.query("AppVersion"), "UserID": req.query("UserID"), "AppEnvironment": req.query("AppEnvironment"), "UploadType": req.query("UploadType")});
    let mut out = String::new();
    for e in &events {
        out.push_str(&json!({"received": util::iso_now(), "meta": meta, "event": e}).to_string());
        out.push('\n');
    }
    match std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| f.write_all(out.as_bytes())) {
        Ok(()) => Response::empty(204),
        Err(e) => Response::json_status(500, &json!({"errorCode": "errors.com.epicgames.datarouter.storage", "errorMessage": e.to_string()})),
    }
}

fn lightswitch_entry(message: &str, actions: Value) -> Value {
    json!({
        "serviceInstanceId": "fortnite",
        "status": "UP",
        "message": message,
        "maintenanceUri": null,
        "overrideCatalogIds": ["a7f138b2e51945ffbfdacc1af0541053"],
        "allowedActions": actions,
        "banned": false,
        "launcherInfoDTO": {"appName": "Fortnite", "catalogItemId": "4fe75bbc5a674f4f9b356b5c90567da5", "namespace": "fn"},
    })
}

fn timeline() -> Response {
    let mut t: Value = serde_json::from_str(data::TIMELINE).unwrap_or(json!({}));
    t["currentTime"] = json!(util::iso_now());
    Response::json(&t)
}

fn stats(account: &str) -> Response {
    Response::json(&json!({"startTime": 0, "endTime": 0, "stats": {}, "accountId": account}))
}

fn tournament(account: &str) -> Response {
    let mut t: Value = serde_json::from_str(data::TOURNAMENT).unwrap_or(json!({}));
    if t.get("player").map_or(false, Value::is_object) {
        t["player"]["accountId"] = json!(account);
    }
    Response::json(&t)
}

fn history(event: &str, account: &str) -> Response {
    let mut h: Value = serde_json::from_str(data::HISTORY).unwrap_or(json!([]));
    if let Some(first) = h.get_mut(0) {
        first["scoreKey"]["eventId"] = json!(event);
        first["teamId"] = json!(account);
        first["teamAccountIds"] = json!([account]);
    }
    Response::json(&h)
}

fn version() -> Response {
    Response::json(&json!({
        "app": "fortnite",
        "serverDate": util::iso_now(),
        "overridePropertiesVersion": "unknown",
        "cln": "14617919",
        "build": "1",
        "moduleName": "Fortnite-Core",
        "buildDate": "2020-10-27T00:00:00.000Z",
        "version": "14.50",
        "branch": "Release-14.50",
        "modules": {},
    }))
}

fn matchmaking_session(fnt: &Fortnite, req: &Request, id: &str) -> Response {
    let ip = crate::cfg("online_gameserver_ip").unwrap_or_else(|| "127.0.0.1".into());
    let port = crate::cfg("online_gameserver_port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(7777);
    let _ = fnt;
    Response::json(&json!({
        "id": id,
        "ownerId": util::random_hex32().to_uppercase(),
        "ownerName": "[DS]xboxemulator",
        "serverName": "[DS]xboxemulator",
        "serverAddress": ip,
        "serverPort": port,
        "maxPublicPlayers": 220,
        "openPublicPlayers": 175,
        "maxPrivatePlayers": 0,
        "openPrivatePlayers": 0,
        "attributes": {
            "REGION_s": "NA",
            "GAMEMODE_s": "FORTATHENA",
            "ALLOWBROADCASTING_b": true,
            "SUBREGION_s": "NA",
            "DCID_s": "XBOXEMULATOR",
            "tenant_s": "Fortnite",
            "MATCHMAKINGPOOL_s": "Any",
            "STORMSHIELDDEFENSETYPE_i": 0,
            "HOTFIXVERSION_i": 0,
            "PLAYLISTNAME_s": "Playlist_DefaultSolo",
            "SESSIONKEY_s": util::random_hex32().to_uppercase(),
            "TENANT_s": "Fortnite",
            "BEACONPORT_i": 15009,
        },
        "publicPlayers": [],
        "privatePlayers": [],
        "totalPlayers": 45,
        "allowJoinInProgress": false,
        "shouldAdvertise": false,
        "isDedicated": false,
        "usesStats": false,
        "allowInvites": false,
        "usesPresence": false,
        "allowJoinViaPresence": true,
        "allowJoinViaPresenceFriendsOnly": false,
        "buildUniqueId": req.query("bucketId").and_then(|b| b.split(':').next()).unwrap_or("0"),
        "lastUpdated": util::iso_now(),
        "started": false,
    }))
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    Some(match (m, seg) {
        ("GET", ["lightswitch", "api", "service", "bulk", "status"]) => Response::json(&json!([lightswitch_entry("fortnite is up.", json!(["PLAY", "DOWNLOAD"]))])),
        ("GET", ["lightswitch", "api", "service", _, "status"]) => Response::json(&lightswitch_entry("Fortnite is online", json!([]))),
        ("GET", ["fortnite", "api", "version"]) => version(),
        ("GET", ["fortnite", "api", "v2", "versioncheck", ..]) | ("GET", ["fortnite", "api", "versioncheck", ..]) => Response::json(&json!({"type": "NO_UPDATE"})),
        ("GET", ["waitingroom", "api", "waitingroom"]) => Response::empty(204),
        ("GET", ["fortnite", "api", "calendar", "v1", "timeline"]) => timeline(),
        ("GET", ["fortnite", "api", "storefront", "v2", "catalog"]) => Response::json_str(data::CATALOG),
        ("GET", ["fortnite", "api", "storefront", "v2", "keychain"]) => Response::json_str(data::KEYCHAIN),
        ("GET", ["catalog", "api", "shared", "bulk", "offers"]) => Response::json(&json!({})),
        ("GET", ["content", "api", "pages", ..]) => Response::json_str(data::CONTENT_PAGES),
        ("GET", ["fortnite", "api", "game", "v2", "enabled_features"]) => Response::json(&json!([])),
        ("GET", ["fortnite", "api", "receipts", "v1", "account", _, "receipts"]) => Response::json(&json!([])),
        ("GET", ["fortnite", "api", "game", "v2", "events", "tournamentandhistory", ..]) => Response::json_str(data::TOURNAMENT_AND_HISTORY),
        ("GET", ["api", "v1", "events", "Fortnite", "download", account]) => tournament(account),
        ("GET", ["api", "v1", "events", "Fortnite", event, "history", account]) => history(event, account),
        ("GET", ["api", "v1", "players", "Fortnite", "tokens"]) => {
            let ids = req.query("teamAccountIds").unwrap_or("").to_string();
            let t: Value = serde_json::from_str(data::TOURNAMENT).unwrap_or(json!({}));
            let tokens = t.get("player").and_then(|p| p.get("tokens")).cloned().unwrap_or(json!([]));
            Response::json(&json!({"accounts": ids.split(',').filter(|s| !s.is_empty()).map(|a| json!({"accountId": a, "tokens": tokens})).collect::<Vec<_>>()}))
        }
        ("GET", ["api", "v1", "Fortnite", "get"]) => Response::json(&json!({})),
        ("GET", ["fortnite", "api", "statsv2", "account", account]) => stats(account),
        ("GET", ["statsproxy", "api", "statsv2", "account", account]) => stats(account),
        ("GET", ["fortnite", "api", "stats", "accountId", account, ..]) => stats(account),
        ("POST", ["fortnite", "api", "statsv2", "query"]) | ("POST", ["statsproxy", "api", "statsv2", "query"]) => Response::json(&json!([])),
        ("GET", ["fortnite", "api", "game", "v2", "leaderboards", "cohort", account]) => Response::json(&json!({
            "accountId": account, "cohortAccounts": [account], "expiresAt": "9999-12-31T00:00:00.000Z", "playlist": req.query("playlist")
        })),
        ("POST", ["datarouter", "api", "v1", "public", "data", ..]) | ("POST", ["telemetry", "data", "datarouter", "api", "v1", "public", "data", ..]) => ingest_telemetry(req),
        ("POST", ["api", "v1", "user", "setting"]) => Response::json(&json!([])),
        ("GET", ["eulatracking", "api", "shared", "agreements", ..]) => Response::json(&json!({})),
        ("GET", ["fortnite", "api", "game", "v2", "friendcodes", ..]) => Response::json(&json!([])),
        ("POST", ["fortnite", "api", "game", "v2", "grant_access", ..]) => Response::json(&json!({})),
        ("POST", ["fortnite", "api", "game", "v2", "events", "v2", "setSubgroup", ..]) => Response::empty(204),
        ("GET", ["fortnite", "api", "game", "v2", "twitch", ..]) => Response::bytes(200, "", Vec::new()),
        ("POST", ["fortnite", "api", "feedback", ..]) => Response::bytes(200, "", Vec::new()),
        ("GET", ["fortnite", "api", "game", "v2", "br-inventory", "account", _]) => Response::json(&json!({"stash": {"globalcash": 0}})),
        ("GET" | "POST", ["v1", "epic-settings", "public", "users", _, "values"]) => Response::json_str(data::EPIC_SETTINGS),
        ("GET", ["region"]) => Response::json(&json!({
            "continent": {"code": "NA", "names": {"en": "North America"}},
            "country": {"iso_code": "US", "is_in_european_union": false, "names": {"en": "United States"}},
            "subdivisions": [],
        })),
        ("GET", ["fortnite", "api", "matchmaking", "session", "findPlayer", ..]) => Response::bytes(200, "", Vec::new()),
        ("GET", ["fortnite", "api", "game", "v2", "matchmakingservice", "ticket", "player", _]) => Response::json(&json!({
            "serviceUrl": "ws://127.0.0.1", "ticketType": "mms-player", "payload": "69=", "signature": "420="
        })),
        ("GET", ["fortnite", "api", "game", "v2", "matchmaking", "account", account, "session", session]) => Response::json(&json!({
            "accountId": account, "sessionId": session, "key": "AOJEv8uTFmUh7XM2328kq9rlAzeQ5xzWzPIiyKn2s7s="
        })),
        ("POST", ["fortnite", "api", "matchmaking", "session", "matchMakingRequest"]) => Response::json(&json!([])),
        ("POST", ["fortnite", "api", "matchmaking", "session", _, "join"]) => Response::empty(204),
        ("GET", ["fortnite", "api", "matchmaking", "session", id]) => matchmaking_session(fnt, req, id),
        ("GET", ["rootDesc.xml"]) => Response::empty(404),
        _ => return None,
    })
}
