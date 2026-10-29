//! requirement:read-contract over a real server: the joppa_tests sweep over one page of 100
//! and the links of a detail read in pages of 25.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

include!("read_pages_http/part01.rs");
include!("read_pages_http/part02.rs");
