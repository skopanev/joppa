//! The test status of one criterion, from the recorded test results.
use super::*;
use crate::ac_tests::{Boundary, Listed, Report, TestResult, listed};

/// The stored test status of one node: a test node, or a node with test nodes below it.
pub async fn test_status(pool: &Pool, workspace: &str, actor: &str, target: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    criterion_test_statuses(
        &**conn,
        &workspace,
        &[target.to_owned()],
        &Default::default(),
    )
    .await?
    .remove(target)
    .with_context(|| format!("no test status for {target}"))
}

/// The stored test statuses of the requested objects, from the status projection of the
/// last write. A missing or stale projection is an error, never an old status. `optional`
/// stays for the callers: a stored status is always readable.
pub(super) async fn criterion_test_statuses(
    client: &impl GenericClient,
    workspace: &str,
    targets: &[String],
    _optional: &std::collections::BTreeSet<String>,
) -> Result<std::collections::BTreeMap<String, Value>> {
    super::objects::stored_statuses(client, workspace, targets).await
}

/// Statuses of the criteria from the current transaction's texts and test lists.
pub(super) async fn state_criterion_statuses(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    targets: &[String],
) -> Result<std::collections::BTreeMap<String, Value>> {
    let criteria = criteria_of(state, targets);
    let rules = crate::test_rules::rules(state)?;
    statuses_for_criteria(client, workspace, criteria, &rules).await
}

/// The test list and the boundary of each target that has an object.
pub(super) fn criteria_of(
    state: &crate::model::Workspace,
    targets: &[String],
) -> Vec<(String, Option<Listed>, Boundary)> {
    // The test names of each parent come from one pass over the test nodes.
    let names = state
        .test_nodes_migrated
        .then(|| crate::test_nodes::nodes_by_parent(state));
    targets
        .iter()
        .filter_map(|target| {
            state.objects.get(target)?;
            let boundary =
                Boundary::read(state.criterion_boundaries.get(target).map(String::as_str));
            let tests = match &names {
                Some(names) => crate::ac_tests::listed_node(state, target, names),
                None => listed(state, target),
            };
            Some((target.clone(), tests, boundary))
        })
        .collect()
}

async fn statuses_for_criteria(
    client: &impl GenericClient,
    workspace: &str,
    criteria: Vec<(String, Option<Listed>, Boundary)>,
    rules: &crate::result_shape::ResultShape,
) -> Result<std::collections::BTreeMap<String, Value>> {
    crate::probe::count(&crate::probe::STATUS);
    if criteria.is_empty() {
        return Ok(Default::default());
    }
    let (reports, results) = results_of(client, workspace, &criteria).await?;
    compute(criteria, &reports, &results, rules)
}

/// The reports of the workspace and the results of the tests of `criteria`.
pub(super) async fn results_of(
    client: &impl GenericClient,
    workspace: &str,
    criteria: &[(String, Option<Listed>, Boundary)],
) -> Result<(Vec<Report>, Vec<(i64, TestResult)>)> {
    let tests: std::collections::BTreeSet<String> = criteria
        .iter()
        .filter_map(|(_, saved, _)| saved.as_ref())
        .flat_map(|current| current.tests.iter().cloned())
        .collect();
    let reports: Vec<Report> = client
        .query("SELECT r.id,r.commit_sha,r.commit_order,(extract(epoch FROM r.received_at)*1000000)::bigint,(extract(epoch FROM greatest(r.received_at,(SELECT max(c.received_at) FROM joppa.test_report_receipts c WHERE c.report_id=r.id)))*1000000)::bigint,r.recorded FROM joppa.test_reports r WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty", &[&workspace])
        .await?.iter().map(|r| Report { id:r.get(0),commit:r.get(1),order:r.get(2),received_us:r.get(3),receipt_us:r.get(4),recorded:r.get(5) }).collect();
    let tests: Vec<String> = tests.into_iter().collect();
    let results: Vec<(i64,TestResult)> = client
        .query("SELECT t.report_id,t.address,t.status,t.duration_ms,t.message,t.status_name,t.fields::text FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty AND t.address=ANY($2)", &[&workspace,&tests])
        .await?.iter().map(|r| (r.get(0),TestResult { address:r.get(1),status:r.get(2),duration_ms:r.get(3),message:r.get(4),status_name:r.get(5),fields:r.get::<_,Option<String>>(6).and_then(|f| serde_json::from_str(&f).ok()) })).collect();
    Ok((reports, results))
}

/// The status of each criterion from loaded reports and results.
pub(super) fn compute(
    criteria: Vec<(String, Option<Listed>, Boundary)>,
    reports: &[Report],
    results: &[(i64, TestResult)],
    rules: &crate::result_shape::ResultShape,
) -> Result<std::collections::BTreeMap<String, Value>> {
    // Each criterion reads only the results of its own tests. The grouping keeps every row,
    // duplicates included, and preserves the input order for each address (the query has no
    // ORDER BY), so a status costs its own results and not every result of the workspace.
    let mut by_test: std::collections::BTreeMap<&str, Vec<(i64, TestResult)>> = Default::default();
    for (id, result) in results {
        by_test
            .entry(result.address.as_str())
            .or_default()
            .push((*id, result.clone()));
    }
    let mut statuses = std::collections::BTreeMap::new();
    for (address, saved, boundary) in criteria {
        let current = saved.as_ref();
        let tests = current.map(|r| r.tests.as_slice()).unwrap_or_default();
        let draft = current.is_some_and(|r| r.draft);
        let own: Vec<(i64, TestResult)> = tests
            .iter()
            .flat_map(|test| by_test.get(test.as_str()).into_iter().flatten().cloned())
            .collect();
        // After migrate_test_nodes, each test counts from the reset of its own node.
        let status = match current.and_then(|r| r.boundaries.as_ref()) {
            Some(boundaries) => crate::test_nodes::combine(
                tests
                    .iter()
                    .zip(boundaries)
                    .map(|(test, b)| {
                        let own = by_test.get(test.as_str()).cloned().unwrap_or_default();
                        crate::ac_tests::criterion_status(
                            std::slice::from_ref(test),
                            false,
                            *b,
                            reports,
                            &own,
                            rules,
                        )
                    })
                    .collect(),
            ),
            None => crate::ac_tests::criterion_status(tests, draft, boundary, reports, &own, rules),
        };
        let mut status = serde_json::to_value(status)?;
        status["address"] = json!(address);
        status["tests_revision"] = json!(current.map(|r| r.revision));
        status["tests_saved_by"] = json!(current.map(|r| &r.actor));
        statuses.insert(address, status);
    }
    Ok(statuses)
}
