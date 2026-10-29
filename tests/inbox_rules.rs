//! Decisions on Inbox signals, one test per criterion.
mod common;
use common::World;
use serde_json::{Value, json};

include!("inbox_rules/part01.rs");
include!("inbox_rules/part02.rs");
