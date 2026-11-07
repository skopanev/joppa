//! requirement:readiness-per-criterion: migrate_decision_required runs at server start on an
//! old journal under the real trigger protect_state_revision. It changes no stored criterion
//! record of a requirement, so the start succeeds and each document loses the flag.
mod common;
use anyhow::{Result, ensure};
use common::*;
use joppa::store;
use serde_json::json;
use uuid::Uuid;

// readiness-per-criterion-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_start_migration_keeps_each_stored_criterion_record() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let ws = format!("decision-{}", Uuid::new_v4().simple());
    // One criterion names the owner of its requirement, one has its own owner and a stored
    // flag: a changed criterion record would break the stored contract.
    let mut journal = setup();
    let last = journal.len() - 1;
    journal[last]["acs"] = json!([
        {"id":"ios","text":"On iOS paid access survives login","owner":"owner"},
        {"id":"key","text":"Before production key has exactly read and verify permissions",
            "before_launch":true,"owner":"legal","decision_required":false}
    ]);
    journal.push(json!({"op":"confirm","req":"r","revision":1}));
    let conn = pool.get().await?;
    conn.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&ws])
        .await?;
    for rule in joppa::commands::REPLAY_RULES {
        conn.execute(
            "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) VALUES($1,$2,1)",
            &[&ws, &rule],
        )
        .await?;
    }
    for (index, command) in journal.iter().enumerate() {
        let actor = if command["op"] == "assess" {
            "agent"
        } else {
            "owner"
        };
        conn.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb)", &[&ws,&((index+1) as i64),&actor,&Uuid::new_v4().to_string(),&command.to_string()]).await?;
    }
    store::backfill_board(&pool).await?;
    let records = "SELECT jsonb_agg(value ORDER BY revision)::text FROM joppa.state_revisions WHERE workspace=$1 AND section='requirements'";
    let before: String = conn.query_one(records, &[&ws]).await?.get(0);
    store::migrate_notes(&pool).await?;
    let after: String = conn.query_one(records, &[&ws]).await?.get(0);
    ensure!(before == after, "{before}\n{after}");
    let events: i64 = conn.query_one("SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='migrate_decision_required'", &[&ws]).await?.get(0);
    ensure!(events == 1, "{events}");
    let flagged: i64 = conn.query_one("SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND present AND document->'data' ? 'decision_required'", &[&ws]).await?.get(0);
    ensure!(flagged == 0, "{flagged}");
    let owners: Vec<(String, String)> = conn.query("SELECT address, document->'data'->>'owner' FROM joppa.objects WHERE workspace=$1 AND kind='ac' ORDER BY address", &[&ws]).await?.iter().map(|r| (r.get(0), r.get(1))).collect();
    ensure!(
        owners
            == [("ac:ios", "owner"), ("ac:key", "legal")]
                .map(|(a, o)| (a.to_owned(), o.to_owned())),
        "{owners:?}"
    );
    // A second start finds the workspace migrated and changes nothing.
    store::migrate_notes(&pool).await?;
    let again: i64 = conn.query_one("SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='migrate_decision_required'", &[&ws]).await?.get(0);
    ensure!(again == 1, "{again}");
    Ok(())
}
