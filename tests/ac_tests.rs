mod common;
use common::World;
use joppa::ac_tests::{Boundary, Report, TestResult, criterion_status};
use serde_json::json;

include!("ac_tests/part01.rs");
include!("ac_tests/part02.rs");
