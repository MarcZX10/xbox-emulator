macro_rules! data {
    ($f:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/backend_data/fortnite/", $f))
    };
}

pub const CATALOG: &str = data!("catalog.json");
pub const KEYCHAIN: &str = data!("keychain.json");
pub const CONTENT_PAGES: &str = data!("contentpages.json");
pub const EPIC_SETTINGS: &str = data!("epic-settings.json");
pub const SDK_V1: &str = data!("sdkv1.json");
pub const TIMELINE: &str = data!("timeline.json");
pub const TOURNAMENT: &str = data!("tournament.json");
pub const TOURNAMENT_AND_HISTORY: &str = data!("tournamentandhistory.json");
pub const HISTORY: &str = data!("history.json");
pub const PRIVACY: &str = data!("privacy.json");
pub const QUESTS_ATHENA_S14: &str = data!("quests_athena_s14.json");

pub const CLOUD_SYSTEM: &[(&str, &str)] = &[
    ("DefaultEngine.ini", data!("DefaultEngine.ini")),
    ("DefaultGame.ini", data!("DefaultGame.ini")),
    ("DefaultInput.ini", data!("DefaultInput.ini")),
    ("DefaultRuntimeOptions.ini", data!("DefaultRuntimeOptions.ini")),
];

pub fn profile(id: &str) -> Option<&'static str> {
    Some(match id {
        "athena" => data!("profile_athena.json"),
        "common_core" => data!("profile_common_core.json"),
        "common_public" => data!("profile_common_public.json"),
        "creative" => data!("profile_creative.json"),
        "collections" => data!("profile_collections.json"),
        "metadata" => data!("profile_metadata.json"),
        "campaign" => data!("profile_campaign.json"),
        _ => return None,
    })
}
