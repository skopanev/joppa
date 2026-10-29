mod common;
#[path = "common/resolve_id.rs"]
mod resolve_id;
use common::*;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects, tasks, tree_order,
};
use serde_json::{Value, json};

include!("schema_objects/part01.rs");
include!("schema_objects/part02.rs");
include!("schema_objects/part03.rs");
include!("schema_objects/part04.rs");
