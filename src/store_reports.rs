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
    if targets.is_empty() {
        return Ok(Default::default());
    }
    let criteria = criteria_of(state, targets);
    let rules = crate::test_rules::rules(state)?;
    let checks = crate::manual_checks::Checks::of(state, targets);
    statuses_for_criteria(client, workspace, criteria, &rules, &checks).await
}

/// The test list and the boundary of each target that has an object.
pub(super) fn criteria_of(
    state: &crate::model::Workspace,
    targets: &[String],
) -> Vec<(String, Option<Listed>, Boundary)> {
    // The test names of each parent come from one pass over the test nodes.
    let parents: std::collections::BTreeSet<&str> = targets.iter().map(String::as_str).collect();
    let names = state
        .test_nodes_migrated
        .then(|| crate::test_nodes::nodes_below(state, Some(&parents)));
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
    checks: &crate::manual_checks::Checks,
) -> Result<std::collections::BTreeMap<String, Value>> {
    crate::probe::count(&crate::probe::STATUS);
    if criteria.is_empty() {
        return Ok(Default::default());
    }
    let (reports, results) = results_of(client, workspace, &criteria, &[]).await?;
    let mut statuses = compute(criteria, &reports, &results, rules, checks)?;
    drop(results);
    super::reports_details::fill(client, workspace, &mut statuses, rules).await?;
    Ok(statuses)
}

/// The reports of the workspace and the results of the tests of `criteria`. A large workspace
/// has hundreds of thousands of results, so the rows stream, and each keeps only what decides a
/// status: the report, the test, the status, the status name and the `keys` of its fields (the
/// dimensions). reports_details.rs reads the message and the fields of the deciding rows.
pub(super) async fn results_of(
    client: &impl GenericClient,
    workspace: &str,
    criteria: &[(String, Option<Listed>, Boundary)],
    keys: &[String],
) -> Result<(std::sync::Arc<Vec<Report>>, Vec<(i64, TestResult)>)> {
    let tests: std::collections::BTreeSet<String> = criteria
        .iter()
        .filter_map(|(_, saved, _)| saved.as_ref())
        .flat_map(|current| current.keys.iter().flatten().cloned())
        .collect();
    // A criterion without tests reads no report.
    if tests.is_empty() {
        return Ok((Default::default(), vec![]));
    }
    let reports = super::reports_cache::reports(client, workspace).await?;
    let tests: Vec<String> = tests.into_iter().collect();
    let sql = "SELECT t.report_id,t.address,t.status,t.status_name,CASE WHEN cardinality($3::text[])>0 THEN (SELECT jsonb_object_agg(key,value) FROM jsonb_each(t.fields) WHERE key=ANY($3))::text END FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty AND t.address=ANY($2)";
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 3] = [&workspace, &tests, &keys];
    let rows = client.query_raw(sql, params).await?;
    futures_util::pin_mut!(rows);
    let mut results = vec![];
    while let Some(r) = futures_util::TryStreamExt::try_next(&mut rows).await? {
        let fields = r.get::<_, Option<String>>(4);
        let result = TestResult {
            address: r.get(1),
            status: r.get(2),
            duration_ms: None,
            message: None,
            status_name: r.get(3),
            fields: fields.and_then(|f| serde_json::from_str(&f).ok()),
        };
        results.push((r.get(0), result));
    }
    Ok((reports, results))
}

/// The status of each criterion from loaded reports and results. The manual checks decide a
/// criterion without test nodes (manual_checks.rs).
pub(super) fn compute(
    criteria: Vec<(String, Option<Listed>, Boundary)>,
    reports: &[Report],
    results: &[(i64, TestResult)],
    rules: &crate::result_shape::ResultShape,
    checks: &crate::manual_checks::Checks,
) -> Result<std::collections::BTreeMap<String, Value>> {
    // Each criterion reads only the results of its own tests. The grouping keeps every row,
    // duplicates included, and preserves the input order for each address (the query has no
    // ORDER BY), so a status costs its own results and not every result of the workspace.
    let mut by_test: std::collections::BTreeMap<&str, Vec<(i64, &TestResult)>> = Default::default();
    for (id, result) in results {
        by_test
            .entry(result.address.as_str())
            .or_default()
            .push((*id, result));
    }
    let mut statuses = std::collections::BTreeMap::new();
    for (address, saved, boundary) in criteria {
        let current = saved.as_ref();
        let tests = current.map(|r| r.tests.as_slice()).unwrap_or_default();
        let draft = current.is_some_and(|r| r.draft);
        let own: Vec<(i64, TestResult)> = tests
            .iter()
            .flat_map(|test| by_test.get(test.as_str()).into_iter().flatten())
            .map(|(id, row)| (*id, (*row).clone()))
            .collect();
        // After migrate_test_nodes, each test counts from the reset of its own node. The rows of
        // each stored name of the node count under the name of the test.
        let keys = current.map(|r| r.keys.as_slice()).unwrap_or_default();
        let status = match current.and_then(|r| r.boundaries.as_ref()) {
            Some(boundaries) => crate::test_nodes::combine(
                tests
                    .iter()
                    .zip(boundaries)
                    .zip(keys)
                    .map(|((test, b), keys)| {
                        let own: Vec<(i64, TestResult)> = keys
                            .iter()
                            .flat_map(|key| by_test.get(key.as_str()).into_iter().flatten())
                            .map(|(id, row)| {
                                let address = test.clone();
                                (
                                    *id,
                                    TestResult {
                                        address,
                                        ..(*row).clone()
                                    },
                                )
                            })
                            .collect();
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
        let manual = tests
            .is_empty()
            .then(|| checks.decide(&address, boundary, rules.reset_on_change));
        let (status, manual) = match manual.flatten() {
            Some((manual, expires)) => (manual, Some(expires)),
            None => (status, None),
        };
        // The deciding row of each test: its report and the stored names of the test, for the
        // message and the fields (reports_details.rs).
        let names: Vec<Vec<String>> = match current.and_then(|r| r.boundaries.as_ref()) {
            Some(_) => keys.to_vec(),
            None => tests.iter().map(|t| vec![t.clone()]).collect(),
        };
        let rows: Vec<Option<i64>> = status.tests.iter().map(|t| t.row).collect();
        let mut status = serde_json::to_value(status)?;
        let each = status["tests"].as_array_mut().into_iter().flatten();
        for ((test, row), names) in each.zip(rows).zip(names) {
            if let Some(report) = row {
                test["row"] = json!({"report": report, "names": names});
            }
        }
        // A manual status also gives the tone of a criterion without test nodes, and the date
        // of a deciding pass that expires: the check run finds it by this value.
        if let Some(expires) = manual {
            status["manual"] = json!(true);
            if let Some(day) = expires {
                status["valid_until"] = json!(day.to_string());
            }
        }
        status["address"] = json!(address);
        status["tests_revision"] = json!(current.map(|r| r.revision));
        status["tests_saved_by"] = json!(current.map(|r| &r.actor));
        statuses.insert(address, status);
    }
    Ok(statuses)
}
