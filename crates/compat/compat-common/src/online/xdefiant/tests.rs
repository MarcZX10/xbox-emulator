use super::*;

fn req(method: &str, path: &str, query: &[(&str, &str)], headers: &[(&str, &str)], body: &str) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        query: query.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        body: body.as_bytes().to_vec(),
    }
}

fn body(r: &Response) -> Value {
    serde_json::from_slice(&r.body).unwrap_or(Value::Null)
}

fn x() -> XDefiant {
    XDefiant::with("XboxEmulator", DEFAULT_SPACE_ID)
}

#[test]
fn session_lifecycle() {
    let x = x();
    let r = x.route(&req("POST", "/v3/profiles/sessions", &[], &[("Ubi-AppId", "app"), ("Authorization", "xbl3.0 x=1;t")], "{}")).unwrap();
    let s = body(&r);
    for k in ["ticket", "environment", "expiration", "spaceId", "clientIp", "clientIpCountry", "serverTime", "sessionId", "profileId"] {
        assert!(s[k].is_string(), "{k} must be a non-null string");
    }
    assert_eq!(s["platformType"], "xbl");
    let e = body(&x.route(&req("PUT", "/v3/profiles/sessions", &[], &[], "")).unwrap());
    assert_eq!(e["sessionId"], s["sessionId"]);
    assert_eq!(x.route(&req("DELETE", "/v3/profiles/sessions", &[], &[], "")).unwrap().status, 200);
    assert_eq!(x.route(&req("GET", "/v3/profiles/sessions", &[], &[], "")).unwrap().status, 401);
}

#[test]
fn parameters_match_real_service_layout() {
    let x = x();
    let c = body(&x.route(&req("GET", "/v1/applications/abc/configuration", &[], &[], "")).unwrap());
    let p = &c["parameters"];
    assert_eq!(p["us-sdkClientFeaturesSwitches"]["fields"]["CreateSession"], true);
    assert_eq!(p["us-sdkClientFeaturesSwitches"]["fields"]["createSession"], true);
    assert_eq!(p["us-sdkClientFeaturesSwitches"]["fields"]["NotificationWebsocket"], true);
    assert_eq!(p["us-sdkClientFeaturesSwitches"]["fields"]["Mocks"], false);
    assert!(p["us-sdkClientUrls"]["fields"]["websocketNotifications"].as_str().unwrap().starts_with("wss://"));
    assert!(p["us-sdkClientUrlsPlaceholders"]["fields"]["baseurl_aws"]["Standard"].is_string());
    let names: Vec<&str> = include_str!("../../../backend_data/xdefiant/ubiservices_parameters.json").lines().collect();
    assert!(!names.is_empty());
    for r in ["profiles", "sessions", "populations", "configsEvents", "connections", "profilesFriends", "spacesParties", "usersOnlineStatuses", "eventsDefinitions"] {
        assert!(p["us-sdkClientUrls"]["fields"][r].is_string(), "missing url for {r}");
    }
    let only = body(&x.route(&req("GET", "/v1/spaces/s/parameters", &[("parameterGroups", "us-sdkClientSettings")], &[], "")).unwrap());
    assert_eq!(only["parameters"].as_object().unwrap().len(), 1);
}

