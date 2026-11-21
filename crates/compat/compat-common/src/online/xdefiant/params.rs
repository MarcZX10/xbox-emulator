use super::XDefiant;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const DATA: &str = include_str!("../../../backend_data/xdefiant/ubiservices_parameters.json");

pub const FEATURE_SWITCHES: &[&str] = &[
    "ApplicationUsed", "ApplicationInformation", "Battlepass", "Blocklist", "Calendar", "Challenge", "ClubApplication", "ClubService", "CreateSession",
    "EntitiesProfile", "EntitiesSpace", "Event", "EventsDefinitions", "ExtendSession", "FirstPartyAccessToMultiplayerInformation",
    "ForcePrimarySecondaryStoreSyncOnReturnFromBackground", "FriendsLookup", "FriendsRequest", "FriendsSpaceSpecific", "GamesPlayed", "Groups",
    "GroupsInvitations", "HttpClient", "LeaderboardMe", "LeaderboardProfiles", "LeaderboardSpaces", "Localization", "Matchmaking", "Moderation", "Mocks",
    "News", "NotificationDynamicUpdate", "NotificationRequestConnections", "NotificationSend", "NotificationSendBatch", "NotificationSendNoBroker",
    "NotificationWebsocket", "NotificationRemoteLogReceivedData", "Parameters", "Party", "Persona", "PlayerActivity", "PlayerConsents", "PlayerOnlineStatus",
    "PlayerPreferences", "PlayerPrivileges", "PlayerReports", "Populations", "PrimarySecondaryStoreForceSyncAtLogin", "PrimaryStore",
    "PrimaryStoreAutomaticFetch", "PrimaryStoreCatalogGetGameMediaType", "Profiles", "Recommendations", "RemoteGaming", "Reputation", "RichPresence",
    "SecondaryStore", "SecondaryStoreInstantiation", "SecondaryStoreInventoryRules", "SecondaryStoreTransactions", "SessionTicket", "Stats",
    "SyncOnResumeToForegroundForNintendoSwitch", "Telemetry", "TerminateSessionOnConfigurationUpdateMismatchSpaceId", "TLog", "Token", "Trades", "Ugc",
    "Uplay", "Users", "UsersCreateAndLink", "UsersLegalOptins", "Voicechat", "WebSocketClient",
];

const LOCAL_OFF: &[&str] = &["Mocks", "TLog", "Uplay", "RemoteGaming", "TerminateSessionOnConfigurationUpdateMismatchSpaceId", "SyncOnResumeToForegroundForNintendoSwitch"];

const NOTIFICATION_SWITCHES: &[&str] = &["NotificationWebsocket", "WebSocketClient", "NotificationDynamicUpdate", "NotificationRequestConnections", "NotificationSend", "NotificationSendBatch", "NotificationSendNoBroker"];

pub fn data() -> &'static Value {
    static D: OnceLock<Value> = OnceLock::new();
    D.get_or_init(|| serde_json::from_str(DATA).unwrap_or(Value::Null))
}

pub fn urls() -> Map<String, Value> {
    data()["urls"].as_object().cloned().unwrap_or_default()
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_ascii_lowercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

pub fn switches(x: &XDefiant) -> Value {
    let doc = data()["space_groups"]["us-sdkClientFeaturesSwitches"]["fields"].as_object().cloned().unwrap_or_default();
    let mut out = Map::new();
    for name in FEATURE_SWITCHES {
        let doc_key = doc.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
        let mut on = doc_key.as_ref().and_then(|k| doc[k].as_bool()).unwrap_or(true);
        if LOCAL_OFF.contains(name) {
            on = false;
        }
        if NOTIFICATION_SWITCHES.contains(name) {
            on = x.notifications;
        }
        out.insert(doc_key.unwrap_or_else(|| lower_first(name)), json!(on));
        out.insert((*name).to_string(), json!(on));
    }
    for (k, v) in &doc {
        if !out.keys().any(|o| o.eq_ignore_ascii_case(k)) {
            out.insert(k.clone(), v.clone());
        }
    }
    json!({ "fields": Value::Object(out), "relatedPopulation": Value::Null })
}

fn url_group() -> Value {
    let mut u = urls();
    if let Some(v) = u.get("profilesMeBattlepassesSeasons").cloned() {
        u.insert("ProfilesMeBattlepassesSeasons".into(), v);
    }
    let b = "https://{env}public-ubiservices.ubi.com/{version}";
    for (k, path) in [("blocklist", "/profiles/me/blocklist"), ("groups", "/spaces/{spaceId}/groups"), ("telemetry", "/profiles/{profileId}/events")] {
        u.entry(k.to_string()).or_insert_with(|| json!(format!("{b}{path}")));
    }
    json!({ "fields": Value::Object(u), "relatedPopulation": Value::Null })
}

fn filter(groups: Map<String, Value>, wanted: Option<&str>) -> Value {
    let Some(w) = wanted.filter(|w| !w.trim().is_empty()) else { return Value::Object(groups) };
    let names: Vec<&str> = w.split(',').map(|s| s.trim()).collect();
    Value::Object(groups.into_iter().filter(|(k, _)| names.iter().any(|n| n.eq_ignore_ascii_case(k))).collect())
}

pub fn app_groups(x: &XDefiant) -> Map<String, Value> {
    let mut g = data()["app_groups"].as_object().cloned().unwrap_or_default();
    g.insert("us-staging".into(), json!({ "fields": { "spaceId": "" }, "relatedPopulation": Value::Null }));
    g.insert("us-sdkClientFeaturesSwitches".into(), switches(x));
    g.insert("us-sdkClientUrls".into(), url_group());
    g
}

pub fn space_groups(x: &XDefiant, space: &str) -> Map<String, Value> {
    let mut g = data()["space_groups"].as_object().cloned().unwrap_or_default();
    g.insert("us-sdkClientFeaturesSwitches".into(), switches(x));
    g.insert("us-sdkClientUrls".into(), url_group());
    g.insert("us-sdkClientNotificationsSpaceIds".into(), json!({ "fields": { "FriendsService": space }, "relatedPopulation": Value::Null }));
    if let Some(f) = g.get_mut("us-sdkClientStorm").and_then(|s| s["fields"].as_object_mut()) {
        f.insert("matchmakingSandboxSpaceId".into(), json!(space));
    }
    g.insert("us-sdkClientNotificationsGame".into(), json!({ "fields": { "additionalSpaces": [] }, "relatedPopulation": Value::Null }));
    g
}

pub fn configuration(x: &XDefiant, _app: &str, wanted: Option<&str>) -> Value {
    json!({ "parameters": filter(app_groups(x), wanted) })
}

pub fn application_parameters(x: &XDefiant, wanted: Option<&str>) -> Value {
    json!({ "parameters": filter(app_groups(x), wanted) })
}

pub fn space_parameters(x: &XDefiant, space: &str, wanted: Option<&str>) -> Value {
    json!({ "parameters": filter(space_groups(x, space), wanted) })
}
