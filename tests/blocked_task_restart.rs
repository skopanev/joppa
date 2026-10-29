//! Blocked Tasks keep their block state and queue availability, and claimed and done
//! Tasks keep their holder, history, comments and test results, when a real server
//! restarts and rebuilds the workspace projections from the journal.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    process::{Child, Command, Stdio},
    time::Duration,
};
#[path = "common/record_http.rs"]
mod record_http;

include!("blocked_task_restart/part01.rs");
include!("blocked_task_restart/part02.rs");
include!("blocked_task_restart/part03.rs");
include!("blocked_task_restart/part04.rs");
include!("blocked_task_restart/part05.rs");
include!("blocked_task_restart/part06.rs");
include!("blocked_task_restart/part07.rs");
