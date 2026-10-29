//! Part of `web_auth`: `cookie` and the items after it, moved from web_auth.rs unchanged.
use super::*;

pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| {
            let (key, value) = p.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}

pub fn set_cookie(headers: &HeaderMap, name: &str, value: &str, age: u64) -> String {
    let secure = headers
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|o| o.starts_with("https://"));
    format!(
        "{name}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={age}{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn check_origin(headers: &HeaderMap) -> Result<()> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("origin required"))?;
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("host required"))?;
    ensure!(
        origin == format!("http://{host}") || origin == format!("https://{host}"),
        "cross-origin request denied"
    );
    Ok(())
}

#[cfg(test)]
impl Login {
    /// One invited user and no rules file.
    pub(crate) fn for_tests() -> Self {
        Login {
            users: BTreeMap::from([("owner@example.test".into(), "owner".into())]),
            rules: None,
            google: Some(
                Google::new(crate::google_auth::Config {
                    client_id: "joppa-client".into(),
                    client_secret: "private".into(),
                    redirect_uri: "https://joppa.example/auth/google/callback".into(),
                })
                .unwrap(),
            ),
            pending: Default::default(),
        }
    }
    /// The callback step after Google verified the identity.
    pub(crate) async fn verified_for_tests(
        &self,
        pool: &deadpool_postgres::Pool,
        id: &str,
        identity: &Identity,
    ) {
        let outcome = outcome(self.enroll(pool, identity).await, false);
        self.pending.lock().unwrap().get_mut(id).unwrap().result = Some(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn login() -> Login {
        Login::for_tests()
    }
    #[tokio::test]
    async fn state_is_browser_bound_and_single_use() {
        let l = login();
        let pool = joppa::store::pool(joppa::store::DEFAULT_URL).unwrap();
        let (id, url) = l.start(None).await.unwrap();
        assert!(
            url["verification_url"]
                .as_str()
                .unwrap()
                .starts_with("https://accounts.google.com/")
        );
        assert!(!url.to_string().contains(&id));
        assert!(l.poll(&id).await.unwrap().is_none());
        assert!(l.poll("another-browser").await.is_err());
        assert!(
            l.callback(&pool, &id, "wrong-state", None, true)
                .await
                .is_err()
        );
        let state = l.pending.lock().unwrap()[&id].state.clone();
        assert!(
            l.callback(&pool, "another-browser", &state, None, true)
                .await
                .is_err()
        );
        l.callback(&pool, &id, &state, None, true).await.unwrap();
        assert!(l.callback(&pool, &id, &state, None, true).await.is_err());
        assert!(l.poll(&id).await.is_err());
        let (id, _) = l.start(None).await.unwrap();
        l.pending.lock().unwrap().get_mut(&id).unwrap().result = Some(Ok(("owner".into(), None)));
        let (token, actor, hd) = l.poll(&id).await.unwrap().unwrap();
        assert_eq!(actor, "owner");
        assert_eq!(hd, None);
        assert!(l.poll(&id).await.is_err());
        assert_eq!(token.len(), 64);
        let (id, _) = l.start(None).await.unwrap();
        l.pending.lock().unwrap().get_mut(&id).unwrap().expires =
            Instant::now() - Duration::from_secs(1);
        assert!(l.poll(&id).await.is_err());
    }
    #[test]
    fn domain_enrollment_uses_google_subject_not_colliding_email_initials() {
        let path = std::env::temp_dir().join(format!("joppa-access-{}.json", random()));
        std::fs::write(
            &path,
            r#"{"domains":{"alpha.example":["alpha"],"beta.example":["beta"]}}"#,
        )
        .unwrap();
        let mut l = login();
        l.rules = Some(path.clone());
        let identity = |sub: &str, email: &str, hd: Option<&str>| {
            serde_json::from_value::<Identity>(
                json!({"sub":sub,"email":email,"email_verified":true,"hd":hd,"nonce":"n"}),
            )
            .unwrap()
        };
        let a = l
            .admit(
                &identity("google-alpha", "same@alpha.example", Some("alpha.example")),
                false,
            )
            .unwrap();
        let b = l
            .admit(
                &identity("google-beta", "same@beta.example", Some("beta.example")),
                false,
            )
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(
            a,
            l.admit(
                &identity("google-alpha", "same@alpha.example", Some("alpha.example")),
                false
            )
            .unwrap()
        );
        assert!(
            l.admit(&identity("personal", "same@alpha.example", None), false)
                .is_err()
        );
        // A grant admits an email that the rules file does not name.
        let maria = identity("google-maria", "maria@example.com", None);
        assert!(l.admit(&maria, false).is_err());
        assert!(l.admit(&maria, true).unwrap().starts_with("user-"));
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn failed_exchange_and_unverified_identity_keep_the_generic_sign_in_error() {
        use axum::{Router, routing::post};
        let generic =
            "Sign-in could not be completed. Check your account has access, then try again.";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route(
            "/token",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    r#"{"error":"invalid_grant","id_token":"leaked-id-token"}"#,
                )
            }),
        );
        let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
        let mut l = login();
        l.google = l.google.map(|g| g.with_token_url(format!("{base}/token")));
        let pool = joppa::store::pool(joppa::store::DEFAULT_URL).unwrap();
        let (id, _) = l.start(None).await.unwrap();
        let state = l.pending.lock().unwrap()[&id].state.clone();
        l.callback(&pool, &id, &state, Some("one-time-code"), false)
            .await
            .unwrap();
        let error = l.poll(&id).await.unwrap_err().to_string();
        assert_eq!(error, generic);
        assert!(!error.contains("one-time-code") && !error.contains("leaked-id-token"));
        server.abort();
        // An unverified identity is refused before any grant lookup, with the same text.
        let unverified = serde_json::from_value::<Identity>(
            json!({"sub":"google-1","email":"owner@example.test","email_verified":false,"nonce":"n"}),
        )
        .unwrap();
        let (id, _) = l.start(None).await.unwrap();
        l.verified_for_tests(&pool, &id, &unverified).await;
        assert_eq!(l.poll(&id).await.unwrap_err().to_string(), generic);
    }
    #[test]
    fn verified_google_email_requires_an_explicit_joppa_invitation() {
        let l = login();
        let identity = |email: &str, verified: bool| {
            serde_json::from_value::<Identity>(
                json!({"sub":"google-1","email":email,"email_verified":verified,"nonce":"n"}),
            )
            .unwrap()
        };
        assert_eq!(
            l.admit(&identity("OWNER@example.test", true), false)
                .unwrap(),
            "owner"
        );
        assert!(
            l.admit(&identity("outsider@example.test", true), false)
                .is_err()
        );
        assert!(
            l.admit(&identity("owner@example.test", false), false)
                .is_err()
        );
        // An owner email grant admits a verified email that no user entry names.
        assert!(
            l.admit(&identity("outsider@example.test", true), true)
                .unwrap()
                .starts_with("user-")
        );
        assert!(
            l.admit(&identity("outsider@example.test", false), true)
                .is_err()
        );
    }
}
