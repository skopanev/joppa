use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path = "model_workspace.rs"]
mod workspace;
pub use workspace::*;
#[path = "model_verification_scope.rs"]
mod verification_scope;
pub use verification_scope::*;
