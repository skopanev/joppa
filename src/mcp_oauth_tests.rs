use super::*;
#[test]
fn callback_validation_does_not_confuse_host_prefixes_with_loopback() {
    for u in [
        "https://client.example/callback",
        "http://localhost:3456/callback",
        "http://127.0.0.1:3456/callback",
    ] {
        assert!(valid_redirect(u), "{u}");
    }
    for u in [
        "http://localhost.attacker.example/cb",
        "http://127.0.0.1.attacker.example/cb",
        "https://user:pass@client.example/cb",
        "https://client.example/#frag",
        "javascript:alert(1)",
        "https://client.example/cb?code=bad",
    ] {
        assert!(!valid_redirect(u), "{u}");
    }
}
