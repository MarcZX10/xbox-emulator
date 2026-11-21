use super::util::iso_now;
use super::{Request, Response};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

const HEARTBEAT_SECS: u32 = 300;

#[derive(Default)]
struct State {
    progress: BTreeMap<(String, String), (u32, String)>,
    presence: Option<Value>,
}

fn st() -> std::sync::MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default())).lock().unwrap_or_else(|e| e.into_inner())
}

fn me() -> u64 {
    crate::gdk::user::user().xuid
}

fn owner(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("me") {
        return Some(me());
    }
    s.strip_prefix("xuid(").and_then(|r| r.strip_suffix(')')).unwrap_or(s).parse().ok()
}

fn err(status: u16, code: &str, msg: &str) -> Response {
    Response::json_status(status, &json!({"code": code, "description": msg}))
}

fn achievement_json(scid: &str, id: &str, pct: u32, unlocked: &str, title: &str) -> Value {
    let state = if pct >= 100 { "Achieved" } else if pct > 0 { "InProgress" } else { "NotStarted" };
    json!({
        "id": id, "serviceConfigId": scid, "name": format!("Achievement {id}"), "titleAssociations": [{"name": "", "id": title.parse::<u64>().unwrap_or(0)}],
        "progressState": state, "progression": {"requirements": [{"id": "00000000-0000-0000-0000-000000000000", "current": pct.to_string(), "target": "100", "operationType": "Sum", "valueType": "Integer", "ruleParticipationType": "Individual"}], "timeUnlocked": if pct >= 100 { unlocked } else { "0001-01-01T00:00:00.0000000Z" }},
        "mediaAssets": [], "platforms": ["Scarlett", "XboxOne"], "isSecret": false, "description": "", "lockedDescription": "", "productId": "", "achievementType": "Persistent",
        "participationType": "Individual", "timeWindow": Value::Null, "rewards": [], "estimatedTime": "00:00:00", "deeplink": "", "isRevoked": false,
    })
}

