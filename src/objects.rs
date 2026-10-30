//! Common document revisions derived from the immutable workspace journal.
use crate::{
    commands::{self, Command},
    model::*,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[path = "objects_process.rs"]
mod process_facts;
pub use process_facts::{process, requirement_summary, run_process, signal, task_signal};
#[path = "objects_tree_type.rs"]
mod tree_type;
pub use tree_type::*;
#[path = "objects_schema_migration_defaults.rs"]
mod schema_migration_defaults;
pub use schema_migration_defaults::*;
#[path = "objects_stored_reference_targets.rs"]
mod stored_reference_targets;
pub use stored_reference_targets::*;
#[path = "objects_parent_valid.rs"]
mod parent_valid;
pub use parent_valid::*;
#[path = "objects_set_schema.rs"]
mod set_schema;
pub use set_schema::*;
#[path = "objects_criterion_address.rs"]
mod criterion_address;
pub use criterion_address::*;
#[path = "objects_sync_with.rs"]
mod sync_with;
pub(crate) use sync_with::*;
#[path = "objects_validate_links.rs"]
mod validate_links;
pub(crate) use validate_links::*;
#[path = "objects_typed_command.rs"]
mod typed_command;
pub(crate) use typed_command::*;
#[path = "objects_workflow_command.rs"]
mod workflow_command;
pub(crate) use workflow_command::*;
#[path = "objects_ancestor.rs"]
mod ancestor;
pub use ancestor::*;
#[path = "objects_saved.rs"]
mod saved;
pub(crate) use saved::*;
#[path = "objects_typed_save.rs"]
mod typed_save;
pub use typed_save::*;
#[path = "objects_contract_revision.rs"]
mod contract_revision;
pub use contract_revision::*;
#[path = "objects_wait_cycle.rs"]
mod wait_cycle;
pub(crate) use wait_cycle::*;
#[path = "objects_validate_current.rs"]
mod validate_current;
pub use validate_current::*;
#[path = "objects_readiness_blocker.rs"]
mod readiness_blocker;
pub use readiness_blocker::*;
#[path = "objects_restore.rs"]
mod restore;
pub use restore::*;
