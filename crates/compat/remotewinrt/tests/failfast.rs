#[test]
fn sta_initialization_fail_fasts_like_console() {
    if std::env::var("TOMBOX_SVC_FAILFAST_CHILD").is_ok() {
        remotewinrt::OnInitializationOfSingleThreadedApartment();
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sta_initialization_fail_fasts_like_console", "--nocapture"])
        .env("TOMBOX_SVC_FAILFAST_CHILD", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code().map(|c| c as u32), Some(0xC000_0409));
}
