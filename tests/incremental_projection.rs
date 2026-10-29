//! The incremental projection must agree with a full rebuild, row for row.
//!
//! Reading the dirty closure cannot prove that it is complete: a missing edge
//! looks correct until the data disagrees. So this test runs a command
//! sequence, and after every single command it compares the projection tables
//! written incrementally against the same tables rebuilt from current state.
//!
//! Two rebuilds run at each step, because they fail for different reasons.
//! Dropping only board_cache catches an update the closure never made.
//! Dropping the workspace's projection rows as well catches a row the closure
//! should have removed and left behind.
mod common;
use common::*;
use joppa::store;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

include!("incremental_projection/part01.rs");
include!("incremental_projection/part02.rs");