pub fn achievements(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let x = owner(seg.get(1)?)?;
    Some(match (m, &seg[2..]) {
        ("GET", ["achievements"]) => {
            let scid_filter = req.query("serviceConfigId").map(|s| s.to_ascii_lowercase());
            let title = req.query("titleId").unwrap_or("0").to_string();
            let unlocked_only = req.query("unlockedOnly").map_or(false, |v| v.eq_ignore_ascii_case("true"));
            let skip = req.query("skipItems").or(req.query("continuationToken")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let max = req.query("maxItems").and_then(|v| v.parse::<usize>().ok()).unwrap_or(usize::MAX);
            let all: Vec<Value> = if x != me() {
                Vec::new()
            } else {
                st().progress.iter().filter(|((scid, _), (pct, _))| scid_filter.as_ref().map_or(true, |f| f == scid) && (!unlocked_only || *pct >= 100)).map(|((scid, id), (pct, t))| achievement_json(scid, id, *pct, t, &title)).collect()
            };
            let total = all.len();
            let page: Vec<Value> = all.into_iter().skip(skip).take(max).collect();
            let next = (skip + page.len() < total).then(|| (skip + page.len()).to_string());
            Response::json(&json!({"achievements": page, "pagingInfo": {"continuationToken": next, "totalRecords": total}}))
        }
        ("GET", ["achievements", scid, id]) => {
            let key = (scid.to_ascii_lowercase(), id.to_string());
            match st().progress.get(&key).cloned() {
                Some((pct, t)) if x == me() => Response::json(&json!({"achievement": achievement_json(&key.0, id, pct, &t, req.query("titleId").unwrap_or("0"))})),
                _ => err(404, "NotFound", "no achievement with that id has been reported for this user"),
            }
        }
        ("POST", ["achievements", scid, "update"]) => {
            if x != me() {
                return Some(err(403, "Forbidden", "achievements can only be updated for the signed-in user"));
            }
            let b = req.json();
            let Some(list) = b["achievements"].as_array() else { return Some(err(400, "BadRequest", "achievements array is required")) };
            let mut s = st();
            for a in list {
                let (Some(id), Some(pct)) = (a["id"].as_str(), a["percentComplete"].as_u64().or_else(|| a["percentComplete"].as_str().and_then(|v| v.parse().ok()))) else {
                    return Some(err(400, "BadRequest", "each achievement needs id and percentComplete"));
                };
                if pct > 100 {
                    return Some(err(400, "BadRequest", "percentComplete must be 0..100"));
                }
                let e = s.progress.entry((scid.to_ascii_lowercase(), id.to_string())).or_insert((0, String::new()));
                if pct as u32 > e.0 {
                    e.0 = pct as u32;
                    if e.0 >= 100 {
                        e.1 = iso_now();
                    }
                }
            }
            Response::empty(204)
        }
        _ => return None,
    })
}

fn person(x: u64, name: &str, online: bool) -> Value {
    let mut v = json!({
        "xuid": x.to_string(), "isFavorite": false, "isFollowingCaller": x != me(), "isFollowedByCaller": x != me(), "isIdentityShared": false, "addedDateTimeUtc": Value::Null,
        "displayName": name, "realName": "", "displayPicRaw": "", "showUserAsAvatar": "0", "gamertag": name, "gamerScore": "0", "modernGamertag": name, "modernGamertagSuffix": "",
        "uniqueModernGamertag": name, "xboxOneRep": "GoodPlayer", "presenceState": if online { "Online" } else { "Offline" }, "presenceText": if online { "Online" } else { "Offline" },
    });
    let rest = json!({
        "presenceDevices": Value::Null, "isBroadcasting": false, "isCloaked": Value::Null, "isQuarantined": false, "isXbox360Gamerpic": false, "lastSeenDateTimeUtc": Value::Null,
        "suggestion": Value::Null, "recommendation": Value::Null, "search": Value::Null, "titleHistory": Value::Null, "multiplayerSummary": {"InMultiplayerSession": 0, "InParty": 0},
        "recentPlayer": Value::Null, "follower": Value::Null, "preferredColor": Value::Null, "presenceDetails": [], "titlePresence": Value::Null, "titleSummaries": Value::Null,
        "presenceTitleIds": Value::Null, "detail": Value::Null, "communityManagerTitles": Value::Null, "socialManager": Value::Null, "broadcast": Value::Null, "tournamentSummary": Value::Null, "avatar": Value::Null,
    });
    if let (Some(a), Value::Object(b)) = (v.as_object_mut(), rest) {
        a.extend(b);
    }
    v
}

pub fn peoplehub(_req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    if m != "GET" || seg.first() != Some(&"users") {
        return None;
    }
    let x = owner(seg.get(1)?)?;
    if x != me() {
        return Some(err(403, "Forbidden", "people lists are only available for the signed-in user"));
    }
    let rest = &seg[2..];
    let friends = crate::gdk::social::friends();
    let people: Vec<Value> = match rest {
        ["people", "social", ..] | ["people", "friends", ..] => friends.iter().filter(|f| f.0 != me()).map(|f| person(f.0, &f.1, false)).collect(),
        ["people", ids, ..] if ids.starts_with("xuids(") => {
            let inner = ids.trim_start_matches("xuids(").trim_end_matches(')');
            inner.split(',').filter_map(|i| i.trim().parse::<u64>().ok()).map(|i| if i == me() { person(i, &crate::gdk::user::user().gamertag, true) } else { person(i, &crate::gdk::social::name_for(i), false) }).collect()
        }
        ["people", "me", ..] | ["people"] => vec![person(me(), &crate::gdk::user::user().gamertag, true)],
        _ => return None,
    };
    Some(Response::json(&json!({"people": people, "recommendationSummary": Value::Null, "friendFinderState": Value::Null, "accountLinkDetails": Value::Null})))
}

fn heartbeat(status: u16) -> Response {
    let mut r = Response::empty(status);
    r.headers.push(("X-Heartbeat-After".into(), HEARTBEAT_SECS.to_string()));
    r
}

fn presence_record(x: u64) -> Value {
    if x != me() {
        return json!({"xuid": x.to_string(), "state": "Offline"});
    }
    match st().presence.clone() {
        Some(t) => json!({"xuid": x.to_string(), "state": "Online", "devices": [{"type": "Scarlett", "titles": [t]}], "lastSeen": Value::Null}),
        None => json!({"xuid": x.to_string(), "state": "Online", "devices": [{"type": "Scarlett", "titles": []}]}),
    }
}

pub fn userpresence(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    if seg.first() != Some(&"users") {
        return None;
    }
    Some(match (m, &seg[1..]) {
        ("POST", ["batch"]) => {
            let b = req.json();
            let Some(users) = b["users"].as_array() else { return Some(err(400, "BadRequest", "users array is required")) };
            let out: Vec<Value> = users.iter().filter_map(|u| u.as_str().and_then(|s| s.parse::<u64>().ok()).or(u.as_u64())).map(presence_record).collect();
            Response::json(&json!(out))
        }
        (_, [who, rest @ ..]) => {
            let Some(x) = owner(who) else { return Some(err(400, "BadRequest", "user must be xuid(n) or me")) };
            match (m, rest) {
                ("GET", []) => Response::json(&presence_record(x)),
                ("POST", ["devices", "current", "titles", "current"]) => {
                    if x != me() {
                        return Some(err(403, "Forbidden", "presence can only be set for the signed-in user"));
                    }
                    let b = req.json();
                    let Some(id) = b["id"].as_str().map(String::from).or_else(|| b["id"].as_u64().map(|v| v.to_string())) else { return Some(err(400, "BadRequest", "title id is required")) };
                    let state = b["state"].as_str().unwrap_or("active");
                    if !state.eq_ignore_ascii_case("active") && !state.eq_ignore_ascii_case("inactive") {
                        return Some(err(400, "BadRequest", "state must be active or inactive"));
                    }
                    st().presence = Some(json!({"id": id, "name": "", "placement": b["placement"].as_str().unwrap_or("full"), "state": state, "lastModified": iso_now(), "activity": b["activity"].clone()}));
                    heartbeat(204)
                }
                ("DELETE", ["devices", "current", "titles", "current"]) => {
                    st().presence = None;
                    Response::empty(204)
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, path: &str, query: &[(&str, &str)], body: Value) -> Request {
        Request { method: method.into(), path: path.into(), query: query.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), headers: vec![], body: if body.is_null() { vec![] } else { body.to_string().into_bytes() } }
    }

    fn seg(p: &str) -> Vec<&str> {
        p.split('/').filter(|s| !s.is_empty()).collect()
    }

    #[test]
    fn achievements_presence_and_people() {
        let x = me();
        let p = format!("/users/xuid({x})/achievements");
        let r = achievements(&req("GET", &p, &[("titleId", "1")], Value::Null), "GET", &seg(&p)).unwrap();
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert!(v["achievements"].is_array() && v["pagingInfo"]["totalRecords"].is_number());
        let up = format!("/users/xuid({x})/achievements/SCID-A/update");
        assert_eq!(achievements(&req("POST", &up, &[], json!({"achievements": [{"id": "3", "percentComplete": 100}]})), "POST", &seg(&up)).unwrap().status, 204);
        let r = achievements(&req("GET", &p, &[("unlockedOnly", "true")], Value::Null), "GET", &seg(&p)).unwrap();
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert!(v["achievements"].as_array().unwrap().iter().any(|a| a["id"] == "3" && a["progressState"] == "Achieved"));
        assert_eq!(achievements(&req("POST", &up, &[], json!({"achievements": [{"id": "3", "percentComplete": 150}]})), "POST", &seg(&up)).unwrap().status, 400);

        let pp = format!("/users/xuid({x})/devices/current/titles/current");
        let r = userpresence(&req("POST", &pp, &[], json!({"id": "2040239934", "state": "active", "placement": "full"})), "POST", &seg(&pp)).unwrap();
        assert_eq!(r.status, 204);
        assert!(r.headers.iter().any(|(k, _)| k == "X-Heartbeat-After"));
        let g = format!("/users/xuid({x})");
        let v: Value = serde_json::from_slice(&userpresence(&req("GET", &g, &[], Value::Null), "GET", &seg(&g)).unwrap().body).unwrap();
        assert_eq!(v["devices"][0]["titles"][0]["id"], "2040239934");

        let h = format!("/users/xuid({x})/people/social/decoration/presenceDetail");
        let v: Value = serde_json::from_slice(&peoplehub(&req("GET", &h, &[], Value::Null), "GET", &seg(&h)).unwrap().body).unwrap();
        assert!(v["people"].is_array());
        assert_eq!(peoplehub(&req("GET", "/users/xuid(5)/people/social", &[], Value::Null), "GET", &seg("/users/xuid(5)/people/social")).unwrap().status, 403);
    }
}
