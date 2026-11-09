mod common;
use common::*;
use joppa::objects::{self, TreeType};
#[path = "common/relation_edges.rs"]
mod relation_edges;
use serde_json::{Value, json};

include!("objects/part01.rs");
include!("objects/part02.rs");
include!("objects/part03.rs");
include!("objects/part04.rs");
