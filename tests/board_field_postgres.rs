//! A board field of the result shape: each value gets its own criterion status and node
//! tone, and the criterion status combines the values.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

include!("board_field_postgres/part01.rs");
include!("board_field_postgres/part02.rs");
