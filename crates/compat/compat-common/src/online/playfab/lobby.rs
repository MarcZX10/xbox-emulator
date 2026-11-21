use super::super::util::{iso_now, now_millis, random_uuid};
use super::{err, invalid, ok, st, Entity};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Lobbies {
    lobbies: BTreeMap<String, Value>,
    invites: Vec<Value>,
    subscriptions: BTreeMap<String, Value>,
    tickets: BTreeMap<String, Value>,
    matches: BTreeMap<String, Value>,
}

fn lobby_missing() -> super::Response {
    err(404, "LobbyDoesNotExist", 11000, "The lobby does not exist.")
}

fn key_of(v: &Value) -> Option<(String, String)> {
    Some((v["Id"].as_str()?.to_string(), v["Type"].as_str().unwrap_or("title_player_account").to_string()))
}

fn same(a: &Value, b: &Value) -> bool {
    key_of(a).map(|x| x.0) == key_of(b).map(|x| x.0) && key_of(a).is_some()
}

fn caller_key(e: &Entity) -> Value {
    json!({"Id": e.id, "Type": e.kind})
}

fn merge(dst: &mut Value, src: &Value, delete: &Value, limit: usize) -> Result<(), String> {
    if !dst.is_object() {
        *dst = json!({});
    }
    let o = dst.as_object_mut().expect("object");
    if let Some(m) = src.as_object() {
        for (k, v) in m {
            o.insert(k.clone(), v.clone());
        }
    }
    for k in delete.as_array().cloned().unwrap_or_default() {
        if let Some(k) = k.as_str() {
            o.remove(k);
        }
    }
    if o.len() > limit {
        return Err(format!("at most {limit} keys are allowed"));
    }
    Ok(())
}

fn check_search_keys(v: &Value) -> Result<(), String> {
    for k in v.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let ok_key = ["string_key", "number_key"].iter().any(|p| k.strip_prefix(p).and_then(|n| n.parse::<u32>().ok()).map_or(false, |n| (1..=30).contains(&n)));
        if !ok_key {
            return Err(format!("search key '{k}' must be string_key1..30 or number_key1..30"));
        }
    }
    Ok(())
}

fn connection_string(id: &str) -> String {
    format!("{id}|tombox|{}", &random_uuid()[..8])
}

fn view(l: &Value) -> Value {
    let mut v = l.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("PreventRejoin");
        o.remove("Arrangement");
    }
    v
}

fn summary(l: &Value) -> Value {
    json!({
        "LobbyId": l["LobbyId"], "ConnectionString": l["ConnectionString"], "Owner": l["Owner"], "MaxPlayers": l["MaxPlayers"],
        "CurrentPlayers": l["Members"].as_array().map_or(0, |m| m.len()), "SearchData": l["SearchData"],
    })
}

fn joinable(l: &Value, who: &Value) -> Result<(), super::Response> {
    if l["Members"].as_array().map_or(false, |m| m.iter().any(|x| same(&x["MemberEntity"], who))) {
        return Err(err(400, "LobbyPlayerAlreadyJoined", 11002, "The player is already a member of the lobby."));
    }
    if l["PreventRejoin"].as_array().map_or(false, |p| p.iter().any(|x| same(x, who))) {
        return Err(err(403, "LobbyPlayerNotAllowedToJoin", 11003, "The player was removed and may not rejoin."));
    }
    if l["MembershipLock"] == "Locked" {
        return Err(err(400, "LobbyNotJoinable", 11004, "The lobby membership is locked."));
    }
    if l["Members"].as_array().map_or(0, |m| m.len()) as u64 >= l["MaxPlayers"].as_u64().unwrap_or(0) {
        return Err(err(400, "LobbyPlayerMaxPlayersExceeded", 11005, "The lobby is full."));
    }
    Ok(())
}

fn bump(l: &mut Value) {
    l["ChangeNumber"] = json!(l["ChangeNumber"].as_u64().unwrap_or(0) + 1);
}

