//! A signed-in browser user creates a workspace through the real server.
#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use joppa::{commands::Command, sessions, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command as Process, Stdio},
    time::Duration,
};

include!("create_workspace/part01.rs");
include!("create_workspace/part02.rs");
include!("create_workspace/part02b.rs");
