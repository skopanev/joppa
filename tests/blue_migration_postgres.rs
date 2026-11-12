//! requirement:vqlbur46: migration 063 moves the stored tone grey to blue without a replay.
//! The test writes the rows as the older release wrote them (older_tones.rs): a saved status
//! list with grey and the older pair, and grey in each stored status. `joppa migrate` runs,
//! then the server starts twice. A rebuild from the journal must write the same rows.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/state_compare.rs"]
mod compare;
#[path = "common/older_tones.rs"]
mod older_tones;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use blue_world::{NEW_PAIR, T, blue, blue_entry, statuses, test_node};
use older_tones::{KEPT, ROWS, grey_places, snapshot, store_older_form};
use record_world::World;
use serde_json::{Value, json};

/// The tables that a rebuild from the journal writes, with the order of their rows.
const REBUILT: [(&str, &str); 3] = [
    ("objects", "address"),
    ("board_cache", "workspace"),
    ("state_entries", "section,key"),
];

async fn ok(w: &World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command).await?.map_err(anyhow::Error::msg)
}

/// A saved status list with blue, the dimension field stage and T at stage done. Gives the
/// schema revision of the status list.
async fn fixture(w: &World) -> Result<u64> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let mut rules = tree["schema"]["test_results"].clone();
    if let Some(fields) = rules["fields"].as_array_mut() {
        fields.push(json!({"name":"stage","type":"enum","values":["draft","done"]}));
    }
    let list = json!([{"tone":"green","light":"#22a559","dark":"#2fbf6a"},
        blue(NEW_PAIR.0, NEW_PAIR.1)]);
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "statuses":list,"filters":["stage"],"test_results":rules,"reason":"Statuses and stage"});
    ok(w, &w.owner, schema).await?;
    let t = test_node(w).await?;
    let object = w.object(&t).await?;
    let (kind, id) = t.split_once(':').context("address")?;
    let stage = json!({"op":"save_object","kind":kind,"id":id,"revision":object["revision"],
        "data":{"stage":"done"},"reason":"T is done"});
    ok(w, &w.bot, stage).await?;
    let revision = tree["schema"]["revision"].as_u64().context("revision")?;
    Ok(revision + 1)
}

/// The normalized rows of W in the tables of REBUILT.
async fn rows(w: &World) -> Result<Vec<(String, Vec<Value>)>> {
    let names = compare::names(&w.pool, &w.ws).await?;
    let conn = w.pool.get().await?;
    let mut out = Vec::new();
    for (table, order) in REBUILT {
        let sql = format!(
            "SELECT row_to_json(t)::text FROM joppa.{table} t WHERE workspace=$1 ORDER BY {order}"
        );
        let mut found = Vec::new();
        for row in conn.query(&sql, &[&w.ws]).await? {
            let value: Value = serde_json::from_str(row.get(0))?;
            found.push(compare::normalize(&value, &names)?);
        }
        out.push((table.to_owned(), found));
    }
    Ok(out)
}

/// The projection versions of the board row of W.
async fn versions(w: &World) -> Result<(Option<String>, Option<String>)> {
    let sql = "SELECT first_view->>'projection_version',first_view->'status_projection'->>'version' FROM joppa.board_cache WHERE workspace=$1";
    let row = w.pool.get().await?.query_one(sql, &[&w.ws]).await?;
    Ok((row.get(0), row.get(1)))
}

/// Run `joppa migrate` on the test database.
fn migrate() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_joppa"))
        .arg("migrate")
        .env("JOPPA_DATABASE_URL", &url)
        .output()?;
    let error = String::from_utf8_lossy(&out.stderr);
    ensure!(out.status.success(), "joppa migrate: {error}");
    Ok(())
}

// ac:krnb06z3
// test:naoza0nc
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn migration_063_moves_the_stored_tones_without_a_replay() -> Result<()> {
    let mut w = record_world::world(&[T]).await?;
    let revision = fixture(&w).await?;
    w.stop();
    store_older_form(&w).await?;
    let older = grey_places(&w).await?;
    let places = ["objects", "board_cache", "state_entries"];
    for place in places {
        let found = older.iter().any(|p| p.starts_with(place));
        ensure!(found, "the older form holds grey in {place}: {older:?}");
    }
    let before = snapshot(&w, &KEPT).await?;

    migrate()?;
    w.start().await?;
    let left = grey_places(&w).await?;
    ensure!(left.is_empty(), "grey after 063: {left:?}");
    let kept = "events, object_revisions, state_revisions and command_receipts keep every row";
    ensure!(snapshot(&w, &KEPT).await? == before, "{kept}");
    let (board, status) = versions(&w).await?;
    ensure!(
        board.as_deref() == Some("22") && status.as_deref() == Some("13"),
        "projection versions {board:?} {status:?}"
    );
    // The stored schema revision of the older list reads blue with the new pair.
    let list = statuses(&w, &w.ws, Some(revision)).await?;
    ensure!(blue_entry(&list)? == blue(NEW_PAIR.0, NEW_PAIR.1), "{list}");
    let t = test_node(&w).await?;
    ensure!(w.object(&t).await?["status"]["tone"] == "blue", "T");

    // A second start writes no row.
    let first = snapshot(&w, &ROWS).await?;
    w.stop();
    w.start().await?;
    ensure!(
        snapshot(&w, &ROWS).await? == first,
        "the second start writes a row"
    );

    // A rebuild from the journal writes the same rows as 063.
    let migrated = rows(&w).await?;
    w.stop();
    let sql = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
    w.pool.get().await?.execute(sql, &[&w.ws]).await?;
    w.start().await?;
    let rebuilt = rows(&w).await?;
    for ((table, a), (_, b)) in migrated.iter().zip(&rebuilt) {
        let diff = compare::difference(a, b);
        ensure!(
            diff.is_none(),
            "{table}: 063 and the rebuild differ\n{diff:?}"
        );
    }
    Ok(())
}
