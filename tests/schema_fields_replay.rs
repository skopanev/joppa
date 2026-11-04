//! requirement:generic-nodes: an older set_tree_schema with process_fields and
//! verification_subject still replays, while a live write refuses it.
mod common;

use common::*;
use joppa::commands::{self, Command};
use joppa::objects;
use joppa::removed_ops::SCHEMA_FIELDS;
use serde_json::json;

// generic-nodes-ac6
#[test]
fn an_older_schema_with_the_removed_fields_replays_and_a_live_write_refuses_it() {
    let mut w = World::new();
    for op in ["migrate_notes", "migrate_tasks", "migrate_schema"] {
        let cmd: Command = serde_json::from_value(json!({ "op": op })).unwrap();
        commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
    let schema = objects::schema(&w.state);
    let mut types = serde_json::to_value(&schema.types).unwrap();
    types["ac"]["process_fields"] = json!(["confirmed"]);
    types["task"]["references"]["projects"]["verification_subject"] = json!(true);
    let older = json!({"op":"set_tree_schema","revision":schema.revision,"types":types,
        "reason":"An older schema"});
    let live = w.apply("owner", older.clone()).unwrap_err().to_string();
    assert_eq!(live, SCHEMA_FIELDS);
    let cmd: Command = serde_json::from_value(older).unwrap();
    commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
    let replayed = objects::schema(&w.state);
    assert_eq!(replayed.revision, schema.revision + 1);
    assert_eq!(
        replayed.types["task"].references["projects"].verification_subject,
        Some(true),
        "the replay keeps the subject of the older Task saves"
    );
    // No read shows the removed fields.
    let shown = serde_json::to_string(&replayed.types).unwrap();
    assert!(
        !shown.contains("process_fields") && !shown.contains("verification_subject"),
        "{shown}"
    );
}

// generic-nodes-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stored_schema_revision_with_the_removed_fields_survives_a_reprojection()
-> anyhow::Result<()> {
    let pool = joppa::store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    joppa::store::migrate(&pool).await?;
    let ws = format!("schema-fields-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":"Schema fields"});
    joppa::store::execute_input(&pool, &ws, "owner", "workspace", &create).await?;
    // An older server stored the schema with both fields. The trigger keeps such rows, so the
    // seed writes it in its own session without triggers.
    let seed = "SET LOCAL session_replication_role = replica; \
        UPDATE joppa.state_revisions SET value = jsonb_set(jsonb_set(value, '{types,ac,process_fields}', '[\"confirmed\"]'), \
        '{types,ac,references,consumer,verification_subject}', 'false') \
        WHERE workspace = '{ws}' AND section = 'tree_schema'";
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.batch_execute(&seed.replace("{ws}", &ws)).await?;
    tx.commit().await?;
    let stored = "SELECT jsonb_agg(value ORDER BY revision)::text FROM joppa.state_revisions \
        WHERE workspace = $1 AND section = 'tree_schema'";
    let before: String = conn.query_one(stored, &[&ws]).await?.get(0);
    assert!(before.contains("process_fields") && before.contains("verification_subject"));
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace = $1", &[&ws])
        .await?;
    joppa::store::migrate(&pool).await?;
    joppa::store::backfill_board(&pool).await?;
    let after: String = conn.query_one(stored, &[&ws]).await?.get(0);
    assert_eq!(
        after, before,
        "a reprojection keeps each stored schema revision"
    );
    Ok(())
}
