use super::super::util::{now_secs, random_uuid};
use super::super::xdefiant::is_uuid;
use super::super::{Request, Response};
use super::core::{nadeo_error, Core};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

const PB: &str = "Personal_Best";

#[derive(Default)]
struct State {
    favorites: BTreeSet<String>,
    clubs: BTreeMap<u64, Value>,
    members: BTreeMap<(u64, String), Value>,
    activities: BTreeMap<u64, Value>,
    campaigns: BTreeMap<u64, Value>,
    next_id: u64,
    pinned: u64,
    tag_club: u64,
}

pub struct Live {
    st: Mutex<State>,
}

fn data_path() -> std::path::PathBuf {
    crate::cfg("trackmania_live_data").map(std::path::PathBuf::from).unwrap_or_else(|| crate::gdk::data_root().join("trackmania").join("live.json"))
}

pub fn seed_maps(core: &Core) -> usize {
    let dir = data_path().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut n = 0;
    for m in list("maps") {
        let Some(file) = m["file"].as_str() else { continue };
        let p = std::path::Path::new(file);
        let p = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
        match std::fs::read(&p).map(|bytes| core.seed_map(&m, bytes).is_some()) {
            Ok(true) => n += 1,
            Ok(false) => crate::record_raw("online", "trackmania", &format!("live map entry without mapUid: {m}")),
            Err(e) => crate::record_raw("online", "trackmania", &format!("live map file {} unreadable: {e}", p.display())),
        }
    }
    n
}

fn data() -> &'static Value {
    static D: OnceLock<Value> = OnceLock::new();
    D.get_or_init(|| {
        let path = data_path();
        match std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            Some(v) => {
                crate::record_raw("online", "trackmania", &format!("live-services content loaded from {}", path.display()));
                v
            }
            None => {
                crate::record_raw("online", "trackmania", &format!("no live-services content at {} (official campaigns, track of the day, weekly and maniapub lists are empty)", path.display()));
                json!({})
            }
        }
    })
}

fn list(key: &str) -> Vec<Value> {
    data()[key].as_array().cloned().unwrap_or_default()
}

