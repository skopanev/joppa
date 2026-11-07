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
    /// The data fields of the board filter drop-downs; without it the schema keeps its list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<String>>,
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
    /// The new links. A live save without them keeps the current links (omitted_links).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<crate::objects::ObjectLink>>,
    /// An older journal and an older client send it. No write checks it.
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

/// The fields of `RenameWorkspace`, which only old journals hold. Live writes send
/// `update_workspace`.
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

/// The fields of `UpdateWorkspace`. The command changes each field that it sends, and needs
/// one field at least.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspaceOp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_prefix: Option<String>,
    /// A hidden workspace leaves the web lists. MCP and a direct board link still read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl From<&RenameWorkspaceOp> for UpdateWorkspaceOp {
    fn from(op: &RenameWorkspaceOp) -> Self {
        UpdateWorkspaceOp {
            title: Some(op.title.clone()),
            workspace_id: op.workspace_id.clone(),
            reference_prefix: op.reference_prefix.clone(),
            hidden: None,
            reason: Some(op.reason.clone()),
        }
    }
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

/// The removed `assess` of an older journal: the readiness mark of a node, or the older one of
/// a criterion in a requirement revision. Its replay has no effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssessOp {
    Node(AssessNode),
    Criterion(AssessCriterion),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessNode {
    pub target: String,
    pub ready: bool,
    pub reason: String,
    /// The position of the mark that the writer read, 0 for no mark. A newer mark refuses
    /// the write, so a retry never replaces the mark of another member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_position: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessCriterion {
    pub req: String,
    pub revision: u32,
    pub ac: String,
    #[serde(skip_serializing)]
    pub assessment: crate::model::Removed,
}
