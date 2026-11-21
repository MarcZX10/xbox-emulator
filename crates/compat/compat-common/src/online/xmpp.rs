use super::util::{b64_decode, random_uuid};
use super::ws::{Outlet, WsReply, WsSession};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const DOMAIN: &str = "prod.ol.epicgames.com";
pub const MUC_DOMAIN: &str = "muc.prod.ol.epicgames.com";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| local(&c.name) == name)
    }

    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }
}

fn local(n: &str) -> &str {
    n.rsplit(':').next().unwrap_or(n)
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let ent = &tail[1..end];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_misc(&mut self) {
        loop {
            let r = &self.s[self.i..];
            let t = r.trim_start();
            self.i += r.len() - t.len();
            if t.starts_with("<?") {
                match t.find("?>") {
                    Some(e) => self.i += e + 2,
                    None => self.i = self.s.len(),
                }
            } else if t.starts_with("<!--") {
                match t.find("-->") {
                    Some(e) => self.i += e + 3,
                    None => self.i = self.s.len(),
                }
            } else {
                return;
            }
        }
    }

    fn element(&mut self, depth: usize) -> Option<Element> {
        if depth > 64 {
            return None;
        }
        self.skip_misc();
        let r = &self.s[self.i..];
        if !r.starts_with('<') || r.starts_with("</") {
            return None;
        }
        let end = find_tag_end(r)?;
        let tag = &r[1..end];
        self.i += end + 1;
        let self_closing = tag.ends_with('/');
        let tag = tag.trim_end_matches('/').trim();
        let (name, attrs) = parse_tag(tag)?;
        let mut el = Element { name, attrs, children: Vec::new(), text: String::new() };
        if self_closing {
            return Some(el);
        }
        loop {
            let r = &self.s[self.i..];
            if r.is_empty() {
                return Some(el);
            }
            if r.starts_with("</") {
                let e = r.find('>')?;
                self.i += e + 1;
                return Some(el);
            }
            if r.starts_with("<!--") || r.starts_with("<?") {
                self.skip_misc();
                continue;
            }
            if r.starts_with("<![CDATA[") {
                let e = r.find("]]>")?;
                el.text.push_str(&r[9..e]);
                self.i += e + 3;
                continue;
            }
            if r.starts_with('<') {
                let c = self.element(depth + 1)?;
                el.children.push(c);
                continue;
            }
            let e = r.find('<').unwrap_or(r.len());
            el.text.push_str(&unescape(&r[..e]));
            self.i += e;
        }
    }
}

