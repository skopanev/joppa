//! Part of `objects`: the rule of one configured reference field.
use super::*;

/// The rule of one reference field of a type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRule {
    pub target: String,
    pub title: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
    /// The removed subject flag. Only the replay of a Task save before migrate_generic_nodes
    /// reads it; a live write refuses it, and no read shows it.
    #[serde(default, skip_serializing)]
    pub verification_subject: Option<bool>,
    /// The reference also takes the explicit value all: every object of the target type.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_all: bool,
}

/// A missing subject flag equals false, as the older journals read it: a replay that
/// compares rules, such as migrate_consumers, gives the same result as before.
impl PartialEq for ReferenceRule {
    fn eq(&self, other: &Self) -> bool {
        (
            &self.target,
            &self.title,
            self.required,
            self.multiple,
            self.allow_all,
        ) == (
            &other.target,
            &other.title,
            other.required,
            other.multiple,
            other.allow_all,
        ) && self.verification_subject.unwrap_or(false)
            == other.verification_subject.unwrap_or(false)
    }
}
