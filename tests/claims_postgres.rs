//! Parallel claims on the real store: one task has one winner.
use anyhow::{Result, ensure};
use joppa::{objects::TreeType, store};
use serde_json::json;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn two_parallel_claims_give_exactly_one_winner() -> Result<()> {
    // atomic-claim-ac1
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("claims-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Claims"}),
    )
    .await?;
    for bot in ["bot1", "bot2"] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            bot,
            &json!({"op":"member","id":bot,"role":"agent"}),
        )
        .await?;
    }
    let current = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut types = std::collections::BTreeMap::new();
    for (kind, role, root) in [("spec", "document", true), ("task", "task", false)] {
        types.insert(
            kind,
            TreeType {
                index: None,
                references: Default::default(),
                parent_decision: false,
                inbox: None,
                launch_blocker: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: vec!["*".into()],
                parent_gate: None,
            },
        );
    }
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Specifications and tasks only"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","root",&json!({"op":"save_object","kind":"spec","id":"root","revision":0,"parent":null,"data":{"title":"root","owner":"owner"},"reason":"Create a specification"})).await?;
    for run in 0..50 {
        let task = store::execute_remote_input(&pool,&ws,"owner",&format!("task-{run}"),&json!({"op":"save_object","kind":"task","revision":0,"parent":"spec:root","data":{"title":"Implement","owner":"owner","contract":"Implement the specification","purpose":"implementation","projects":[]},"reason":"Create work"})).await?;
        let claim = json!({"op":"claim_task","id":task["object"]["address"],"revision":task["object"]["revision"]});
        let (ra, rb) = (format!("a-{run}"), format!("b-{run}"));
        let (a, b) = tokio::join!(
            store::execute_remote_input(&pool, &ws, "bot1", &ra, &claim),
            store::execute_remote_input(&pool, &ws, "bot2", &rb, &claim)
        );
        let (won, lost) = match (a, b) {
            (Ok(won), Err(lost)) | (Err(lost), Ok(won)) => (won, lost),
            (a, b) => anyhow::bail!("run {run}: {a:?} / {b:?}"),
        };
        ensure!(won["command"]["op"] == "claim_task", "run {run}: {won}");
        ensure!(
            format!("{lost:#}").contains("task is already claimed"),
            "run {run}: {lost:#}"
        );
    }
    Ok(())
}
