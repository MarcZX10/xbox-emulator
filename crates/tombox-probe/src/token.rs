use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenHit {
    pub file: PathBuf,
    pub offset: usize,
    pub wide: bool,
    pub context: String,
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn lower(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

fn context(data: &[u8], pos: usize, len: usize, wide: bool) -> String {
    let step = if wide { 2 } else { 1 };
    let mut lo = pos;
    while lo >= step && (0x20..0x7f).contains(&data[lo - step]) && (!wide || data[lo - step + 1] == 0) && pos - lo < 80 * step {
        lo -= step;
    }
    let mut hi = pos + len;
    while hi + step <= data.len() && (0x20..0x7f).contains(&data[hi]) && (!wide || data[hi + 1] == 0) && hi - pos < 120 * step {
        hi += step;
    }
    data[lo..hi].iter().step_by(step).map(|&c| c as char).collect()
}

pub fn find_in_bytes(data: &[u8], token: &str, exact: bool) -> Vec<(usize, bool, String)> {
    let t: Vec<u8> = token.bytes().map(lower).collect();
    let mut out = Vec::new();
    if t.is_empty() {
        return out;
    }
    for wide in [false, true] {
        let step = if wide { 2 } else { 1 };
        let span = t.len() * step;
        if data.len() < span {
            continue;
        }
        let mut i = 0usize;
        while i + span <= data.len() {
            if lower(data[i]) == t[0] {
                let ok = (0..t.len()).all(|k| lower(data[i + k * step]) == t[k] && (!wide || data[i + k * step + 1] == 0));
                if ok {
                    let before_word = i >= step && is_word(data[i - step]) && (!wide || data[i - step + 1] == 0);
                    let after = i + span;
                    let after_word = after < data.len() && is_word(data[after]) && (!wide || after + 1 < data.len() && data[after + 1] == 0);
                    if !exact || (!before_word && !after_word) {
                        out.push((i, wide, context(data, i, span, wide)));
                    }
                }
            }
            i += 1;
        }
    }
    out
}

pub fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut items: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

pub fn scannable(p: &Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "dll" | "exe" | "dat" | "json" | "txt" | "config" | "xml" | "ini")
}

pub fn search(root: &Path, token: &str, exact: bool) -> Vec<TokenHit> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut hits = Vec::new();
    for f in files.into_iter().filter(|f| scannable(f)) {
        let Ok(data) = std::fs::read(&f) else { continue };
        for (offset, wide, context) in find_in_bytes(&data, token, exact) {
            hits.push(TokenHit { file: f.clone(), offset, wide, context });
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_token_respects_identifier_boundaries() {
        let data = b"\0PartyXboxLive.dll\0xboxlive.dll\0";
        let hits = find_in_bytes(data, "xboxlive.dll", true);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 19);
        assert_eq!(hits[0].2, "xboxlive.dll");
        let near = find_in_bytes(data, "xboxlive.dll", false);
        assert_eq!(near.len(), 2);
        assert_eq!(near[0].2, "PartyXboxLive.dll");
    }

    #[test]
    fn wide_tokens_are_found() {
        let mut data = vec![0u8, 0];
        for c in "LoadXBOXLIVE.DLL".encode_utf16() {
            data.extend_from_slice(&c.to_le_bytes());
        }
        data.extend_from_slice(&[0, 0, 0, 0]);
        for c in " xboxlive.dll ".encode_utf16() {
            data.extend_from_slice(&c.to_le_bytes());
        }
        let exact = find_in_bytes(&data, "xboxlive.dll", true);
        assert_eq!(exact.len(), 1);
        assert!(exact[0].1);
        assert_eq!(exact[0].2, " xboxlive.dll ");
        assert_eq!(find_in_bytes(&data, "xboxlive.dll", false).len(), 2);
    }
}
