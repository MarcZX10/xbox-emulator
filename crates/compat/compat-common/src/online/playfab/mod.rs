pub mod lobby;
pub mod qos;
#[cfg(test)]
mod tests;

use super::util::{b64, hex, iso_millis, iso_now, now_millis, random_uuid, sha256};
use super::{Request, Response, TitleService};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const TOKEN_SECS: u64 = 86_400;
pub const QOS_REGIONS: &[&str] = &["EastUs", "WestUs", "NorthCentralUs", "SouthCentralUs", "NorthEurope", "WestEurope", "EastAsia", "SoutheastAsia", "JapanEast", "AustraliaEast", "BrazilSouth"];

#[derive(Clone)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    pub expires: u64,
}

#[derive(Default)]
pub struct State {
    pub title_id: String,
    pub logged_in: bool,
    pub created: String,
    pub last_login: Option<String>,
    pub display_name: Option<String>,
    pub tickets: HashMap<String, u64>,
    pub tokens: HashMap<String, Entity>,
    pub user_data: Map<String, Value>,
    pub data_version: u64,
    pub statistics: Map<String, Value>,
    pub parties: HashMap<String, Value>,
    pub cognitive: Vec<Value>,
    pub events_written: u64,
    pub lobby: lobby::Lobbies,
}

pub fn host_of(req: &Request) -> String {
    req.header("x-forwarded-host").or(req.header("host")).map(|h| h.split(':').next().unwrap_or("").to_ascii_lowercase()).unwrap_or_default()
}

pub fn wants(req: &Request) -> bool {
    let h = host_of(req);
    h == "playfabapi.com" || h.ends_with(".playfabapi.com")
}

pub fn host_exe_is_terraria() -> bool {
    std::env::current_exe().ok().and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_ascii_lowercase())).map_or(false, |f| f == "terraria.exe")
}

fn st() -> MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State { created: "2019-11-20T00:00:00.000Z".into(), ..Default::default() })).lock().unwrap_or_else(|e| e.into_inner())
}

pub fn id16(seed: &str) -> String {
    hex(&sha256(format!("tombox-playfab:{seed}").as_bytes())[..8]).to_ascii_uppercase()
}

pub fn xuid() -> u64 {
    crate::gdk::user::user().xuid
}

pub fn master_id() -> String {
    id16(&format!("master:{}", xuid()))
}

pub fn title_player_for_xuid(title: &str, x: u64) -> String {
    id16(&format!("title_player:{title}:{x}"))
}

pub fn title_player_id() -> String {
    title_player_for_xuid(&title_id(), xuid())
}

pub fn title_id() -> String {
    let t = st().title_id.clone();
    if t.is_empty() { crate::cfg("playfab_title_id").unwrap_or_else(|| "40467".into()) } else { t }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "BadRequest",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "NotFound",
        409 => "Conflict",
        429 => "TooManyRequests",
        _ => "InternalServerError",
    }
}

pub fn ok(data: Value) -> Response {
    Response::json(&json!({"code": 200, "status": "OK", "data": data}))
}

pub fn err(http: u16, error: &str, code: u32, msg: &str) -> Response {
    Response::json_status(http, &json!({"code": http, "status": status_text(http), "error": error, "errorCode": code, "errorMessage": msg}))
}

pub fn invalid(field: &str, msg: &str) -> Response {
    Response::json_status(400, &json!({"code": 400, "status": "BadRequest", "error": "InvalidParams", "errorCode": 1000, "errorMessage": "Invalid input parameters", "errorDetails": { field: [msg] }}))
}

fn not_authenticated() -> Response {
    err(401, "NotAuthenticated", 1074, "This API method does not allow anonymous callers.")
}

pub fn entity_key(id: &str, kind: &str) -> Value {
    json!({"Id": id, "Type": kind, "TypeString": kind})
}

