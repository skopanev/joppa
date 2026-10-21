//! From the human_members epoch on, the member command accepts only people. A journal from
//! before the epoch keeps its agent membership in a replay.
use anyhow::{Result, ensure};
use joppa::{
    commands::{self, Command},
    model::Workspace,
    store,
};
use serde_json::json;

async fn pool() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok(pool)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_command_refuses_agent_and_runner_roles() -> Result<()> {
    // ac:267133e7: the owner of W sends member lane-1 with role agent, then with role runner.
    let pool = pool().await?;
    let ws = format!("members-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Members"}),
    )
    .await?;
    let members = |pool: deadpool_postgres::Pool, ws: String| async move {
        let read = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
        anyhow::Ok(read["index"]["members"].clone())
    };
    let m = members(pool.clone(), ws.clone()).await?;
    let p = store::read(&pool, &ws, "owner", &store::Scope::full()).await?["position"].clone();
    for role in ["agent", "runner"] {
        let error = store::execute_remote_input(
            &pool,
            &ws,
            "owner",
            &uuid::Uuid::new_v4().to_string(),
            &json!({"op":"member","id":"lane-1","role":role}),
        )
        .await
        .expect_err("the role must be refused");
        ensure!(
            error.to_string().contains("unknown role"),
            "{role}: {error}"
        );
    }
    // No command field chooses the rule epoch.
    let error = store::execute_remote_input(
        &pool,
        &ws,
        "owner",
        "epoch",
        &json!({"op":"member","id":"lane-1","role":"agent","epoch":0}),
    )
    .await
    .expect_err("an epoch field must be refused");
    ensure!(
        error.to_string().contains("unknown field `epoch`"),
        "{error}"
    );
    // The internal enrollment of a service identity is not a public operation.
    let error = store::execute_remote_input(
        &pool,
        &ws,
        "owner",
        "service",
        &json!({"op":"enroll_service","id":"lane-1","role":"agent"}),
    )
    .await
    .expect_err("enroll_service is internal");
    ensure!(error.to_string().contains("unknown operation"), "{error}");
    ensure!(
        members(pool.clone(), ws.clone()).await? == m,
        "W got a new member"
    );
    let now = store::read(&pool, &ws, "owner", &store::Scope::full()).await?["position"].clone();
    ensure!(now == p, "the journal position moved from {p} to {now}");
    Ok(())
}

#[test]
fn a_replay_keeps_an_agent_membership_from_before_the_epoch() -> Result<()> {
    // ac:267133e7: journal J adds lane-1 with role agent before the epoch at position 3.
    let journal: Vec<Command> = vec![
        serde_json::from_value(json!({"op":"workspace","title":"Old journal"}))?,
        serde_json::from_value(json!({"op":"member","id":"lane-1","role":"agent"}))?,
    ];
    let mut j = Workspace {
        replay_epochs: commands::REPLAY_RULES
            .iter()
            .map(|rule| ((*rule).to_owned(), 1))
            .collect(),
        ..Default::default()
    };
    j.replay_epochs.insert(commands::HUMAN_MEMBERS.into(), 3);
    for (i, command) in journal.iter().enumerate() {
        commands::apply_replay(
            &mut j,
            "owner",
            command,
            i as u64 + 1,
            "2026-10-01T00:00:00Z",
        )?;
    }
    // S: the stored member snapshot of J.
    let snapshot = json!({"owner":"human","lane-1":"agent"});
    ensure!(json!(j.members) == snapshot, "{:?}", j.members);
    // A live member command after the epoch is refused in the same workspace.
    let mut live = j.clone();
    let error = commands::apply(
        &mut live,
        "owner",
        &serde_json::from_value(json!({"op":"member","id":"lane-2","role":"agent"}))?,
        3,
        "2026-10-20T00:00:00Z",
    )
    .expect_err("a live agent membership must be refused");
    ensure!(error.to_string().contains("unknown role"), "{error}");
    ensure!(json!(live.members) == snapshot, "{:?}", live.members);
    Ok(())
}
