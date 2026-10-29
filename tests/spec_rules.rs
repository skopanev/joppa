//! Specification rules that the server already enforces, one test per criterion.
//! Each test name says what it proves; the criterion lists it by this address.
mod common;
use common::World;
use serde_json::{Value, json};

include!("spec_rules/part01.rs");
include!("spec_rules/part02.rs");
