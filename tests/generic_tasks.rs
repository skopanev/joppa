mod common;
use common::*;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects::{self, TreeType},
    tasks,
};
use serde_json::{Value, json};

include!("generic_tasks/part01.rs");
include!("generic_tasks/part02.rs");
include!("generic_tasks/part03.rs");
