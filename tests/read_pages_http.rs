//! requirement:read-contract over a real server: the links of a detail read, the list pages
//! of joppa_objects and the tone of a list item.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

include!("read_pages_http/part01.rs");
include!("read_pages_http/part02.rs");
