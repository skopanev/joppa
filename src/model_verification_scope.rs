//! Part of `model`: `verification_scope` and the items after it, moved from model.rs unchanged.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationScope {
    pub subject_keys: Vec<String>,
    pub scope: String,
    pub actor: String,
    pub at: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: String,
    pub uri: String,
    pub external_id: Option<String>,
    pub version: String,
    pub relation: String,
    pub actor: String,
    pub at: String,
    #[serde(default)]
    pub withdrawn: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ac {
    pub id: String,
    pub text: String,
    /// Пояснение к условию: оговорки, границы, чего оно НЕ требует.
    /// Сам текст AC остаётся коротким — он должен читаться одним взглядом,
    /// а подробности, без которых спорят, живут здесь.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    #[serde(alias = "decider")]
    pub owner: Option<String>,
    #[serde(default)]
    pub before_launch: bool,
    #[serde(default)]
    pub assessment: Option<Assessment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub ready: bool,
    pub reason: String,
    #[serde(alias = "recipient")]
    pub owner: Option<String>,
    pub decision: Option<String>,
}

/// A member's readiness mark of a signal: Tasks under it are claimable only when ready.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalAssessment {
    pub ready: bool,
    pub reason: String,
    pub actor: String,
    pub position: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub revision: u32,
    pub actor: String,
    /// Время записи в журнал, а НЕ присланное клиентом. Второй источник
    /// правды о времени означал бы, что его можно подделать, а по этим
    /// отметкам потом восстанавливают, кто что знал и когда.
    #[serde(default)]
    pub at: String,
    pub text: String,
    pub kind: String,
    pub reply_to: Option<String>,
    pub ac: Option<String>,
    pub withdrawn: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanningClaim {
    pub actor: String,
    pub position: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<TaskClaim>,
    pub title: String,
    pub acs: Vec<String>,
    pub projects: Vec<String>,
    pub purpose: String,
    /// An unassigned task becomes in progress only after an atomic claim.
    pub assignee: Option<String>,
    pub done: Option<String>,
    /// The git sha where the Task is ready, from complete_task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub contract: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub created_by: String,
    /// A copied task stays visible after REQ revision, but its fit to the new
    /// wording must be confirmed before unfinished work can be completed.
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub origin_revision: u32,
    #[serde(default)]
    pub retired: Option<String>,
    /// A failed Task waits for planning; claims refuse it until planning clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<TaskBlock>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskBlock {
    pub actor: String,
    pub reason: String,
    pub evidence: String,
    pub position: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskClaim {
    pub heartbeat_at_ms: i64,
    pub deadline_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub ac: String,
    pub method: String,
    pub expected: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskBinding {
    pub address: String,
    pub origin_revision: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub check: String,
    pub outcome: String,
    pub observed: Vec<String>,
    pub note: String,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub subject: BTreeMap<String, String>,
    pub plan: Vec<Check>,
    pub discovery_complete: bool,
    pub scope: String,
    pub reporter: String,
    pub started: u64,
    pub finished: Option<u64>,
    pub results: BTreeMap<String, Evidence>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub task_contexts: BTreeMap<String, Option<(String, u32)>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub ac: String,
    pub comment: String,
    pub opened: u64,
    pub resolved: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Acceptance {
    pub run: String,
    pub actor: String,
    pub at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Intake {
    pub said: String,
    pub author: String,
    #[serde(default)]
    pub owner: Option<String>,
    pub points: BTreeMap<String, Point>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Point {
    pub said: String,
    pub kind: String,
    pub answers: BTreeMap<String, String>,
    pub outcome: Option<String>,
    pub reason: Option<String>,
    pub requirement: Option<String>,
}
