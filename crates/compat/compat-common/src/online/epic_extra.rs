use super::{Request, Response};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn store() -> &'static Mutex<HashMap<String, Map<String, Value>>> {
    static S: OnceLock<Mutex<HashMap<String, Map<String, Value>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn merge_body(map: &mut Map<String, Value>, body: &Value) {
    match body {
        Value::Object(o) => {
            for (k, v) in o {
                map.insert(k.clone(), v.clone());
            }
        }
        Value::Array(a) => {
            for item in a {
                if let (Some(k), Some(v)) = (item.get("key").and_then(|k| k.as_str()), item.get("value")) {
                    map.insert(k.to_string(), v.clone());
                }
            }
        }
        _ => {}
    }
}

fn metadata(req: &Request, account: &str, key: Option<&str>) -> Response {
    let mut g = store().lock().unwrap_or_else(|p| p.into_inner());
    let map = g.entry(account.to_string()).or_default();
    match (req.method.as_str(), key) {
        ("GET", None) => Response::json(&Value::Object(map.clone())),
        ("GET", Some(k)) => match map.get(k) {
            Some(v) => Response::json(&json!({"key": k, "value": v})),
            None => Response::empty(204),
        },
        ("PUT" | "POST" | "PATCH", None) => {
            merge_body(map, &req.json());
            Response::json(&Value::Object(map.clone()))
        }
        ("PUT" | "POST" | "PATCH", Some(k)) => {
            let body = req.json();
            let v = match &body {
                Value::Object(o) if o.contains_key("value") => o["value"].clone(),
                Value::Null => Value::String(req.body_str()),
                other => other.clone(),
            };
            map.insert(k.to_string(), v.clone());
            Response::json(&json!({"key": k, "value": v}))
        }
        ("DELETE", None) => {
            map.clear();
            Response::empty(204)
        }
        ("DELETE", Some(k)) => {
            map.remove(k);
            Response::empty(204)
        }
        _ => Response::empty(405),
    }
}

pub fn route(req: &Request) -> Option<Response> {
    let seg = req.segments();
    let r = match (req.method.as_str(), seg.as_slice()) {
        (_, ["account", "api", "accounts", id, "metadata"]) => metadata(req, id, None),
        (_, ["account", "api", "accounts", id, "metadata", key]) => metadata(req, id, Some(key)),
        ("POST", ["presence", "api", "v1", _, _, "subscriptions", "broadcast"]) => Response::empty(204),
        ("DELETE", ["presence", "api", "v1", _, _, "subscriptions", "broadcast"]) => Response::empty(204),
        ("POST" | "DELETE", ["presence", "api", "v1", _, _, "subscriptions", ..]) => Response::empty(204),
        _ => return None,
    };
    crate::record_raw("online", "epic-extra", &format!("{} {} -> {}", req.method, req.path, r.status));
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, path: &str, body: &str) -> Request {
        Request { method: method.into(), path: path.into(), query: Vec::new(), headers: Vec::new(), body: body.as_bytes().to_vec() }
    }

    #[test]
    fn metadata_roundtrip_and_missing() {
        let p = "/account/api/accounts/test-acct-1/metadata/FGVoiceChatSettings";
        assert_eq!(route(&req("GET", p, "")).unwrap().status, 204);
        assert_eq!(route(&req("PUT", p, "{\"value\":\"on\"}")).unwrap().status, 200);
        let r = route(&req("GET", p, "")).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&r.body).unwrap()["value"], "on");
        let all = route(&req("GET", "/account/api/accounts/test-acct-1/metadata", "")).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&all.body).unwrap()["FGVoiceChatSettings"], "on");
        assert_eq!(route(&req("DELETE", p, "")).unwrap().status, 204);
        assert_eq!(route(&req("GET", p, "")).unwrap().status, 204);
    }

    #[test]
    fn broadcast_subscription() {
        let r = route(&req("POST", "/presence/api/v1/_/acct/subscriptions/broadcast", "{}")).unwrap();
        assert_eq!(r.status, 204);
        assert!(route(&req("GET", "/nothing/here", "")).is_none());
    }
}
