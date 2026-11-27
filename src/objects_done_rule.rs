//! Part of `objects`: the check of the `done` rule of a type (dependency_sql.rs). The rule names
//! the data values that make a node of the type satisfy a `depends_on` edge.
use super::*;

/// Refuse a `done` rule without a field, with a field that is not a data field, with a null
/// value or with a value that the typed field refuses.
pub(super) fn check_done_rule(t: &TreeType) -> Result<()> {
    let Some(done) = &t.done else {
        return Ok(());
    };
    ensure!(!done.is_empty(), "done rule must name at least one field");
    for (field, value) in done {
        commands::identifier(field)?;
        let edges = ["relations", "links", "depends_on"].contains(&field.as_str());
        ensure!(
            !edges && !process_field(field),
            "done rule field {field} must be a data field"
        );
        ensure!(
            !value.is_null(),
            "done rule value of {field} must not be null"
        );
        if let Some(rule) = t.fields.get(field)
            && let Some(refusal) = crate::field_types::refusal(field, rule, value)
        {
            bail!("done rule: {refusal}");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "objects_done_rule_tests.rs"]
mod tests;
