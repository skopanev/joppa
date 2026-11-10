//! requirement:member-rights: every journal replays without error after the change of rights.
//! The fixture journal J was written under the owner rule: alex, a human member who did not
//! create W, saved capability c and domain d2, and the agent bot then saved a new title of
//! each. Under the owner rule the saves of alex gave drafts, and an agent may edit a draft
//! title. The replay keeps that meaning before the epoch of the new rule.
use anyhow::{Result, ensure};
use joppa::{
    commands::{self, Command},
    model::Workspace,
    store,
};
use serde_json::{Value, json};

/// The events of J, each as (actor, command).
fn journal() -> Vec<(&'static str, Value)> {
    vec![
        ("owner", json!({"op":"workspace","title":"Old rule"})),
        (
            "owner",
            json!({"op":"enroll_service","id":"bot","role":"agent"}),
        ),
        ("owner", json!({"op":"member","id":"alex","role":"human"})),
        (
            "owner",
            json!({"op":"domain","id":"d","title":"Access","owner":"owner"}),
        ),
        (
            "alex",
            json!({"op":"domain","id":"d2","title":"Drafts","owner":"alex"}),
        ),
        (
            "alex",
            json!({"op":"capability","id":"c","title":"Keep access","domain":"d"}),
        ),
        (
            "bot",
            json!({"op":"edit_capability","id":"c","title":"Keep paid access","reason":"Sharper title"}),
        ),
        (
            "bot",
            json!({"op":"edit_domain","id":"d2","title":"Draft area","owner":"alex","reason":"Sharper title"}),
        ),
    ]
}

/// Replay J with the first position `epoch` for every replay rule.
fn replay(epoch: u64) -> Result<Workspace> {
    let mut w = Workspace {
        replay_epochs: commands::REPLAY_RULES
            .iter()
            .map(|rule| ((*rule).to_owned(), epoch))
            .collect(),
        ..Default::default()
    };
    for (i, (actor, value)) in journal().into_iter().enumerate() {
        let command: Command = serde_json::from_value(value.clone())?;
        commands::apply_replay(
            &mut w,
            actor,
            &command,
            i as u64 + 1,
            "2026-10-01T00:00:00Z",
        )
        .map_err(|e| anyhow::anyhow!("event {}: {value}: {e}", i + 1))?;
    }
    Ok(w)
}

#[test]
fn a_journal_of_the_owner_rule_replays_and_the_capability_of_a_member_stays_a_draft() -> Result<()>
{
    // Every rule of this server has its epoch after the last event of J.
    let after = journal().len() as u64 + 1;
    let w = replay(after)?;
    let members = json!({"alex":"human","bot":"agent","owner":"human"});
    ensure!(json!(w.members) == members, "{:?}", w.members);
    let c = &w.capabilities["c"];
    ensure!(
        c.title == "Keep paid access" && !c.confirmed,
        "capability c: {c:?}"
    );
    let d2 = &w.domains["d2"];
    ensure!(
        d2.title == "Draft area" && !d2.confirmed,
        "domain d2: {d2:?}"
    );
    ensure!(w.domains["d"].confirmed, "domain d of the creator");
    // A replay of the same journal gives the same state.
    let again = replay(after)?;
    ensure!(
        serde_json::to_value(&again)? == serde_json::to_value(&w)?,
        "two replays differ"
    );
    // From the epoch on, the save of a human member is confirmed, and the title of bot is
    // refused: without the epoch, J would not replay.
    let error = replay(1).expect_err("the new rule refuses the title of bot");
    ensure!(
        error.to_string().contains(
            "only a human member edits confirmed capabilities; agents may edit draft titles"
        ),
        "{error}"
    );
    Ok(())
}

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
async fn a_stored_journal_of_the_owner_rule_replays_on_the_server_and_c_stays_a_draft() -> Result<()>
{
    let pool = pool().await?;
    let ws = format!("owner-rule-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":"Old rule"});
    store::execute_input(&pool, &ws, "owner", "workspace", &create).await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    // The other events of J are stored as an older server wrote them.
    let conn = pool.get().await?;
    for (actor, command) in journal().into_iter().skip(2) {
        let request = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,$2,$3,$4::text::jsonb FROM joppa.events WHERE workspace=$1",
            &[&ws, &actor, &request, &command.to_string()],
        )
        .await?;
    }
    // Every rule of this server has its epoch after the last event of J.
    let rules: Vec<&str> = commands::REPLAY_RULES.to_vec();
    conn.execute(
        "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) SELECT $1,r,(SELECT MAX(position)+1 FROM joppa.events WHERE workspace=$1) FROM unnest($2::text[]) r ON CONFLICT (workspace,rule) DO UPDATE SET first_position=EXCLUDED.first_position",
        &[&ws, &rules],
    )
    .await?;
    conn.execute(
        "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    drop(conn);
    store::backfill_board(&pool).await?;
    store::check_journal(&pool).await?;
    let full = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    let members = json!({"alex":"human","bot":"agent","owner":"human"});
    ensure!(full["index"]["members"] == members, "{}", full["index"]);
    let c = &full["state"]["capabilities"]["c"];
    ensure!(
        c["title"] == "Keep paid access" && c["confirmed"] != true,
        "capability c: {c}"
    );
    let d2 = &full["state"]["domains"]["d2"];
    ensure!(
        d2["title"] == "Draft area" && d2["confirmed"] != true,
        "domain d2: {d2}"
    );
    Ok(())
}
