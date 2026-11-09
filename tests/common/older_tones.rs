//! The stored rows of the older release (7cff677, before requirement:vqlbur46) for the test
//! of migration 063: the tone grey and the older default pair where this release stores blue
//! and the new pair. Also the row reads of the test.
//!
//! The older form replaces the text "blue" with "grey", #6fa8dc with #c8c3b8 and #8bbbe8 with
//! #8a8f96 (the pair that production holds) in:
//! - the jsonb columns of joppa.objects other than document;
//! - the jsonb columns of joppa.board_cache;
//! - the rows of state_entries and state_revisions;
//! - the set_tree_schema events of the journal, and command_receipts.
#![allow(dead_code)]
use super::record_world::World;
use anyhow::Result;

/// The tables of the rows of one workspace that a server start can write.
pub const ROWS: [&str; 8] = [
    "events",
    "object_revisions",
    "state_revisions",
    "state_entries",
    "objects",
    "board_cache",
    "command_receipts",
    "replay_epochs",
];

/// The tables that keep every row: migration 063 does not write them.
pub const KEPT: [&str; 4] = [
    "events",
    "object_revisions",
    "state_revisions",
    "command_receipts",
];

/// The older text of a column: the SQL of `column` with the replacements.
fn older(column: &str) -> String {
    format!(
        "replace(replace(replace({column}::text,'\"blue\"','\"grey\"'),'\"#6fa8dc\"','\"#c8c3b8\"'),'\"#8bbbe8\"','\"#8a8f96\"')::jsonb"
    )
}

/// Rewrite the rows of W as the older release wrote them.
pub async fn store_older_form(w: &World) -> Result<()> {
    let mut client = w.pool.get().await?;
    let conn = client.transaction().await?;
    // The journal, the receipts and the schema revisions are write-once. The test gives them
    // the older form with the guards off, in this transaction only.
    let guards = [
        ("events", "events_worm"),
        ("command_receipts", "command_receipts_worm"),
        ("state_revisions", "state_revision_contract"),
    ];
    for (table, trigger) in guards {
        let sql = format!("ALTER TABLE joppa.{table} DISABLE TRIGGER {trigger}");
        conn.batch_execute(&sql).await?;
    }
    let updates = [
        format!(
            "UPDATE joppa.objects SET signal={},process={},rollup={},test_status={},dim_tones={} WHERE workspace=$1",
            older("signal"),
            older("process"),
            older("rollup"),
            older("test_status"),
            older("dim_tones")
        ),
        format!(
            "UPDATE joppa.board_cache SET state={},summary={},signals={},first_view={} WHERE workspace=$1",
            older("state"),
            older("summary"),
            older("signals"),
            older("first_view")
        ),
        format!(
            "UPDATE joppa.state_entries SET value={},compact={} WHERE workspace=$1",
            older("value"),
            older("compact")
        ),
        format!(
            "UPDATE joppa.state_revisions SET value={} WHERE workspace=$1",
            older("value")
        ),
        format!(
            "UPDATE joppa.events SET command={} WHERE workspace=$1 AND command->>'op'='set_tree_schema'",
            older("command")
        ),
        format!(
            "UPDATE joppa.command_receipts SET input={},result={} WHERE workspace=$1",
            older("input"),
            older("result")
        ),
    ];
    for sql in &updates {
        conn.execute(sql.as_str(), &[&w.ws]).await?;
    }
    for (table, trigger) in guards {
        let sql = format!("ALTER TABLE joppa.{table} ENABLE TRIGGER {trigger}");
        conn.batch_execute(&sql).await?;
    }
    conn.commit().await?;
    Ok(())
}

/// The xmin of each row of W in each table of `tables`.
pub async fn snapshot(w: &World, tables: &[&str]) -> Result<Vec<Vec<String>>> {
    let conn = w.pool.get().await?;
    let mut out = Vec::new();
    for table in tables {
        let sql = format!("SELECT xmin::text FROM joppa.{table} WHERE workspace=$1 ORDER BY 1");
        let rows = conn.query(&sql, &[&w.ws]).await?;
        out.push(rows.iter().map(|r| r.get(0)).collect());
    }
    Ok(out)
}

/// The places of the tone that migration 063 rewrites, as text: each row of W that holds
/// "grey" there, by table and key.
pub async fn grey_places(w: &World) -> Result<Vec<String>> {
    let reads = [
        "SELECT 'objects '||address FROM joppa.objects WHERE workspace=$1 AND concat(signal::text,rollup::text,test_status::text,dim_tones::text) LIKE '%\"grey\"%'",
        "SELECT 'board_cache '||workspace FROM joppa.board_cache WHERE workspace=$1 AND concat((state->'tree_schema')::text,(first_view->'statuses')::text) LIKE '%\"grey\"%'",
        "SELECT 'state_entries meta' FROM joppa.state_entries WHERE workspace=$1 AND section='meta' AND concat(value::text,compact::text) LIKE '%\"grey\"%'",
    ];
    let conn = w.pool.get().await?;
    let mut out = Vec::new();
    for sql in reads {
        for row in conn.query(sql, &[&w.ws]).await? {
            out.push(row.get(0));
        }
    }
    Ok(out)
}
