mod common;
#[path = "common/resolve_id.rs"]
mod resolve_id;
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};

include!("authoring/part01.rs");
include!("authoring/part02.rs");
include!("authoring/part03.rs");
