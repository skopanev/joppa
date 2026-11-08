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
    /// The source of the values: none for own values, or children for the sorted union of
    /// the values of the present children (computed_fields.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl ReferenceRule {
    /// Refuse a source other than children, and children on a single, required or allow_all
    /// reference or on a name that a typed field of the type also has.
    pub fn validate_from(&self, field: &str, t: &TreeType) -> Result<()> {
        let Some(from) = self.from.as_deref() else {
            return Ok(());
        };
        ensure!(
            from == "children",
            "{field}: a reference takes from children only"
        );
        ensure!(
            self.multiple,
            "{field}: only a reference with multiple: true takes its values from children"
        );
        ensure!(
            !self.required && !self.allow_all,
            "{field}: a reference from children cannot be required or allow_all: the server sets it"
        );
        ensure!(
            !t.fields.contains_key(field),
            "{field}: a reference from children cannot also be a typed field"
        );
        Ok(())
    }
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
            &self.from,
        ) == (
            &other.target,
            &other.title,
            other.required,
            other.multiple,
            other.allow_all,
            &other.from,
        ) && self.verification_subject.unwrap_or(false)
            == other.verification_subject.unwrap_or(false)
    }
}
