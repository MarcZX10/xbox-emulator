mod account;
mod cloud;
pub mod data;
mod mcp;
mod misc;
mod social;
pub mod video;
pub mod party; pub mod qos; pub mod matchmaking; pub mod session;

use super::{Request, Response, TitleService};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Fortnite {
    pub account_id: String,
    pub display_name: String,
    mcp: mcp::Store,
    user_files: Mutex<HashMap<String, Vec<u8>>>,
    privacy_opt_out: Mutex<bool>,
}

fn clean_name(v: String) -> Option<String> {
    let v = v.trim().to_string();
    (!v.is_empty() && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_graphic() && b != b'/' && b != b'?' && b != b'"' && b != b'\\')).then_some(v)
}

impl Fortnite {
    pub fn new() -> Fortnite {
        let account_id = std::env::var("TOMBOX_ONLINE_ACCOUNT").ok().or_else(|| crate::cfg("online_account_id"))
            .and_then(clean_name).unwrap_or_else(|| "XboxEmulator".into());
        let display_name = std::env::var("TOMBOX_ONLINE_NAME").ok().or_else(|| crate::cfg("online_display_name"))
            .and_then(clean_name).unwrap_or_else(|| account_id.clone());
        Fortnite::with_account(&account_id, &display_name)
    }

    pub fn with_account(account_id: &str, display_name: &str) -> Fortnite {
        Fortnite {
            account_id: account_id.to_string(),
            display_name: display_name.to_string(),
            mcp: mcp::Store::default(),
            user_files: Mutex::new(HashMap::new()),
            privacy_opt_out: Mutex::new(false),
        }
    }

    pub fn season(req: &Request) -> u32 {
        req.header("user-agent")
            .and_then(|ua| ua.split("Release-").nth(1))
            .and_then(|r| r.split('.').next())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(14)
    }
}

impl Default for Fortnite {
    fn default() -> Self {
        Fortnite::new()
    }
}

pub fn epic_error(status: u16, code: &str, message: &str, numeric: u32, vars: &[&str]) -> Response {
    Response::json_status(status, &json!({
        "errorCode": code,
        "errorMessage": message,
        "messageVars": vars,
        "numericErrorCode": numeric,
        "originatingService": "fortnite",
        "intent": "prod",
        "error_description": message,
        "error": code.rsplit('.').next().unwrap_or("error"),
    }))
}

impl TitleService for Fortnite {
    fn name(&self) -> &'static str {
        "fortnite"
    }

    fn route(&self, req: &Request) -> Option<Response> {
        let seg = req.segments();
        let m = req.method.as_str();
        party::route(self, req, m, &seg)
            .or_else(|| account::route(self, req, m, &seg))
            .or_else(|| mcp::route(self, req, m, &seg))
            .or_else(|| cloud::route(self, req, m, &seg))
            .or_else(|| social::route(self, req, m, &seg))
            .or_else(|| session::route(self, req, m, &seg))
            .or_else(|| matchmaking::route(self, req, m, &seg))
            .or_else(|| misc::route(self, req, m, &seg))
            .or_else(|| video::route(req, m, &seg))
    }

    fn not_found(&self, req: &Request) -> Response {
        epic_error(404, "errors.com.epicgames.common.not_found", "Sorry the resource you were trying to find could not be found", 1004, &[req.path.as_str()])
    }
}