fn eval_filter(filter: &str, sd: &Value) -> Result<bool, String> {
    let f = filter.trim();
    if f.is_empty() {
        return Ok(true);
    }
    for clause in split_and(f) {
        let parts: Vec<&str> = clause.trim().splitn(3, ' ').collect();
        if parts.len() != 3 {
            return Err(format!("cannot parse filter clause '{clause}'"));
        }
        let (key, op, lit) = (parts[0], parts[1].to_ascii_lowercase(), parts[2].trim());
        let actual = &sd[key];
        let pass = if let Some(s) = lit.strip_prefix('\'').and_then(|l| l.strip_suffix('\'')) {
            let a = actual.as_str();
            match op.as_str() {
                "eq" => a == Some(s),
                "ne" => a != Some(s),
                _ => return Err(format!("operator '{op}' is not valid for strings")),
            }
        } else {
            let want: f64 = lit.parse().map_err(|_| format!("bad literal '{lit}'"))?;
            let a = actual.as_f64().or_else(|| actual.as_str().and_then(|s| s.parse().ok()));
            match (op.as_str(), a) {
                (_, None) => false,
                ("eq", Some(a)) => a == want,
                ("ne", Some(a)) => a != want,
                ("lt", Some(a)) => a < want,
                ("le", Some(a)) => a <= want,
                ("gt", Some(a)) => a > want,
                ("ge", Some(a)) => a >= want,
                _ => return Err(format!("unknown operator '{op}'")),
            }
        };
        if !pass {
            return Ok(false);
        }
    }
    Ok(true)
}