fn issue_entity_token(id: &str, kind: &str) -> Value {
    let exp = now_millis() / 1000 + TOKEN_SECS;
    let token = format!("{}|{}", b64(format!("tombox-pf-entity:{kind}:{id}:{}", random_uuid()).as_bytes()), b64(&sha256(random_uuid().as_bytes())[..16]));
    st().tokens.insert(token.clone(), Entity { id: id.into(), kind: kind.into(), expires: exp });
    json!({"EntityToken": token, "TokenExpiration": iso_millis(exp * 1000), "Entity": entity_key(id, kind)})
}

pub fn caller(req: &Request) -> Option<Entity> {
    let t = req.header("x-entitytoken")?.trim().to_string();
    let e = st().tokens.get(&t).cloned()?;
    (e.expires > now_millis() / 1000).then_some(e)
}

fn session_ok(req: &Request) -> bool {
    req.header("x-authorization").map_or(false, |t| st().tickets.get(t.trim()).map_or(false, |e| *e > now_millis() / 1000))
}

fn profile(entity: &str) -> Value {
    let title = title_id();
    let s = st();
    let name = s.display_name.clone().unwrap_or_else(|| crate::gdk::user::user().gamertag.clone());
    json!({
        "Entity": entity_key(entity, "title_player_account"),
        "EntityChain": format!("title_player_account!{}/{}/{}/{}", master_id(), title, master_id(), entity),
        "DisplayName": name,
        "Lineage": {"MasterPlayerAccountId": master_id(), "NamespaceId": id16(&format!("ns:{title}")), "TitleId": title, "TitlePlayerAccountId": entity},
        "Created": s.created,
        "Language": "en-US",
        "Files": {},
        "Objects": {},
        "Permissions": [],
        "VersionNumber": s.data_version,
    })
}

fn player_profile() -> Value {
    let title = title_id();
    let s = st();
    json!({
        "PlayerId": master_id(),
        "TitleId": title,
        "DisplayName": s.display_name.clone().unwrap_or_else(|| crate::gdk::user::user().gamertag.clone()),
        "Created": s.created,
        "LastLogin": s.last_login,
        "LinkedAccounts": [{"Platform": "XboxLive", "PlatformUserId": xuid().to_string()}],
    })
}

