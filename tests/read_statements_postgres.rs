//! tac:m7qjm2lj and tac:jogrtcra over a real server: a joppa_objects read of 2 to 25
//! addresses runs in one snapshot with a fixed number of SQL statements, and a read with
//! fields runs only the queries that its fields need. pg_stat_statements counts the
//! statements, as tests/read_latency.rs reads it.
#[path = "common/http_command.rs"]
mod http_command;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};

include!("read_statements_postgres/part01.rs");
include!("read_statements_postgres/part02.rs");
include!("read_statements_postgres/part03.rs");
