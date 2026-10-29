//! A group change keeps grey criterion A below group G and green criterion D in sibling
//! group H when a real server restarts and rebuilds the workspace from the journal.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
#[path = "common/record_http.rs"]
mod record_http;

include!("grey_restart/part01.rs");
include!("grey_restart/part02.rs");
