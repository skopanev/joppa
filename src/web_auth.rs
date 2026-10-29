//! Joppa-owned Google sign-in attempts and browser sessions.
use crate::google_auth::{Google, Identity};
use anyhow::{Result, ensure};
use axum::http::{HeaderMap, header};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[path = "web_auth_pending.rs"]
mod pending;
pub use pending::*;
#[path = "web_auth_cookie.rs"]
mod cookie;
#[path = "web_auth_login.rs"]
mod login;
pub use cookie::*;
#[cfg(test)]
#[path = "web_auth_tests.rs"]
mod tests;
