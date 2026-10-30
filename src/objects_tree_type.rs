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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_blocker: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox: Option<bool>,
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parent_decision: bool,
    /// How a node of this type gets its tone: tests, children, links or none. Without a value the
    /// role decides: the criterion role uses tests, group and requirement use children.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The order in which a children or links rule takes the first status that it finds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_order: Option<Vec<String>>,
    /// The process fields that an object of this type shows: confirmed for a requirement or a
    /// criterion, acceptance for a requirement. A type without the list shows neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRule {
    pub target: String,
    pub title: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub verification_subject: bool,
    /// The reference also takes the explicit value all: every object of the target type.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_all: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_colors: Option<BTreeMap<String, String>>,
    /// The ordered status tones with a light and a dark colour; a higher entry wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<crate::status_colors::Status>>,
    /// The test rules: the block test_results, see result_shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_results: Option<crate::result_shape::ResultShape>,
    /// With true, set_tree_schema refuses a type without a status rule. With false, it saves
    /// the type, and its nodes take the tone of an unset rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_required: Option<bool>,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObjectLink {
    pub target: String,
    pub relation: String,
    #[serde(default)]
    pub revision: Option<u32>,
    #[serde(default)]
    pub blocking: bool,
}

/// An ordering dependency stores no revision field. Other links keep their stored null,
/// so the documents saved before ordering links serialize as they were stored.
impl Serialize for ObjectLink {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let unpinned = self.relation == "depends_on" && self.revision.is_none();
        let mut link = serializer.serialize_struct("ObjectLink", 4)?;
        link.serialize_field("target", &self.target)?;
        link.serialize_field("relation", &self.relation)?;
        if unpinned {
            link.skip_field("revision")?;
        } else {
            link.serialize_field("revision", &self.revision)?;
        }
        link.serialize_field("blocking", &self.blocking)?;
        link.end()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectRevision {
    pub revision: u32,
    pub schema_revision: u32,
    pub parent: Option<String>,
    pub data: Value,
    pub links: Vec<ObjectLink>,
    pub actor: String,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// The built-in tree of a new workspace: each criterion names one consumer.
pub fn defaults() -> BTreeMap<String, TreeType> {
    let mut types = schema_migration_defaults();
    crate::consumer_migration::convert_types(&mut types);
    // The built-in tree names the status rule of each type. An inbox type has no parent.
    for t in types.values_mut() {
        t.status = Some(role_status_rule(t).to_owned());
        if t.inbox == Some(true) {
            t.parents.clear();
            t.root = true;
        }
    }
    types
}
