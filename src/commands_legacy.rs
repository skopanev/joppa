//! The fields of the operations that only journal replay reads. Live writes refuse them
//! (removed_ops). A stored event keeps its JSON: the fields stay next to `op`.
use super::*;
#[path = "commands_legacy_2.rs"]
mod part_2;
pub use part_2::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetNoteStatusOp {
    pub target: String,
    pub revision: u32,
    pub status: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerOp {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub owner: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditConsumerOp {
    pub id: String,
    pub title: String,
    pub description: String,
    pub owner: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainOp {
    pub id: String,
    pub title: String,
    #[serde(alias = "authority")]
    pub owner: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityOp {
    pub id: String,
    pub title: String,
    pub domain: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetDomainArchivedOp {
    pub id: String,
    pub archived: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmTreeOp {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditDomainOp {
    pub id: String,
    pub title: String,
    #[serde(alias = "authority")]
    pub owner: String,
    #[serde(default)]
    pub description: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditCapabilityOp {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub description: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawDecisionOp {
    pub req: String,
    pub revision: u32,
    pub comment: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectOp {
    pub id: String,
    pub repository: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviseOp {
    pub req: String,
    pub expected_revision: u32,
    pub capability: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumers: Vec<String>,
    pub title: String,
    pub body: String,
    pub owner: String,
    pub reason: String,
    pub acs: Vec<Ac>,
    /// Persisted compatibility marker: old journal revisions predate
    /// cross-revision coverage review and remain replayable unchanged.
    #[serde(default)]
    pub coverage_policy: bool,
    #[serde(default)]
    pub carry_tasks: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCoverageOp {
    pub req: String,
    pub revision: u32,
    pub run: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefineVerificationScopeOp {
    pub req: String,
    pub revision: u32,
    pub subject_keys: Vec<String>,
    pub scope: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSourceOp {
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub external_id: Option<String>,
    pub version: String,
    pub relation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawSourceOp {
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeOp {
    pub target: String,
    pub text: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskOp {
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub title: String,
    pub acs: Vec<String>,
    pub projects: Vec<String>,
    pub purpose: String,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub contract: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmendTaskOp {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub title: String,
    pub contract: String,
    pub projects: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanOp {
    pub req: String,
    pub revision: u32,
    pub run: String,
    pub subject: BTreeMap<String, String>,
    pub scope: String,
    pub discovery_complete: bool,
    pub checks: Vec<Check>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enforce_coverage: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultOp {
    pub req: String,
    pub revision: u32,
    pub run: String,
    pub evidence: Evidence,
}
