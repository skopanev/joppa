//! Part of `web_auth`: `impl Login` and the items after it, moved from web_auth.rs unchanged.
use super::*;

impl Login {
    pub fn public_origin(&self) -> Result<Option<String>> {
        let origin = std::env::var("JOPPA_PUBLIC_URL").ok().or_else(|| {
            self.google.as_ref().map(|g| {
                reqwest::Url::parse(&g.config.redirect_uri)
                    .unwrap()
                    .origin()
                    .ascii_serialization()
            })
        });
        if let Some(raw) = &origin {
            let u = reqwest::Url::parse(raw)?;
            ensure!(
                (u.scheme() == "https"
                    || (u.scheme() == "http"
                        && matches!(u.host_str(), Some("localhost" | "127.0.0.1"))))
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
                    && u.query().is_none()
                    && u.fragment().is_none()
                    && u.path() == "/",
                "JOPPA_PUBLIC_URL must be an HTTPS origin (HTTP only on loopback)"
            );
            return Ok(Some(u.origin().ascii_serialization()));
        }
        Ok(None)
    }

    pub fn from_env() -> Result<Self> {
        // Keys are verified Google email addresses, never another product's user IDs.
        let users: BTreeMap<String, String> = match std::env::var("JOPPA_LOGIN_USERS_FILE") {
            Ok(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
            Err(_) => BTreeMap::new(),
        };
        for (email, actor) in &users {
            ensure!(
                email.contains('@'),
                "login users must be Google email addresses"
            );
            joppa::commands::identifier(actor)?;
            ensure!(actor != "joppa-access", "reserved system identity");
        }
        let normalized: BTreeMap<_, _> = users
            .iter()
            .map(|(email, actor)| (email.to_ascii_lowercase(), actor.clone()))
            .collect();
        ensure!(
            normalized.len() == users.len(),
            "duplicate email identities"
        );
        ensure!(
            users
                .values()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == users.len(),
            "each Google email must have its own actor"
        );
        let rules = std::env::var_os("JOPPA_ACCESS_RULES_FILE").map(std::path::PathBuf::from);
        if let Some(path) = &rules {
            joppa::access::Rules::read(path)?;
        }
        Ok(Self {
            users: normalized,
            rules,
            google: Google::from_env()?,
            pending: Default::default(),
        })
    }
    pub async fn start(&self, previous: Option<&str>) -> Result<(String, Value)> {
        let google = self.google.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Google sign-in is being configured for Joppa. Please try again later.")
        })?;
        ensure!(
            !self.users.is_empty() || self.rules.is_some(),
            "No Joppa accounts have been invited yet."
        );
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, p| p.expires > Instant::now());
        if let Some(id) = previous {
            pending.remove(id);
        }
        ensure!(
            pending.len() < 100,
            "Too many sign-in attempts. Please try again later."
        );
        let id = random();
        let p = Pending {
            state: random(),
            nonce: random(),
            verifier: random(),
            expires: Instant::now() + Duration::from_secs(300),
            processing: false,
            result: None,
        };
        let url = google.authorize(&p.state, &p.nonce, &p.verifier);
        pending.insert(id.clone(), p);
        Ok((id, json!({"verification_url":url,"expires_in":300})))
    }
    pub async fn callback(
        &self,
        pool: &deadpool_postgres::Pool,
        id: &str,
        state: &str,
        code: Option<&str>,
        cancelled: bool,
    ) -> Result<()> {
        let p = {
            let mut all = self.pending.lock().unwrap();
            let p = all
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("Sign-in expired. Please try again."))?;
            ensure!(
                p.expires > Instant::now()
                    && p.state == state
                    && !p.processing
                    && p.result.is_none(),
                "Invalid or expired sign-in state"
            );
            p.processing = true;
            p.clone()
        };
        let result = if cancelled {
            Err(anyhow::anyhow!(
                "Google sign-in was cancelled. Please try again."
            ))
        } else if let Some(code) = code {
            match &self.google {
                Some(google) => match google.exchange(code, &p.nonce, &p.verifier).await {
                    Ok(identity) => self.enroll(pool, &identity).await,
                    Err(error) => Err(error),
                },
                None => Err(anyhow::anyhow!("Google sign-in has not been configured")),
            }
        } else {
            Err(anyhow::anyhow!("Google did not return a sign-in code"))
        };
        if let Some(p) = self.pending.lock().unwrap().get_mut(id) {
            p.result = Some(outcome(result, cancelled));
        }
        Ok(())
    }
    /// A rule, a user entry or an owner email or domain grant admits a verified Google identity.
    pub(crate) fn admit(&self, identity: &Identity, granted: bool) -> Result<String> {
        ensure!(
            identity.email_verified && !identity.sub.is_empty(),
            "Unverified Google identity"
        );
        let derived = || {
            format!(
                "user-{}",
                joppa::digest_hex::sha256_hex(identity.sub.as_bytes())
            )
        };
        if let Some(path) = &self.rules {
            let rules = joppa::access::Rules::read(path)?;
            ensure!(
                granted
                    || !rules
                        .resolve(&identity.email, identity.hd.as_deref())
                        .is_empty(),
                NoAccess
            );
            return Ok(self
                .users
                .get(&identity.email.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(derived));
        }
        match self.users.get(&identity.email.to_ascii_lowercase()) {
            Some(actor) => Ok(actor.clone()),
            None if granted => Ok(derived()),
            None => Err(NoAccess.into()),
        }
    }
    pub(crate) async fn enroll(
        &self,
        pool: &deadpool_postgres::Pool,
        identity: &Identity,
    ) -> Result<SignedIn> {
        let email = identity.email.to_ascii_lowercase();
        // Grants are read only for a verified email; admit refuses any other.
        let granted = if identity.email_verified {
            joppa::store::granted_workspaces(pool, &email, identity.hd.as_deref()).await?
        } else {
            Default::default()
        };
        let actor = self.admit(identity, !granted.is_empty())?;
        let mut workspaces = match &self.rules {
            Some(path) => {
                joppa::access::Rules::read(path)?.resolve(&identity.email, identity.hd.as_deref())
            }
            None => Default::default(),
        };
        workspaces.extend(granted);
        if self.rules.is_some() || !workspaces.is_empty() {
            joppa::store::enroll(
                pool,
                &actor,
                &identity.sub,
                &email,
                identity.hd.as_deref(),
                &workspaces,
            )
            .await?;
        }
        if let Some(name) = identity.display_name() {
            joppa::store::update_identity_name(pool, &actor, &identity.sub, &name).await?;
        }
        Ok((actor, identity.hd.clone()))
    }
    /// A new session token, the actor and the hd of this sign-in.
    pub async fn poll(&self, id: &str) -> Result<Option<(String, String, Option<String>)>> {
        let mut pending = self.pending.lock().unwrap();
        let p = pending
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Sign-in expired. Please try again."))?;
        if p.expires <= Instant::now() {
            pending.remove(id);
            anyhow::bail!("Sign-in expired. Please try again.");
        }
        let Some(result) = p.result.clone() else {
            return Ok(None);
        };
        pending.remove(id);
        let (actor, hd) = result.map_err(anyhow::Error::msg)?;
        let token = random();
        Ok(Some((token, actor, hd)))
    }
}
