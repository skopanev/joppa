use super::*;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
fn claims() -> Value {
    json!({"iss":"https://accounts.google.com","aud":"joppa-test-client","exp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+300,"sub":"google-user-1","email":"owner@example.test","email_verified":true,"nonce":"browser-nonce"})
}
fn token(claims: &Value) -> String {
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("test-only".into());
    encode(
        &h,
        claims,
        &EncodingKey::from_rsa_pem(include_bytes!("../tests/fixtures/google-test-key.pem"))
            .unwrap(),
    )
    .unwrap()
}
fn keys() -> Keys {
    serde_json::from_str(include_str!("../tests/fixtures/google-test-jwks.json")).unwrap()
}
#[test]
fn google_identity_requires_signature_audience_issuer_expiry_nonce_and_verified_email() {
    assert_eq!(
        verify(
            &token(&claims()),
            "joppa-test-client",
            "browser-nonce",
            &keys()
        )
        .unwrap()
        .email,
        "owner@example.test"
    );
    for (key, value) in [
        ("aud", json!("another-client")),
        ("iss", json!("https://attacker.example")),
        ("exp", json!(1)),
        ("email_verified", json!(false)),
        ("nonce", json!("another-browser")),
        ("sub", json!("")),
    ] {
        let mut c = claims();
        c[key] = value;
        assert!(
            verify(&token(&c), "joppa-test-client", "browser-nonce", &keys()).is_err(),
            "{key} must be checked"
        );
    }
    let mut tampered = token(&claims());
    let i = tampered.rfind('.').unwrap() + 2;
    tampered.replace_range(i..i + 1, if &tampered[i..i + 1] == "A" { "B" } else { "A" });
    assert!(verify(&tampered, "joppa-test-client", "browser-nonce", &keys()).is_err());
    let hmac = encode(
        &Header::new(Algorithm::HS256),
        &claims(),
        &EncodingKey::from_secret(b"not-a-google-key"),
    )
    .unwrap();
    assert!(verify(&hmac, "joppa-test-client", "browser-nonce", &keys()).is_err());
}

#[test]
fn verified_profile_names_are_optional_and_do_not_replace_identity_keys() {
    let read = |value: &serde_json::Value| {
        verify(&token(value), "joppa-test-client", "browser-nonce", &keys()).unwrap()
    };
    let mut value = claims();
    assert_eq!(read(&value).display_name(), None);
    value["given_name"] = json!("Rita");
    value["family_name"] = json!("Petrov");
    assert_eq!(read(&value).display_name().as_deref(), Some("Rita Petrov"));
    value["name"] = json!(" Rita Ivanova ");
    assert_eq!(read(&value).display_name().as_deref(), Some("Rita Ivanova"));
    assert_eq!(read(&value).sub, "google-user-1");
    value["name"] = json!("Bad\nName");
    assert_eq!(read(&value).display_name(), None);
}
#[test]
fn authorization_goes_directly_to_google_and_uses_pkce() {
    let g = Google::new(Config {
        client_id: "joppa-test-client".into(),
        client_secret: "private-secret".into(),
        redirect_uri: "https://joppa.example/auth/google/callback".into(),
    })
    .unwrap();
    let u = reqwest::Url::parse(&g.authorize("state", "nonce", "verifier")).unwrap();
    assert_eq!(u.host_str(), Some("accounts.google.com"));
    let q: std::collections::BTreeMap<_, _> = u.query_pairs().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["state"], "state");
    assert_eq!(q["nonce"], "nonce");
    assert!(!u.as_str().contains("private-secret"));
    assert!(!q.contains_key("code_verifier"));
}
#[tokio::test]
async fn code_exchange_stays_server_side_and_validates_google_response() {
    use axum::{
        Form, Json, Router,
        routing::{get, post},
    };
    async fn exchange(Form(form): Form<std::collections::BTreeMap<String, String>>) -> Json<Value> {
        assert_eq!(form["code"], "one-time-code");
        assert_eq!(form["code_verifier"], "pkce-verifier");
        assert_eq!(form["client_secret"], "private-secret");
        Json(json!({"id_token":token(&claims())}))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route("/token", post(exchange)).route(
        "/keys",
        get(|| async {
            Json(
                serde_json::from_str::<Value>(include_str!(
                    "../tests/fixtures/google-test-jwks.json"
                ))
                .unwrap(),
            )
        }),
    );
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let mut g = Google::new(Config {
        client_id: "joppa-test-client".into(),
        client_secret: "private-secret".into(),
        redirect_uri: "https://joppa.example/auth/google/callback".into(),
    })
    .unwrap();
    g.token_url = format!("{base}/token");
    g.keys_url = format!("{base}/keys");
    assert_eq!(
        g.exchange("one-time-code", "browser-nonce", "pkce-verifier")
            .await
            .unwrap()
            .sub,
        "google-user-1"
    );
    server.abort();
}
