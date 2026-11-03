use crate::{
    commands::{self, Command},
    model::{Task, Workspace},
    objects,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
#[path = "tasks_address.rs"]
mod address;
pub use address::*;
#[path = "tasks_subjects.rs"]
mod subjects;
pub use subjects::*;
#[path = "tasks_save.rs"]
mod save;
pub use save::*;
#[path = "tasks_apply.rs"]
mod apply;
pub use apply::*;
#[path = "tasks_migrate.rs"]
mod migrate;
pub use migrate::*;
