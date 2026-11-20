//! Part of `reports`: the criteria of a status read, each with its test list and boundary.
use super::*;

/// A criterion with its test list and its boundary.
pub(in crate::store) type Criterion = (String, Option<Listed>, Boundary);

/// The test lists and boundaries of `targets`, and the test rules. The reset rule of each
/// criterion is resolved in its boundaries, so the rules keep every boundary.
pub(in crate::store) fn criteria_and_rules(
    state: &crate::model::Workspace,
    targets: &[String],
) -> Result<(Vec<Criterion>, crate::result_shape::ResultShape)> {
    let mut rules = crate::test_rules::rules(state)?;
    let criteria = criteria_of(state, targets, rules.reset_on_change);
    rules.reset_on_change = true;
    Ok((criteria, rules))
}

/// The test list and the boundary of each target that has an object. `reset_on_change` of the
/// type of a criterion overrides `reset`, the rule of test_results. A criterion without the
/// rule has no boundary.
fn criteria_of(
    state: &crate::model::Workspace,
    targets: &[String],
    reset: bool,
) -> Vec<(String, Option<Listed>, Boundary)> {
    let types = crate::objects::schema(state).types;
    // The test names of each parent come from one pass over the test nodes.
    let parents: std::collections::BTreeSet<&str> = targets.iter().map(String::as_str).collect();
    let names = state
        .test_nodes_migrated
        .then(|| crate::test_nodes::nodes_below(state, Some(&parents)));
    targets
        .iter()
        .filter_map(|target| {
            let object = state.objects.get(target)?;
            let criterion_kind = match types.get(&object.kind).map(|t| t.role.as_str()) {
                Some(crate::test_nodes::ROLE) => object
                    .current()
                    .parent
                    .as_deref()
                    .and_then(|p| state.objects.get(crate::objects::resolve_address(state, p)))
                    .map(|p| p.kind.as_str()),
                _ => Some(object.kind.as_str()),
            };
            let reset = criterion_kind
                .and_then(|k| types.get(k))
                .and_then(|t| t.reset_on_change)
                .unwrap_or(reset);
            let boundary = match reset {
                true => Boundary::read(state.criterion_boundaries.get(target).map(String::as_str)),
                false => Boundary::None,
            };
            let mut tests = match &names {
                Some(names) => crate::ac_tests::listed_node(state, target, names),
                None => listed(state, target),
            };
            if !reset && let Some(own) = tests.as_mut().and_then(|t| t.boundaries.as_mut()) {
                own.iter_mut().for_each(|b| *b = Boundary::None);
            }
            Some((target.clone(), tests, boundary))
        })
        .collect()
}
