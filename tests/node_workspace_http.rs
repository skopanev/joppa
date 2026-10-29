//! A node can stand for another workspace, and an object can link to an object of another
//! workspace, against a real server. Workspace P is the product, workspace C a component.
//! Owner and agent bot belong to both. Member alex belongs only to P.
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

include!("node_workspace_http/part01.rs");
include!("node_workspace_http/part02.rs");
include!("node_workspace_http/part03.rs");
include!("node_workspace_http/part04.rs");
include!("node_workspace_http/part05.rs");
include!("node_workspace_http/part06.rs");
