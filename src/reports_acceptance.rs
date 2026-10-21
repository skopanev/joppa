//! The acceptance check of a requirement against the records of one commit.
use super::*;

/// An acceptance needs a commit where every listed test of every present criterion of the
/// requirement has a record, and every recorded case of it passed. A criterion whose text
/// changed after its test list was saved has a draft list, so it blocks acceptance.
pub async fn check_acceptance(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    target: &str,
    commit: &str,
) -> Result<()> {
    const NEEDS: &str = "acceptance needs records on the commit where every listed test passes";
    let key = crate::objects::resolve_address(state, target);
    let mut tests = vec![];
    let criteria = crate::acceptance::criteria(state, key);
    ensure!(!criteria.is_empty(), NEEDS);
    for criterion in criteria {
        let listed = state.ac_tests.get(&criterion).and_then(AcTests::current);
        let data = &state.objects[&criterion].current().data;
        let list = listed
            .filter(|l| !l.tests.is_empty() && l.text_hash == text_hash(data))
            .with_context(|| NEEDS)?;
        tests.extend(list.tests.iter().cloned());
    }
    let rows = client
        .query(
            "SELECT t.address,t.status FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.commit_sha=$2 AND r.recorded AND t.address=ANY($3)",
            &[&workspace, &commit, &tests],
        )
        .await?;
    // Every listed test has a record on the commit, and every case of it passed.
    let passed = tests.iter().all(|test| {
        let cases: Vec<&str> = rows
            .iter()
            .filter(|r| r.get::<_, &str>(0) == test)
            .map(|r| r.get(1))
            .collect();
        !cases.is_empty() && cases.iter().all(|s| *s == "pass")
    });
    ensure!(passed, NEEDS);
    Ok(())
}
