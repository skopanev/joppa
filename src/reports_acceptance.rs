//! The acceptance check of a node against the records of one commit.
use super::*;

/// An acceptance needs a green node and a commit where every present test node at and below
/// the node has a record, and the outcome of every recorded case of it is in the green_if_all
/// list of the schema. With reset_on_change, only a record received after the last reset of
/// the test node counts. A criterion without a test node is orange, so the node is not green.
pub async fn check_acceptance(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    target: &str,
    commit: &str,
) -> Result<()> {
    use crate::ac_tests::Boundary;
    const NEEDS: &str = "acceptance needs records on the commit where every test below passes";
    let rules = crate::test_rules::rules(state)?;
    let key = crate::objects::resolve_address(state, target);
    let listed: Vec<(String, Boundary)> = crate::acceptance::tests(state, key)
        .into_iter()
        .map(|(node, name)| {
            let at = state.criterion_boundaries.get(&node).map(String::as_str);
            let boundary = if rules.reset_on_change {
                Boundary::read(at)
            } else {
                Boundary::None
            };
            (name, boundary)
        })
        .collect();
    ensure!(!listed.is_empty(), NEEDS);
    let tone = format!(
        "SELECT {} FROM joppa.objects o WHERE o.workspace=$1 AND o.address=$2",
        crate::node_filter::TONE
    );
    let tone: Option<String> = client
        .query_opt(&tone, &[&workspace, &key])
        .await?
        .and_then(|r| r.get(0));
    ensure!(tone.as_deref() == Some("green"), NEEDS);
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
