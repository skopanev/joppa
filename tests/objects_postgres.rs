mod common;
#[path = "common/link_page.rs"]
mod link_page;
#[path = "common/resolve_id.rs"]
mod resolve_id;
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

include!("objects_postgres/part01.rs");
include!("objects_postgres/part01b.rs");
include!("objects_postgres/part02.rs");
include!("objects_postgres/part02b.rs");
