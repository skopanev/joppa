//! A change of a group, a test-list save and a spec archive reset the criteria below
//! them in the store: in the same batch, after journal replay, and not at all when the
//! command fails. An identical repeat of a record after the change is a new
//! receipt that counts after it.
use anyhow::{Result, ensure};
use joppa::store::{self, BatchOperation};
use serde_json::{Value, json};

include!("criterion_reset_postgres/part01.rs");
include!("criterion_reset_postgres/part02.rs");
include!("criterion_reset_postgres/part03.rs");
