use crate::{
    commands::{Command, identifier, nonempty, ready, revision},
    model::*,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[path = "checks_arms_1.rs"]
mod arms_1;
use arms_1::*;
#[path = "checks_arms_2.rs"]
mod arms_2;
use arms_2::*;
#[path = "checks_arms_3.rs"]
mod arms_3;
use arms_3::*;
#[path = "checks_arms_4.rs"]
mod arms_4;
use arms_4::*;
#[path = "checks_latest.rs"]
mod latest;
pub use latest::*;
#[path = "checks_validate_claim.rs"]
mod validate_claim;
pub use validate_claim::*;
