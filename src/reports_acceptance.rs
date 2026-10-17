//! The acceptance check of a requirement against one full report.
use super::*;

/// An acceptance needs a clean full run on a default-branch commit where every listed
/// test of every present criterion of the requirement passed. A criterion whose text
/// changed after its test list was saved has a draft list, so it blocks acceptance.
pub async fn check_acceptance(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    target: &str,
    commit: &str,
) -> Result<()> {
    const NEEDS: &str = "acceptance needs a full run on the commit where every listed test passes";
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
    let reports = client
        .query(
            "SELECT id,default_branch,full_run,dirty FROM joppa.test_reports WHERE workspace=$1 AND commit_sha=$2",
            &[&workspace, &commit],
        )
        .await?;
    ensure!(
        reports.is_empty() || reports.iter().any(|r| r.get::<_, bool>(1)),
        "acceptance needs a default-branch commit"
    );
    for report in reports
        .iter()
        .filter(|r| r.get::<_, bool>(1) && r.get::<_, bool>(2) && !r.get::<_, bool>(3))
    {
        let id: i64 = report.get(0);
        let rows = client
            .query(
                "SELECT address,status FROM joppa.test_results WHERE report_id=$1 AND address=ANY($2)",
                &[&id, &tests],
            )
            .await?;
        // Every listed test ran, and every case of it passed.
        let passed = tests.iter().all(|test| {
            let cases: Vec<&str> = rows
                .iter()
                .filter(|r| r.get::<_, &str>(0) == test)
                .map(|r| r.get(1))
                .collect();
            !cases.is_empty() && cases.iter().all(|s| *s == "pass")
        });
        if passed {
            return Ok(());
        }
    }
    anyhow::bail!(NEEDS)
}
