use crate::{checks, model::*};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[path = "commands_legacy.rs"]
mod legacy;
pub use legacy::*;
#[path = "commands_types.rs"]
mod types;
pub use types::*;
#[path = "commands_arms_1.rs"]
mod arms_1;
use arms_1::*;
#[path = "commands_arms_2.rs"]
mod arms_2;
use arms_2::*;
#[path = "commands_arms_3.rs"]
mod arms_3;
use arms_3::*;
#[path = "commands_arms_4.rs"]
mod arms_4;
use arms_4::*;
#[path = "commands_arms_5.rs"]
mod arms_5;
use arms_5::*;
#[path = "commands_arms_6.rs"]
mod arms_6;
use arms_6::*;
#[path = "commands_command.rs"]
mod command;
pub use command::*;
#[path = "commands_operation.rs"]
mod operation;
pub use operation::*;
#[path = "commands_person.rs"]
mod person;
pub use person::*;
#[path = "commands_apply_inner.rs"]
mod apply_inner;
pub(crate) use apply_inner::*;
#[path = "commands_apply_first.rs"]
mod apply_first;
use apply_first::*;
#[path = "commands_validate_acs.rs"]
mod validate_acs;
pub use validate_acs::*;
