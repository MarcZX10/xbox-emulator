use super::super::{util, Request, Response};
use super::{data, Fortnite};
use serde_json::{json, Value};

fn client_id(req: &Request) -> String {
    req.header("authorization")
        .and_then(|a| a.split_whitespace().nth(1))
        .map(|b| String::from_utf8_lossy(&util::b64_decode(b)).into_owned())
        .and_then(|s| s.split(':').next().map(|c| c.to_string()))
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "ec684b8c687f479fadea3cb2ad83f5c6".into())
}

fn jwt(payload: &Value) -> String {
    let header = json!({"kid": "xboxemulator", "typ": "JWT", "alg": "RS512"});
    format!(
        "eg1~{}.{}.{}",
        util::b64url(header.to_string().as_bytes()),
        util::b64url(payload.to_string().as_bytes()),
        util::b64url(b"xboxemulator-signature")
    )
}

fn oauth_token(fnt: &Fortnite, req: &Request) -> Response {
    let grant = req.form("grant_type").unwrap_or_else(|| "exchange_code".into());
    let eg1 = req.form("token_type").map_or(false, |t| t == "eg1");
    let clid = client_id(req);
    let now = util::now_secs();
    let is_client = grant == "client_credentials";
    let token = |exp: u64, kind: &str| -> String {
        if !eg1 {
            return util::random_hex32();
        }
        let payload = if is_client {
            json!({"p": "", "clsvc": "fortnite", "t": kind, "mver": false, "clid": clid, "ic": true,
                   "exp": now + exp, "am": "client_credentials", "iat": now, "jti": util::random_hex32()})
        } else {
            json!({"app": "fortnite", "sub": fnt.account_id, "dvid": "xboxemulator", "mver": false, "clid": clid,
                   "dn": fnt.display_name, "am": grant, "p": "", "iai": fnt.account_id, "sec": 1, "clsvc": "fortnite",
                   "t": kind, "ic": true, "exp": now + exp, "iat": now, "jti": util::random_hex32()})
        };
        jwt(&payload)
    };
    if is_client {
        return Response::json(&json!({
            "access_token": token(28800, "s"),
            "expires_in": 28800,
            "expires_at": util::iso_in(28800),
            "token_type": "bearer",
            "client_id": clid,
            "internal_client": true,
            "client_service": "fortnite",
            "product_id": "prod-fn",
            "application_id": "fghi4567FNFBKFz3E4TROb0bmPS8h1GW",
        }));
    }
    Response::json(&json!({
        "access_token": token(28800, "s"),
        "expires_in": 28800,
        "expires_at": util::iso_in(28800),
        "token_type": "bearer",
        "refresh_token": token(86400, "r"),
        "refresh_expires": 86400,
        "refresh_expires_at": util::iso_in(86400),
        "account_id": fnt.account_id,
        "client_id": clid,
        "internal_client": true,
        "client_service": "fortnite",
        "displayName": fnt.display_name,
        "app": "fortnite",
        "in_app_id": fnt.account_id,
        "device_id": "xboxemulatordevice",
        "product_id": "prod-fn",
        "application_id": "fghi4567FNFBKFz3E4TROb0bmPS8h1GW",
    }))
}

