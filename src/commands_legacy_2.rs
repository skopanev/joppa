//! The fields of the operations that only journal replay reads. Live writes refuse them
//! (removed_ops). A stored event keeps its JSON: the fields stay next to `op`.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishOp {
    pub req: String,
    pub revision: u32,
    pub run: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptOp {
    pub req: String,
    pub revision: u32,
    pub run: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeOp {
    pub req: String,
    pub revision: u32,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueOp {
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub ac: String,
    pub comment: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveOp {
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub comment: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeOp {
    pub id: String,
    pub said: String,
    pub author: String,
    #[serde(default)]
    pub owner: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitOp {
    pub id: String,
    pub points: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionsOp {
    pub kind: String,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortOp {
    pub id: String,
    pub point: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerOp {
    pub id: String,
    pub point: String,
    pub field: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteOp {
    pub id: String,
    pub point: String,
    pub outcome: String,
    pub reason: String,
    pub req: Option<String>,
}
