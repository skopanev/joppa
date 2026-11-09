//! requirement:check-list-frozen in a replay: an event before the epoch of source_change_reset
//! keeps its old meaning, so a change of a source sets no boundary of the criteria that name
//! it. Migration 060 gives each existing workspace the epoch, and a new workspace stores it.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/source_world.rs"]
mod source_world;
use anyhow::{Context, Result, ensure};
use joppa::{commands, model::Workspace};
use serde_json::json;
use source_world::{NAMED, record, save};
use std::collections::{BTreeMap, BTreeSet};

const RULE: &str = "source_change_reset";

/// Replay the journal of `ws` through commands::apply_replay with the stored replay epochs,
/// and source_change_reset at `epoch`. Gives the criterion_boundaries of the replay.
async fn replay(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    epoch: u64,
) -> Result<BTreeMap<String, String>> {
    let conn = pool.get().await?;
    let mut state = Workspace::default();
    let sql = "SELECT rule,first_position FROM joppa.replay_epochs WHERE workspace=$1";
    for row in conn.query(sql, &[&ws]).await? {
        let position: i64 = row.get(1);
        state.replay_epochs.insert(row.get(0), position as u64);
    }
    state.replay_epochs.insert(RULE.into(), epoch);
    let sql = "SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position";
    for row in conn.query(sql, &[&ws]).await? {
        let position: i64 = row.get(0);
        let command: commands::Command = serde_json::from_str(row.get(3))?;
        commands::apply_replay(
            &mut state,
            row.get(1),
            &command,
            position as u64,
            row.get(2),
        )
        .with_context(|| format!("replay of event {position}"))?;
    }
    Ok(state.criterion_boundaries)
}

/// The stored criterion_boundaries of the live state.
async fn live(pool: &deadpool_postgres::Pool, ws: &str) -> Result<BTreeMap<String, String>> {
    let sql = "SELECT key, value #>> '{}' FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries'";
    let rows = pool.get().await?.query(sql, &[&ws]).await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// The journal time of the event at `position`.
async fn time(pool: &deadpool_postgres::Pool, ws: &str, position: i64) -> Result<String> {
    let sql = "SELECT recorded_at::text FROM joppa.events WHERE workspace=$1 AND position=$2";
    Ok(pool
        .get()
        .await?
        .query_one(sql, &[&ws, &position])
        .await?
        .get(0))
}

/// The stored epoch of source_change_reset of `ws`, when the row exists.
async fn epoch(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Option<i64>> {
    let sql = "SELECT first_position FROM joppa.replay_epochs WHERE workspace=$1 AND rule=$2";
    let row = pool.get().await?.query_opt(sql, &[&ws, &RULE]).await?;
    Ok(row.map(|r| r.get(0)))
}

// ac:olgpns23
// test:rpb6friz
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_replay_keeps_the_old_meaning_before_the_source_change_reset_epoch() -> Result<()> {
    let w = source_world::world().await?;
    // The journal of the source reset test up to the file on S, with a record after each step.
    save(&w, "src:s", json!({"body":"The new source text"})).await?;
    let body = source_world::position(&w).await?;
    record(&w, 1).await?;
    save(&w, "src:s", json!({"title":"S renamed"})).await?;
    let title = source_world::position(&w).await?;
    record(&w, 2).await?;
    source_world::attach(&w).await?;
    let file = source_world::position(&w).await?;
    record(&w, 3).await?;
    let mut changes = BTreeSet::new();
    for position in [body, title, file] {
        changes.insert(time(&w.pool, &w.ws, position).await?);
    }
    let live = live(&w.pool, &w.ws).await?;
    let file_time = time(&w.pool, &w.ws, file).await?;
    for address in NAMED {
        ensure!(
            live.get(address) == Some(&file_time),
            "given: the live boundary of {address} is the file time {file_time}: {live:?}"
        );
    }
    // Replay 1: the rule holds from the first event.
    let first = replay(&w.pool, &w.ws, 1).await?;
    ensure!(
        first == live,
        "replay 1 gives other boundaries:\nreplay {first:?}\nlive {live:?}"
    );
    // Replay 2: the epoch comes after the file event.
    let second = replay(&w.pool, &w.ws, file as u64 + 1).await?;
    for address in NAMED {
        let boundary = second.get(address);
        ensure!(
            boundary.is_none_or(|b| !changes.contains(b)),
            "replay 2 gives {address} the boundary {boundary:?} of a source change {changes:?}"
        );
    }
    // A workspace that this server creates stores the epoch at creation.
    ensure!(
        epoch(&w.pool, &w.ws).await? == Some(1),
        "the new workspace {} stores no epoch 1 of {RULE}",
        w.ws
    );
    let other = format!("source-epoch-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":"Source epoch"});
    joppa::store::execute_input(&w.pool, &other, "owner", "create", &create).await?;
    ensure!(
        epoch(&w.pool, &other).await? == Some(1),
        "the new workspace {other} stores no epoch 1 of {RULE}"
    );
    // Migration 060 gives an existing workspace the epoch max(position)+1.
    let sql = "DELETE FROM joppa.replay_epochs WHERE workspace=$1 AND rule=$2";
    w.pool.get().await?.execute(sql, &[&w.ws, &RULE]).await?;
    joppa::store::migrate(&w.pool).await?;
    let last = source_world::position(&w).await?;
    let migrated = epoch(&w.pool, &w.ws).await?;
    ensure!(
        migrated == Some(last + 1),
        "migration 060 gives {migrated:?}, not {}",
        last + 1
    );
    Ok(())
}
