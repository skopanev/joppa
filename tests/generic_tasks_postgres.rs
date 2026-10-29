#[path = "common/resolve_id.rs"]
mod resolve_id;
use anyhow::Result;
use joppa::{objects::TreeType, store};
use serde_json::{Value, json};

include!("generic_tasks_postgres/part01.rs");
include!("generic_tasks_postgres/part02.rs");