#[test]
fn events_definitions_and_vivox_tokens() {
    let x = x();
    let d = body(&x.route(&req("GET", &format!("/v1/spaces/{DEFAULT_SPACE_ID}/configs/events/definitions"), &[], &[], "")).unwrap());
    assert_eq!(d["eventsDefinitions"].as_array().unwrap().len(), EVENT_TYPES.len());
    let one = body(&x.route(&req("GET", "/v1/spaces/s/configs/events/definitions", &[("names", "game.start")], &[], "")).unwrap());
    assert_eq!(one["eventsDefinitions"].as_array().unwrap().len(), 1);
    let cfg = body(&x.route(&req("GET", "/v1/spaces/s/configs/voicechat/vivox", &[], &[], "")).unwrap());
    assert!(cfg["domain"].is_string() && cfg["issuer"].is_string());
    let login = body(&x.route(&req("POST", "/v1/profiles/me/voicechattoken/vivox", &[], &[], r#"{"action":"login"}"#)).unwrap());
    let parts: Vec<&str> = login["token"].as_str().unwrap().split('.').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "e30");
    let join = body(&x.route(&req("POST", "/v1/profiles/me/voicechattoken/vivox", &[], &[], r#"{"action":"join","channelName":"squad_1"}"#)).unwrap());
    assert!(join["channel"].as_str().unwrap().starts_with("sip:confctl-g-"));
    assert_eq!(x.route(&req("POST", "/v1/profiles/me/voicechattoken/vivox", &[], &[], r#"{"action":"join"}"#)).unwrap().status, 400);
    assert_eq!(x.route(&req("POST", "/v1/profiles/me/voicechattoken/vivox", &[], &[], r#"{"action":"explode"}"#)).unwrap().status, 400);
    assert_eq!(x.route(&req("POST", "/v1/profiles/00000000-0000-0000-0000-000000000009/voicechattoken/vivox", &[], &[], "{}")).unwrap().status, 403);
}

#[test]
fn unknown_route_is_an_error_not_a_placeholder() {
    let x = x();
    assert!(x.route(&req("GET", "/v1/something/unknown", &[], &[], "")).is_none());
    assert_eq!(x.not_found(&req("GET", "/v1/something/unknown", &[], &[], "")).status, 404);
}

#[test]
fn telemetry_is_stored_and_queryable() {
    let x = x();
    let cfg = body(&x.route(&req("GET", &format!("/v1/spaces/{DEFAULT_SPACE_ID}/configs/events"), &[], &[], "")).unwrap());
    assert!(cfg["publicConfig"]["sendPeriodSeconds"].is_number() && cfg["types"].is_array());
    let r = x.route(&req("POST", &format!("/v1/profiles/{}/events", x.profile_id), &[], &[], "{\"events\":[{\"type\":\"player.start\"},{\"type\":\"game.start\"}]}")).unwrap();
    assert_eq!(r.status, 204);
    x.route(&req("POST", "/v1/profiles/me/remotelog", &[], &[], "{\"msg\":\"hi\"}")).unwrap();
    let t = body(&x.route(&req("GET", "/tombox/telemetry", &[], &[], "")).unwrap());
    assert_eq!(t["events"].as_array().unwrap().len(), 2);
    assert_eq!(t["events"][0]["event"]["type"], "player.start");
    assert_eq!(t["remoteLogs"][0]["log"]["msg"], "hi");
}

#[test]
fn entities_stats_inventory_preferences_are_stateful() {
    let x = x();
    let pid = x.profile_id.clone();
    let e = body(&x.route(&req("POST", &format!("/v1/profiles/{pid}/entities"), &[], &[], "{\"type\":\"loadout\",\"name\":\"slot1\",\"obj\":{\"gun\":\"m4\"}}")).unwrap());
    let eid = e["entityId"].as_str().unwrap().to_string();
    let l = body(&x.route(&req("GET", &format!("/v2/profiles/{pid}/entities"), &[("type", "loadout")], &[], "")).unwrap());
    assert_eq!(l["entities"][0]["obj"]["gun"], "m4");
    let u = body(&x.route(&req("PUT", &format!("/v2/profiles/{pid}/entities/{eid}"), &[], &[], "{\"obj\":{\"gun\":\"ak\"}}")).unwrap());
    assert_eq!(u["revision"], 2);
    assert_eq!(x.route(&req("DELETE", &format!("/v2/profiles/{pid}/entities/{eid}"), &[], &[], "")).unwrap().status, 200);
    x.route(&req("PUT", &format!("/v1/profiles/{pid}/stats"), &[], &[], "{\"stats\":{\"kills\":5}}")).unwrap();
    let s = body(&x.route(&req("GET", &format!("/v1/profiles/{pid}/stats"), &[], &[], "")).unwrap());
    assert_eq!(s["stats"]["kills"]["value"], 5);
    let t = x.route(&req("POST", &format!("/v1/profiles/{pid}/inventory/transactions"), &[], &[], "{\"addItems\":[{\"itemId\":\"skin-1\",\"quantity\":1}]}")).unwrap();
    assert_eq!(t.status, 201);
    let inv = body(&x.route(&req("GET", &format!("/v1/profiles/{pid}/inventory/primarystore"), &[], &[], "")).unwrap());
    assert_eq!(inv["items"][0]["itemId"], "skin-1");
    let p = body(&x.route(&req("PUT", &format!("/v1/profiles/{pid}/playerpreferences/standard"), &[], &[], "{\"crossPlayEnabled\":false}")).unwrap());
    assert_eq!(p["crossPlayEnabled"], false);
    let p = body(&x.route(&req("GET", &format!("/v1/profiles/{pid}/playerpreferences/standard"), &[], &[], "")).unwrap());
    assert_eq!(p["crossPlayEnabled"], false);
    let st = body(&x.route(&req("GET", "/tombox/state", &[], &[], "")).unwrap());
    assert!(st["requests"].as_object().unwrap().len() >= 5);
}

#[test]
fn parties_and_populations() {
    let x = x();
    let p = body(&x.route(&req("POST", &format!("/v1/spaces/{DEFAULT_SPACE_ID}/parties"), &[], &[], "{\"maxMembers\":2}")).unwrap());
    let id = p["partyId"].as_str().unwrap().to_string();
    let other = uuid_for("friend");
    let j = body(&x.route(&req("PUT", &format!("/v1/spaces/{DEFAULT_SPACE_ID}/parties/{id}/members/{other}"), &[], &[], "")).unwrap());
    assert_eq!(j["members"].as_array().unwrap().len(), 2);
    let mine = body(&x.route(&req("GET", "/v1/profiles/me/parties", &[], &[], "")).unwrap());
    assert_eq!(mine["parties"].as_array().unwrap().len(), 1);
    let pop = x.route(&req("PUT", "/v1/profiles/me/populations/data", &[], &[], "{\"spaceId\":\"s\",\"data\":{\"US_SDK_DURABLES\":[]}}")).unwrap();
    assert!(body(&pop)["populations"].is_array());
}

#[test]
fn notifications_reach_websocket_subscribers() {
    let x = XDefiant::with("NotifTest", DEFAULT_SPACE_ID);
    let mut n = Notifications::for_profile(&x.profile_id);
    let (outlet, rx) = super::super::ws::Outlet::detached();
    n.attach(outlet.clone());
    let open = n.on_open();
    let v: Value = serde_json::from_str(&open.send[0]).unwrap();
    assert_eq!(v["notificationType"], "US_CONNECTION_ESTABLISHED");
    assert!(v["content"]["connectionId"].is_string() && v["content"]["contactUrl"].is_string() && v["datePosted"].is_string());
    let c = body(&x.route(&req("GET", "/v1/profiles/connections", &[("profileIds", &x.profile_id)], &[], "")).unwrap());
    assert_eq!(c["connections"].as_array().unwrap().len(), 1);
    let up = x.route(&req("PUT", &format!("/v1/profiles/{}/connections", x.profile_id), &[], &[], "{\"notificationTypes\":[\"FRIENDS_RELATIONSHIP_UPDATE\"]}")).unwrap();
    assert_eq!(body(&up)["notificationTypes"][0], "FRIENDS_RELATIONSHIP_UPDATE");
    let sent = body(&x.route(&req("POST", &format!("/v1/profiles/{}/notifications", x.profile_id), &[], &[], "{\"notificationType\":\"CUSTOM\",\"content\":{\"a\":1}}")).unwrap());
    assert_eq!(sent["delivered"], 1);
    let pushed: Value = serde_json::from_str(&rx.recv().unwrap()).unwrap();
    assert_eq!(pushed["content"]["a"], 1);
    assert_eq!(pushed["sourceProfileId"], json!(x.profile_id));
    n.on_close();
    outlet.close();
    let c = body(&x.route(&req("GET", "/v1/profiles/connections", &[("profileIds", &x.profile_id)], &[], "")).unwrap());
    assert!(c["connections"].as_array().unwrap().is_empty());
}

#[test]
fn title_selection() {
    assert_eq!(super::super::title_for("xdefiant").unwrap().name(), "xdefiant");
    assert_eq!(super::super::title_for("fortnite").unwrap().name(), "fortnite");
}
