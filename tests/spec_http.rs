//! Specification behaviour over HTTP and MCP: a real server, an owner token and a test DB.
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

#[path = "common/claim_time.rs"]
mod claim_time;
#[path = "common/operation_samples.rs"]
mod operation_samples;

include!("spec_http/part01.rs");
include!("spec_http/part02.rs");
include!("spec_http/part03.rs");
include!("spec_http/part04.rs");
include!("spec_http/part05.rs");
include!("spec_http/part06.rs");
include!("spec_http/part07.rs");
include!("spec_http/part08.rs");
include!("spec_http/part09.rs");
include!("spec_http/part10.rs");
include!("spec_http/part11.rs");
include!("spec_http/part12.rs");
include!("spec_http/part13.rs");
include!("spec_http/part14.rs");
include!("spec_http/part15.rs");
include!("spec_http/part16.rs");
include!("spec_http/part17.rs");
include!("spec_http/part18.rs");
include!("spec_http/part19.rs");
include!("spec_http/part20.rs");
include!("spec_http/part21.rs");
include!("spec_http/part22.rs");
include!("spec_http/part23.rs");
include!("spec_http/part24.rs");
include!("spec_http/part25.rs");
include!("spec_http/part26.rs");
include!("spec_http/part27.rs");
include!("spec_http/part28.rs");
include!("spec_http/part29.rs");
include!("spec_http/part30.rs");
include!("spec_http/part31.rs");
include!("spec_http/part32.rs");
include!("spec_http/part33.rs");
include!("spec_http/part34.rs");
include!("spec_http/part35.rs");
include!("spec_http/part36.rs");
include!("spec_http/part37.rs");
