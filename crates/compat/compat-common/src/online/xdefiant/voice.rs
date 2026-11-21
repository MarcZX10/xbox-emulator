use super::super::util::{b64url, hex, hmac_sha256, now_secs, sha256};
use super::super::Request;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const TOKEN_SECS: u64 = 90;
const ACTIONS: &[&str] = &["login", "join", "join_muted", "kick", "mute", "trxn"];

static SERIAL: AtomicU64 = AtomicU64::new(1);

fn issuer() -> String {
    crate::cfg("vivox_issuer").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "tombox-xdefiant".into())
}

fn domain() -> String {
    crate::cfg("vivox_domain").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "mt1s.vivox.com".into())
}

fn secret() -> &'static Vec<u8> {
    static S: OnceLock<Vec<u8>> = OnceLock::new();
    S.get_or_init(|| crate::cfg("vivox_secret").map(|s| s.into_bytes()).unwrap_or_else(|| sha256(format!("tombox-vivox:{}:{}", issuer(), super::super::util::random_uuid()).as_bytes()).to_vec()))
}

pub fn config(space: &str) -> Value {
    let d = domain();
    json!({
        "spaceId": space,
        "server": format!("https://{}/api2", d.replace("vivox.com", "www.vivox.com")),
        "domain": d,
        "issuer": issuer(),
        "tokenExpirationSeconds": TOKEN_SECS,
    })
}

fn user_name(profile: &str) -> String {
    profile.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

pub fn user_uri(profile: &str) -> String {
    format!("sip:.{}.{}.@{}", issuer(), user_name(profile), domain())
}

fn channel_uri(name: &str, kind: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 200 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'!' | b'~' | b'(' | b')' | b'*' | b'+' | b'=')) {
        return Err(format!("invalid Vivox channel name '{name}'"));
    }
    let prefix = match kind.to_ascii_lowercase().as_str() {
        "" | "group" | "nonpositional" | "non_positional" | "g" => "g",
        "echo" | "e" => "e",
        "positional" | "d" => "d",
        other => return Err(format!("unknown Vivox channel type '{other}'")),
    };
    Ok(format!("sip:confctl-{prefix}-{}.{name}@{}", issuer(), domain()))
}

fn field<'a>(b: &'a Value, req: &'a Request, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| b.get(*k).and_then(Value::as_str).map(String::from).or_else(|| req.query(k).map(String::from)))
}

pub fn sign(payload: &Value) -> String {
    let head = b64url(b"{}");
    let body = b64url(payload.to_string().as_bytes());
    let sig = b64url(&hmac_sha256(secret(), format!("{head}.{body}").as_bytes()));
    format!("{head}.{body}.{sig}")
}

pub fn token(profile: &str, space: &str, req: &Request) -> Result<Value, String> {
    let b = req.json();
    let action = field(&b, req, &["action", "vxa"]).unwrap_or_else(|| "login".into()).to_ascii_lowercase();
    if !ACTIONS.contains(&action.as_str()) {
        return Err(format!("unknown Vivox token action '{action}'"));
    }
    let exp = now_secs() + TOKEN_SECS;
    let vxi = SERIAL.fetch_add(1, Ordering::Relaxed);
    let from = user_uri(profile);
    let mut p = json!({ "iss": issuer(), "exp": exp, "vxa": action, "vxi": vxi, "f": from });
    let mut channel = Value::Null;
    if matches!(action.as_str(), "join" | "join_muted" | "kick" | "mute" | "trxn") {
        let name = field(&b, req, &["channelName", "channel", "channelId", "conferenceName"]).ok_or_else(|| "a channel name is required for this action".to_string())?;
        let kind = field(&b, req, &["channelType", "type"]).unwrap_or_default();
        let uri = if name.starts_with("sip:") { name.clone() } else { channel_uri(&name, &kind)? };
        p["t"] = json!(uri);
        channel = json!(uri);
    }
    if matches!(action.as_str(), "kick" | "mute") {
        let target = field(&b, req, &["targetProfileId", "subject", "sub", "target"]).ok_or_else(|| "a target is required for this action".to_string())?;
        p["sub"] = json!(if target.starts_with("sip:") { target } else { user_uri(&target) });
    }
    let t = sign(&p);
    Ok(json!({
        "token": t,
        "vivoxToken": t,
        "action": action,
        "channel": channel,
        "userUri": from,
        "spaceId": space,
        "expiration": exp,
        "expirationTime": super::super::util::iso_in(TOKEN_SECS),
        "fingerprint": hex(&sha256(t.as_bytes())[..8]),
    }))
}
