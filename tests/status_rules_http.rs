//! Status rules of the tree schema over a real server: each type gets its tone from tests,
//! from its children or not at all, and HTTP and MCP give the same tone.
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command, store};
use serde_json::{Value, json};
use std::process::{Child, Command as Process, Stdio};
use std::time::Duration;
#[path = "common/record_http.rs"]
mod record_http;

include!("status_rules_http/part01.rs");
include!("status_rules_http/part02.rs");
include!("status_rules_http/part03.rs");
include!("status_rules_http/part04.rs");
include!("status_rules_http/part05.rs");