fn split_and(f: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let words: Vec<&str> = f.split(' ').collect();
    for w in words {
        if !quoted && w.eq_ignore_ascii_case("and") {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        quoted ^= w.matches('\'').count() % 2 == 1;
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    out.push(cur);
    out
}

fn find(b: &Value, me: &Entity, friends_only: bool) -> super::Response {
    let filter = b["Filter"].as_str().unwrap_or("");
    let size = b["Pagination"]["PageSizeRequested"].as_u64().unwrap_or(50).clamp(1, 50) as usize;
    let skip = b["Pagination"]["ContinuationToken"].as_str().and_then(|t| t.parse::<usize>().ok()).unwrap_or(0);
    let friends: Vec<String> = crate::gdk::social::friends().into_iter().map(|f| super::title_player_for_xuid(&super::title_id(), f.0)).collect();
    let s = st();
    let mut hits = Vec::new();
    for l in s.lobby.lobbies.values() {
        let public = l["AccessPolicy"].as_str().unwrap_or("Public");
        let mine = l["Members"].as_array().map_or(false, |m| m.iter().any(|x| x["MemberEntity"]["Id"] == json!(me.id)));
        if friends_only {
            let has_friend = l["Members"].as_array().map_or(false, |m| m.iter().any(|x| x["MemberEntity"]["Id"].as_str().map_or(false, |i| friends.iter().any(|f| f == i))));
            if !has_friend || public == "Private" {
                continue;
            }
        } else if public != "Public" && !mine {
            continue;
        }
        match eval_filter(filter, &l["SearchData"]) {
            Ok(true) => hits.push(summary(l)),
            Ok(false) => {}
            Err(e) => return invalid("Filter", &e),
        }
    }
    if let Some(order) = b["OrderBy"].as_str().filter(|o| !o.trim().is_empty()) {
        let mut p = order.split_whitespace();
        let key = p.next().unwrap_or("").to_string();
        let desc = p.next().map_or(false, |d| d.eq_ignore_ascii_case("desc"));
        hits.sort_by(|a, c| {
            let x = a["SearchData"][&key].as_f64().unwrap_or(0.0);
            let y = c["SearchData"][&key].as_f64().unwrap_or(0.0);
            let o = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
            if desc { o.reverse() } else { o }
        });
    }
    let total = hits.len();
    let page: Vec<Value> = hits.into_iter().skip(skip).take(size).collect();
    let next = (skip + page.len() < total).then(|| (skip + page.len()).to_string());
    ok(json!({"Lobbies": page, "Pagination": {"ContinuationToken": next, "TotalMatchedLobbyCount": total}}))
}

fn member(b: &Value, me: &Entity) -> Value {
    json!({"MemberEntity": if b["MemberEntity"].is_object() { b["MemberEntity"].clone() } else { caller_key(me) }, "MemberData": if b["MemberData"].is_object() { b["MemberData"].clone() } else { json!({}) }, "PubSubConnectionHandle": b["PubSubConnectionHandle"].clone()})
}

fn acting_for_self(b: &Value, me: &Entity) -> Result<Value, super::Response> {
    let who = if b["MemberEntity"].is_object() { b["MemberEntity"].clone() } else { caller_key(me) };
    if who["Id"] != json!(me.id) {
        return Err(err(403, "NotAuthorized", 1089, "The caller may only act for its own entity."));
    }
    Ok(who)
}

fn ticket_view(t: &Value) -> Value {
    let mut v = t.clone();
    let created = v["CreatedMs"].as_u64().unwrap_or(0);
    let give_up = v["GiveUpAfterSeconds"].as_u64().unwrap_or(0);
    if v["Status"] == "WaitingForMatch" && give_up > 0 && now_millis() > created + give_up * 1000 {
        v["Status"] = json!("Canceled");
        v["CancellationReasonString"] = json!("Timeout");
    }
    if let Some(o) = v.as_object_mut() {
        o.remove("CreatedMs");
    }
    v
}

fn try_match(queue: &str) -> Option<String> {
    let mut s = st();
    let now = now_millis();
    let waiting: Vec<String> = s.lobby.tickets.iter().filter(|(_, t)| t["QueueName"] == json!(queue) && t["Status"] == "WaitingForMatch" && {
        let g = t["GiveUpAfterSeconds"].as_u64().unwrap_or(0);
        g == 0 || now <= t["CreatedMs"].as_u64().unwrap_or(0) + g * 1000
    }).map(|(k, _)| k.clone()).collect();
    if waiting.is_empty() {
        return None;
    }
    let mid = random_uuid();
    let mut members = Vec::new();
    for id in waiting.iter() {
        if let Some(t) = s.lobby.tickets.get_mut(id) {
            t["Status"] = json!("Matched");
            t["MatchId"] = json!(mid);
            for m in t["Members"].as_array().cloned().unwrap_or_default() {
                members.push(json!({"Entity": m["Entity"], "Attributes": m["Attributes"], "TeamId": Value::Null}));
            }
        }
    }
    s.lobby.matches.insert(mid.clone(), json!({"MatchId": mid, "Members": members, "RegionPreferences": super::QOS_REGIONS, "ArrangementString": format!("{mid}|arranged")}));
    drop(s);
    crate::record_raw("online", "playfab", &format!("matchmaking queue '{queue}' resolved into private match {mid} with the local tickets ({}); public queues are never used", waiting.len()));
    Some(mid)
}

pub fn route_local(api: &str, me: &Entity, b: &Value) -> super::Response {
    let lobby_id = b["LobbyId"].as_str().unwrap_or("").to_string();
    match api {
        "Lobby/CreateLobby" => {
            let max = b["MaxPlayers"].as_u64().unwrap_or(0);
            if !(1..=128).contains(&max) {
                return invalid("MaxPlayers", "MaxPlayers must be between 1 and 128.");
            }
            if let Err(e) = check_search_keys(&b["SearchData"]) {
                return invalid("SearchData", &e);
            }
            let owner = if b["Owner"].is_object() { b["Owner"].clone() } else { caller_key(me) };
            let members: Vec<Value> = b["Members"].as_array().cloned().unwrap_or_else(|| vec![member(&json!({}), me)]);
            if members.len() as u64 > max {
                return invalid("Members", "More members than MaxPlayers.");
            }
            let id = random_uuid();
            let cs = connection_string(&id);
            let l = json!({
                "LobbyId": id, "ConnectionString": cs, "Owner": owner, "OwnerMigrationPolicy": b["OwnerMigrationPolicy"].as_str().unwrap_or("Automatic"),
                "AccessPolicy": b["AccessPolicy"].as_str().unwrap_or("Public"), "MaxPlayers": max, "MembershipLock": "Unlocked", "Members": members,
                "LobbyData": if b["LobbyData"].is_object() { b["LobbyData"].clone() } else { json!({}) }, "SearchData": if b["SearchData"].is_object() { b["SearchData"].clone() } else { json!({}) },
                "UseConnections": b["UseConnections"].as_bool().unwrap_or(true), "RestrictInvitesToLobbyOwner": b["RestrictInvitesToLobbyOwner"].as_bool().unwrap_or(false),
                "ChangeNumber": 1, "PubSubConnectionHandle": Value::Null, "PreventRejoin": [], "Created": iso_now(),
            });
            st().lobby.lobbies.insert(id.clone(), l);
            ok(json!({"LobbyId": id, "ConnectionString": cs}))
        }
        "Lobby/GetLobby" => match st().lobby.lobbies.get(&lobby_id) {
            Some(l) => ok(json!({"Lobby": view(l)})),
            None => lobby_missing(),
        },
        "Lobby/DeleteLobby" => {
            let mut s = st();
            match s.lobby.lobbies.get(&lobby_id) {
                None => lobby_missing(),
                Some(l) if l["Owner"]["Id"] != json!(me.id) => err(403, "LobbyNotOwner", 11006, "Only the owner can delete the lobby."),
                Some(_) => {
                    s.lobby.lobbies.remove(&lobby_id);
                    ok(json!({}))
                }
            }
        }
        "Lobby/JoinLobby" => {
            let cs = b["ConnectionString"].as_str().unwrap_or("");
            if cs.is_empty() {
                return invalid("ConnectionString", "The ConnectionString field is required.");
            }
            let who = match acting_for_self(b, me) {
                Ok(w) => w,
                Err(e) => return e,
            };
            let mut s = st();
            let Some(l) = s.lobby.lobbies.values_mut().find(|l| l["ConnectionString"] == json!(cs)) else { return lobby_missing() };
            if let Err(e) = joinable(l, &who) {
                return e;
            }
            l["Members"].as_array_mut().expect("members").push(member(b, me));
            bump(l);
            ok(json!({"LobbyId": l["LobbyId"]}))
        }
        "Lobby/JoinArrangedLobby" => {
            let arr = b["ArrangementString"].as_str().unwrap_or("");
            if arr.is_empty() {
                return invalid("ArrangementString", "The ArrangementString field is required.");
            }
            let who = match acting_for_self(b, me) {
                Ok(w) => w,
                Err(e) => return e,
            };
            let mut s = st();
            if let Some(l) = s.lobby.lobbies.values_mut().find(|l| l["Arrangement"] == json!(arr)) {
                if let Err(e) = joinable(l, &who) {
                    return e;
                }
                l["Members"].as_array_mut().expect("members").push(member(b, me));
                bump(l);
                return ok(json!({"LobbyId": l["LobbyId"]}));
            }
            let max = b["MaxPlayers"].as_u64().unwrap_or(0);
            if !(1..=128).contains(&max) {
                return invalid("MaxPlayers", "MaxPlayers must be between 1 and 128.");
            }
            let id = random_uuid();
            let l = json!({
                "LobbyId": id, "ConnectionString": connection_string(&id), "Owner": who, "OwnerMigrationPolicy": b["OwnerMigrationPolicy"].as_str().unwrap_or("Automatic"),
                "AccessPolicy": b["AccessPolicy"].as_str().unwrap_or("Private"), "MaxPlayers": max, "MembershipLock": "Unlocked", "Members": [member(b, me)],
                "LobbyData": {}, "SearchData": {}, "UseConnections": b["UseConnections"].as_bool().unwrap_or(true), "RestrictInvitesToLobbyOwner": false,
                "ChangeNumber": 1, "PubSubConnectionHandle": Value::Null, "PreventRejoin": [], "Arrangement": arr, "Created": iso_now(),
            });
            s.lobby.lobbies.insert(id.clone(), l);
            ok(json!({"LobbyId": id}))
        }
        "Lobby/LeaveLobby" | "Lobby/RemoveMember" => {
            let who = if b["MemberEntity"].is_object() { b["MemberEntity"].clone() } else { caller_key(me) };
            let mut s = st();
            let Some(l) = s.lobby.lobbies.get_mut(&lobby_id) else { return lobby_missing() };
            let self_leave = who["Id"] == json!(me.id);
            if api == "Lobby/RemoveMember" && !self_leave && l["Owner"]["Id"] != json!(me.id) {
                return err(403, "LobbyNotOwner", 11006, "Only the owner can remove other members.");
            }
            if api == "Lobby/LeaveLobby" && !self_leave {
                return err(403, "NotAuthorized", 1089, "The caller may only leave for itself.");
            }
            let members = l["Members"].as_array_mut().expect("members");
            let before = members.len();
            members.retain(|m| !same(&m["MemberEntity"], &who));
            if members.len() == before {
                return err(400, "LobbyMemberIsNotOwner", 11007, "The entity is not a member of the lobby.");
            }
            if b["PreventRejoin"].as_bool() == Some(true) {
                l["PreventRejoin"].as_array_mut().expect("prevent").push(who.clone());
            }
            let empty = l["Members"].as_array().map_or(true, |m| m.is_empty());
            if !empty && same(&l["Owner"], &who) {
                l["Owner"] = match l["OwnerMigrationPolicy"].as_str() {
                    Some("None") => Value::Null,
                    _ => l["Members"][0]["MemberEntity"].clone(),
                };
            }
            bump(l);
            if empty {
                s.lobby.lobbies.remove(&lobby_id);
            }
            ok(json!({}))
        }
        "Lobby/UpdateLobby" => {
            let mut s = st();
            let Some(l) = s.lobby.lobbies.get_mut(&lobby_id) else { return lobby_missing() };
            let owner = l["Owner"]["Id"] == json!(me.id);
            let owner_fields = ["AccessPolicy", "LobbyData", "LobbyDataToDelete", "MaxPlayers", "MembershipLock", "Owner", "SearchData", "SearchDataToDelete"];
            if !owner && owner_fields.iter().any(|f| !b[*f].is_null()) {
                return err(403, "LobbyNotOwner", 11006, "Only the owner can change lobby properties.");
            }
            if let Some(m) = b["MaxPlayers"].as_u64() {
                if m < l["Members"].as_array().map_or(0, |x| x.len()) as u64 || m > 128 {
                    return invalid("MaxPlayers", "MaxPlayers cannot be below the member count or above 128.");
                }
                l["MaxPlayers"] = json!(m);
            }
            for k in ["AccessPolicy", "MembershipLock"] {
                if let Some(v) = b[k].as_str() {
                    l[k] = json!(v);
                }
            }
            if b["Owner"].is_object() {
                if !l["Members"].as_array().map_or(false, |m| m.iter().any(|x| same(&x["MemberEntity"], &b["Owner"]))) {
                    return invalid("Owner", "The new owner must be a lobby member.");
                }
                l["Owner"] = b["Owner"].clone();
            }
            if let Err(e) = check_search_keys(&b["SearchData"]) {
                return invalid("SearchData", &e);
            }
            let (mut ld, mut sd) = (l["LobbyData"].clone(), l["SearchData"].clone());
            if let Err(e) = merge(&mut ld, &b["LobbyData"], &b["LobbyDataToDelete"], 30) {
                return invalid("LobbyData", &e);
            }
            if let Err(e) = merge(&mut sd, &b["SearchData"], &b["SearchDataToDelete"], 60) {
                return invalid("SearchData", &e);
            }
            l["LobbyData"] = ld;
            l["SearchData"] = sd;
            if !b["MemberData"].is_null() || !b["MemberDataToDelete"].is_null() {
                let who = if b["MemberEntity"].is_object() { b["MemberEntity"].clone() } else { caller_key(me) };
                if who["Id"] != json!(me.id) {
                    return err(403, "NotAuthorized", 1089, "Members can only change their own member data.");
                }
                let Some(m) = l["Members"].as_array_mut().and_then(|ms| ms.iter_mut().find(|x| same(&x["MemberEntity"], &who))) else {
                    return err(400, "LobbyMemberIsNotOwner", 11007, "The entity is not a member of the lobby.");
                };
                let mut md = m["MemberData"].clone();
                if let Err(e) = merge(&mut md, &b["MemberData"], &b["MemberDataToDelete"], 30) {
                    return invalid("MemberData", &e);
                }
                m["MemberData"] = md;
            }
            bump(l);
            ok(json!({}))
        }
        "Lobby/FindLobbies" => find(b, me, false),
        "Lobby/FindFriendLobbies" => find(b, me, true),
        "Lobby/InviteToLobby" => {
            let mut s = st();
            let Some(l) = s.lobby.lobbies.get(&lobby_id) else { return lobby_missing() };
            if !l["Members"].as_array().map_or(false, |m| m.iter().any(|x| x["MemberEntity"]["Id"] == json!(me.id))) {
                return err(403, "NotAuthorized", 1089, "Only lobby members can invite.");
            }
            if l["RestrictInvitesToLobbyOwner"].as_bool() == Some(true) && l["Owner"]["Id"] != json!(me.id) {
                return err(403, "LobbyNotOwner", 11006, "Only the owner can invite to this lobby.");
            }
            if key_of(&b["InviteeEntity"]).is_none() {
                return invalid("InviteeEntity", "The InviteeEntity field is required.");
            }
            let inv = json!({"LobbyId": lobby_id, "ConnectionString": l["ConnectionString"], "Inviter": caller_key(me), "Invitee": b["InviteeEntity"], "Sent": iso_now()});
            s.lobby.invites.push(inv);
            ok(json!({}))
        }
        "Lobby/SubscribeToLobbyResource" | "Match/SubscribeToMatchmakingResource" => {
            if b["PubSubConnectionHandle"].as_str().map_or(true, |h| h.is_empty()) {
                return invalid("PubSubConnectionHandle", "A PubSub connection handle is required.");
            }
            let rid = b["ResourceId"].as_str().unwrap_or("").to_string();
            let ty = b["Type"].as_str().unwrap_or("LobbyChange").to_string();
            let topic = format!("{}!{}!{}", ty, rid, b["SubscriptionVersion"].as_u64().unwrap_or(1));
            st().lobby.subscriptions.insert(topic.clone(), json!({"Entity": b["EntityKey"], "Handle": b["PubSubConnectionHandle"], "Type": ty, "ResourceId": rid}));
            crate::record_raw("online", "playfab", &format!("PubSub subscription {topic} recorded; PlayFab PubSub (SignalR) push is not emulated, so the title only sees changes it polls"));
            ok(json!({"Topic": topic}))
        }
        "Lobby/UnsubscribeFromLobbyResource" | "Match/UnsubscribeFromMatchmakingResource" => {
            let rid = b["ResourceId"].as_str().unwrap_or("");
            let mut s = st();
            let before = s.lobby.subscriptions.len();
            s.lobby.subscriptions.retain(|_, v| v["ResourceId"] != json!(rid));
            if s.lobby.subscriptions.len() == before {
                return err(404, "SubscriptionNotFound", 11008, "No subscription exists for that resource.");
            }
            ok(json!({}))
        }
        "Match/CreateMatchmakingTicket" => {
            let queue = b["QueueName"].as_str().unwrap_or("").to_string();
            if queue.is_empty() {
                return invalid("QueueName", "The QueueName field is required.");
            }
            let give_up = b["GiveUpAfterSeconds"].as_u64().unwrap_or(0);
            if !(1..=3600).contains(&give_up) {
                return invalid("GiveUpAfterSeconds", "GiveUpAfterSeconds must be between 1 and 3600.");
            }
            let creator = &b["Creator"];
            if key_of(&creator["Entity"]).map(|k| k.0) != Some(me.id.clone()) {
                return err(403, "NotAuthorized", 1089, "The ticket creator must be the caller.");
            }
            let id = random_uuid();
            let pending: Vec<Value> = b["MembersToMatchWith"].as_array().cloned().unwrap_or_default();
            let t = json!({
                "TicketId": id, "QueueName": queue, "Creator": creator["Entity"], "GiveUpAfterSeconds": give_up, "Created": iso_now(), "CreatedMs": now_millis(),
                "Members": [{"Entity": creator["Entity"], "Attributes": creator["Attributes"]}], "MembersToMatchWith": pending,
                "Status": if pending.is_empty() { "WaitingForMatch" } else { "WaitingForPlayers" }, "MatchId": Value::Null, "CancellationReasonString": Value::Null,
            });
            st().lobby.tickets.insert(id.clone(), t);
            try_match(&queue);
            ok(json!({"TicketId": id}))
        }
        "Match/JoinMatchmakingTicket" => {
            let tid = b["TicketId"].as_str().unwrap_or("").to_string();
            let queue = {
                let mut s = st();
                let Some(t) = s.lobby.tickets.get_mut(&tid).filter(|t| t["QueueName"] == b["QueueName"]) else { return err(404, "MatchmakingTicketNotFound", 2054, "The ticket does not exist.") };
                if t["Status"] != "WaitingForPlayers" {
                    return err(400, "MatchmakingTicketMembershipLimitExceeded", 2059, "The ticket is not waiting for players.");
                }
                let m = &b["Member"];
                if m["Entity"]["Id"] != json!(me.id) {
                    return err(403, "NotAuthorized", 1089, "Members join tickets for themselves.");
                }
                let pending = t["MembersToMatchWith"].as_array_mut().expect("pending");
                let before = pending.len();
                pending.retain(|p| !same(p, &m["Entity"]));
                if pending.len() == before {
                    return err(403, "MatchmakingPlayerNotInTicket", 2060, "The player was not invited to this ticket.");
                }
                let done = pending.is_empty();
                t["Members"].as_array_mut().expect("members").push(json!({"Entity": m["Entity"], "Attributes": m["Attributes"]}));
                if done {
                    t["Status"] = json!("WaitingForMatch");
                }
                t["QueueName"].as_str().unwrap_or("").to_string()
            };
            try_match(&queue);
            ok(json!({}))
        }
        "Match/GetMatchmakingTicket" => {
            let tid = b["TicketId"].as_str().unwrap_or("");
            match st().lobby.tickets.get(tid).filter(|t| t["QueueName"] == b["QueueName"]) {
                Some(t) => ok(ticket_view(t)),
                None => err(404, "MatchmakingTicketNotFound", 2054, "The ticket does not exist."),
            }
        }
        "Match/CancelMatchmakingTicket" => {
            let tid = b["TicketId"].as_str().unwrap_or("");
            let mut s = st();
            match s.lobby.tickets.get_mut(tid).filter(|t| t["QueueName"] == b["QueueName"]) {
                Some(t) if t["Status"] == "Matched" => err(400, "MatchmakingTicketAlreadyCompleted", 2058, "The ticket already matched."),
                Some(t) => {
                    t["Status"] = json!("Canceled");
                    t["CancellationReasonString"] = json!("Requested");
                    ok(json!({}))
                }
                None => err(404, "MatchmakingTicketNotFound", 2054, "The ticket does not exist."),
            }
        }
        "Match/CancelAllMatchmakingTicketsForPlayer" => {
            let who = if b["Entity"].is_object() { b["Entity"]["Id"].clone() } else { json!(me.id) };
            let mut s = st();
            for t in s.lobby.tickets.values_mut() {
                if t["QueueName"] == b["QueueName"] && t["Status"] != "Matched" && t["Members"].as_array().map_or(false, |m| m.iter().any(|x| x["Entity"]["Id"] == who)) {
                    t["Status"] = json!("Canceled");
                    t["CancellationReasonString"] = json!("Requested");
                }
            }
            ok(json!({}))
        }
        "Match/ListMatchmakingTicketsForPlayer" => {
            let s = st();
            let ids: Vec<Value> = s.lobby.tickets.values().filter(|t| t["QueueName"] == b["QueueName"] && t["Members"].as_array().map_or(false, |m| m.iter().any(|x| x["Entity"]["Id"] == json!(me.id)))).map(|t| t["TicketId"].clone()).collect();
            ok(json!({"TicketIds": ids}))
        }
        "Match/GetMatch" => {
            let mid = b["MatchId"].as_str().unwrap_or("");
            match st().lobby.matches.get(mid) {
                Some(m) => {
                    let mut v = m.clone();
                    if b["ReturnMemberAttributes"].as_bool() != Some(true) {
                        for x in v["Members"].as_array_mut().into_iter().flatten() {
                            if let Some(o) = x.as_object_mut() {
                                o.remove("Attributes");
                            }
                        }
                    }
                    ok(v)
                }
                None => err(404, "MatchmakingMatchNotFound", 2055, "The match does not exist."),
            }
        }
        _ => {
            crate::record_raw("online", "unhandled", &format!("[playfab] {api}"));
            err(404, "APINotFound", 1133, &format!("TomBox has no implementation of /{api} yet"))
        }
    }
}

pub fn invites_for(id: &str) -> Vec<Value> {
    st().lobby.invites.iter().filter(|i| i["Invitee"]["Id"] == json!(id)).cloned().collect()
}

pub fn route(api: &str, me: &Entity, b: &Value) -> super::Response {
    let payload = json!({"api": api, "entity": {"Id": me.id, "Type": me.kind}, "body": b});
    match super::super::lan::invoke("playfab.lobby", &payload) {
        Ok(r) => super::Response::json_status(r["status"].as_u64().unwrap_or(500) as u16, &r["body"]),
        Err(e) => err(503, "ServiceUnavailable", 1123, &format!("the LAN hub could not run {api}: {e}")),
    }
}

pub fn hub_handle(p: &Value) -> Value {
    let api = p["api"].as_str().unwrap_or("");
    if !(api.starts_with("Lobby/") || api.starts_with("Match/")) {
        return json!({"status": 400, "body": {"code": 400, "status": "BadRequest", "error": "InvalidParams", "errorCode": 1000, "errorMessage": "not a lobby or match API"}});
    }
    let Some(id) = p["entity"]["Id"].as_str().filter(|i| !i.is_empty()) else {
        return json!({"status": 401, "body": {"code": 401, "status": "Unauthorized", "error": "NotAuthenticated", "errorCode": 1074, "errorMessage": "no entity"}});
    };
    let me = Entity { id: id.to_string(), kind: p["entity"]["Type"].as_str().unwrap_or("title_player_account").to_string(), expires: u64::MAX };
    let r = route_local(api, &me, &p["body"]);
    json!({"status": r.status, "body": serde_json::from_slice::<Value>(&r.body).unwrap_or(Value::Null)})
}
