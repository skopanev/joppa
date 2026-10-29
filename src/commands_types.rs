//! The fields of the larger live operations, one struct for each, so `Command` stays short.
//! A stored event keeps its JSON: the fields stay next to `op`.
use super::*;

/// The fields of `SetTreeSchema`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTreeSchemaOp {
    pub revision: u32,
    /// The whole type map. Without it, put_types and remove_types change single types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<BTreeMap<String, crate::objects::TreeType>>,
    /// Types to add, or to replace completely. Other types stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub put_types: Option<BTreeMap<String, crate::objects::TreeType>>,
    /// Types to delete. Present objects and other types must not use them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_types: Option<Vec<String>>,
    /// The ordered priority values; without it the schema keeps its current list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priorities: Option<Vec<String>>,
    /// The colour of each status dot; without it the schema keeps its current colours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_colors: Option<std::collections::BTreeMap<String, String>>,
    /// The ordered status tones with light and dark colours; without it they stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<crate::status_colors::Status>>,
    /// The test rules; without them the schema keeps its current rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_results: Option<serde_json::Value>,
    /// With true, each type needs a status rule; without it the schema keeps its flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_required: Option<bool>,
    pub reason: String,
}

/// The fields of `DecideNote`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideNoteOp {
    pub target: String,
    pub revision: u32,
    pub outcome: String,
    pub reason: String,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub duplicate_of: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub recipient: Option<String>,
}

/// The fields of `Comment`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommentOp {
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub text: String,
    pub kind: String,
    pub reply_to: Option<String>,
    pub ac: Option<String>,
}

/// The fields of `Attach`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachOp {
    pub target: String,
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub hash: String,
}

/// The fields of `SaveObject`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveObjectOp {
    pub id: String,
    pub kind: String,
    pub revision: u32,
    pub parent: Option<String>,
    pub data: serde_json::Value,
    #[serde(default)]
    pub links: Vec<crate::objects::ObjectLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_checked: Option<String>,
    pub reason: String,
}

/// The fields of `SetAcTests`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetAcTestsOp {
    pub target: String,
    pub tests: Vec<String>,
    /// The list revision the author read; 0 means no saved list. Absent skips the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u32>,
    pub reason: String,
}

/// The fields of `RenameWorkspace`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameWorkspaceOp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub title: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_prefix: Option<String>,
}

/// The fields of `RecordReport`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordReportOp {
    pub report: i64,
    pub commit: String,
    pub branch: String,
    pub tests: u64,
    /// True for an identical repeat of a stored report.
    #[serde(default)]
    pub repeat: bool,
}

/// The fields of `CompleteTask`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteTaskOp {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub req: String,
    pub revision: u32,
    pub id: String,
    pub evidence: String,
    /// The git sha where the Task is ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// The fields of `ReviewTask`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTaskOp {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub req: String,
    pub revision: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ac: String,
    pub reason: String,
}
