mod common;
use common::World;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects, tasks,
};
use serde_json::{Value, json};

include!("planning_claims/part01.rs");
include!("planning_claims/part02.rs");
include!("planning_claims/part03.rs");
include!("planning_claims/part04.rs");
include!("planning_claims/part05.rs");
include!("planning_claims/part06.rs");
