//! The public origin of this server: JOPPA_PUBLIC_URL, else the origin of the Google redirect
//! URI. The OAuth metadata, the Origin check of /mcp and the upload links use it.
use anyhow::{Context, Result, ensure};
use std::sync::OnceLock;

static ORIGIN: OnceLock<Option<String>> = OnceLock::new();

/// The checked origin from JOPPA_PUBLIC_URL, else from `google_redirect`, or None without
/// both. A value that is not an HTTPS origin is refused. HTTP is only for loopback.
pub fn resolve(google_redirect: Option<&str>) -> Result<Option<String>> {
    let origin = match std::env::var("JOPPA_PUBLIC_URL") {
        Ok(raw) => Some(raw),
        Err(_) => match google_redirect {
            Some(uri) => Some(reqwest::Url::parse(uri)?.origin().ascii_serialization()),
            None => None,
        },
    };
    let Some(raw) = origin else {
        return Ok(None);
    };
    let u = reqwest::Url::parse(&raw)?;
    ensure!(
        (u.scheme() == "https"
            || (u.scheme() == "http" && matches!(u.host_str(), Some("localhost" | "127.0.0.1"))))
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path() == "/",
        "JOPPA_PUBLIC_URL must be an HTTPS origin (HTTP only on loopback)"
    );
    Ok(Some(u.origin().ascii_serialization()))
}

/// Keep the origin that the server start resolved, one time.
pub fn configure(origin: Option<String>) {
    let _ = ORIGIN.set(origin);
}

/// The origin that the server start resolved.
pub fn get() -> Result<&'static str> {
    ORIGIN
        .get()
        .and_then(Option::as_deref)
        .context("Public server URL is not configured")
}