fn find_tag_end(r: &str) -> Option<usize> {
    let mut q: Option<char> = None;
    for (i, c) in r.char_indices() {
        match (q, c) {
            (None, '"') | (None, '\'') => q = Some(c),
            (Some(x), y) if x == y => q = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_tag(tag: &str) -> Option<(String, Vec<(String, String)>)> {
    let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
    let name = tag[..name_end].to_string();
    if name.is_empty() {
        return None;
    }
    let mut attrs = Vec::new();
    let mut rest = tag[name_end..].trim_start();
    while !rest.is_empty() {
        let eq = rest.find('=')?;
        let k = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let q = after.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let close = after[1..].find(q)? + 1;
        attrs.push((k, unescape(&after[1..close])));
        rest = after[close + 1..].trim_start();
    }
    Some((name, attrs))
}

pub fn parse_all(s: &str) -> Vec<Element> {
    let mut p = Parser { s, i: 0 };
    let mut v = Vec::new();
    while p.i < s.len() {
        match p.element(0) {
            Some(e) => v.push(e),
            None => {
                let r = &s[p.i..];
                match r[1.min(r.len())..].find('<') {
                    Some(n) => p.i += n + 1,
                    None => break,
                }
            }
        }
    }
    v
}

#[derive(Clone)]
struct Member {
    jid: String,
    nick: String,
    extra: String,
}

struct Room {
    members: Vec<Member>,
}

fn rooms() -> &'static Mutex<HashMap<String, Room>> {
    static R: OnceLock<Mutex<HashMap<String, Room>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn room_members(room: &str) -> Vec<String> {
    rooms().lock().unwrap_or_else(|p| p.into_inner()).get(room).map(|r| r.members.iter().map(|m| m.jid.clone()).collect()).unwrap_or_default()
}

pub fn default_account() -> String {
    std::env::var("TOMBOX_ONLINE_ACCOUNT").ok().or_else(|| crate::cfg("online_account_id")).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.len() <= 64).unwrap_or_else(|| "XboxEmulator".into())
}

fn bare(j: &str) -> String {
    j.split('/').next().unwrap_or("").to_ascii_lowercase()
}

fn full_ch(j: &str) -> String {
    format!("xmpp-full:{}", j.to_ascii_lowercase())
}

fn muc_presence(to: &str, room: &str, m: &Member, self_codes: &str, unavailable: bool) -> String {
    let ty = if unavailable { " type=\"unavailable\"" } else { "" };
    let role = if unavailable { "role=\"none\" affiliation=\"none\"" } else { "role=\"moderator\" affiliation=\"owner\"" };
    format!(
        "<presence to=\"{}\" from=\"{}@{MUC_DOMAIN}/{}\" xmlns=\"jabber:client\"{ty}>{}<x xmlns=\"http://jabber.org/protocol/muc#user\"><item nick=\"{}\" jid=\"{}\" {role}/>{self_codes}</x></presence>",
        escape(to), escape(room), escape(&m.nick), m.extra, escape(&m.nick), escape(&m.jid))
}

pub fn render(e: &Element) -> String {
    let attrs: String = e.attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", escape(v))).collect();
    if e.children.is_empty() && e.text.is_empty() {
        return format!("<{}{attrs}/>", e.name);
    }
    let kids: String = e.children.iter().map(render).collect();
    format!("<{0}{attrs}>{1}{kids}</{0}>", e.name, escape(&e.text))
}

pub struct XmppSession {
    authed: bool,
    account: String,
    jid: String,
    stream_id: String,
    presence: Option<String>,
    joined: Vec<(String, String)>,
    outlet: Option<Outlet>,
}

impl XmppSession {
    pub fn new() -> XmppSession {
        let account = default_account();
        XmppSession { authed: false, jid: format!("{account}@{DOMAIN}"), account, stream_id: random_uuid(), presence: None, joined: Vec::new(), outlet: None }
    }

    fn subscribe_self(&self) {
        if let Some(o) = &self.outlet {
            o.subscribe(&format!("xmpp:{}", bare(&self.jid)));
            o.subscribe(&full_ch(&self.jid));
        }
    }

    fn deliver(&self, ch: &str, text: String) -> usize {
        match &self.outlet {
            Some(o) => super::ws::publish_except(ch, &text, o.id()),
            None => 0,
        }
    }

    fn open_reply(&mut self) -> Vec<String> {
        self.stream_id = random_uuid();
        let open = format!("<open xmlns=\"urn:ietf:params:xml:ns:xmpp-framing\" from=\"{DOMAIN}\" id=\"{}\" version=\"1.0\" xml:lang=\"en\"/>", self.stream_id);
        let features = if self.authed {
            "<stream:features xmlns:stream=\"http://etherx.jabber.org/streams\"><ver xmlns=\"urn:xmpp:features:rosterver\"/><bind xmlns=\"urn:ietf:params:xml:ns:xmpp-bind\"/><session xmlns=\"urn:ietf:params:xml:ns:xmpp-session\"/></stream:features>"
        } else {
            "<stream:features xmlns:stream=\"http://etherx.jabber.org/streams\"><mechanisms xmlns=\"urn:ietf:params:xml:ns:xmpp-sasl\"><mechanism>PLAIN</mechanism></mechanisms><ver xmlns=\"urn:xmpp:features:rosterver\"/><auth xmlns=\"http://jabber.org/features/iq-auth\"/></stream:features>"
        };
        vec![open, features.to_string()]
    }

    fn iq_result(&self, id: &str, inner: &str) -> String {
        if inner.is_empty() {
            format!("<iq to=\"{}\" from=\"{DOMAIN}\" id=\"{}\" xmlns=\"jabber:client\" type=\"result\"/>", escape(&self.jid), escape(id))
        } else {
            format!("<iq to=\"{}\" from=\"{DOMAIN}\" id=\"{}\" xmlns=\"jabber:client\" type=\"result\">{inner}</iq>", escape(&self.jid), escape(id))
        }
    }

    fn iq_error(&self, id: &str, cond: &str) -> String {
        format!("<iq to=\"{}\" from=\"{DOMAIN}\" id=\"{}\" xmlns=\"jabber:client\" type=\"error\"><error type=\"cancel\"><{cond} xmlns=\"urn:ietf:params:xml:ns:xmpp-stanzas\"/></error></iq>", escape(&self.jid), escape(id))
    }

    fn handle(&mut self, e: &Element) -> WsReply {
        match local(&e.name) {
            "open" => WsReply { send: self.open_reply(), close: false },
            "close" => WsReply { send: vec!["<close xmlns=\"urn:ietf:params:xml:ns:xmpp-framing\"/>".into()], close: true },
            "auth" => {
                let raw = b64_decode(e.text.trim());
                let parts: Vec<String> = raw.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned()).collect();
                if let Some(a) = parts.get(1).filter(|a| !a.is_empty() && a.len() <= 64 && !a.contains(['@', '/', '<', '>', '"'])) {
                    self.account = a.clone();
                }
                self.authed = true;
                self.jid = format!("{}@{DOMAIN}", self.account);
                self.subscribe_self();
                WsReply::one("<success xmlns=\"urn:ietf:params:xml:ns:xmpp-sasl\"/>".into())
            }
            "iq" => self.iq(e),
            "presence" => self.presence(e),
            "message" => self.message(e),
            other => {
                crate::record_raw("online", "xmpp-unhandled", &format!("<{other}> stanza from {}", self.jid));
                WsReply::none()
            }
        }
    }

    fn iq(&mut self, e: &Element) -> WsReply {
        let id = e.attr("id").unwrap_or("").to_string();
        let ty = e.attr("type").unwrap_or("");
        if let Some(b) = e.child("bind") {
            let res = b.child_text("resource").map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).unwrap_or_else(|| format!("V2:Fortnite:XSX::{}", &self.stream_id[..8]));
            self.jid = format!("{}@{DOMAIN}/{res}", self.account);
            self.subscribe_self();
            let inner = format!("<bind xmlns=\"urn:ietf:params:xml:ns:xmpp-bind\"><jid>{}</jid></bind>", escape(&self.jid));
            return WsReply::one(format!("<iq to=\"{}\" id=\"{}\" xmlns=\"jabber:client\" type=\"result\">{inner}</iq>", escape(&self.jid), escape(&id)));
        }
        if let Some(to) = e.attr("to").filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case(DOMAIN) && bare(t) != bare(&self.jid)) {
            let fwd = format!("<iq to=\"{}\" from=\"{}\" id=\"{}\" type=\"{}\" xmlns=\"jabber:client\">{}</iq>", escape(to), escape(&self.jid), escape(&id), escape(ty), e.children.iter().map(render).collect::<String>());
            let ch = if to.contains('/') { full_ch(to) } else { format!("xmpp:{}", bare(to)) };
            if self.deliver(&ch, fwd) > 0 || ty == "result" || ty == "error" {
                return WsReply::none();
            }
            return WsReply::one(self.iq_error(&id, "service-unavailable"));
        }
        if ty == "result" || ty == "error" {
            return WsReply::none();
        }
        if e.child("session").is_some() || e.child("ping").is_some() {
            return WsReply::one(self.iq_result(&id, ""));
        }
        if let Some(q) = e.child("query") {
            match q.attr("xmlns") {
                Some("jabber:iq:roster") => return WsReply::one(self.iq_result(&id, "<query xmlns=\"jabber:iq:roster\"/>")),
                Some("http://jabber.org/protocol/disco#info") => {
                    return WsReply::one(self.iq_result(&id, "<query xmlns=\"http://jabber.org/protocol/disco#info\"><identity category=\"server\" type=\"im\" name=\"TomBox XMPP\"/><feature var=\"urn:xmpp:ping\"/><feature var=\"http://jabber.org/protocol/muc\"/><feature var=\"jabber:iq:roster\"/></query>"))
                }
                Some("http://jabber.org/protocol/disco#items") => return WsReply::one(self.iq_result(&id, &format!("<query xmlns=\"http://jabber.org/protocol/disco#items\"><item jid=\"{MUC_DOMAIN}\"/></query>"))),
                _ => {}
            }
        }
        let what = e.children.first().map(|c| format!("{} {}", c.name, c.attr("xmlns").unwrap_or(""))).unwrap_or_default();
        crate::record_raw("online", "xmpp-unhandled", &format!("iq type={ty} {what}"));
        WsReply::one(self.iq_error(&id, "feature-not-implemented"))
    }

    fn own_presence(&self, to: &str, status: &str, show: Option<&str>) -> String {
        let show = show.map(|s| format!("<show>{}</show>", escape(s))).unwrap_or_default();
        format!("<presence to=\"{}\" xmlns=\"jabber:client\" from=\"{}\" type=\"available\">{show}<status>{}</status></presence>", escape(to), escape(&self.jid), escape(status))
    }

    fn leave_room(&mut self, room: &str, nick: &str) -> Member {
        let me = Member { jid: self.jid.clone(), nick: nick.to_string(), extra: String::new() };
        let others = {
            let mut g = rooms().lock().unwrap_or_else(|p| p.into_inner());
            let o = match g.get_mut(room) {
                Some(r) => {
                    r.members.retain(|m| m.jid != self.jid);
                    r.members.clone()
                }
                None => Vec::new(),
            };
            if o.is_empty() {
                g.remove(room);
            }
            o
        };
        for o in &others {
            self.deliver(&full_ch(&o.jid), muc_presence(&o.jid, room, &me, "", true));
        }
        self.joined.retain(|(r, _)| r != room);
        me
    }

    fn presence(&mut self, e: &Element) -> WsReply {
        let to = e.attr("to").unwrap_or("");
        let ty = e.attr("type").unwrap_or("");
        if let Some((room_jid, nick)) = to.split_once('/').filter(|(r, _)| r.contains("@muc.")) {
            let room = room_jid.split('@').next().unwrap_or("").to_string();
            if ty == "unavailable" {
                let me = self.leave_room(&room, nick);
                return WsReply::one(muc_presence(&self.jid, &room, &me, "<status code=\"110\"/>", true));
            }
            let extra: String = e.children.iter().filter(|c| local(&c.name) != "x").map(render).collect();
            let me = Member { jid: self.jid.clone(), nick: nick.to_string(), extra };
            let (created, others) = {
                let mut g = rooms().lock().unwrap_or_else(|p| p.into_inner());
                let r = g.entry(room.clone()).or_insert_with(|| Room { members: Vec::new() });
                r.members.retain(|m| m.jid != self.jid);
                let created = r.members.is_empty();
                let others = r.members.clone();
                r.members.push(me.clone());
                (created, others)
            };
            if !self.joined.iter().any(|(rr, _)| *rr == room) {
                self.joined.push((room.clone(), nick.to_string()));
            }
            let mut out = Vec::new();
            for o in &others {
                out.push(muc_presence(&self.jid, &room, o, "", false));
                self.deliver(&full_ch(&o.jid), muc_presence(&o.jid, &room, &me, "", false));
            }
            let codes = if created { "<status code=\"110\"/><status code=\"100\"/><status code=\"170\"/><status code=\"201\"/>" } else { "<status code=\"110\"/><status code=\"100\"/><status code=\"170\"/>" };
            out.push(muc_presence(&self.jid, &room, &me, codes, false));
            return WsReply { send: out, close: false };
        }
        if ty == "unavailable" {
            self.presence = None;
            let p = format!("<presence to=\"{}\" from=\"{}\" xmlns=\"jabber:client\" type=\"unavailable\"/>", escape(&bare(&self.jid)), escape(&self.jid));
            self.deliver(&format!("xmpp:{}", bare(&self.jid)), p);
            return WsReply::none();
        }
        if !ty.is_empty() && ty != "available" {
            crate::record_raw("online", "xmpp-unhandled", &format!("presence type={ty} to={to}"));
            return WsReply::none();
        }
        let status = e.child_text("status").unwrap_or("").to_string();
        let show = e.child_text("show").map(|s| s.to_string());
        self.presence = Some(status.clone());
        let b = bare(&self.jid);
        self.deliver(&format!("xmpp:{b}"), self.own_presence(&b, &status, show.as_deref()));
        WsReply::one(self.own_presence(&self.jid, &status, show.as_deref()))
    }

    fn message(&mut self, e: &Element) -> WsReply {
        let to = e.attr("to").unwrap_or("");
        let ty = e.attr("type").unwrap_or("");
        let id = e.attr("id").map(|i| format!(" id=\"{}\"", escape(i))).unwrap_or_default();
        let inner: String = e.children.iter().map(render).collect();
        if ty == "groupchat" {
            let room = to.split('@').next().unwrap_or("").to_string();
            let nick = self.joined.iter().find(|(r, _)| *r == room).map(|(_, n)| n.clone()).unwrap_or_else(|| self.account.clone());
            let build = |m: &str| format!("<message to=\"{}\" from=\"{}@{MUC_DOMAIN}/{}\" xmlns=\"jabber:client\" type=\"groupchat\"{id}>{inner}</message>", escape(m), escape(&room), escape(&nick));
            for m in room_members(&room) {
                if m != self.jid {
                    self.deliver(&full_ch(&m), build(&m));
                }
            }
            return WsReply::one(build(&self.jid));
        }
        let tattr = if ty.is_empty() { String::new() } else { format!(" type=\"{}\"", escape(ty)) };
        let msg = format!("<message to=\"{}\" from=\"{}\" xmlns=\"jabber:client\"{tattr}{id}>{inner}</message>", escape(to), escape(&self.jid));
        let ch = if to.contains('/') { full_ch(to) } else { format!("xmpp:{}", bare(to)) };
        let delivered = self.deliver(&ch, msg.clone());
        if bare(to) == bare(&self.jid) {
            return WsReply::one(msg);
        }
        if delivered == 0 {
            return WsReply::one(format!("<message to=\"{}\" from=\"{}\" xmlns=\"jabber:client\" type=\"error\"{id}><error type=\"cancel\"><service-unavailable xmlns=\"urn:ietf:params:xml:ns:xmpp-stanzas\"/></error></message>", escape(&self.jid), escape(to)));
        }
        WsReply::none()
    }
}