fn qnum(req: &Request, k: &str, d: usize) -> usize {
    req.query(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn paged(req: &Request, items: &[Value], default_len: usize) -> Vec<Value> {
    items.iter().skip(qnum(req, "offset", 0)).take(qnum(req, "length", default_len).min(100)).cloned().collect()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

pub fn secs_of(ts: &str) -> i64 {
    let n: Vec<i64> = ts.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).take(6).filter_map(|s| s.parse().ok()).collect();
    if n.len() < 6 {
        return 0;
    }
    days_from_civil(n[0], n[1], n[2]) * 86400 + n[3] * 3600 + n[4] * 60 + n[5]
}

fn next_request(items: &[Value]) -> Value {
    let now = now_secs() as i64;
    let next = items.iter().filter_map(|c| c["endTimestamp"].as_i64()).filter(|e| *e > now).min().unwrap_or(now + 86400);
    json!({"nextRequestTimestamp": next, "relativeNextRequest": next - now})
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(o), Value::Object(n)) = (a.as_object_mut(), b) {
        o.extend(n);
    }
    a
}

fn campaign_v2(c: &Value) -> Value {
    let mut o = Map::new();
    for k in ["id", "seasonUid", "name", "useCase", "clubId", "startTimestamp", "endTimestamp", "rankingSentTimestamp", "year", "week", "day", "monthYear", "month", "monthDay", "playlist"] {
        o.insert(k.into(), c.get(k).cloned().unwrap_or(Value::Null));
    }
    o.insert("editionTimestamp".into(), c.get("editionTimestamp").or(c.get("publicationTimestamp")).or(c.get("startTimestamp")).cloned().unwrap_or(json!(0)));
    Value::Object(o)
}

fn media_urls(prefix: &str) -> Map<String, Value> {
    let mut o = Map::new();
    o.insert(prefix.into(), json!(""));
    for s in ["PngLarge", "PngMedium", "PngSmall", "Dds"] {
        o.insert(format!("{prefix}{s}"), json!(""));
    }
    o
}

fn body_str(b: &Value, k: &str) -> Option<String> {
    b.get(k).and_then(|v| v.as_str().map(String::from))
}

impl Default for Live {
    fn default() -> Self {
        Live::new()
    }
}

impl Live {
    pub fn new() -> Live {
        Live { st: Mutex::new(State { next_id: 1000, ..State::default() }) }
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn zones(&self, core: &Core, position: i64) -> Value {
        json!(core.zone_chain(&core.account_id).into_iter().map(|(id, name)| json!({"zoneId": id, "zoneName": name, "ranking": {"position": position, "length": 0}})).collect::<Vec<_>>())
    }

    fn my_zone(&self, core: &Core) -> (String, String) {
        core.zone_chain(&core.account_id).pop().unwrap_or_default()
    }

    fn best(&self, core: &Core, group: &str, uid: &str) -> Option<(i64, i64)> {
        let map = core.map_by_uid(uid)?;
        core.records().into_iter().filter(|r| {
            r["accountId"].as_str().map_or(false, |a| a.eq_ignore_ascii_case(&core.account_id)) && r["mapId"] == map["mapId"]
                && if group == PB { r["scopeType"] == json!("PersonalBest") } else { r["scopeId"] == json!(group) }
        }).filter_map(|r| Some((r["recordScore"]["time"].as_i64()?, secs_of(r["timestamp"].as_str().unwrap_or("")))))
            .min_by_key(|(t, _)| *t)
    }

    fn live_map(&self, m: &Value) -> Value {
        let uid = m["mapUid"].as_str().unwrap_or("").to_string();
        let ts = secs_of(m["timestamp"].as_str().unwrap_or(""));
        json!({
            "uid": uid, "mapId": m["mapId"], "name": m["name"], "author": m["author"], "submitter": m["submitter"],
            "authorTime": m["authorScore"], "goldTime": m["goldScore"], "silverTime": m["silverScore"], "bronzeTime": m["bronzeScore"],
            "nbLaps": m.get("nbLaps").cloned().unwrap_or(json!(0)), "valid": m["isPlayable"], "downloadUrl": m["fileUrl"], "thumbnailUrl": m["thumbnailUrl"].as_str().unwrap_or(""),
            "uploadTimestamp": ts, "updateTimestamp": ts, "fileSize": Value::Null, "public": false, "favorite": self.st().favorites.contains(&uid),
            "playable": true, "mapStyle": m["mapStyle"], "mapType": m["mapType"], "collectionName": m["collectionName"],
        })
    }

    fn club_obj(&self, id: u64, b: &Value, author: &str, now: i64) -> Value {
        let mut o = Map::new();
        for p in ["iconUrl", "decalUrl", "decalSponsor4x1Url", "screen16x9Url", "screen64x41Url", "screen16x1Url", "screen8x1Url", "verticalUrl", "backgroundUrl"] {
            o.extend(media_urls(p));
        }
        let fields = json!({
            "id": id, "name": body_str(b, "name").unwrap_or_default(), "tag": body_str(b, "tag").unwrap_or_default(), "description": body_str(b, "description").unwrap_or_default(),
            "authorAccountId": author, "latestEditorAccountId": author, "logoUrl": "", "state": body_str(b, "state").unwrap_or_else(|| "private-closed".into()),
            "featured": false, "walletUid": random_uuid(), "metadata": body_str(b, "metadata").unwrap_or_default(), "creationTimestamp": now, "editionTimestamp": now,
            "iconTheme": "", "decalTheme": "racing", "screen16x9Theme": "mirage", "screen64x41Theme": "mirage", "screen8x1Theme": "mirage", "screen16x1Theme": "mirage",
            "verticalTheme": "valhalla", "backgroundTheme": "valhalla", "verified": false, "popularityLevel": 0,
        });
        merge(Value::Object(o), fields)
    }

    fn member_obj(club: u64, account: &str, role: &str, now: i64) -> Value {
        json!({"accountId": account, "clubId": club, "role": role, "creationTimestamp": now, "vip": false, "moderator": false, "hasFeatured": false, "pin": false, "useTag": false})
    }

    fn role_of(&self, club: u64, account: &str) -> Option<String> {
        self.st().members.get(&(club, account.to_ascii_lowercase())).and_then(|m| m["role"].as_str().map(String::from))
    }

    fn can_edit(&self, club: u64, account: &str) -> bool {
        matches!(self.role_of(club, account).as_deref(), Some("Creator") | Some("Admin") | Some("Content_Creator"))
    }

    fn player_info(&self) -> Value {
        let st = self.st();
        let name = |id: u64| st.clubs.get(&id).and_then(|c| c["name"].as_str()).unwrap_or("").to_string();
        let tag = st.clubs.get(&st.tag_club).and_then(|c| c["tag"].as_str()).unwrap_or("").to_string();
        json!({"hasClubVip": false, "hasPlayerVip": false, "hasFollower": false, "tagClubId": st.tag_club, "tag": tag, "pinnedClub": st.pinned, "pinnedClubName": name(st.pinned)})
    }

    fn add_activity(&self, club: u64, kind: &str, b: &Value, me: &str, now: i64, campaign_id: u64) -> Value {
        let mut st = self.st();
        st.next_id += 1;
        let id = st.next_id;
        let position = st.activities.values().filter(|a| a["clubId"] == json!(club)).count();
        let club_name = st.clubs.get(&club).map(|c| c["name"].clone()).unwrap_or(json!(""));
        let mut a = json!({
            "id": id, "name": body_str(b, "name").unwrap_or_default(), "activityType": kind, "activityId": id, "targetActivityId": id, "campaignId": campaign_id,
            "position": position, "public": b.get("public").and_then(Value::as_bool).unwrap_or(true), "active": b.get("active").and_then(Value::as_bool).unwrap_or(true),
            "externalId": campaign_id, "featured": false, "password": false, "itemsCount": 0, "clubId": club, "clubName": club_name, "editionTimestamp": now, "creationTimestamp": now,
            "creatorAccountId": me, "latestEditorAccountId": me, "mediaTheme": "", "popularityLevel": 0, "folderId": b.get("folderId").cloned().unwrap_or(Value::Null),
        });
        if let Some(o) = a.as_object_mut() {
            o.extend(media_urls("mediaUrl"));
            if kind == "news" {
                o.insert("headline".into(), b.get("headline").cloned().unwrap_or(json!("")));
                o.insert("body".into(), b.get("body").cloned().unwrap_or(json!("")));
            }
        }
        st.activities.insert(id, a.clone());
        a
    }

    fn club_campaign(&self, club: u64, b: &Value, me: &str, now: i64, prev: Option<&Value>) -> Value {
        let mut st = self.st();
        let id = match prev.and_then(|p| p["id"].as_u64()) {
            Some(i) => i,
            None => {
                st.next_id += 1;
                st.next_id
            }
        };
        let season = prev.and_then(|p| p["seasonUid"].as_str().map(String::from)).unwrap_or_else(|| format!("NLS-{}", random_uuid().replace('-', "")));
        let playlist: Vec<Value> = b["playlist"].as_array().cloned().unwrap_or_default().iter().enumerate().map(|(i, p)| json!({"id": id * 100 + i as u64, "position": p.get("position").cloned().unwrap_or(json!(i)), "mapUid": p["mapUid"]})).collect();
        let name = body_str(b, "name").or_else(|| prev.and_then(|p| p["name"].as_str().map(String::from))).unwrap_or_default();
        let c = json!({
            "id": id, "seasonUid": season, "name": name, "color": "", "useCase": 2, "clubId": club, "leaderboardGroupUid": season, "publicationTimestamp": now,
            "startTimestamp": prev.map_or(json!(now), |p| p["startTimestamp"].clone()), "endTimestamp": 0, "rankingSentTimestamp": Value::Null, "year": -1, "week": -1, "day": -1,
            "monthYear": -1, "month": -1, "monthDay": -1, "published": true, "playlist": playlist, "editionTimestamp": now, "video": false, "mediaUrl": "",
            "categories": b.get("categories").cloned().unwrap_or(json!([{"name": name, "position": 0, "length": b["playlist"].as_array().map_or(0, |p| p.len())}])),
            "media": {"buttonBackgroundUrl": "", "buttonForegroundUrl": "", "decalUrl": "", "popUpBackgroundUrl": "", "popUpImageUrl": "", "liveButtonBackgroundUrl": "", "liveButtonForegroundUrl": ""},
            "latestSeasons": [{"uid": season, "name": name, "startTimestamp": now, "endTimestamp": 0, "relativeStart": 0, "relativeEnd": 0, "campaignId": id, "active": true}],
            "editorAccountId": me,
        });
        st.campaigns.insert(id, c.clone());
        c
    }

    fn club_campaign_entry(&self, a: &Value) -> Value {
        let st = self.st();
        let c = a["campaignId"].as_u64().and_then(|i| st.campaigns.get(&i)).cloned().unwrap_or(Value::Null);
        json!({"clubDecalUrl": "", "campaignId": a["campaignId"], "activityId": a["id"], "campaign": c, "popularityLevel": 0, "publicationTimestamp": a["creationTimestamp"],
            "creationTimestamp": a["creationTimestamp"], "creatorAccountId": a["creatorAccountId"], "latestEditorAccountId": a["latestEditorAccountId"], "id": a["id"], "clubId": a["clubId"],
            "clubName": a["clubName"], "name": a["name"], "mediaUrl": "", "mediaTheme": ""})
    }

    pub fn handle(&self, core: &Core, req: &Request) -> Option<Response> {
        let seg: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
        if seg.first() != Some(&"api") {
            return None;
        }
        let Some(claims) = core.bearer(req).filter(|c| !c.refresh) else {
            return Some(nadeo_error(401, "C-AA-00-01", "A valid nadeo_v1 access token is required.", Value::Null));
        };
        if claims.audience != "NadeoLiveServices" && claims.audience != "NadeoClubServices" {
            return Some(nadeo_error(401, "C-AA-00-01", "The token audience is not NadeoLiveServices.", Value::Null));
        }
        let s = &seg[1..];
        let s = if s.first() == Some(&"token") { &s[1..] } else { s };
        let m = req.method.as_str();
        let me = core.account_id.clone();
        let now = now_secs() as i64;
        let b = req.json();
        Some(match (m, s) {
            ("GET", ["campaign", "official"]) => {
                let all = list("official");
                let v2 = seg.get(1) != Some(&"token");
                let page: Vec<Value> = paged(req, &all, 1).iter().map(|c| if v2 { campaign_v2(c) } else { c.clone() }).collect();
                Response::json(&merge(json!({"itemCount": all.len(), "campaignList": page}), next_request(&all)))
            }
            ("GET", ["campaign", "month"]) => {
                let all = list("totd");
                let royal = req.query("royal").map_or(false, |r| r == "true");
                let all: Vec<Value> = all.into_iter().filter(|mo| mo.get("royal").and_then(Value::as_bool).unwrap_or(false) == royal).collect();
                let days: Vec<Value> = all.iter().flat_map(|mo| mo["days"].as_array().cloned().unwrap_or_default()).collect();
                let page: Vec<Value> = paged(req, &all, 1).into_iter().map(|mut mo| {
                    if let Some(ds) = mo["days"].as_array_mut() {
                        for d in ds {
                            let (st, en) = (d["startTimestamp"].as_i64().unwrap_or(0), d["endTimestamp"].as_i64().unwrap_or(0));
                            d["relativeStart"] = json!(st - now);
                            d["relativeEnd"] = json!(en - now);
                        }
                    }
                    mo
                }).collect();
                Response::json(&merge(json!({"monthList": page, "itemCount": all.len()}), next_request(&days)))
            }
            ("GET", ["campaign", kind @ ("cup-of-the-week" | "weekly-grands" | "weekly-shorts")]) => {
                let key = match *kind { "cup-of-the-week" => "cupOfTheWeek", "weekly-grands" => "weeklyGrands", _ => "weeklyShorts" };
                let all = list(key);
                Response::json(&merge(json!({"itemCount": all.len(), "campaignList": paged(req, &all, 1)}), next_request(&all)))
            }
            ("GET", ["campaign", "map", uid]) => {
                let mut v = json!({"officialYear": -1, "season": -1, "officialMaps": [], "totdYear": -1, "week": -1, "totdMaps": []});
                for c in list("official") {
                    let uids: Vec<Value> = c["playlist"].as_array().cloned().unwrap_or_default().iter().map(|p| p["mapUid"].clone()).collect();
                    if uids.contains(&json!(uid)) {
                        v["officialYear"] = c.get("year").filter(|y| y.as_i64() != Some(-1)).cloned().unwrap_or_else(|| json!(c["name"].as_str().and_then(|n| n.rsplit(' ').next()).and_then(|y| y.parse::<i64>().ok()).unwrap_or(-1)));
                        v["season"] = json!(c["name"].as_str().and_then(|n| n.split(' ').next()).unwrap_or(""));
                        v["officialMaps"] = json!(uids);
                        break;
                    }
                }
                for mo in list("totd") {
                    let days = mo["days"].as_array().cloned().unwrap_or_default();
                    if let Some(i) = days.iter().position(|d| d["mapUid"] == json!(uid)) {
                        let w = i / 7;
                        v["totdYear"] = mo["year"].clone();
                        v["week"] = json!(w as i64 + 1);
                        v["totdMaps"] = json!(days.iter().skip(w * 7).take(7).map(|d| d["mapUid"].clone()).collect::<Vec<_>>());
                        break;
                    }
                }
                Response::json(&v)
            }
            ("GET", ["advertising", "display", "active"]) => Response::json(&json!({"displayList": list("maniapubs")})),
            ("GET", ["club", "player", "info"]) => Response::json(&self.player_info()),
            ("GET", ["club", "mine"]) => {
                let st = self.st();
                let all: Vec<Value> = st.members.iter().filter(|((_, a), _)| a.eq_ignore_ascii_case(&me)).filter_map(|((c, _), mem)| st.clubs.get(c).map(|cl| merge(cl.clone(), json!({"role": mem["role"]})))).collect();
                drop(st);
                Response::json(&json!({"clubList": paged(req, &all, 25), "maxPage": all.len().div_ceil(qnum(req, "length", 25).max(1)), "clubCount": all.len()}))
            }
            ("GET", ["club"]) => {
                let name = req.query("name").unwrap_or("").to_ascii_lowercase();
                let all: Vec<Value> = self.st().clubs.values().filter(|c| c["state"].as_str().map_or(false, |s| s.starts_with("public")) || self.role_of(c["id"].as_u64().unwrap_or(0), &me).is_some()).filter(|c| name.is_empty() || c["name"].as_str().unwrap_or("").to_ascii_lowercase().contains(&name)).cloned().collect();
                Response::json(&json!({"clubList": paged(req, &all, 25), "maxPage": all.len().div_ceil(qnum(req, "length", 25).max(1)), "itemCount": all.len()}))
            }
            ("GET", ["club", "campaign"]) => {
                let name = req.query("name").unwrap_or("").to_ascii_lowercase();
                let acts: Vec<Value> = self.st().activities.values().filter(|a| a["activityType"] == json!("campaign") && a["public"] == json!(true)).filter(|a| name.is_empty() || a["name"].as_str().unwrap_or("").to_ascii_lowercase().contains(&name)).cloned().collect();
                let all: Vec<Value> = acts.iter().map(|a| self.club_campaign_entry(a)).collect();
                Response::json(&json!({"clubCampaignList": paged(req, &all, 25), "maxPage": all.len().div_ceil(qnum(req, "length", 25).max(1)), "itemCount": all.len()}))
            }
            ("POST", ["club", "create"]) => {
                if body_str(&b, "name").map_or(true, |n| n.trim().is_empty()) {
                    return Some(nadeo_error(400, "C-AA-00-03", "There was a validation error.", json!({"name": "The club name is required."})));
                }
                let mut st = self.st();
                st.next_id += 1;
                let id = st.next_id;
                drop(st);
                let c = self.club_obj(id, &b, &me, now);
                let mut st = self.st();
                st.clubs.insert(id, c.clone());
                st.members.insert((id, me.to_ascii_lowercase()), Self::member_obj(id, &me, "Creator", now));
                Response::json(&c)
            }
            (_, ["club", cid, rest @ ..]) => {
                let Ok(club) = cid.parse::<u64>() else { return None };
                let Some(c) = self.st().clubs.get(&club).cloned() else { return Some(nadeo_error(404, "C-AA-00-04", "Club not found.", Value::Null)) };
                return self.club_route(m, club, c, rest, req, &b, &me, &core.name, now);
            }
            ("GET", ["map", "favorite"]) => {
                let favs = self.st().favorites.clone();
                let all: Vec<Value> = favs.iter().filter_map(|u| core.map_by_uid(u)).filter(|mp| req.query("onlyMine") != Some("true") || mp["author"] == json!(me)).map(|mp| self.live_map(&mp)).collect();
                Response::json(&json!({"mapList": paged(req, &all, 25), "itemCount": all.len()}))
            }
            ("POST", ["map", "favorite", uid, op @ ("add" | "remove")]) => {
                if core.map_by_uid(uid).is_none() {
                    return Some(nadeo_error(404, "C-AA-00-04", "Map not found.", Value::Null));
                }
                let mut st = self.st();
                if *op == "add" { st.favorites.insert(uid.to_string()); } else { st.favorites.remove(*uid); }
                Response::json(&json!([]))
            }
            ("GET", ["map", "get-multiple"]) => {
                let uids: Vec<String> = req.query("mapUidList").unwrap_or("").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                if uids.len() > 100 {
                    return Some(nadeo_error(400, "C-AA-00-03", "There was a validation error.", json!({"mapUidList": "At most 100 map uids."})));
                }
                Response::json(&json!({"mapList": uids.iter().filter_map(|u| core.map_by_uid(u)).map(|mp| self.live_map(&mp)).collect::<Vec<_>>()}))
            }
            ("GET", ["map"]) => {
                let mut all: Vec<Value> = core.maps().into_iter().filter(|mp| mp["submitter"] == json!(me) || mp["author"] == json!(me)).map(|mp| self.live_map(&mp)).collect();
                all.sort_by_key(|mp| -mp["uploadTimestamp"].as_i64().unwrap_or(0));
                Response::json(&json!({"mapList": paged(req, &all, 25), "itemCount": all.len()}))
            }
            ("GET", ["map", uid]) => match core.map_by_uid(uid) {
                Some(mp) => Response::json(&self.live_map(&mp)),
                None => nadeo_error(404, "C-AA-00-04", "Map not found.", Value::Null),
            },
            ("POST", ["leaderboard", "group", "map"]) => {
                let maps = b["maps"].as_array().cloned().unwrap_or_default();
                let scores: Vec<(String, i64)> = req.query.iter().filter_map(|(k, v)| Some((k.strip_prefix("scores[")?.strip_suffix(']')?.to_string(), v.parse().ok()?))).collect();
                let out: Vec<Value> = maps.iter().filter_map(|e| {
                    let uid = e["mapUid"].as_str()?;
                    let group = e["groupUid"].as_str().unwrap_or(PB);
                    let mine = self.best(core, group, uid);
                    if !scores.is_empty() {
                        let score = scores.iter().find(|(u, _)| u == uid)?.1;
                        let pos = if mine.map_or(false, |(t, _)| t < score) { 2 } else { 1 };
                        return Some(json!({"groupUid": group, "mapUid": uid, "score": score, "zones": self.zones(core, pos)}));
                    }
                    let (t, ts) = mine?;
                    Some(json!({"groupUid": group, "mapUid": uid, "score": t, "timestamp": ts, "zones": self.zones(core, 1)}))
                }).collect();
                Response::json(&json!(out))
            }
            ("GET", ["leaderboard", "group", group, "map", uid, tail @ ..]) => {
                let mine = self.best(core, group, uid);
                let (zid, zname) = self.my_zone(core);
                let entry = |pos: i64| mine.map(|(t, ts)| json!({"accountId": me, "zoneId": zid, "zoneName": zname, "position": pos, "score": t, "timestamp": ts}));
                let only_world = req.query("onlyWorld").map_or(true, |v| v != "false");
                let chain = core.zone_chain(&me);
                let zones: Vec<(String, String)> = if only_world { chain.into_iter().take(1).collect() } else { chain };
                match tail {
                    ["top"] => {
                        let off = qnum(req, "offset", 0);
                        let tops: Vec<Value> = zones.iter().map(|(i, n)| json!({"zoneId": i, "zoneName": n, "top": if off == 0 && qnum(req, "length", 5) > 0 { entry(1).into_iter().collect::<Vec<_>>() } else { vec![] }})).collect();
                        Response::json(&json!({"groupUid": group, "mapUid": uid, "tops": tops}))
                    }
                    ["surround", lo, hi] => {
                        let (Ok(_), Ok(_)) = (lo.parse::<i64>(), hi.parse::<i64>()) else { return Some(nadeo_error(400, "C-AA-00-03", "There was a validation error.", json!({"range": "Invalid surround range."}))) };
                        let tops: Vec<Value> = zones.iter().map(|(i, n)| json!({"zoneId": i, "zoneName": n, "top": entry(1).into_iter().collect::<Vec<_>>()})).collect();
                        Response::json(&json!({"groupUid": group, "mapUid": uid, "tops": tops}))
                    }
                    ["medals"] => {
                        let map = core.map_by_uid(uid);
                        let medals: Vec<Value> = [("Author", "authorScore"), ("Gold", "goldScore"), ("Silver", "silverScore"), ("Bronze", "bronzeScore")].iter().filter_map(|(name, k)| {
                            let limit = map.as_ref()?[*k].as_i64()?;
                            let (t, _) = mine.filter(|(t, _)| *t <= limit)?;
                            Some(json!({"medal": name, "accountId": me, "zoneId": zid, "zoneName": zname, "score": t}))
                        }).collect();
                        Response::json(&json!({"groupUid": group, "mapUid": uid, "medals": medals}))
                    }
                    _ => return None,
                }
            }
            ("GET", ["leaderboard", "group", group]) => {
                let sp = self.campaign_points(core, group);
                Response::json(&json!({"groupUid": group, "sp": sp.to_string(), "zones": self.zones(core, if sp > 0 { 1 } else { 0 })}))
            }
            ("GET", ["leaderboard", "group", group, "top"]) => {
                let sp = self.campaign_points(core, group);
                let (zid, zname) = self.my_zone(core);
                let top: Vec<Value> = if sp > 0 && qnum(req, "offset", 0) == 0 { vec![json!({"accountId": me, "zoneId": zid, "zoneName": zname, "position": 1, "sp": sp.to_string()})] } else { vec![] };
                let world = core.zone_chain(&me).into_iter().next().unwrap_or_default();
                Response::json(&json!({"groupUid": group, "tops": [{"zoneId": world.0, "zoneName": world.1, "top": top}]}))
            }
            ("GET", ["leaderboard", "trophy"]) => {
                let pts = core.trophy_points();
                Response::json(&json!({"countPoint": pts, "zones": self.zones(core, 1), "accountId": me, "echelon": echelon(pts)}))
            }
            ("POST", ["leaderboard", "trophy", "player"]) => {
                let ids: Vec<String> = b["listPlayer"].as_array().cloned().unwrap_or_default().iter().filter_map(|p| p["accountId"].as_str().map(String::from)).collect();
                if ids.iter().any(|i| !is_uuid(i)) {
                    return Some(nadeo_error(400, "C-AA-00-03", "There was a validation error.", json!({"accountId": "Invalid account id."})));
                }
                let pts = core.trophy_points();
                let rankings: Vec<Value> = ids.iter().filter(|i| i.eq_ignore_ascii_case(&me)).map(|_| json!({"countPoint": pts, "zones": self.zones(core, 1), "accountId": me, "echelon": echelon(pts)})).collect();
                Response::json(&json!({"rankings": rankings, "length": rankings.len()}))
            }
            _ => return None,
        })
    }

    fn campaign_points(&self, core: &Core, group: &str) -> i64 {
        let mut uids: Vec<String> = Vec::new();
        for c in list("official").into_iter().chain(self.st().campaigns.values().cloned()) {
            if c["seasonUid"] == json!(group) || c["leaderboardGroupUid"] == json!(group) {
                uids.extend(c["playlist"].as_array().cloned().unwrap_or_default().iter().filter_map(|p| p["mapUid"].as_str().map(String::from)));
            }
        }
        uids.iter().filter(|u| self.best(core, group, u).is_some() || self.best(core, PB, u).is_some()).count() as i64 * 1000
    }

    #[allow(clippy::too_many_arguments)]
    fn club_route(&self, m: &str, club: u64, c: Value, rest: &[&str], req: &Request, b: &Value, me: &str, me_name: &str, now: i64) -> Option<Response> {
        let forbidden = || nadeo_error(403, "C-AA-00-02", "You do not have the rights for this club.", Value::Null);
        let ok_empty = || Response::json(&json!([]));
        Some(match (m, rest) {
            ("GET", []) => Response::json(&c),
            ("POST", ["edit"]) => {
                if !self.can_edit(club, me) {
                    return Some(forbidden());
                }
                let mut c = c;
                for k in ["name", "tag", "description", "state", "metadata"] {
                    if let Some(v) = body_str(b, k) {
                        c[k] = json!(v);
                    }
                }
                c["latestEditorAccountId"] = json!(me);
                c["editionTimestamp"] = json!(now);
                self.st().clubs.insert(club, c.clone());
                Response::json(&c)
            }
            ("POST", ["delete"]) => {
                if self.role_of(club, me).as_deref() != Some("Creator") {
                    return Some(forbidden());
                }
                let mut st = self.st();
                st.clubs.remove(&club);
                st.members.retain(|(cl, _), _| *cl != club);
                st.activities.retain(|_, a| a["clubId"] != json!(club));
                if st.pinned == club { st.pinned = 0; }
                if st.tag_club == club { st.tag_club = 0; }
                ok_empty()
            }
            ("POST", ["pin"]) | ("POST", ["tag"]) => {
                if self.role_of(club, me).is_none() {
                    return Some(forbidden());
                }
                let mut st = self.st();
                if rest == ["pin"] { st.pinned = club; } else { st.tag_club = club; }
                let (pinned, tc) = (st.pinned, st.tag_club);
                let tag = st.clubs.get(&tc).and_then(|x| x["tag"].as_str()).unwrap_or("").to_string();
                Response::json(&json!({"accountId": me, "tag": tag, "tagClubId": tc, "pinnedClub": pinned}))
            }
            ("GET", ["member"]) => {
                let all: Vec<Value> = self.st().members.iter().filter(|((cl, _), _)| *cl == club).map(|(_, v)| v.clone()).collect();
                let len = qnum(req, "length", 25).max(1);
                Response::json(&json!({"clubMemberList": paged(req, &all, 25), "maxPage": all.len().div_ceil(len), "itemCount": all.len()}))
            }
            ("POST", ["member", "create"]) => {
                if let Some(mem) = self.st().members.get(&(club, me.to_ascii_lowercase())).cloned() {
                    return Some(Response::json(&mem));
                }
                if c["state"].as_str().map_or(true, |s| !s.ends_with("open")) {
                    return Some(forbidden());
                }
                let mem = Self::member_obj(club, me, "Member", now);
                self.st().members.insert((club, me.to_ascii_lowercase()), mem.clone());
                Response::json(&mem)
            }
            ("GET", ["member", who]) | ("GET", ["member", who, "from-login"]) => {
                let account = if rest.len() == 3 { if who.eq_ignore_ascii_case(me_name) { me.to_string() } else { String::new() } } else { who.to_string() };
                let st = self.st();
                match st.members.get(&(club, account.to_ascii_lowercase())).cloned() {
                    Some(v) => Response::json(&v),
                    None => nadeo_error(404, "C-AA-00-04", "Member not found.", Value::Null),
                }
            }
            ("POST", ["member", who, "delete"]) => {
                if !who.eq_ignore_ascii_case(me) && !self.can_edit(club, me) {
                    return Some(forbidden());
                }
                self.st().members.remove(&(club, who.to_ascii_lowercase()));
                ok_empty()
            }
            ("GET", ["activity"]) => {
                let active = req.query("active").map(|v| v == "true");
                let folder = req.query("folderId").and_then(|f| f.parse::<u64>().ok());
                let all: Vec<Value> = self.st().activities.values().filter(|a| a["clubId"] == json!(club) && active.map_or(true, |x| a["active"] == json!(x)) && folder.map_or(true, |f| a["folderId"] == json!(f))).cloned().collect();
                Response::json(&json!({"activityList": paged(req, &all, 25), "maxPage": all.len().div_ceil(qnum(req, "length", 25).max(1)), "itemCount": all.len()}))
            }
            ("POST", ["activity", aid, op @ ("edit" | "delete")]) => {
                if !self.can_edit(club, me) {
                    return Some(forbidden());
                }
                let Ok(aid) = aid.parse::<u64>() else { return None };
                let mut st = self.st();
                let Some(a) = st.activities.get_mut(&aid).filter(|a| a["clubId"] == json!(club)) else { return Some(nadeo_error(404, "C-AA-00-04", "Activity not found.", Value::Null)) };
                if *op == "delete" {
                    st.activities.remove(&aid);
                    return Some(ok_empty());
                }
                for k in ["name", "public", "active", "position", "featured", "folderId"] {
                    if let Some(v) = b.get(k) {
                        a[k] = v.clone();
                    }
                }
                a["editionTimestamp"] = json!(now);
                a["latestEditorAccountId"] = json!(me);
                Response::json(a)
            }
            ("POST", [kind @ ("news" | "folder"), "create"]) => {
                if !self.can_edit(club, me) {
                    return Some(forbidden());
                }
                Response::json(&self.add_activity(club, kind, b, me, now, 0))
            }
            ("GET", ["news", aid]) => {
                let Ok(aid) = aid.parse::<u64>() else { return None };
                match self.st().activities.get(&aid).filter(|a| a["clubId"] == json!(club) && a["activityType"] == json!("news")) {
                    Some(a) => Response::json(a),
                    None => nadeo_error(404, "C-AA-00-04", "News not found.", Value::Null),
                }
            }
            ("POST", ["campaign", "create"]) => {
                if !self.can_edit(club, me) {
                    return Some(forbidden());
                }
                let camp = self.club_campaign(club, b, me, now, None);
                let mut a = self.add_activity(club, "campaign", b, me, now, camp["id"].as_u64().unwrap_or(0));
                a["campaign"] = camp;
                Response::json(&a)
            }
            ("GET", ["campaign"]) => {
                let acts: Vec<Value> = self.st().activities.values().filter(|a| a["clubId"] == json!(club) && a["activityType"] == json!("campaign")).cloned().collect();
                let all: Vec<Value> = acts.iter().map(|a| self.club_campaign_entry(a)).collect();
                Response::json(&json!({"clubCampaignList": paged(req, &all, 25), "maxPage": all.len().div_ceil(qnum(req, "length", 25).max(1)), "itemCount": all.len()}))
            }
            ("GET", ["campaign", cid]) | ("POST", ["campaign", cid, "edit"]) => {
                let Ok(cid) = cid.parse::<u64>() else { return None };
                let act = self.st().activities.values().find(|a| a["clubId"] == json!(club) && a["campaignId"] == json!(cid)).cloned();
                let Some(act) = act else { return Some(nadeo_error(404, "C-AA-00-04", "Campaign not found.", Value::Null)) };
                if m == "POST" {
                    if !self.can_edit(club, me) {
                        return Some(forbidden());
                    }
                    let prev = self.st().campaigns.get(&cid).cloned();
                    self.club_campaign(club, b, me, now, prev.as_ref());
                }
                Response::json(&self.club_campaign_entry(&act))
            }
            _ => return None,
        })
    }
}

fn echelon(points: i64) -> i64 {
    [100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000, i64::MAX].iter().position(|t| points < *t).unwrap_or(9) as i64
}
