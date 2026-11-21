use super::{Request, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct State {
    avoid: BTreeSet<u64>,
    mute: BTreeSet<u64>,
    stats: BTreeMap<(u64, String, String), Value>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

fn me() -> (u64, String) {
    let u = crate::gdk::user::user();
    (u.xuid, u.gamertag.clone())
}

fn gamertag(xuid: u64) -> String {
    let (x, g) = me();
    if xuid == x {
        g
    } else {
        crate::gdk::social::name_for(xuid)
    }
}

fn parse_xuid(s: &str) -> Option<u64> {
    let s = s.trim();
    let inner = s.strip_prefix("xuid(").and_then(|r| r.strip_suffix(')')).unwrap_or(s);
    inner.trim().parse().ok()
}

fn parse_owner(s: &str) -> Option<u64> {
    if s.trim().eq_ignore_ascii_case("me") { Some(me().0) } else { parse_xuid(s) }
}

fn field<'a>(v: &'a Value, name: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    v.as_object().and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)).unwrap_or(&NULL)
}

fn xuid_value(v: &Value) -> Option<u64> {
    v.as_str().and_then(parse_xuid).or_else(|| v.as_u64())
}

pub fn privacy_list(owner: &str, list: &str) -> Option<Vec<u64>> {
    if parse_owner(owner) != Some(me().0) {
        return None;
    }
    let st = state().lock().unwrap_or_else(|e| e.into_inner());
    match list {
        "mute" => Some(st.mute.iter().copied().collect()),
        "avoid" => Some(st.avoid.iter().copied().collect()),
        _ => None,
    }
}

fn list_uri(owner: u64, list: &str) -> String {
    format!("https://privacy.xboxlive.com/users/xuid({owner})/{}", if list == "mute" { "mutelist" } else { "neverlist" })
}

fn is_xbl_host(req: &Request) -> bool {
    req.header("x-forwarded-host").map_or(false, |h| {
        let h = h.split(':').next().unwrap_or("").to_ascii_lowercase();
        h == "xboxlive.com" || h.ends_with(".xboxlive.com")
    })
}

pub fn wants(req: &Request) -> bool {
    is_xbl_host(req)
}

fn xbl_error(status: u16, code: &str, description: &str) -> Response {
    Response::json_status(status, &json!({"code": code, "description": description}))
}

const PERMISSIONS: &[&str] = &[
    "CommunicateUsingText", "CommunicateUsingVideo", "CommunicateUsingVoice", "ViewTargetProfile", "ViewTargetGameHistory",
    "ViewTargetVideoHistory", "ViewTargetMusicHistory", "ViewTargetExerciseInfo", "ViewTargetPresence", "ViewTargetVideoStatus",
    "ViewTargetMusicStatus", "PlayMultiplayer", "BroadcastWithTwitch", "ViewTargetUserCreatedContent", "WriteComment",
    "ShareItem", "ShareTargetContentToExternalNetworks",
];

fn check(perm: &str, target: Option<u64>, anonymous: Option<&str>) -> Value {
    if !PERMISSIONS.iter().any(|p| p.eq_ignore_ascii_case(perm)) {
        return json!({"permissionRequested": perm, "isAllowed": false, "reasons": [{"reason": "Unknown", "restrictedSetting": perm}]});
    }
    let (mine, _) = me();
    let st = state().lock().unwrap_or_else(|e| e.into_inner());
    let mut reasons = Vec::new();
    if let Some(t) = target {
        if t != mine && st.avoid.contains(&t) {
            reasons.push(json!({"reason": "BlockListRestrictsTarget"}));
        }
        if t != mine && perm.eq_ignore_ascii_case("CommunicateUsingVoice") && st.mute.contains(&t) {
            reasons.push(json!({"reason": "MuteListRestrictsTarget"}));
        }
    }
    if let Some(a) = anonymous {
        if !matches!(a, "AllUsers" | "CrossNetworkUser" | "CrossNetworkFriend") {
            reasons.push(json!({"reason": "Unknown"}));
        }
    }
    json!({"permissionRequested": perm, "isAllowed": reasons.is_empty(), "reasons": reasons})
}

