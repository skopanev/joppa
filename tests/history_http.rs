//! History over HTTP and MCP against a real server.
#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use joppa::{sessions, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
#[path = "common/mcp_shape.rs"]
mod mcp_shape;
#[path = "common/record_http.rs"]
mod record_http;

include!("history_http/part01.rs");
include!("history_http/part02.rs");
include!("history_http/part03.rs");
