//! Parallel claims on the real store: one task has one winner.
#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use joppa::{objects::TreeType, store};
use serde_json::json;
#[path = "common/claim_time.rs"]
mod claim_time;

include!("claims_postgres/part01.rs");
include!("claims_postgres/part02.rs");
include!("claims_postgres/part03.rs");