fn user_data_view(keys: &[String]) -> Value {
    let s = st();
    let mut out = Map::new();
    for (k, v) in &s.user_data {
        if keys.is_empty() || keys.contains(k) {
            out.insert(k.clone(), v.clone());
        }
    }
    json!({"Data": out, "DataVersion": s.data_version})
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn login_with_xbox(req: &Request, b: &Value) -> Response {
    let Some(tok) = b["XboxToken"].as_str() else { return invalid("XboxToken", "The XboxToken field is required.") };
    if !tok.trim_start().to_ascii_uppercase().starts_with("XBL3.0 X=") || !tok.contains(';') {
        return err(400, "InvalidXboxLiveToken", 1158, "The Xbox Live token is not valid.");
    }
    if let Some(t) = b["TitleId"].as_str().filter(|t| !t.is_empty()) {
        if !t.eq_ignore_ascii_case(&title_id()) && !host_of(req).starts_with(&t.to_ascii_lowercase()) {
            return err(400, "InvalidTitleId", 1004, "The title id in the request does not match the title.");
        }
    }
    let create = b["CreateAccount"].as_bool().unwrap_or(false);
    let existing = crate::cfg("playfab_new_account").map_or(true, |v| v != "1");
    let mut newly = false;
    {
        let mut s = st();
        if !s.logged_in && !existing {
            if !create {
                return err(400, "AccountNotFound", 1001, "User not found");
            }
            newly = true;
            s.created = iso_now();
        }
        s.logged_in = true;
    }
    let last = st().last_login.replace(iso_now());
    let ticket = format!("{}-{}-{}-{}-{}", master_id(), id16(&random_uuid()), title_id(), id16(&random_uuid()), b64(&sha256(random_uuid().as_bytes())[..18]));
    st().tickets.insert(ticket.clone(), now_millis() / 1000 + TOKEN_SECS);
    let entity = issue_entity_token(&title_player_id(), "title_player_account");
    let mut data = json!({
        "SessionTicket": ticket,
        "PlayFabId": master_id(),
        "NewlyCreated": newly,
        "SettingsForUser": {"NeedsAttribution": false, "GatherDeviceInfo": true, "GatherFocusInfo": true},
        "LastLoginTime": last,
        "EntityToken": entity,
        "TreatmentAssignment": {"Variants": [], "Variables": []},
    });
    let p = &b["InfoRequestParameters"];
    if p.is_object() {
        let mut info = json!({});
        if p["GetUserAccountInfo"].as_bool() == Some(true) {
            info["AccountInfo"] = account_info();
        }
        if p["GetUserData"].as_bool() == Some(true) {
            let v = user_data_view(&str_list(&p["UserDataKeys"]));
            info["UserData"] = v["Data"].clone();
            info["UserDataVersion"] = v["DataVersion"].clone();
        }
        if p["GetUserReadOnlyData"].as_bool() == Some(true) {
            info["UserReadOnlyData"] = json!({});
            info["UserReadOnlyDataVersion"] = json!(0);
        }
        if p["GetTitleData"].as_bool() == Some(true) {
            info["TitleData"] = title_data(&str_list(&p["TitleDataKeys"]));
        }
        if p["GetPlayerStatistics"].as_bool() == Some(true) {
            info["PlayerStatistics"] = statistics(&str_list(&p["PlayerStatisticNames"]));
        }
        if p["GetPlayerProfile"].as_bool() == Some(true) {
            info["PlayerProfile"] = player_profile();
        }
        if p["GetUserInventory"].as_bool() == Some(true) {
            info["UserInventory"] = json!([]);
        }
        if p["GetUserVirtualCurrency"].as_bool() == Some(true) {
            info["UserVirtualCurrency"] = json!({});
            info["UserVirtualCurrencyRechargeTimes"] = json!({});
        }
        if p["GetCharacterList"].as_bool() == Some(true) {
            info["CharacterList"] = json!([]);
        }
        if p["GetCharacterInventories"].as_bool() == Some(true) {
            info["CharacterInventories"] = json!([]);
        }
        data["InfoResultPayload"] = info;
    }
    ok(data)
}

fn account_info() -> Value {
    let tp = title_player_id();
    let s = st();
    json!({
        "PlayFabId": master_id(),
        "Created": s.created,
        "TitleInfo": {"DisplayName": s.display_name, "Origination": "XboxLive", "Created": s.created, "LastLogin": s.last_login, "FirstLogin": s.created, "isBanned": false, "TitlePlayerAccount": entity_key(&tp, "title_player_account")},
        "XboxInfo": {"XboxUserId": xuid().to_string(), "XboxUserSandbox": "RETAIL"},
    })
}

fn title_data(keys: &[String]) -> Value {
    let all: Map<String, Value> = crate::cfg("playfab_title_data").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default();
    Value::Object(all.into_iter().filter(|(k, _)| keys.is_empty() || keys.contains(k)).collect())
}

fn statistics(names: &[String]) -> Value {
    let s = st();
    json!(s.statistics.iter().filter(|(k, _)| names.is_empty() || names.contains(k)).map(|(k, v)| json!({"StatisticName": k, "Value": v["Value"], "Version": v["Version"]})).collect::<Vec<_>>())
}

fn write_events(b: &Value, telemetry: bool) -> Response {
    use std::io::Write;
    let Some(events) = b["Events"].as_array() else { return invalid("Events", "The Events field is required.") };
    if events.is_empty() || events.len() > 200 {
        return invalid("Events", "Between 1 and 200 events are allowed per request.");
    }
    let mut ids = Vec::new();
    let mut out = String::new();
    for e in events {
        if e["EventNamespace"].as_str().map_or(true, |n| n.is_empty()) || e["Name"].as_str().map_or(true, |n| n.is_empty()) {
            return invalid("Events", "Each event needs an EventNamespace and a Name.");
        }
        let id = random_uuid().replace('-', "");
        out.push_str(&json!({"received": iso_now(), "telemetry": telemetry, "assignedId": id, "event": e}).to_string());
        out.push('\n');
        ids.push(id);
    }
    let dir = crate::gdk::data_root().join("telemetry");
    let path = dir.join("playfab_events.jsonl");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::metadata(&path).map_or(false, |m| m.len() > 64 * 1024 * 1024) {
        let _ = std::fs::rename(&path, dir.join("playfab_events.1.jsonl"));
    }
    if let Err(e) = std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| f.write_all(out.as_bytes())) {
        return err(500, "InternalServerError", 1123, &e.to_string());
    }
    st().events_written += ids.len() as u64;
    ok(json!({"AssignedEventIds": ids}))
}

