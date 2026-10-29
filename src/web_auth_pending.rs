//! Part of `web_auth`: `pending` and the items after it, moved from web_auth.rs unchanged.
use super::*;

#[derive(Clone)]
pub(crate) struct Pending {
    pub(crate) state: String,
    pub(crate) nonce: String,
    pub(crate) verifier: String,
    pub(crate) expires: Instant,
    pub(crate) processing: bool,
    pub(crate) result: Option<std::result::Result<SignedIn, String>>,
}

/// The admitted actor and the verified Google hd of this sign-in.
pub(crate) type SignedIn = (String, Option<String>);

#[derive(Clone)]
pub struct Login {
    pub users: BTreeMap<String, String>,
    pub(crate) rules: Option<std::path::PathBuf>,
    pub(crate) google: Option<Google>,
    pub(crate) pending: Arc<Mutex<BTreeMap<String, Pending>>>,
}

/// A verified Google email that no grant, rule or user entry admits.
#[derive(Debug)]
pub(crate) struct NoAccess;

impl std::fmt::Display for NoAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("No workspace access")
    }
}

impl std::error::Error for NoAccess {}
/// Error details never include Google's tokens or the authorization code.
pub(crate) fn outcome(
    result: Result<SignedIn>,
    cancelled: bool,
) -> std::result::Result<SignedIn, String> {
    result.map_err(|e| {
        if cancelled || e.is::<NoAccess>() {
            e.to_string()
        } else {
            "Sign-in could not be completed. Check your account has access, then try again.".into()
        }
    })
}

pub(crate) fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
