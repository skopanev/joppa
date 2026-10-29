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
