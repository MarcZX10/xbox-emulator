use super::super::{util, Request, Response};
use super::{data, Fortnite};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
pub struct Store {
    profiles: Mutex<HashMap<String, Value>>,
}

fn base_profile(fnt: &Fortnite, id: &str) -> Value {
    let mut p: Value = data::profile(id).and_then(|s| serde_json::from_str(s).ok()).unwrap_or_else(|| json!({
        "created": "2020-01-01T00:00:00.000Z",
        "updated": "2020-01-01T00:00:00.000Z",
        "rvn": 0,
        "wipeNumber": 1,
        "profileId": id,
        "version": "no_version",
        "items": {},
        "stats": {"attributes": {}},
        "commandRevision": 0,
    }));
    p["_id"] = json!(fnt.account_id);
    p["accountId"] = json!(fnt.account_id);
    p["profileId"] = json!(id);
    for key in ["rvn", "commandRevision"] {
        if !p[key].is_i64() && !p[key].is_u64() {
            p[key] = json!(0);
        }
    }
    if !p["items"].is_object() {
        p["items"] = json!({});
    }
    if !p["stats"].is_object() {
        p["stats"] = json!({});
    }
    if !p["stats"]["attributes"].is_object() {
        p["stats"]["attributes"] = json!({});
    }
    p
}

fn as_i64(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn bump(p: &mut Value) {
    p["rvn"] = json!(as_i64(&p["rvn"]) + 1);
    p["commandRevision"] = json!(as_i64(&p["commandRevision"]) + 1);
    p["updated"] = json!(util::iso_now());
}

fn attrs(p: &mut Value) -> &mut Map<String, Value> {
    p["stats"]["attributes"].as_object_mut().expect("attributes object")
}

fn item_attrs<'a>(p: &'a mut Value, id: &str) -> Option<&'a mut Map<String, Value>> {
    let item = p.get_mut("items")?.get_mut(id)?;
    if !item.get("attributes").map_or(false, Value::is_object) {
        item["attributes"] = json!({});
    }
    item.get_mut("attributes")?.as_object_mut()
}

fn attr_changed(id: &str, name: &str, value: Value) -> Value {
    json!({"changeType": "itemAttrChanged", "itemId": id, "attributeName": name, "attributeValue": value})
}

fn stat_modified(name: &str, value: Value) -> Value {
    json!({"changeType": "statModified", "name": name, "value": value})
}

fn str_of<'a>(b: &'a Value, k: &str) -> &'a str {
    b.get(k).and_then(Value::as_str).unwrap_or("")
}

fn quests_s14() -> &'static Value {
    static Q: OnceLock<Value> = OnceLock::new();
    Q.get_or_init(|| serde_json::from_str(data::QUESTS_ATHENA_S14).unwrap_or(json!({})))
}

fn client_quest_login(p: &mut Value, season: u32, changes: &mut Vec<Value>) -> bool {
    if p["profileId"] != "athena" {
        return false;
    }
    let mut changed = false;
    let prefix = format!("QS{season:02}-");
    let stale: Vec<String> = p["items"].as_object().map(|m| m.keys().filter(|k| {
        let b = k.as_bytes();
        b.len() > 4 && k.starts_with("QS") && b[2].is_ascii_digit() && b[3].is_ascii_digit() && b[4] == b'-' && !k.starts_with(&prefix)
    }).cloned().collect()).unwrap_or_default();
    for k in stale {
        p["items"].as_object_mut().map(|m| m.remove(&k));
        changes.push(json!({"changeType": "itemRemoved", "itemId": k}));
        changed = true;
    }
    if season != 14 {
        return changed;
    }
    let q = quests_s14();
    let now = util::iso_now();
    let mut add = |p: &mut Value, id: &str, item: Value, changes: &mut Vec<Value>| {
        if p["items"].get(id).is_none() {
            p["items"][id] = item.clone();
            changes.push(json!({"changeType": "itemAdded", "itemId": id, "item": item}));
            changed = true;
        }
    };
    if let Some(sched) = q["ChallengeBundleSchedules"].as_object() {
        for (id, s) in sched {
            add(p, id, json!({"templateId": s["templateId"], "attributes": {
                "unlock_epoch": now, "max_level_bonus": 0, "level": 1, "item_seen": true, "xp": 0, "favorite": false,
                "granted_bundles": s["granted_bundles"],
            }, "quantity": 1}), changes);
        }
    }
    let mut quest_ids = Vec::new();
    if let Some(bundles) = q["ChallengeBundles"].as_object() {
        for (id, b) in bundles {
            let granted = b["grantedquestinstanceids"].as_array().cloned().unwrap_or_default();
            quest_ids.extend(granted.iter().filter_map(|v| v.as_str().map(String::from)));
            add(p, id, json!({"templateId": b["templateId"], "attributes": {
                "has_unlock_by_completion": false, "num_quests_completed": 0, "level": 0,
                "grantedquestinstanceids": granted, "item_seen": true, "max_allowed_bundle_level": 0,
                "num_granted_bundle_quests": granted.len(), "max_level_bonus": 0,
                "challenge_bundle_schedule_id": b["challenge_bundle_schedule_id"],
                "num_progress_quests_completed": 0, "xp": 0, "favorite": false,
            }, "quantity": 1}), changes);
        }
    }
    for id in quest_ids {
        let Some(quest) = q["Quests"].get(&id) else { continue };
        let mut a = json!({
            "creation_time": now, "level": -1, "item_seen": true, "sent_new_notification": true,
            "challenge_bundle_id": quest.get("challenge_bundle_id").cloned().unwrap_or(json!("")),
            "xp_reward_scalar": 1, "quest_state": "Active", "last_state_change_time": now,
            "max_level_bonus": 0, "xp": 0, "favorite": false,
        });
        if let Some(obj) = quest["objectives"].as_object() {
            for k in obj.keys() {
                a[format!("completion_{k}")] = json!(0);
            }
        }
        add(p, &id, json!({"templateId": quest["templateId"], "attributes": a, "quantity": 1}), changes);
    }
    changed
}

