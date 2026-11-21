use super::super::{util, Request, Response};
use super::{data, epic_error, Fortnite};
use serde_json::{json, Value};

const UPLOADED: &str = "2020-01-01T00:00:00.000Z";

fn file_entry(name: &str, bytes: &[u8]) -> Value {
    json!({
        "uniqueFilename": name,
        "filename": name,
        "hash": util::hex(&util::sha1(bytes)),
        "hash256": util::hex(&util::sha256(bytes)),
        "length": bytes.len(),
        "contentType": "application/octet-stream",
        "uploaded": UPLOADED,
        "storageType": "S3",
        "storageIds": {},
        "doNotCache": true,
    })
}

fn system_file(name: &str) -> Option<&'static str> {
    data::CLOUD_SYSTEM.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, c)| *c)
}

pub fn route(fnt: &Fortnite, req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    let ["fortnite", "api", "cloudstorage", rest @ ..] = seg else { return None };
    Some(match (m, rest) {
        ("GET", ["system"]) => Response::json(&Value::Array(
            data::CLOUD_SYSTEM.iter().map(|(n, c)| file_entry(n, super::qos::system_file(n, c).as_bytes())).collect(),
        )),
        ("GET", ["system", "config"]) => Response::bytes(200, "application/octet-stream", Vec::new()),
        ("GET", ["system", name]) => match system_file(name) {
            Some(c) => Response::bytes(200, "application/octet-stream", super::qos::system_file(name, c).as_bytes().to_vec()),
            None => Response::bytes(200, "application/octet-stream", Vec::new()),
        },
        ("GET", ["user", account]) => {
            let files = fnt.user_files.lock().map(|g| g.clone()).unwrap_or_default();
            let mut list: Vec<Value> = files.iter().filter(|(k, _)| k.starts_with(&format!("{account}/"))).map(|(k, v)| {
                let mut e = file_entry(k.split('/').nth(1).unwrap_or(""), v);
                e["accountId"] = json!(account);
                e
            }).collect();
            list.sort_by(|a, b| a["filename"].as_str().cmp(&b["filename"].as_str()));
            Response::json(&Value::Array(list))
        }
        ("GET", ["user", account, file]) => {
            let key = format!("{account}/{file}");
            let found = fnt.user_files.lock().ok().and_then(|g| g.iter().find(|(k, _)| k.eq_ignore_ascii_case(&key)).map(|(_, v)| v.clone()));
            match found {
                Some(v) => Response::bytes(200, "application/octet-stream", v),
                None if file.eq_ignore_ascii_case("ClientSettings.Sav") => Response::bytes(200, "application/octet-stream", Vec::new()),
                None => epic_error(404, "errors.com.epicgames.cloudstorage.file_not_found", "Sorry, we couldn't find a file for this account", 12004, &[file]),
            }
        }
        ("PUT", ["user", account, file]) => {
            if let Ok(mut g) = fnt.user_files.lock() {
                g.retain(|k, _| !k.eq_ignore_ascii_case(&format!("{account}/{file}")));
                g.insert(format!("{account}/{file}"), req.body.clone());
            }
            Response::empty(204)
        }
        ("DELETE", ["user", account, file]) => {
            if let Ok(mut g) = fnt.user_files.lock() {
                g.retain(|k, _| !k.eq_ignore_ascii_case(&format!("{account}/{file}")));
            }
            Response::empty(204)
        }
        _ => return None,
    })
}