impl Default for XmppSession {
    fn default() -> Self {
        XmppSession::new()
    }
}

impl WsSession for XmppSession {
    fn name(&self) -> &'static str {
        "xmpp"
    }

    fn attach(&mut self, outlet: Outlet) {
        self.outlet = Some(outlet);
    }

    fn on_text(&mut self, text: &str) -> WsReply {
        let mut out = WsReply::none();
        for e in parse_all(text) {
            let r = self.handle(&e);
            out.send.extend(r.send);
            if r.close {
                out.close = true;
                break;
            }
        }
        out
    }

    fn on_close(&mut self) {
        let joined = self.joined.clone();
        for (room, nick) in joined {
            self.leave_room(&room, &nick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::util::b64;

    #[test]
    fn parser_handles_attrs_children_entities() {
        let v = parse_all("<?xml version='1.0'?><presence to=\"a@b\"><status>{&quot;x&quot;:1}</status><show>away</show></presence><iq id='1' type='get'/>");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].attr("to"), Some("a@b"));
        assert_eq!(v[0].child_text("status"), Some("{\"x\":1}"));
        assert_eq!(v[1].attr("id"), Some("1"));
    }

    #[test]
    fn full_login_presence_party() {
        let mut s = XmppSession::new();
        let r = s.on_text("<open xmlns=\"urn:ietf:params:xml:ns:xmpp-framing\" to=\"prod.ol.epicgames.com\" version=\"1.0\"/>");
        assert!(r.send[0].starts_with("<open "));
        assert!(r.send[1].contains("PLAIN"));
        let token = b64(b"\0XboxEmulator\0eg1~token");
        let r = s.on_text(&format!("<auth mechanism=\"PLAIN\" xmlns=\"urn:ietf:params:xml:ns:xmpp-sasl\">{token}</auth>"));
        assert!(r.send[0].contains("success"));
        let r = s.on_text("<open xmlns=\"urn:ietf:params:xml:ns:xmpp-framing\" to=\"prod.ol.epicgames.com\" version=\"1.0\"/>");
        assert!(r.send[1].contains("xmpp-bind"));
        let r = s.on_text("<iq type=\"set\" id=\"_xmpp_bind1\" xmlns=\"jabber:client\"><bind xmlns=\"urn:ietf:params:xml:ns:xmpp-bind\"><resource>V2:Fortnite:XSX::ABC</resource></bind></iq>");
        assert!(r.send[0].contains("<jid>XboxEmulator@prod.ol.epicgames.com/V2:Fortnite:XSX::ABC</jid>"), "{:?}", r.send);
        let r = s.on_text("<iq type=\"set\" id=\"_xmpp_session1\"><session xmlns=\"urn:ietf:params:xml:ns:xmpp-session\"/></iq>");
        assert!(r.send[0].contains("id=\"_xmpp_session1\"") && r.send[0].contains("type=\"result\""));
        let r = s.on_text("<iq type=\"get\" id=\"r1\"><query xmlns=\"jabber:iq:roster\"/></iq>");
        assert!(r.send[0].contains("jabber:iq:roster"));
        let r = s.on_text("<iq type=\"get\" id=\"p1\"><ping xmlns=\"urn:xmpp:ping\"/></iq>");
        assert!(r.send[0].contains("id=\"p1\""));
        let r = s.on_text("<presence><status>{\"Status\":\"Battle Royale Lobby\",\"bIsPlaying\":false}</status><show>away</show></presence>");
        assert!(r.send[0].contains("&quot;Status&quot;") && r.send[0].contains("<show>away</show>"));
        let r = s.on_text("<presence to=\"Party-abc@muc.prod.ol.epicgames.com/XboxEmulator:XboxEmulator:V2:Fortnite:XSX::ABC\"><x xmlns=\"http://jabber.org/protocol/muc\"><history maxstanzas=\"50\"/></x></presence>");
        assert!(r.send[0].contains("status code=\"110\"") && r.send[0].contains("status code=\"201\""));
        let r = s.on_text("<message to=\"Party-abc@muc.prod.ol.epicgames.com\" type=\"groupchat\"><body>hi</body></message>");
        assert!(r.send[0].contains("type=\"groupchat\"") && r.send[0].contains("<body>hi</body>"));
        let r = s.on_text("<presence to=\"Party-abc@muc.prod.ol.epicgames.com/XboxEmulator:XboxEmulator:V2:Fortnite:XSX::ABC\" type=\"unavailable\"/>");
        assert!(r.send[0].contains("unavailable"));
        let r = s.on_text("<close xmlns=\"urn:ietf:params:xml:ns:xmpp-framing\"/>");
        assert!(r.close);
    }
}
