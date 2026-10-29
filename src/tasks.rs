use crate::{
    commands::{self, Command},
    model::{Task, Workspace},
    objects,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
#[path = "tasks_address.rs"]
mod address;
#[path = "tasks_completion.rs"]
mod completion;
pub use address::*;
#[path = "tasks_validate_claim.rs"]
mod validate_claim;
pub use validate_claim::*;
#[path = "tasks_save.rs"]
mod save;
pub use save::*;
#[path = "tasks_apply.rs"]
mod apply;
pub use apply::*;
#[path = "tasks_release_authority.rs"]
mod release_authority;
pub use release_authority::*;
