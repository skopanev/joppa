use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Workspace {
    pub title: String,
    #[serde(default)]
    pub reference_prefix: Option<String>,
    pub admin: String,
    pub members: BTreeMap<String, String>,
    pub domains: BTreeMap<String, Domain>,
    #[serde(default)]
    pub tree_order: BTreeMap<String, Vec<String>>,
    pub capabilities: BTreeMap<String, Capability>,
    pub projects: BTreeMap<String, String>,
    pub requirements: BTreeMap<String, Requirement>,
    pub intake: BTreeMap<String, Intake>,
    pub questions: BTreeMap<String, Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Domain {
    #[serde(default)]
    pub archived: bool,
    pub title: String,
    pub authority: String,
    pub confirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub title: String,
    pub domain: String,
    pub confirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Requirement {
    pub capability: String,
    pub revisions: Vec<Revision>,
    pub comments: BTreeMap<String, Comment>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Revision {
    pub number: u32,
    pub title: String,
    pub body: String,
    pub owner: String,
    pub reason: String,
    pub acs: Vec<Ac>,
    pub confirmed: bool,
    pub tasks: BTreeMap<String, Task>,
    pub runs: BTreeMap<String, Run>,
    pub issues: BTreeMap<String, Issue>,
    pub acceptance: Option<Acceptance>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ac {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub decider: Option<String>,
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
    pub recipient: Option<String>,
    pub decision: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub revision: u32,
    pub actor: String,
    pub text: String,
    pub kind: String,
    pub reply_to: Option<String>,
    pub ac: Option<String>,
    pub withdrawn: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub title: String,
    pub acs: Vec<String>,
    pub projects: Vec<String>,
    pub purpose: String,
    pub assignee: String,
    pub done: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub ac: String,
    pub method: String,
    pub expected: Vec<String>,
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
