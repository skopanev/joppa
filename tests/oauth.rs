#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use joppa::{commands::Command as Operation, sessions, store};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

include!("oauth/part01.rs");
include!("oauth/part02.rs");
include!("oauth/part02b.rs");
include!("oauth/part02c.rs");
