//! Part of `objects`: the links of an older journal. A save_object event of an older release
//! holds `links`, and the replay maps them to relations (relations.rs).
use super::*;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(from = "LinkInput")]
pub struct ObjectLink {
    pub target: String,
    pub relation: String,
    pub revision: Option<u32>,
    pub blocking: bool,
}

/// A link as an older write sent it. A detail read gives each link with the title, the current
/// revision and the presence of its target; a write takes that item back and ignores them.
/// A link without a relation is `related`, as a detail read shows it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkInput {
    target: String,
    #[serde(default = "related")]
    relation: String,
    #[serde(default)]
    revision: Option<u32>,
    #[serde(default)]
    blocking: bool,
    #[serde(default, rename = "title")]
    _title: serde::de::IgnoredAny,
    #[serde(default, rename = "current_revision")]
    _current_revision: serde::de::IgnoredAny,
    #[serde(default, rename = "present")]
    _present: serde::de::IgnoredAny,
}

fn related() -> String {
    "related".into()
}

impl From<LinkInput> for ObjectLink {
    fn from(l: LinkInput) -> Self {
        ObjectLink {
            target: l.target,
            relation: l.relation,
            revision: l.revision,
            blocking: l.blocking,
        }
    }
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
