//! Part of `objects`: `tree_type` and the items after it, moved from objects.rs unchanged.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TreeType {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub root: bool,
    pub role: String,
    /// The removed readiness gate of an older journal: only its presence stays, so the replay
    /// of the older migrations gives the same schema revisions.
    #[serde(default, skip_serializing)]
    pub parent_gate: Option<crate::model::Removed>,
    /// The removed launch flag of an older journal: only its presence stays, so the replay of
    /// the older migrations gives the same schema revisions.
    #[serde(default, skip_serializing)]
    pub launch_blocker: Option<crate::model::Removed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox: Option<bool>,
    /// A node of this type is a source: a new title, body or file resets each criterion that
    /// names it or links with it (source_reset.rs). A false value is not stored.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub source: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "title_max_length"
    )]
    pub title_preview_length: Option<u32>,
    /// The longest new or changed value in the data of an object of this type, in
    /// characters: 1 to 262144. Without a value it is crate::field_limit::FIELD_MAX.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, ReferenceRule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_fields: BTreeMap<String, RequiredFieldRule>,
    /// The type of each typed data field (field_types.rs).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, crate::field_types::FieldType>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parent_decision: bool,
    /// How a node of this type gets its tone: own, children, links or none. Without a value the
    /// rule is unset and the node is orange; with status_required the type is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The removed list of process fields. Only an older set_tree_schema in the journal has
    /// it: the replay accepts it and nothing reads it. A live write refuses it, no read shows it.
    #[serde(default, skip_serializing)]
    pub process_fields: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RequiredFieldRule {
    Presence(String),
    Text(TextFieldRule),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextFieldRule {
    pub contains: Vec<String>,
    pub error: String,
}

impl RequiredFieldRule {
    pub(crate) fn error(&self) -> &str {
        match self {
            Self::Presence(error) => error,
            Self::Text(rule) => &rule.error,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaRevision {
    pub revision: u32,
    pub types: BTreeMap<String, TreeType>,
    /// The ordered priority values, the first value first in work order. Without a saved
    /// list the values are p0, p1 and p2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priorities: Option<Vec<String>>,
    /// The colour of each status dot, as #RRGGBB per tone. Without it the board colours stay.
    /// A stored revision reads the older tone as blue (status_colors_alias.rs).
    #[serde(
        default,
        deserialize_with = "crate::status_colors::read_colors",
        skip_serializing_if = "Option::is_none"
    )]
    pub status_colors: Option<BTreeMap<String, String>>,
    /// The ordered status tones with a light and a dark colour each. A stored revision reads
    /// the older tone as blue.
    #[serde(
        default,
        deserialize_with = "crate::status_colors::read_statuses",
        skip_serializing_if = "Option::is_none"
    )]
    pub statuses: Option<Vec<crate::status_colors::Status>>,
    /// The test rules: the block test_results, see result_shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_results: Option<crate::result_shape::ResultShape>,
    /// With true, set_tree_schema refuses a type without a status rule. With false, it saves
    /// the type, and its nodes take the tone of an unset rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_required: Option<bool>,
    /// The data fields that the board offers as filter drop-downs (filter_fields).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<String>>,
    /// The registry of relations. A revision stored without it has the system relations.
    #[serde(default = "crate::relations::system")]
    pub relations: BTreeMap<String, crate::relations::RelationType>,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
}

/// A revision of a document. A stored revision with the older links reads as relations
/// (relations_stored.rs).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(from = "crate::relations::StoredRevision")]
pub struct ObjectRevision {
    pub revision: u32,
    pub schema_revision: u32,
    pub parent: Option<String>,
    pub data: Value,
    pub relations: crate::relations::Relations,
    /// The links of an event before the epoch of relations_reset; never stored.
    #[serde(skip)]
    pub older_links: crate::relations::LinkForm,
    pub actor: String,
    /// The alias of the event that saved the revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub at: String,
    pub position: u64,
    pub reason: String,
    pub present: bool,
    /// When this revision changes only the test list: [this revision, the revision of the
    /// contract]. Pins and Task reviews compare the contract, so a new test list asks for no
    /// review. A revision that copies this one has another number, so the pair stops counting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<[u32; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Object {
    pub kind: String,
    pub id: String,
    pub revisions: Vec<ObjectRevision>,
    #[serde(default)]
    pub parent_override: bool,
}

impl Object {
    pub fn current(&self) -> &ObjectRevision {
        self.revisions.last().expect("object has a revision")
    }
}

/// The built-in tree of a new workspace, as the migrations leave it: each criterion names one
/// consumer, and a test type sits under each criterion type.
pub fn defaults() -> BTreeMap<String, TreeType> {
    let mut types = schema_migration_defaults();
    crate::consumer_migration::convert_types(&mut types);
    // The built-in tree names the status rule of each type. An inbox type has no parent, and a
    // Task type has no decision rule.
    for t in types.values_mut() {
        t.status = Some(role_status_rule(t).to_owned());
        t.parent_decision = false;
        // Only the replay of migrate_schema keeps the subject flag of the older Task saves.
        for r in t.references.values_mut() {
            r.verification_subject = None;
        }
        if t.inbox == Some(true) {
            t.parents.clear();
            t.root = true;
        }
    }
    crate::test_nodes::convert_types(&mut types).expect("the built-in tree has a free test name");
    types
}
