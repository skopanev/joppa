//! requirement:read-contract over a real server: lists, fields, details, empty keys,
//! no item IDs and joppa_tests by kind.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

include!("read_contract_http/part01.rs");
include!("read_contract_http/part02.rs");