fn equip_br(p: &mut Value, b: &Value, changes: &mut Vec<Value>) -> bool {
    let slot = str_of(b, "slotName").to_string();
    let item = str_of(b, "itemToSlot").to_string();
    let a = attrs(p);
    if !a.get("favorite_dance").map_or(false, Value::is_array) {
        a.insert("favorite_dance".into(), json!(["", "", "", "", "", ""]));
    }
    if !a.get("favorite_itemwraps").map_or(false, Value::is_array) {
        a.insert("favorite_itemwraps".into(), json!(["", "", "", "", "", "", ""]));
    }
    let idx = b.get("indexWithinSlot").and_then(Value::as_i64).unwrap_or(0);
    let category = match slot.as_str() {
        "Character" | "Backpack" | "Pickaxe" | "Glider" | "SkyDiveContrail" | "MusicPack" | "LoadingScreen" => {
            let k = format!("favorite_{}", slot.to_ascii_lowercase());
            a.insert(k.clone(), json!(item));
            k
        }
        "Dance" => {
            if let Some(arr) = a.get_mut("favorite_dance").and_then(Value::as_array_mut) {
                if idx >= 0 && (idx as usize) < arr.len() {
                    arr[idx as usize] = json!(item);
                }
            }
            "favorite_dance".into()
        }
        "ItemWrap" => {
            if let Some(arr) = a.get_mut("favorite_itemwraps").and_then(Value::as_array_mut) {
                if idx < 0 {
                    for v in arr.iter_mut() {
                        *v = json!(item);
                    }
                } else if (idx as usize) < arr.len() {
                    arr[idx as usize] = json!(item);
                }
            }
            "favorite_itemwraps".into()
        }
        _ => return false,
    };
    let value = a.get(&category).cloned().unwrap_or(Value::Null);
    changes.push(stat_modified(&category, value));
    if let Some(updates) = b.get("variantUpdates").and_then(Value::as_array).filter(|u| !u.is_empty()) {
        if let Some(ia) = item_attrs(p, &item) {
            let variants = ia.entry("variants").or_insert(json!([]));
            if variants.as_array().map_or(true, |v| v.is_empty()) {
                *variants = json!(updates);
            }
            if let Some(arr) = variants.as_array_mut() {
                for (i, v) in arr.iter_mut().enumerate() {
                    if let Some(u) = updates.get(i) {
                        if str_of(v, "channel").eq_ignore_ascii_case(str_of(u, "channel")) {
                            v["active"] = u.get("active").cloned().unwrap_or(json!(""));
                        }
                    }
                }
            }
            let val = ia["variants"].clone();
            changes.push(attr_changed(&item, "variants", val));
        }
    }
    true
}

