//! The acceptance check of a requirement against the records of one commit.
use super::*;

/// An acceptance needs a commit where every listed test of every present criterion of the
/// requirement has a record, and the outcome of every recorded case of it is in the
/// green_if_all list of the schema. With reset_on_change, only a record received after the
/// last change of the criterion counts. A criterion with an older draft list blocks it.
pub async fn check_acceptance(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    target: &str,
    commit: &str,
) -> Result<()> {
    use crate::ac_tests::Boundary;
    const NEEDS: &str = "acceptance needs records on the commit where every listed test passes";
    let rules = crate::test_rules::rules(state)?;
    let key = crate::objects::resolve_address(state, target);
    let mut listed: Vec<(String, Boundary)> = vec![];
    let criteria = crate::acceptance::criteria(state, key);
    ensure!(!criteria.is_empty(), NEEDS);
    for criterion in criteria {
        let list = crate::ac_tests::listed(state, &criterion)
            .filter(|l| !l.tests.is_empty() && !l.draft)
            .with_context(|| NEEDS)?;
        let boundary = if rules.reset_on_change {
            Boundary::read(
                state
                    .criterion_boundaries
                    .get(&criterion)
                    .map(String::as_str),
            )
        } else {
            Boundary::None
        };
        listed.extend(list.tests.iter().map(|t| (t.clone(), boundary)));
    }
    let tests: Vec<&str> = listed.iter().map(|(t, _)| t.as_str()).collect();
    let rows = client
        .query(
            "SELECT t.address,t.status,(extract(epoch FROM greatest(r.received_at,(SELECT max(c.received_at) FROM joppa.test_report_receipts c WHERE c.report_id=r.id)))*1000000)::bigint FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.commit_sha=$2 AND r.recorded AND t.address=ANY($3)",
            &[&workspace, &commit, &tests],
        )
        .await?;
    // Every listed test has a counted record on the commit, and every case of it is green.
    let passed = listed.iter().all(|(test, boundary)| {
        let cases: Vec<&str> = rows
            .iter()
            .filter(|r| r.get::<_, &str>(0) == test)
            .filter(|r| match boundary {
                Boundary::None => true,
                Boundary::After(at) => r.get::<_, i64>(2) > *at,
                Boundary::Unreadable => false,
            })
            .map(|r| r.get(1))
            .collect();
        !cases.is_empty()
            && cases.iter().all(|s| {
                let outcome = crate::test_rules::outcome(s);
                rules.green_if_all.iter().any(|o| o == outcome)
            })
    });
    ensure!(passed, NEEDS);
    Ok(())
}
