#[test]
fn service_main_runs_console_22000_bootsh_against_sandbox() {
    bootshsvc::sandbox_test::run_console_bootsh(r"D:\xbox-pc-port\games\XDefiant\extracted\EmbeddedXvd\Windows\System32\bootshsvc.dll");
}