fn account_entry(fnt: &Fortnite, id: &str) -> Value {
    let id = id.split('@').next().unwrap_or(id);
    let dn = if id == fnt.account_id { fnt.display_name.as_str() } else { id };
    json!({"id": id, "displayName": dn, "externalAuths": {}})
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    Some(match (m, seg) {
        ("POST", ["account", "api", "oauth", "token"]) => oauth_token(fnt, req),
        ("GET", ["account", "api", "oauth", "verify"]) => Response::json(&json!({
            "token": req.header("authorization").and_then(|a| a.split_whitespace().nth(1)).unwrap_or(""),
            "session_id": util::random_hex32(),
            "token_type": "bearer",
            "client_id": client_id(req),
            "internal_client": true,
            "client_service": "fortnite",
            "account_id": fnt.account_id,
            "expires_in": 28800,
            "expires_at": util::iso_in(28800),
            "auth_method": "exchange_code",
            "display_name": fnt.display_name,
            "app": "fortnite",
            "in_app_id": fnt.account_id,
            "device_id": "xboxemulatordevice",
        })),
        ("DELETE", ["account", "api", "oauth", "sessions", "kill", ..]) => Response::empty(204),
        ("POST", ["account", "api", "oauth", "exchange"]) => Response::json(&json!({})),
        ("GET", ["account", "api", "oauth", "exchange"]) => Response::json(&json!({
            "expiresInSeconds": 300, "code": util::random_hex32(), "creatingClientId": client_id(req)
        })),
        ("GET", ["account", "api", "epicdomains", "ssodomains"]) => Response::json(&json!(["unrealengine.com", "unrealtournament.com", "fortnite.com", "epicgames.com"])),
        ("GET", ["account", "api", "public", "account"]) => {
            let ids = req.query_all("accountId");
            Response::json(&Value::Array(ids.iter().map(|id| account_entry(fnt, id)).collect()))
        }
        ("GET", ["account", "api", "public", "account", _, "externalAuths"]) => Response::json(&json!([])),
        ("GET", ["account", "api", "public", "account", "displayName", name]) => Response::json(&account_entry(fnt, name)),
        ("GET", ["account", "api", "public", "account", id]) => Response::json(&json!({
            "id": id,
            "displayName": if *id == fnt.account_id { fnt.display_name.as_str() } else { id },
            "name": fnt.display_name,
            "email": format!("{}@xboxemulator.local", fnt.account_id),
            "failedLoginAttempts": 0,
            "lastLogin": util::iso_now(),
            "numberOfDisplayNameChanges": 0,
            "ageGroup": "UNKNOWN",
            "headless": false,
            "country": "US",
            "lastName": "Player",
            "preferredLanguage": "en",
            "canUpdateDisplayName": false,
            "tfaEnabled": false,
            "emailVerified": true,
            "minorVerified": false,
            "minorExpected": false,
            "minorStatus": "NOT_MINOR",
            "cabinedMode": false,
            "hasHashedEmail": false,
        })),
        ("GET", ["sdk", "v1", ..]) => Response::json_str(data::SDK_V1),
        ("POST", ["auth", "v1", "oauth", "token"]) => Response::json(&json!({
            "access_token": util::random_hex32(),
            "token_type": "bearer",
            "expires_in": 28800,
            "expires_at": util::iso_in(28800),
            "nonce": util::random_hex32(),
            "features": ["AntiCheat", "CommerceService", "Connect", "ContentService", "Ecom", "EpicConnect", "Inventories", "LockerService", "MagpieService", "Matchmaking Service", "PCBService", "QuestService", "Stats"],
            "deployment_id": "62a9473a2dca46b29ccf17577fcf42d7",
            "organization_id": "o-aa83a0a9bc45e98c80c1b1c9d92e9e",
            "organization_user_id": format!("{}-org", fnt.account_id),
            "product_id": "prod-fn",
            "product_user_id": format!("{}-pu", fnt.account_id),
            "product_user_id_created": false,
            "id_token": util::random_hex32(),
            "sandbox_id": "fn",
        })),
        ("GET", ["epic", "id", "v2", "sdk", "accounts"]) => Response::json(&json!([{
            "accountId": fnt.account_id, "displayName": fnt.display_name, "preferredLanguage": "en", "cabinedMode": false, "empty": false
        }])),
        ("POST", ["epic", "oauth", "v2", "token"]) => Response::json(&json!({
            "scope": "basic_profile friends_list openid presence",
            "token_type": "bearer",
            "access_token": util::random_hex32(),
            "expires_in": 28800,
            "expires_at": util::iso_in(28800),
            "refresh_token": util::random_hex32(),
            "refresh_expires_in": 86400,
            "refresh_expires_at": util::iso_in(86400),
            "account_id": fnt.account_id,
            "client_id": client_id(req),
            "application_id": "fghi4567FNFBKFz3E4TROb0bmPS8h1GW",
            "selected_account_id": fnt.account_id,
            "id_token": util::random_hex32(),
        })),
        ("POST", ["fortnite", "api", "game", "v2", "tryPlayOnPlatform", "account", _]) => Response::bytes(200, "text/plain", b"true".to_vec()),
        _ => return None,
    })
}
