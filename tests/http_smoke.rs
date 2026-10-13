//! Runs tests/http_smoke.py in the Rust suite against the server binary of this build.
#[test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
fn http_smoke() {
    let status = std::process::Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/http_smoke.py"))
        .env("JOPPA_BIN", env!("CARGO_BIN_EXE_joppa"))
        .status()
        .expect("python3 starts");
    assert!(status.success(), "tests/http_smoke.py failed: {status}");
}