fn locker_slot(p: &mut Value, b: &Value, changes: &mut Vec<Value>) -> bool {
    let locker = str_of(b, "lockerItem").to_string();
    let category = str_of(b, "category").to_string();
    let item = str_of(b, "itemToSlot").to_string();
    if locker.is_empty() || category.is_empty() {
        return false;
    }
    let updates = b.get("variantUpdates").and_then(Value::as_array).cloned().unwrap_or_default();
    if !updates.is_empty() && p["profileId"] == "athena" {
        if let Some(ia) = item_attrs(p, &item) {
            let variants = ia.entry("variants").or_insert(json!([]));
            if let Some(arr) = variants.as_array_mut() {
                for u in &updates {
                    match arr.iter_mut().find(|v| v.get("channel") == u.get("channel")) {
                        Some(v) => v["active"] = u.get("active").cloned().unwrap_or(Value::Null),
                        None => arr.push(u.clone()),
                    }
                }
            }
            let val = ia["variants"].clone();
            changes.push(attr_changed(&item, "variants", val));
        }
    }
    let Some(la) = item_attrs(p, &locker) else { return false };
    let slots = la.entry("locker_slots_data").or_insert(json!({"slots": {}}));
    if !slots["slots"].is_object() {
        slots["slots"] = json!({});
    }
    let slot = &mut slots["slots"][category.as_str()];
    if !slot.is_object() {
        *slot = json!({"items": []});
    }
    if !slot["items"].is_array() {
        slot["items"] = json!([]);
    }
    if !updates.is_empty() {
        let av: Vec<Value> = updates.iter().map(|u| json!({"channel": u.get("channel"), "active": u.get("active")})).collect();
        slot["activeVariants"] = json!([{"variants": av}]);
    }
    let idx = b.get("slotIndex").and_then(Value::as_i64).unwrap_or(0);
    let items = slot["items"].as_array_mut().expect("items array");
    match category.as_str() {
        "Character" | "Backpack" | "Pickaxe" | "Glider" | "SkyDiveContrail" | "MusicPack" | "LoadingScreen" => {
            *items = vec![json!(item)];
        }
        "Dance" | "ItemWrap" => {
            let len = if category == "Dance" { 6 } else { 7 };
            while items.len() < len {
                items.push(json!(""));
            }
            if idx < 0 && category == "ItemWrap" {
                for v in items.iter_mut() {
                    *v = json!(item);
                }
            } else if idx >= 0 && (idx as usize) < items.len() {
                items[idx as usize] = json!(item);
            }
        }
        _ => {
            if idx >= 0 {
                while items.len() <= idx as usize {
                    items.push(json!(""));
                }
                items[idx as usize] = json!(item);
            }
        }
    }
    let val = la["locker_slots_data"].clone();
    changes.push(attr_changed(&locker, "locker_slots_data", val));
    true
}

fn set_item_attr(p: &mut Value, id: &str, name: &str, value: Value, changes: &mut Vec<Value>) -> bool {
    match item_attrs(p, id) {
        Some(a) => {
            a.insert(name.into(), value.clone());
            changes.push(attr_changed(id, name, value));
            true
        }
        None => false,
    }
}

fn run_op(op: &str, p: &mut Value, b: &Value, season: u32, changes: &mut Vec<Value>) -> Option<bool> {
    Some(match op {
        "QueryProfile" | "QueryPublicProfile" => false,
        "ClientQuestLogin" => client_quest_login(p, season, changes),
        "SetMtxPlatform" => {
            let plat = b.get("newPlatform").and_then(Value::as_str).unwrap_or("EpicPC").to_string();
            attrs(p).insert("current_mtx_platform".into(), json!(plat));
            changes.push(stat_modified("current_mtx_platform", json!(plat)));
            true
        }
        "SetReceiveGiftsEnabled" => {
            let v = b.get("bReceiveGifts").and_then(Value::as_bool).unwrap_or(true);
            attrs(p).insert("allowed_to_receive_gifts".into(), json!(v));
            changes.push(stat_modified("allowed_to_receive_gifts", json!(v)));
            true
        }
        "EquipBattleRoyaleCustomization" => equip_br(p, b, changes),
        "SetCosmeticLockerSlot" => locker_slot(p, b, changes),
        "SetBattleRoyaleBanner" => {
            let (icon, color) = (str_of(b, "homebaseBannerIconId").to_string(), str_of(b, "homebaseBannerColorId").to_string());
            if icon.is_empty() || color.is_empty() {
                return Some(false);
            }
            attrs(p).insert("banner_icon".into(), json!(icon));
            attrs(p).insert("banner_color".into(), json!(color));
            changes.push(stat_modified("banner_icon", json!(icon)));
            changes.push(stat_modified("banner_color", json!(color)));
            true
        }
        "SetCosmeticLockerBanner" => {
            let locker = str_of(b, "lockerItem").to_string();
            let icon = json!(str_of(b, "bannerIconTemplateName"));
            let color = json!(str_of(b, "bannerColorTemplateName"));
            let a = set_item_attr(p, &locker, "banner_icon_template", icon, changes);
            a && set_item_attr(p, &locker, "banner_color_template", color, changes)
        }
        "MarkItemSeen" => {
            let mut any = false;
            for id in b.get("itemIds").and_then(Value::as_array).cloned().unwrap_or_default() {
                if let Some(id) = id.as_str() {
                    any |= set_item_attr(p, id, "item_seen", json!(true), changes);
                }
            }
            any
        }
        "SetItemFavoriteStatus" => {
            let id = str_of(b, "targetItemId").to_string();
            let fav = b.get("bFavorite").and_then(Value::as_bool).unwrap_or(false);
            set_item_attr(p, &id, "favorite", json!(fav), changes)
        }
        "SetItemFavoriteStatusBatch" => {
            let favs = b.get("itemFavStatus").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut any = false;
            for (i, id) in b.get("itemIds").and_then(Value::as_array).cloned().unwrap_or_default().iter().enumerate() {
                if let Some(id) = id.as_str() {
                    let fav = favs.get(i).and_then(Value::as_bool).unwrap_or(false);
                    any |= set_item_attr(p, id, "favorite", json!(fav), changes);
                }
            }
            any
        }
        "SetItemArchivedStatusBatch" => {
            let archived = b.get("archived").and_then(Value::as_bool).unwrap_or(false);
            let mut any = false;
            for id in b.get("itemIds").and_then(Value::as_array).cloned().unwrap_or_default() {
                if let Some(id) = id.as_str() {
                    any |= set_item_attr(p, id, "archived", json!(archived), changes);
                }
            }
            any
        }
        _ => return None,
    })
}

