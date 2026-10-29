//! Browser consent and OAuth authorization-code/PKCE for direct HTTPS MCP clients.
#![allow(clippy::result_large_err)] // OAuth handlers return HTTP responses for both outcomes.
use super::*;
use axum::{
    Form,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[path = "mcp_oauth_part.rs"]
mod part;
pub(crate) use part::*;
#[path = "mcp_oauth_approve.rs"]
mod approve;
pub(crate) use approve::*;
#[cfg(test)]
#[path = "mcp_oauth_tests.rs"]
mod tests;
