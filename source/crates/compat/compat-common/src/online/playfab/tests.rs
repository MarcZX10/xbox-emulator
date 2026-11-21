use super::*;
use std::net::UdpSocket;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn req(path: &str, headers: &[(&str, &str)], body: Value) -> Request {
    let mut h: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    h.push(("X-Forwarded-Host".into(), "40467.playfabapi.com".into()));
    Request { method: "POST".into(), path: path.into(), query: vec![], headers: h, body: body.to_string().into_bytes() }
}

fn call(path: &str, headers: &[(&str, &str)], body: Value) -> (u16, Value) {
    let r = route(&req(path, headers, body)).expect("routed");
    (r.status, serde_json::from_slice(&r.body).unwrap_or(Value::Null))
}

fn login() -> (String, String) {
    let (s, v) = call("/Client/LoginWithXbox", &[], json!({"TitleId": "40467", "XboxToken": "XBL3.0 x=123;tok", "CreateAccount": true}));
    assert_eq!(s, 200, "{v}");
    (v["data"]["SessionTicket"].as_str().unwrap().into(), v["data"]["EntityToken"]["EntityToken"].as_str().unwrap().into())
}

#[test]
fn playfab_flow() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("TOMBOX_PLAYFAB_QOS_PORT", "0");
    reset_for_tests();

    let (s, q) = call("/MultiplayerServer/ListPartyQosServers", &[], json!({"Version": "1.0"}));
    assert_eq!(s, 200);
    assert_eq!(q["status"], "OK");
    let servers = q["data"]["QosServers"].as_array().unwrap();
    assert!(!servers.is_empty() && servers[0]["ServerUrl"].as_str().unwrap().ends_with(".cloudapp.azure.com"));
    assert_eq!(q["data"]["PageSize"], json!(servers.len()));

    assert_eq!(call("/Party/RequestParty", &[], json!({"PreferredRegions": ["EastUs"]})).0, 401);
    assert_eq!(call("/Client/LoginWithXbox", &[], json!({"XboxToken": "garbage"})).0, 400);
    let (ticket, et) = login();
    let eh = [("X-EntityToken", et.as_str())];

    let (s, p) = call("/Party/RequestParty", &eh, json!({"PreferredRegions": ["WestUs", "EastUs"], "PartyId": "abc"}));
    assert_eq!(s, 200, "{p}");
    assert_eq!(p["data"]["Region"], "WestUs");
    assert_eq!(p["data"]["PartyId"], "abc");
    assert_eq!(p["data"]["Ports"][0]["Protocol"], "UDP");
    assert_eq!(call("/Party/RequestParty", &eh, json!({"PreferredRegions": []})).0, 400);

    let (s, c) = call("/MultiplayerServer/GetCognitiveServicesToken", &eh, json!({"CognitiveServicesType": "SpeechToText"}));
    assert_eq!(s, 200);
    assert!(c["data"]["Token"].is_string() && c["data"]["ExpirationTimeUtc"].is_string());

    let (s, e) = call("/Event/WriteTelemetryEvents", &eh, json!({"Events": [{"EventNamespace": "playfab.party", "Name": "client_qos_completed", "Payload": {}}]}));
    assert_eq!(s, 200);
    assert_eq!(e["data"]["AssignedEventIds"].as_array().unwrap().len(), 1);
    assert_eq!(call("/Event/WriteEvents", &eh, json!({"Events": [{"Name": "x"}]})).0, 400);

    let me = xuid().to_string();
    let (_, t) = call("/Profile/GetTitlePlayersFromXboxLiveIDs", &eh, json!({"XboxLiveIds": [me, "1"], "Sandbox": "RETAIL"}));
    assert_eq!(t["data"]["TitlePlayerAccounts"][&me]["Id"], json!(title_player_id()));
    assert!(t["data"]["TitlePlayerAccounts"]["1"].is_null());

    let th = [("X-Authorization", ticket.as_str())];
    call("/Client/UpdateUserData", &th, json!({"Data": {"a": "1"}}));
    let (_, d) = call("/Client/GetUserData", &th, json!({}));
    assert_eq!(d["data"]["Data"]["a"]["Value"], "1");
    assert_eq!(call("/Client/GetUserData", &[], json!({})).0, 401);

    let (s, l) = call("/Lobby/CreateLobby", &eh, json!({"MaxPlayers": 4, "AccessPolicy": "Public", "SearchData": {"string_key1": "forest", "number_key1": 3}}));
    assert_eq!(s, 200, "{l}");
    let lid = l["data"]["LobbyId"].as_str().unwrap().to_string();
    let (_, f) = call("/Lobby/FindLobbies", &eh, json!({"Filter": "string_key1 eq 'forest' and number_key1 ge 2"}));
    assert_eq!(f["data"]["Lobbies"].as_array().unwrap().len(), 1);
    let (_, f) = call("/Lobby/FindLobbies", &eh, json!({"Filter": "string_key1 eq 'desert'"}));
    assert_eq!(f["data"]["Lobbies"].as_array().unwrap().len(), 0);
    assert_eq!(call("/Lobby/CreateLobby", &eh, json!({"MaxPlayers": 4, "SearchData": {"bogus": 1}})).0, 400);
    call("/Lobby/UpdateLobby", &eh, json!({"LobbyId": lid, "LobbyData": {"world": "w1"}, "MembershipLock": "Locked"}));
    let (_, g) = call("/Lobby/GetLobby", &eh, json!({"LobbyId": lid}));
    assert_eq!(g["data"]["Lobby"]["LobbyData"]["world"], "w1");
    assert_eq!(g["data"]["Lobby"]["ChangeNumber"], 2);
    assert_eq!(call("/Lobby/LeaveLobby", &eh, json!({"LobbyId": lid})).0, 200);
    assert_eq!(call("/Lobby/GetLobby", &eh, json!({"LobbyId": lid})).0, 404);

    let ent = json!({"Id": title_player_id(), "Type": "title_player_account"});
    let (_, tk) = call("/Match/CreateMatchmakingTicket", &eh, json!({"QueueName": "q", "GiveUpAfterSeconds": 60, "Creator": {"Entity": ent}}));
    let tid = tk["data"]["TicketId"].as_str().unwrap().to_string();
    let (_, gt) = call("/Match/GetMatchmakingTicket", &eh, json!({"QueueName": "q", "TicketId": tid}));
    assert_eq!(gt["data"]["Status"], "Matched");
    let mid = gt["data"]["MatchId"].as_str().unwrap().to_string();
    let (_, m) = call("/Match/GetMatch", &eh, json!({"QueueName": "q", "MatchId": mid}));
    assert_eq!(m["data"]["Members"].as_array().unwrap().len(), 1);
    assert_eq!(call("/Match/CancelMatchmakingTicket", &eh, json!({"QueueName": "q", "TicketId": tid})).0, 400);
    let other = json!({"Id": "0000000000000001", "Type": "title_player_account"});
    let (_, tk) = call("/Match/CreateMatchmakingTicket", &eh, json!({"QueueName": "q2", "GiveUpAfterSeconds": 60, "Creator": {"Entity": ent}, "MembersToMatchWith": [other]}));
    let tid = tk["data"]["TicketId"].as_str().unwrap().to_string();
    let (_, gt) = call("/Match/GetMatchmakingTicket", &eh, json!({"QueueName": "q2", "TicketId": tid}));
    assert_eq!(gt["data"]["Status"], "WaitingForPlayers");
    call("/Match/CancelMatchmakingTicket", &eh, json!({"QueueName": "q2", "TicketId": tid}));
    let (_, gt) = call("/Match/GetMatchmakingTicket", &eh, json!({"QueueName": "q2", "TicketId": tid}));
    assert_eq!(gt["data"]["Status"], "Canceled");

    assert!(route(&req("/Client/NoSuchThing", &th, json!({}))).is_none());
}

#[test]
fn qos_probe_is_echoed_with_cleared_header() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(qos::reply_for(&[0xFF, 0xFF, 1, 2, 3]).unwrap(), vec![0, 0, 1, 2, 3]);
    assert!(qos::reply_for(&[1, 2]).is_none());
    std::env::set_var("TOMBOX_PLAYFAB_QOS_PORT", "0");
    if let Some(port) = qos::ensure_started() {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        s.send_to(&[0xFF, 0xFF, 9, 9], ("127.0.0.1", port)).unwrap();
        let mut buf = [0u8; 16];
        let (n, _) = s.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[0, 0, 9, 9]);
    }
}
