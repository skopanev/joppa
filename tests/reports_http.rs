//! Test results over HTTP: a real server, a member token and records of test outcomes.
//! Each test proves one Evidence criterion through the endpoints, not the status function.
#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, sessions, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
#[path = "common/compact_of.rs"]
mod compact_of;
#[path = "common/record_http.rs"]
mod record_http;
include!("reports_http_parts/planning_people.rs");
include!("reports_http_parts/planning_people_wait.rs");

include!("reports_http/part01.rs");
include!("reports_http/part02.rs");
include!("reports_http/part03.rs");
include!("reports_http/part03b.rs");
include!("reports_http/part04.rs");
include!("reports_http/part05.rs");
include!("reports_http/part06.rs");
include!("reports_http/part07.rs");
include!("reports_http/part08.rs");
include!("reports_http/part09.rs");
include!("reports_http/part10.rs");
include!("reports_http/part11.rs");
include!("reports_http/part12.rs");
include!("reports_http/part13.rs");
include!("reports_http/part14.rs");
include!("reports_http/part15.rs");
include!("reports_http/part16.rs");
include!("reports_http/part17.rs");
include!("reports_http/part18.rs");
include!("reports_http/part19.rs");
include!("reports_http/part20.rs");
include!("reports_http/part21.rs");
include!("reports_http/part22.rs");
include!("reports_http/part23.rs");
include!("reports_http/part24.rs");
include!("reports_http/part25.rs");
include!("reports_http/part26.rs");
include!("reports_http/part27.rs");
include!("reports_http/part28.rs");
include!("reports_http/part29.rs");
include!("reports_http/part30.rs");
include!("reports_http/part31.rs");
include!("reports_http/part32.rs");
include!("reports_http/part33.rs");
