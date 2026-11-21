use super::super::{util, Request, Response};
use super::{data, Fortnite};
use serde_json::{json, Value};

fn party_create(req: &Request) -> Response {
    let body = req.json();
    let Some(conn) = body.get("join_info").and_then(|j| j.get("connection")) else {
        return Response::json(&json!({}));
    };
    let now = util::iso_now();
    let conn_id = conn.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let mut config = json!({"type": "DEFAULT"});
    if let Some(c) = body.get("config").and_then(Value::as_object) {
        for (k, v) in c {
            config[k] = v.clone();
        }
    }
    config["discoverability"] = json!("ALL");
    config["sub_type"] = json!("default");
    config["invite_ttl"] = json!(14400);
    config["intention_ttl"] = json!(60);
    Response::json(&json!({
        "id": util::random_hex32(),
        "created_at": now,
        "updated_at": now,
        "config": config,
        "members": [{
            "account_id": conn_id.split("@prod").next().unwrap_or(""),
            "meta": body["join_info"].get("meta").cloned().unwrap_or(json!({})),
            "connections": [{
                "id": conn_id,
                "connected_at": now,
                "updated_at": now,
                "yield_leadership": false,
                "meta": conn.get("meta").cloned().unwrap_or(json!({})),
            }],
            "revision": 0,
            "updated_at": now,
            "joined_at": now,
            "role": "CAPTAIN",
        }],
        "applicants": [],
        "meta": body.get("meta").cloned().unwrap_or(json!({})),
        "invites": [],
        "revision": 0,
        "intentions": [],
    }))
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    Some(match (m, seg) {
        ("GET", ["friends", "api", "v1", _, "settings"]) => Response::json(&json!({})),
        ("PUT" | "POST" | "PATCH", ["friends", "api", "v1", _, "settings"]) => Response::json(&json!({})),
        ("GET", ["friends", "api", "v1", _, "blocklist"]) => Response::json(&json!([])),
        ("GET", ["friends", "api", "v1", _, "recent", _]) => Response::json(&json!([])),
        ("GET", ["friends", "api", "v1", _, "summary"]) => Response::json(&json!({
            "friends": [], "incoming": [], "outgoing": [], "suggested": [], "blocklist": [],
            "settings": {"acceptInvites": "public"},
        })),
        ("GET", ["friends", "api", "v1", _, "friends", ..]) => Response::json(&json!([])),
        ("GET", ["friends", "api", "v1", _, "incoming" | "outgoing" | "suggested"]) => Response::json(&json!([])),
        ("GET", ["friends", "api", "public", "friends", _]) => Response::json(&json!([])),
        ("GET", ["friends", "api", "public", "list", "fortnite", _, "recentPlayers"]) => Response::json(&json!([])),
        ("GET", ["friends", "api", "public", "blocklist", _]) => Response::json(&json!({"blockedUsers": []})),
        ("GET", ["party", "api", "v1", "Fortnite", "user", _, "notifications", "undelivered", "count"]) => Response::json(&json!({"pings": 0, "invites": 0})),
        ("GET", ["party", "api", "v1", "Fortnite", "user", ..]) => Response::json(&json!({
            "current": [], "pending": [], "invites": [], "pings": []
        })),
        ("POST", ["party", "api", "v1", "Fortnite", "parties"]) => party_create(req),
        (_, ["party", "api", "v1", "Fortnite", "parties", ..]) => Response::empty(204),
        ("GET", ["presence", "api", "v1", _, _, "last-online"]) => Response::json(&json!({})),
        ("GET", ["presence", "api", "v1", _, _, "settings", "subscriptions"]) => Response::json(&json!([])),
        ("GET", ["socialban", "api", "public", "v1", ..]) => Response::json(&json!({"bans": [], "warnings": []})),
        ("GET", ["content-controls", id, ..]) => Response::json(&json!({
            "accountId": id,
            "pinRequired": false,
            "isRestricted": false,
            "contentControlsEnabled": false,
            "ageGroup": "ADULT",
            "settings": {},
        })),
        ("GET", ["fortnite", "api", "game", "v2", "privacy", "account", id]) => {
            let mut p: Value = serde_json::from_str(data::PRIVACY).unwrap_or(json!({}));
            p["accountId"] = json!(id);
            p["optOutOfPublicLeaderboards"] = json!(fnt.privacy_opt_out.lock().map(|g| *g).unwrap_or(false));
            Response::json(&p)
        }
        ("POST", ["fortnite", "api", "game", "v2", "privacy", "account", id]) => {
            let v = req.json().get("optOutOfPublicLeaderboards").and_then(Value::as_bool).unwrap_or(false);
            if let Ok(mut g) = fnt.privacy_opt_out.lock() {
                *g = v;
            }
            Response::json(&json!({"accountId": id, "optOutOfPublicLeaderboards": v}))
        }
        ("POST", ["fortnite", "api", "game", "v2", "chat", _, "recommendGeneralChatRooms", _]) => Response::json(&json!({})),
        ("POST", ["fortnite", "api", "game", "v2", "chat", _, _, _, _]) => Response::json(&json!({"GlobalChatRooms": [{"roomName": "xboxemulatorglobal"}]})),
        _ => return None,
    })
}
