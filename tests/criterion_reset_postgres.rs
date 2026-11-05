//! A change of a group, a test-node change and a spec archive reset the criteria and test
//! nodes below them in the store: in the same batch, after journal replay, and not at all when the
//! command fails. An identical repeat of a record after the change is a new
//! receipt that counts after it. A change removes the readiness mark of the changed node only.
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

include!("criterion_reset_postgres/part01.rs");
include!("criterion_reset_postgres/part02.rs");
include!("criterion_reset_postgres/part03.rs");
