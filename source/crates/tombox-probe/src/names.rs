pub fn normalize_module(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase().replace('/', "\\");
    let base = lower.rsplit('\\').next().unwrap_or(&lower).to_string();
    if base.contains('.') {
        base
    } else {
        format!("{base}.dll")
    }
}

pub fn is_api_set(name: &str) -> bool {
    let n = normalize_module(name);
    n.starts_with("api-ms-") || n.starts_with("ext-ms-")
}

pub fn api_set_contract(name: &str) -> Option<String> {
    if !is_api_set(name) {
        return None;
    }
    let n = normalize_module(name);
    let stem = n.strip_suffix(".dll").unwrap_or(&n);
    match stem.rfind('-') {
        Some(i) => Some(stem[..i].to_string()),
        None => Some(stem.to_string()),
    }
}

pub fn is_vc_redist(name: &str) -> bool {
    let n = normalize_module(name);
    const REDIST: [&str; 4] = ["concrt140", "msvcp140", "vcruntime140", "vccorlib140"];
    REDIST.iter().any(|r| n.starts_with(r))
}

pub fn same_module(a: &str, b: &str) -> bool {
    normalize_module(a) == normalize_module(b)
}

pub fn is_loader_api(name: &str) -> bool {
    matches!(
        name,
        "LoadLibraryExW" | "LoadLibraryExA" | "LoadLibraryW" | "LoadLibraryA" | "GetModuleHandleW" | "GetModuleHandleA" | "GetModuleHandleExW" | "GetModuleHandleExA" | "LoadPackagedLibrary"
    )
}

pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '?' => {}
        _ => return false,
    }
    s.len() >= 2 && s.chars().all(|c| c.is_ascii_alphanumeric() || "_?@$".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_module_names() {
        assert_eq!(normalize_module("WINHTTP.dll"), "winhttp.dll");
        assert_eq!(normalize_module("PartyXboxLive"), "partyxboxlive.dll");
        assert_eq!(normalize_module("Data/Plugins/PartyWin.dll"), "partywin.dll");
        assert_eq!(normalize_module(" C:\\x\\CONCRT140.DLL "), "concrt140.dll");
    }

    #[test]
    fn api_set_contracts() {
        assert!(is_api_set("api-ms-win-core-synch-l1-2-0.dll"));
        assert!(is_api_set("EXT-MS-win-foo-l1-1-0"));
        assert!(!is_api_set("msvcp_win.dll"));
        assert_eq!(api_set_contract("api-ms-win-core-synch-l1-2-0.dll").as_deref(), Some("api-ms-win-core-synch-l1-2"));
        assert_eq!(api_set_contract("kernel32.dll"), None);
    }

    #[test]
    fn redist_and_identifiers() {
        assert!(is_vc_redist("CONCRT140.dll"));
        assert!(is_vc_redist("msvcp140_atomic_wait.dll"));
        assert!(!is_vc_redist("msvcp_win.dll"));
        assert!(is_identifier("CreateOpusDecoderHardwareInterface"));
        assert!(is_identifier("?lock@critical_section@Concurrency@@QEAAXXZ"));
        assert!(!is_identifier("application/json"));
        assert!(!is_identifier("%llu"));
        assert!(is_loader_api("LoadLibraryExW"));
        assert!(!is_loader_api("GetProcAddress"));
        assert!(same_module("GameChat2", "gamechat2.DLL"));
    }
}
