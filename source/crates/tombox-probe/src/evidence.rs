use crate::console::ConsoleHit;
use crate::image::Entry;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Default)]
pub struct PcHit {
    pub resolved: bool,
    pub host_path: Option<String>,
    pub ordinal: Option<u32>,
    pub export_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub title: String,
    pub binary: String,
    pub module: String,
    pub entry: Entry,
    pub kind: String,
    pub check: String,
    pub pc: PcHit,
    pub console: ConsoleHit,
    pub iat_match: Option<bool>,
    pub null_checked: Option<bool>,
    pub note: Option<String>,
}

pub fn ordinal_meaning(entry: &Entry, pc: &PcHit, con: &ConsoleHit) -> String {
    let pc_part = match (&pc.host_path, pc.ordinal) {
        (Some(h), Some(o)) => {
            let file = h.rsplit(['\\', '/']).next().unwrap_or(h);
            format!("PC {file} ordinal {o}{}", pc.export_name.as_ref().map(|n| format!(" = {n}")).unwrap_or_default())
        }
        (Some(h), None) => format!("PC {} (ordinal not mapped)", h.rsplit(['\\', '/']).next().unwrap_or(h)),
        _ => "PC unresolved".to_string(),
    };
    let con_part = match (&con.host, con.ordinal) {
        (Some(h), Some(o)) => format!(
            "console {h} ordinal {o}{}{}",
            con.name_at_ordinal.as_ref().map(|n| format!(" = {n}")).unwrap_or_else(|| " (unnamed)".into()),
            if con.other_hosts > 0 { format!(" (+{} other console hosts)", con.other_hosts) } else { String::new() }
        ),
        _ if con.module_present => "console module present, entry not exported".to_string(),
        _ => "console host not found".to_string(),
    };
    let head = match entry {
        Entry::Name(n) => format!("by name {n}"),
        Entry::Ordinal(o) => format!("by ordinal {o}"),
    };
    format!("{head}; {pc_part}; {con_part}")
}

impl Row {
    pub fn function_name(&self) -> String {
        match &self.entry {
            Entry::Name(n) => n.clone(),
            Entry::Ordinal(o) => self
                .console
                .name_at_ordinal
                .clone()
                .or_else(|| self.pc.export_name.clone())
                .unwrap_or_else(|| format!("#{o}")),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("title".into(), json!(self.title));
        m.insert("binary".into(), json!(self.binary));
        m.insert("module".into(), json!(self.module));
        m.insert("function".into(), json!(self.function_name()));
        if let Entry::Ordinal(o) = &self.entry {
            m.insert("import_ordinal".into(), json!(o));
        }
        m.insert("kind".into(), json!(self.kind));
        m.insert("resolved".into(), json!(self.pc.resolved));
        m.insert("ordinal_meaning".into(), json!(ordinal_meaning(&self.entry, &self.pc, &self.console)));
        m.insert("check".into(), json!(self.check));
        m.insert("pc_host".into(), json!(self.pc.host_path));
        m.insert("pc_ordinal".into(), json!(self.pc.ordinal));
        m.insert("console_host".into(), json!(self.console.host));
        m.insert("console_ordinal".into(), json!(self.console.ordinal));
        m.insert("console_module_present".into(), json!(self.console.module_present));
        if let Some(v) = self.iat_match {
            m.insert("iat_match".into(), json!(v));
        }
        if let Some(v) = self.null_checked {
            m.insert("null_checked".into(), json!(v));
        }
        if let Some(e) = &self.pc.error {
            m.insert("error".into(), json!(e));
        }
        if let Some(n) = &self.note {
            m.insert("note".into(), json!(n));
        }
        Value::Object(m)
    }
}

pub fn summarize(rows: &[Row]) -> Value {
    let total = rows.len();
    let resolved = rows.iter().filter(|r| r.pc.resolved).count();
    let failed: Vec<Value> = rows
        .iter()
        .filter(|r| !r.pc.resolved)
        .map(|r| json!({"binary": r.binary, "module": r.module, "function": r.function_name(), "kind": r.kind, "error": r.pc.error}))
        .collect();
    let iat_mismatch = rows.iter().filter(|r| r.iat_match == Some(false)).count();
    json!({"rows": total, "resolved": resolved, "unresolved": total - resolved, "iat_mismatch": iat_mismatch, "failures": failed})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(resolved: bool) -> Row {
        Row {
            title: "Fortnite".into(),
            binary: "GameChat2.dll".into(),
            module: "WS2_32.dll".into(),
            entry: Entry::Name("htonl".into()),
            kind: "import".into(),
            check: "loader+GetProcAddress".into(),
            pc: PcHit {
                resolved,
                host_path: Some(r"C:\Windows\System32\WS2_32.dll".into()),
                ordinal: Some(8),
                export_name: Some("htonl".into()),
                error: if resolved { None } else { Some("ERROR_PROC_NOT_FOUND".into()) },
            },
            console: ConsoleHit { module_present: true, host: Some("ws2_32.dll".into()), ordinal: Some(8), name_at_ordinal: Some("htonl".into()), other_hosts: 0 },
            iat_match: Some(true),
            null_checked: None,
            note: None,
        }
    }

    #[test]
    fn row_json_has_g9_fields() {
        let v = row(true).to_json();
        for k in ["binary", "module", "function", "resolved", "ordinal_meaning", "kind", "pc_host", "console_host"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        assert_eq!(v["function"], "htonl");
        assert_eq!(v["resolved"], true);
        assert_eq!(v["ordinal_meaning"], "by name htonl; PC WS2_32.dll ordinal 8 = htonl; console ws2_32.dll ordinal 8 = htonl");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn ordinal_rows_and_summary() {
        let mut r = row(false);
        r.entry = Entry::Ordinal(1);
        r.console = ConsoleHit { module_present: true, ..Default::default() };
        r.pc.export_name = None;
        let v = r.to_json();
        assert_eq!(v["function"], "#1");
        assert_eq!(v["import_ordinal"], 1);
        let mut named = row(true);
        named.entry = Entry::Ordinal(8);
        assert_eq!(named.to_json()["function"], "htonl");
        assert!(v["ordinal_meaning"].as_str().unwrap().contains("console module present, entry not exported"));
        let s = summarize(&[row(true), r]);
        assert_eq!(s["rows"], 2);
        assert_eq!(s["unresolved"], 1);
        assert_eq!(s["failures"][0]["error"], "ERROR_PROC_NOT_FOUND");
    }
}