fn privacy(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let (mine, _) = me();
    Some(match (m, seg) {
        ("GET", [owner, "permission", "validate"]) => {
            if parse_owner(owner) != Some(mine) {
                return Some(xbl_error(403, "Forbidden", "permissions can only be checked for the signed-in user"));
            }
            let Some(setting) = req.query("setting") else { return Some(xbl_error(400, "BadRequest", "setting is required")) };
            let target = req.query("target").and_then(parse_xuid);
            if target.is_none() && req.query("target").is_some() {
                return Some(xbl_error(400, "BadRequest", "target must be xuid(n)"));
            }
            let r = check(setting, target, None);
            let mut out = json!({"isAllowed": r["isAllowed"]});
            if r["reasons"].as_array().map_or(false, |a| !a.is_empty()) {
                out["reasons"] = r["reasons"].clone();
            }
            Response::json(&out)
        }
        ("POST", [owner, "permission", "validate"]) => {
            if parse_owner(owner) != Some(mine) {
                return Some(xbl_error(403, "Forbidden", "permissions can only be checked for the signed-in user"));
            }
            let b = req.json();
            let (Some(users), Some(perms)) = (field(&b, "users").as_array(), field(&b, "permissions").as_array()) else {
                return Some(xbl_error(400, "BadRequest", "users and permissions are required"));
            };
            let responses: Vec<Value> = users.iter().map(|u| {
                let target = xuid_value(field(u, "xuid"));
                let anon = field(u, "anonymousUser").as_str();
                let list: Vec<Value> = perms.iter().filter_map(Value::as_str).map(|p| check(p, target, anon)).collect();
                let user = match (target, anon) {
                    (Some(t), _) => json!({"xuid": t.to_string()}),
                    (None, Some(a)) => json!({"anonymousUser": a}),
                    (None, None) => u.clone(),
                };
                json!({"user": user, "permissions": list})
            }).collect();
            Response::json(&json!({"responses": responses}))
        }
        ("GET", [owner, list @ ("mutelist" | "neverlist")]) => match privacy_list(owner, if *list == "mutelist" { "mute" } else { "avoid" }) {
            Some(x) => Response::json(&json!({"Xuids": x.iter().map(|v| v.to_string()).collect::<Vec<_>>()})),
            None => xbl_error(403, "Forbidden", "only the signed-in user's lists can be read"),
        },
        (_, [owner, "people", list @ ("avoid" | "mute"), rest @ ..]) => {
            if parse_owner(owner) != Some(mine) {
                return Some(xbl_error(403, "Forbidden", "only the signed-in user's lists can be read"));
            }
            let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
            let set = if *list == "avoid" { &mut st.avoid } else { &mut st.mute };
            match (m, rest) {
                ("GET", []) => Response::json(&json!({"users": set.iter().map(|x| json!({"xuid": x.to_string()})).collect::<Vec<_>>()})),
                ("PUT", [target]) => match parse_xuid(target) {
                    Some(t) => {
                        let added = set.insert(t);
                        drop(st);
                        if added {
                            super::rta::publish(&list_uri(mine, list), &json!({"NotificationType": "Added", "Xuids": [t.to_string()]}));
                        }
                        Response::empty(204)
                    }
                    None => xbl_error(400, "BadRequest", "target must be xuid(n)"),
                },
                ("DELETE", [target]) => match parse_xuid(target) {
                    Some(t) => {
                        let removed = set.remove(&t);
                        drop(st);
                        if removed {
                            super::rta::publish(&list_uri(mine, list), &json!({"NotificationType": "Removed", "Xuids": [t.to_string()]}));
                        }
                        Response::empty(204)
                    }
                    None => xbl_error(400, "BadRequest", "target must be xuid(n)"),
                },
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn person(xuid: u64) -> Value {
    json!({"xuid": xuid.to_string(), "isFavorite": false, "isFollowingCaller": true, "socialNetworks": [], "addedDateTimeUtc": "2020-01-01T00:00:00Z"})
}

fn social(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let (mine, _) = me();
    let friends: Vec<u64> = crate::gdk::social::friends().into_iter().map(|f| f.0).collect();
    Some(match (m, seg) {
        ("GET", [owner, "people"]) if parse_owner(owner) == Some(mine) => {
            let start: usize = match req.query("startIndex").map(str::parse) {
                None => 0,
                Some(Ok(n)) => n,
                Some(Err(_)) => return Some(xbl_error(400, "BadRequest", "startIndex must be an unsigned integer")),
            };
            let max: usize = match req.query("maxItems").map(str::parse) {
                None => usize::MAX,
                Some(Ok(n)) => n,
                Some(Err(_)) => return Some(xbl_error(400, "BadRequest", "maxItems must be an unsigned integer")),
            };
            let people: Vec<Value> = friends.iter().skip(start).take(max).map(|x| person(*x)).collect();
            Response::json(&json!({"totalCount": friends.len(), "people": people}))
        }
        ("GET", [owner, "people", target]) if parse_owner(owner) == Some(mine) => match parse_xuid(target) {
            Some(t) if friends.contains(&t) => Response::json(&person(t)),
            Some(_) => xbl_error(404, "NotFound", "the target is not followed by the user"),
            None => xbl_error(400, "BadRequest", "target must be xuid(n)"),
        },
        ("POST", [owner, "people", "xuids"]) if parse_owner(owner) == Some(mine) => {
            let b = req.json();
            let asked: Vec<u64> = b["xuids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().and_then(parse_xuid).or(v.as_u64())).collect()).unwrap_or_default();
            let people: Vec<Value> = asked.into_iter().filter(|x| friends.contains(x)).map(person).collect();
            Response::json(&json!({"totalCount": people.len(), "people": people}))
        }
        ("GET", [owner, "summary"]) if parse_owner(owner) == Some(mine) => Response::json(&json!({
            "targetFollowingCount": friends.len(), "targetFollowerCount": friends.len(), "isCallerFollowingTarget": false,
            "isTargetFollowingCaller": false, "hasCallerMarkedTargetAsFavorite": false, "hasCallerMarkedTargetAsIdentityShared": false,
            "legacyFriendStatus": "None", "availablePeopleSlots": 1000 - friends.len(),
        })),
        _ => return None,
    })
}

fn setting_value(xuid: u64, id: &str) -> Option<Value> {
    let tag = gamertag(xuid);
    Some(json!(match id {
        "Gamertag" | "GameDisplayName" | "AppDisplayName" | "ModernGamertag" | "UniqueModernGamertag" => tag,
        "ModernGamertagSuffix" | "RealName" | "Bio" | "Location" | "Watermarks" => String::new(),
        "GameDisplayPicRaw" | "AppDisplayPicRaw" => format!("https://images-eds-ssl.xboxlive.com/image?url=xuid{xuid}"),
        "AccountTier" => "Gold".into(),
        "TenureLevel" | "Gamerscore" => "0".into(),
        "XboxOneRep" => "GoodPlayer".into(),
        "PreferredColor" => "https://dlassets-ssl.xboxlive.com/public/content/ppl/colors/00000.json".into(),
        "IsQuarantined" => "0".into(),
        "SpeechAccessibility" => crate::cfg("speech_accessibility").unwrap_or_default(),
        _ => return None,
    }))
}

fn profile_user(xuid: u64, settings: &[&str]) -> Value {
    let list: Vec<Value> = settings.iter().filter_map(|s| setting_value(xuid, s).map(|v| json!({"id": s, "value": v}))).collect();
    json!({"id": xuid.to_string(), "hostId": xuid.to_string(), "settings": list, "isSponsoredUser": false})
}

fn profile(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    Some(match (m, seg) {
        ("POST", ["users", "batch", "profile", "settings"]) => {
            let b = req.json();
            let ids: Vec<u64> = b["userIds"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().and_then(parse_xuid).or(v.as_u64())).collect()).unwrap_or_default();
            let settings: Vec<&str> = b["settings"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if ids.is_empty() || settings.is_empty() {
                return Some(xbl_error(400, "BadRequest", "userIds and settings are required"));
            }
            Response::json(&json!({"profileUsers": ids.iter().map(|x| profile_user(*x, &settings)).collect::<Vec<_>>()}))
        }
        ("GET", ["users", owner, "profile", "settings"]) => {
            let x = if owner.eq_ignore_ascii_case("me") { Some(me().0) } else { parse_xuid(owner) };
            let Some(x) = x else { return Some(xbl_error(400, "BadRequest", "user must be xuid(n) or me")) };
            let settings: Vec<&str> = req.query("settings").map(|s| s.split(',').collect()).unwrap_or_default();
            Response::json(&json!({"profileUsers": [profile_user(x, &settings)]}))
        }
        _ => return None,
    })
}

pub const REPUTATION_SCID: &str = "7492baca-c1b4-440d-a391-b7ef364a8d40";

fn service_stat(scid: &str, name: &str) -> Option<Value> {
    match (scid.to_ascii_lowercase().as_str(), name.to_ascii_lowercase().as_str()) {
        (REPUTATION_SCID, "overallreputationisbad" | "fairplayreputationisbad" | "commsreputationisbad" | "usercontentreputationisbad") => Some(json!(0)),
        _ => None,
    }
}

fn stat_entry(xuid: u64, scid: &str, name: &str) -> Option<Value> {
    let st = state().lock().unwrap_or_else(|e| e.into_inner());
    st.stats.get(&(xuid, scid.to_ascii_lowercase(), name.to_string())).cloned().or_else(|| service_stat(scid, name)).map(|v| {
        let ty = if v.is_i64() || v.is_u64() { "Integer" } else if v.is_f64() { "Double" } else { "String" };
        json!({"statname": name, "type": ty, "value": v.to_string().trim_matches('"')})
    })
}

pub fn record_stat(xuid: u64, scid: &str, name: &str, value: Value) {
    state().lock().unwrap_or_else(|e| e.into_inner()).stats.insert((xuid, scid.to_ascii_lowercase(), name.to_string()), value);
}

fn userstats(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    Some(match (m, seg) {
        ("POST", ["batch"]) if req.query("operation").map_or(true, |o| o.eq_ignore_ascii_case("read")) => {
            let b = req.json();
            let users: Vec<u64> = field(&b, "requestedusers").as_array().map(|a| a.iter().filter_map(|v| v.as_str().and_then(parse_xuid).or(v.as_u64())).collect()).unwrap_or_default();
            let scids = field(&b, "requestedscids").as_array().cloned().unwrap_or_default();
            if users.is_empty() || scids.is_empty() {
                return Some(xbl_error(400, "BadRequest", "requestedusers and requestedscids are required"));
            }
            let out: Vec<Value> = users.iter().map(|x| {
                let s: Vec<Value> = scids.iter().map(|sc| {
                    let id = sc["scid"].as_str().unwrap_or("");
                    let stats: Vec<Value> = sc["requestedstats"].as_array().map(|a| a.iter().filter_map(Value::as_str).filter_map(|n| stat_entry(*x, id, n)).collect()).unwrap_or_default();
                    json!({"scid": id, "stats": stats})
                }).collect();
                json!({"xuid": x.to_string(), "gamertag": gamertag(*x), "scids": s})
            }).collect();
            Response::json(&json!({"users": out}))
        }
        ("GET", [owner, "scids", scid, "stats", names]) => {
            let Some(x) = parse_owner(owner) else { return Some(xbl_error(400, "BadRequest", "user must be xuid(n)")) };
            let stats: Vec<Value> = names.split(',').filter_map(|n| stat_entry(x, scid, n)).collect();
            Response::json(&json!({"user": {"xuid": x.to_string(), "gamertag": gamertag(x), "stats": stats}}))
        }
        _ => return None,
    })
}

pub fn route(req: &Request) -> Option<Response> {
    if !is_xbl_host(req) {
        return None;
    }
    let host = req.header("x-forwarded-host").unwrap_or("").split(':').next().unwrap_or("").to_ascii_lowercase();
    let seg = req.segments();
    let m = req.method.as_str();
    let users_tail = || if seg.first() == Some(&"users") { Some(&seg[1..]) } else { None };
    match host.split('.').next().unwrap_or("") {
        "privacy" => users_tail().and_then(|s| privacy(req, m, s)),
        "social" => users_tail().and_then(|s| social(req, m, s)),
        "profile" => profile(req, m, &seg),
        "sessiondirectory" => super::mpsd::route(req, m, &seg),
        "achievements" => super::xbl_title::achievements(req, m, &seg),
        "peoplehub" => super::xbl_title::peoplehub(req, m, &seg),
        "userpresence" => super::xbl_title::userpresence(req, m, &seg),
        "multiplayeractivity" => super::mpsd::activity(req, m, &seg),
        "userstats" => match users_tail() {
            Some(s) => userstats(req, m, s),
            None => userstats(req, m, &seg),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, host: &str, path: &str, body: Value) -> Request {
        let (path, query) = match path.split_once('?') {
            Some((p, q)) => (p.to_string(), super::super::util::parse_pairs(q)),
            None => (path.to_string(), Vec::new()),
        };
        Request { method: method.into(), path, query, headers: vec![("X-Forwarded-Host".into(), host.into())], body: if body.is_null() { Vec::new() } else { body.to_string().into_bytes() } }
    }

    fn body(r: &Response) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    #[test]
    fn privacy_is_stateful_and_validates() {
        let x = me().0;
        let r = route(&req("GET", "privacy.xboxlive.com", &format!("/users/xuid({x})/permission/validate?setting=CommunicateUsingVoice&target=xuid(42)"), Value::Null)).unwrap();
        assert_eq!(body(&r)["isAllowed"], true);
        assert_eq!(route(&req("PUT", "privacy.xboxlive.com", &format!("/users/xuid({x})/people/avoid/xuid(42)"), Value::Null)).unwrap().status, 204);
        let r = route(&req("GET", "privacy.xboxlive.com", &format!("/users/xuid({x})/permission/validate?setting=PlayMultiplayer&target=xuid(42)"), Value::Null)).unwrap();
        assert_eq!(body(&r)["isAllowed"], false);
        let r = route(&req("GET", "privacy.xboxlive.com", &format!("/users/xuid({x})/people/avoid"), Value::Null)).unwrap();
        assert_eq!(body(&r)["users"][0]["xuid"], "42");
        let r = route(&req("POST", "privacy.xboxlive.com", &format!("/users/xuid({x})/permission/validate"), json!({"users": [{"anonymousUser": "CrossNetworkUser"}, {"xuid": "42"}], "permissions": ["PlayMultiplayer"]}))).unwrap();
        let b = body(&r);
        assert_eq!(b["responses"][0]["permissions"][0]["isAllowed"], true);
        assert_eq!(b["responses"][1]["permissions"][0]["isAllowed"], false);
        assert_eq!(route(&req("DELETE", "privacy.xboxlive.com", &format!("/users/xuid({x})/people/avoid/xuid(42)"), Value::Null)).unwrap().status, 204);
        assert_eq!(route(&req("GET", "privacy.xboxlive.com", &format!("/users/xuid({x})/permission/validate?setting=Nonsense"), Value::Null)).map(|r| body(&r)["isAllowed"].clone()), Some(json!(false)));
        assert_eq!(route(&req("GET", "privacy.xboxlive.com", "/users/xuid(7)/permission/validate?setting=PlayMultiplayer", Value::Null)).unwrap().status, 403);
    }

    #[test]
    fn profile_social_and_stats_answer_like_the_services() {
        let (x, tag) = me();
        let r = route(&req("POST", "profile.xboxlive.com", "/users/batch/profile/settings", json!({"userIds": [x.to_string()], "settings": ["Gamertag", "GameDisplayName", "NotASetting"]}))).unwrap();
        let b = body(&r);
        assert_eq!(b["profileUsers"][0]["settings"][0]["value"], json!(tag));
        assert_eq!(b["profileUsers"][0]["settings"].as_array().unwrap().len(), 2);
        let r = route(&req("GET", "social.xboxlive.com", &format!("/users/xuid({x})/people"), Value::Null)).unwrap();
        assert!(body(&r)["people"].is_array());
        record_stat(x, "ABC", "Wins", json!(3));
        let r = route(&req("POST", "userstats.xboxlive.com", "/batch?operation=read", json!({"requestedusers": [x.to_string()], "requestedscids": [{"scid": "abc", "requestedstats": ["Wins", "Kills"]}]}))).unwrap();
        let b = body(&r);
        let stats = b["users"][0]["scids"][0]["stats"].as_array().unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0]["value"], "3");
        assert!(route(&req("GET", "unknown.example.com", "/users/batch/profile/settings", Value::Null)).is_none());
    }

    #[test]
    fn gamechat2_console_requests_get_json() {
        let x = me().0;
        let r = route(&req("GET", "social.xboxlive.com", "/users/me/people?startIndex=0&maxItems=1000", Value::Null)).unwrap();
        let b = body(&r);
        assert_eq!(r.status, 200);
        assert!(b["people"].is_array());
        assert!(b["totalCount"].is_u64());
        assert_eq!(route(&req("GET", "social.xboxlive.com", "/users/me/people?startIndex=x", Value::Null)).unwrap().status, 400);
        let r = route(&req("POST", "privacy.xboxlive.com", &format!("/users/xuid({x})/permission/validate"), json!({"Users": [{"Xuid": "2535405290000002"}], "Permissions": ["CommunicateUsingVoice"]}))).unwrap();
        let b = body(&r);
        assert_eq!(b["responses"][0]["user"]["xuid"], "2535405290000002");
        assert_eq!(b["responses"][0]["permissions"][0]["permissionRequested"], "CommunicateUsingVoice");
        assert_eq!(b["responses"][0]["permissions"][0]["isAllowed"], true);
        let r = route(&req("GET", "privacy.xboxlive.com", &format!("/users/xuid({x})/mutelist"), Value::Null)).unwrap();
        assert!(body(&r)["Xuids"].is_array());
        let r = route(&req("GET", "privacy.xboxlive.com", &format!("/users/xuid({x})/neverlist"), Value::Null)).unwrap();
        assert!(body(&r)["Xuids"].is_array());
        let r = route(&req("GET", "profile.xboxlive.com", "/users/me/profile/settings?settings=SpeechAccessibility", Value::Null)).unwrap();
        let b = body(&r);
        assert_eq!(b["profileUsers"][0]["id"], x.to_string());
        assert_eq!(b["profileUsers"][0]["settings"][0]["id"], "SpeechAccessibility");
        assert!(b["profileUsers"][0]["settings"][0]["value"].is_string());
        let r = route(&req("POST", "userstats.xboxlive.com", "/batch", json!({"requestedusers": ["2535405290000002"], "requestedscids": [{"scid": REPUTATION_SCID, "requestedstats": ["overallreputationisbad"]}]}))).unwrap();
        let b = body(&r);
        assert_eq!(b["users"][0]["scids"][0]["stats"][0]["statname"], "overallreputationisbad");
        assert_eq!(b["users"][0]["scids"][0]["stats"][0]["value"], "0");
    }
}