fn with_full_profile(env: &Value, profile: &Value) -> Vec<u8> {
    const MARK: &str = "\u{1}fullProfileUpdate\u{1}";
    let mut e = env.clone();
    e["profileChanges"] = json!(MARK);
    let head = serde_json::to_vec(&e).unwrap_or_default();
    let mark = serde_json::to_vec(&json!(MARK)).unwrap_or_default();
    let Some(at) = head.windows(mark.len()).position(|w| w == mark.as_slice()) else { return head };
    let mut out = Vec::with_capacity(head.len() + 64 * 1024);
    out.extend_from_slice(&head[..at]);
    out.extend_from_slice(br#"[{"changeType":"fullProfileUpdate","profile":"#);
    if serde_json::to_writer(&mut out, profile).is_err() {
        return head;
    }
    out.extend_from_slice(b"}]");
    out.extend_from_slice(&head[at + mark.len()..]);
    out
}
pub fn handle(fnt: &Fortnite, req: &Request, route_kind: &str, op: &str) -> Response {
    let Some(profile_id) = req.query("profileId").map(String::from) else {
        return Response::json_status(404, &json!({"error": "Profile not defined."}));
    };
    let query_rvn: i64 = req.query("rvn").and_then(|v| v.parse().ok()).unwrap_or(-1);
    let season = Fortnite::season(req);
    let body = req.json();
    let mut store = fnt.mcp.profiles.lock().unwrap_or_else(|e| e.into_inner());
    let p = store.entry(profile_id.clone()).or_insert_with(|| base_profile(fnt, &profile_id));
    if profile_id == "athena" {
        attrs(p).insert("season_num".into(), json!(season));
    }
    let base_rvn = as_i64(&p["rvn"]);
    let mut changes = Vec::new();
    let known = if route_kind == "client" {
        match run_op(op, p, &body, season, &mut changes) {
            Some(changed) => {
                if changed {
                    bump(p);
                }
                true
            }
            None => {
                crate::record_raw("online", "mcp-unhandled", &format!("fortnite {route_kind}/{op} profileId={profile_id}"));
                false
            }
        }
    } else {
        true
    };
    let full = known && query_rvn != base_rvn;
    let env = json!({
        "profileRevision": as_i64(&p["rvn"]),
        "profileId": profile_id,
        "profileChangesBaseRevision": base_rvn,
        "profileChanges": if full { json!([{"changeType": "fullProfileUpdate"}]) } else { json!(changes) },
        "profileCommandRevision": as_i64(&p["commandRevision"]),
        "serverTime": util::iso_now(),
        "responseVersion": 1,
    });
    let bytes = if full { with_full_profile(&env, p) } else { serde_json::to_vec(&env).unwrap_or_default() };
    drop(store);
    Response::raw_json(200, bytes)
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    match (m, seg) {
        ("POST", ["fortnite", "api", "game", "v2", "profile", _, kind, op]) => Some(handle(fnt, req, kind, op)),
        _ => None,
    }
}