fn qos_servers(b: &Value) -> Response {
    if b.get("Version").map_or(false, |v| !v.is_string() && !v.is_null()) {
        return invalid("Version", "The Version field must be a string.");
    }
    let regions: Vec<String> = crate::cfg("playfab_qos_regions").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_else(|| QOS_REGIONS.iter().map(|s| s.to_string()).collect());
    qos::ensure_started();
    let servers: Vec<Value> = regions.iter().map(|r| json!({"Region": r, "ServerUrl": format!("pfmsqosprod2-0.{}.cloudapp.azure.com", r.to_ascii_lowercase())})).collect();
    ok(json!({"PageSize": servers.len(), "QosServers": servers, "SkipToken": Value::Null}))
}

fn request_party(e: &Entity, b: &Value) -> Response {
    let regions = str_list(&b["PreferredRegions"]);
    if regions.is_empty() {
        return invalid("PreferredRegions", "At least one preferred region is required.");
    }
    let known: Vec<String> = QOS_REGIONS.iter().map(|s| s.to_string()).collect();
    let Some(region) = regions.iter().find(|r| known.iter().any(|k| k.eq_ignore_ascii_case(r))).cloned() else {
        return err(400, "MultiplayerServerBadRequest", 2002, "None of the preferred regions has Party relays.");
    };
    let party_id = b["PartyId"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(random_uuid);
    let port = crate::cfg("playfab_party_relay_port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(30000);
    let server = json!({
        "PartyId": party_id,
        "Region": region,
        "ServerId": random_uuid(),
        "VmId": format!("xcloudvm{}", &id16(&party_id)[..8].to_ascii_lowercase()),
        "IPV4Address": "127.0.0.1",
        "FQDN": format!("{}.{}.cloudapp.azure.com", id16(&party_id).to_ascii_lowercase(), region.to_ascii_lowercase()),
        "Ports": [{"Name": "relay", "Num": port, "Protocol": "UDP"}],
        "State": "Active",
        "ConnectedPlayers": [{"PlayerId": e.id}],
        "LastStateTransitionTime": iso_now(),
        "DTLSCertificateSHA2Thumbprint": hex(&sha256(format!("tombox-party-relay:{party_id}").as_bytes())).to_ascii_uppercase(),
        "SessionCookie": b["SessionCookie"].clone(),
    });
    st().parties.insert(party_id.clone(), server.clone());
    crate::record_raw("online", "playfab", &format!("Party relay {party_id} in {region} handed out at 127.0.0.1:{port}; the relay's DTLS protocol is not emulated"));
    ok(server)
}

fn cognitive_token(b: &Value) -> Response {
    let kind = b["CognitiveServicesType"].as_str().unwrap_or("SpeechToText");
    if !matches!(kind, "SpeechToText" | "SpeechToTextTranslation" | "TextToSpeech" | "TextToText") {
        return invalid("CognitiveServicesType", "Unknown cognitive services type.");
    }
    let exp = now_millis() + 600_000;
    let v = json!({
        "Token": b64(format!("tombox-cognitive:{kind}:{}", random_uuid()).as_bytes()),
        "ServiceEndpoint": format!("https://eastus.{}", if kind == "TextToSpeech" { "tts.speech.microsoft.com" } else { "stt.speech.microsoft.com" }),
        "ExpirationTimeUtc": iso_millis(exp),
    });
    st().cognitive.push(json!({"type": kind, "issued": iso_now()}));
    ok(v)
}

fn titles_from_xuids(b: &Value) -> Response {
    let ids = str_list(&b["XboxLiveIds"]);
    if ids.is_empty() || ids.len() > 10 {
        return invalid("XboxLiveIds", "Between 1 and 10 Xbox Live ids are required.");
    }
    let title = b["TitleId"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(title_id);
    let known: Vec<u64> = std::iter::once(xuid()).chain(crate::gdk::social::friends().into_iter().map(|f| f.0)).collect();
    let mut out = Map::new();
    for i in ids {
        if let Ok(x) = i.parse::<u64>() {
            if known.contains(&x) {
                out.insert(i.clone(), entity_key(&title_player_for_xuid(&title, x), "title_player_account"));
            }
        }
    }
    ok(json!({"TitlePlayerAccounts": out, "TitleId": title}))
}

fn playfab_ids_from_xuids(b: &Value) -> Response {
    let ids = str_list(&b["XboxLiveAccountIDs"]);
    let me = xuid().to_string();
    ok(json!({"Data": ids.iter().map(|i| json!({"XboxLiveAccountId": i, "PlayFabId": if *i == me { json!(master_id()) } else { Value::Null }})).collect::<Vec<_>>()}))
}

pub fn route(req: &Request) -> Option<Response> {
    let host = host_of(req);
    if let Some(t) = host.strip_suffix(".playfabapi.com").filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_hexdigit())) {
        let mut s = st();
        if s.title_id.is_empty() {
            s.title_id = t.to_ascii_uppercase();
        }
    }
    let seg: Vec<&str> = req.segments();
    if seg.len() != 2 {
        return None;
    }
    if req.method != "POST" {
        return Some(err(405, "MethodNotAllowed", 1124, "PlayFab APIs are POST only."));
    }
    let b = if req.body.is_empty() { json!({}) } else { req.json() };
    if !b.is_object() {
        return Some(invalid("body", "The request body must be a JSON object."));
    }
    let api = format!("{}/{}", seg[0], seg[1]);
    let open = matches!(api.as_str(), "Client/LoginWithXbox" | "MultiplayerServer/ListPartyQosServers" | "Client/GetTitlePublicKey");
    let client = seg[0] == "Client" && !open;
    let me = if open || client { None } else { caller(req) };
    if client && !session_ok(req) && caller(req).is_none() {
        return Some(not_authenticated());
    }
    if !open && !client && me.is_none() {
        return Some(not_authenticated());
    }
    let ent = || me.clone().expect("entity");
    let r = match api.as_str() {
        "Client/LoginWithXbox" => login_with_xbox(req, &b),
        "MultiplayerServer/ListPartyQosServers" => qos_servers(&b),
        "MultiplayerServer/GetCognitiveServicesToken" => cognitive_token(&b),
        "Party/RequestParty" => request_party(&ent(), &b),
        "Event/WriteEvents" => write_events(&b, false),
        "Event/WriteTelemetryEvents" => write_events(&b, true),
        "Profile/GetTitlePlayersFromXboxLiveIDs" => titles_from_xuids(&b),
        "Profile/GetProfile" => {
            let id = b["Entity"]["Id"].as_str().map(String::from).unwrap_or_else(|| ent().id);
            if id != title_player_id() {
                err(404, "EntityProfileNotFound", 1330, "The entity has no profile.")
            } else {
                ok(json!({"Profile": profile(&id)}))
            }
        }
        "Profile/GetProfiles" => {
            let ids: Vec<String> = b["Entities"].as_array().map(|a| a.iter().filter_map(|e| e["Id"].as_str().map(String::from)).collect()).unwrap_or_default();
            ok(json!({"Profiles": ids.iter().filter(|i| **i == title_player_id()).map(|i| profile(i)).collect::<Vec<_>>()}))
        }
        "Profile/SetProfileLanguage" => ok(json!({"OperationResult": "Updated", "VersionNumber": st().data_version})),
        "Authentication/GetEntityToken" => {
            let e = ent();
            ok(issue_entity_token(&e.id, &e.kind))
        }
        "Authentication/ValidateEntityToken" => {
            let t = b["EntityToken"].as_str().unwrap_or("");
            let found = st().tokens.get(t).cloned();
            match found {
                Some(v) if v.expires > now_millis() / 1000 => ok(json!({"Entity": entity_key(&v.id, &v.kind), "IdentityProvider": "XboxLive", "IdentifiedDeviceType": "XboxOne", "Lineage": {"MasterPlayerAccountId": master_id(), "TitleId": title_id(), "TitlePlayerAccountId": v.id}})),
                _ => err(400, "InvalidEntityToken", 1335, "The entity token is not valid."),
            }
        }
        "Client/GetTime" => ok(json!({"Time": iso_now()})),
        "Client/GetTitleData" => ok(json!({"Data": title_data(&str_list(&b["Keys"]))})),
        "Client/GetUserData" => ok(user_data_view(&str_list(&b["Keys"]))),
        "Client/GetUserReadOnlyData" | "Client/GetUserPublisherData" | "Client/GetUserPublisherReadOnlyData" => ok(json!({"Data": {}, "DataVersion": 0})),
        "Client/UpdateUserData" => {
            let mut s = st();
            if let Some(d) = b["Data"].as_object() {
                for (k, v) in d {
                    match v {
                        Value::Null => {
                            s.user_data.remove(k);
                        }
                        _ => {
                            s.user_data.insert(k.clone(), json!({"Value": v.as_str().map(String::from).unwrap_or_else(|| v.to_string()), "LastUpdated": iso_now(), "Permission": b["Permission"].as_str().unwrap_or("Private")}));
                        }
                    }
                }
            }
            for k in str_list(&b["KeysToRemove"]) {
                s.user_data.remove(&k);
            }
            s.data_version += 1;
            ok(json!({"DataVersion": s.data_version}))
        }
        "Client/GetPlayerProfile" => ok(json!({"PlayerProfile": player_profile()})),
        "Client/GetAccountInfo" => ok(json!({"AccountInfo": account_info()})),
        "Client/GetPlayFabIDsFromXboxLiveIDs" => playfab_ids_from_xuids(&b),
        "Client/UpdateUserTitleDisplayName" => match b["DisplayName"].as_str() {
            Some(n) if (3..=25).contains(&n.chars().count()) => {
                st().display_name = Some(n.to_string());
                ok(json!({"DisplayName": n}))
            }
            _ => invalid("DisplayName", "The display name must be 3 to 25 characters."),
        },
        "Client/GetPlayerStatistics" => ok(json!({"Statistics": statistics(&str_list(&b["StatisticNames"]))})),
        "Client/UpdatePlayerStatistics" => {
            let mut s = st();
            for u in b["Statistics"].as_array().cloned().unwrap_or_default() {
                if let (Some(n), Some(v)) = (u["StatisticName"].as_str(), u["Value"].as_i64()) {
                    s.statistics.insert(n.to_string(), json!({"Value": v, "Version": 0}));
                }
            }
            ok(json!({}))
        }
        "Client/WritePlayerEvent" | "Client/WriteTitleEvent" => match b["EventName"].as_str() {
            Some(n) => write_events(&json!({"Events": [{"EventNamespace": "com.playfab.events.legacy", "Name": n, "Payload": b["Body"], "OriginalTimestamp": b["Timestamp"]}]}), false),
            None => invalid("EventName", "The EventName field is required."),
        },
        a if a.starts_with("Lobby/") || a.starts_with("Match/") => lobby::route(a, &ent(), &b),
        _ => return None,
    };
    Some(r)
}

pub struct Terraria;

impl TitleService for Terraria {
    fn name(&self) -> &'static str {
        "terraria"
    }

    fn route(&self, req: &Request) -> Option<Response> {
        if wants(req) { route(req) } else { None }
    }

    fn not_found(&self, req: &Request) -> Response {
        if wants(req) {
            err(404, "APINotFound", 1133, &format!("TomBox has no implementation of {} yet", req.path))
        } else {
            Response::json_status(404, &json!({"error": "not_found", "method": req.method, "path": req.path}))
        }
    }
}

pub fn reset_for_tests() {
    *st() = State { created: "2019-11-20T00:00:00.000Z".into(), ..Default::default() };
}
