#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use joppa::{sessions, store};
use serde_json::json;
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

include!("sessions/part01.rs");
include!("sessions/part02.rs");
