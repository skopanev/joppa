//! Migration 063 moves the stored tone grey of the older release to blue without a replay
//! (Deploy 19b, B5). The test writes the rows as the older release wrote them: the server of
//! this release writes blue, and the test gives the rows the older names and colours. The
//! server starts twice, and then a rebuild from the journal must write the same rows.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// The tables that keep every row.
const KEPT: [&str; 4] = [
    "events",
    "object_revisions",
    "state_revisions",
    "command_receipts",
];
/// The tables that 063 moves.
const MOVED: [&str; 3] = ["objects", "board_cache", "state_entries"];

/// The xmin of each row of `workspaces` in each table.
async fn xmins(w: &World, workspaces: &[&str], tables: &[&str]) -> Result<Vec<String>> {
    let mut out = vec![];
    for table in tables {
        let sql = format!(
            "SELECT workspace||' '||xmin::text FROM joppa.{table} WHERE workspace=ANY($1) ORDER BY 1"
        );
        let rows = w.pool.get().await?.query(&sql, &[&workspaces]).await?;
        out.extend(
            rows.iter()
                .map(|r| format!("{table} {}", r.get::<_, String>(0))),
        );
    }
    Ok(out)
}

/// The rows of the places of migration 063, as text.
async fn rows(w: &World, workspaces: &[&str]) -> Result<Vec<String>> {
    let sql = "SELECT workspace||' '||address||' '||signal::text||' '||COALESCE(rollup::text,'')||' '||COALESCE(test_status::text,'')||' '||COALESCE(dim_tones::text,'') FROM joppa.objects WHERE workspace=ANY($1)
        UNION ALL SELECT workspace||' '||state::text||' '||(first_view->'statuses')::text FROM joppa.board_cache WHERE workspace=ANY($1)
        UNION ALL SELECT workspace||' '||value::text FROM joppa.state_entries WHERE workspace=ANY($1) AND section='meta'
        ORDER BY 1";
    let rows = w.pool.get().await?.query(sql, &[&workspaces]).await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// Give the rows of `workspaces` the older tone and its older default colours, with the guards
/// of the journal and of the schema revisions off, in one transaction.
async fn store_older_form(w: &World, workspaces: &[&str]) -> Result<()> {
    let older = |column: &str| {
        format!(
            "replace(replace(replace(({column})::text,'\"blue\"','\"grey\"'),'\"#6fa8dc\"','\"#b8b3a8\"'),'\"#8bbbe8\"','\"#b4b8bf\"')::jsonb"
        )
    };
    let mut client = w.pool.get().await?;
    let tx = client.transaction().await?;
    tx.batch_execute("ALTER TABLE joppa.events DISABLE TRIGGER events_worm; ALTER TABLE joppa.state_revisions DISABLE TRIGGER state_revision_contract").await?;
    let updates = [
        format!(
            "UPDATE joppa.events SET command={} WHERE workspace=ANY($1) AND command->>'op'='set_tree_schema'",
            older("command")
        ),
        format!(
            "UPDATE joppa.state_revisions SET value={} WHERE workspace=ANY($1) AND section='tree_schema'",
            older("value")
        ),
        format!(
            "UPDATE joppa.objects SET signal={},rollup={},test_status={},dim_tones={} WHERE workspace=ANY($1)",
            older("signal"),
            older("rollup"),
            older("test_status"),
            older("dim_tones")
        ),
        format!(
            "UPDATE joppa.board_cache SET state={},first_view=first_view||jsonb_build_object('statuses',{}) WHERE workspace=ANY($1)",
            older("state"),
            older("first_view->'statuses'")
        ),
        format!(
            "UPDATE joppa.state_entries SET value={} WHERE workspace=ANY($1) AND section='meta'",
            older("value")
        ),
    ];
    for sql in updates {
        tx.execute(&sql, &[&workspaces]).await?;
    }
    tx.batch_execute("ALTER TABLE joppa.events ENABLE TRIGGER events_worm; ALTER TABLE joppa.state_revisions ENABLE TRIGGER state_revision_contract").await?;
    tx.commit().await?;
    Ok(())
}

/// Rebuild `workspaces` from the journal, as a raise of the board projection version does.
async fn rebuild(w: &World, workspaces: &[&str]) -> Result<()> {
    let sql = "UPDATE joppa.board_cache SET first_view=first_view||'{\"projection_version\":\"0\"}' WHERE workspace=ANY($1)";
    w.pool.get().await?.execute(sql, &[&workspaces]).await?;
    joppa::store::migrate(&w.pool).await
}

/// The status list of schema revision `n` of `ws`, as joppa_tree gives it.
async fn statuses(w: &World, ws: &str, n: u32) -> Result<Vec<(String, String, String)>> {
    let tree = joppa::store::tree_schema(&w.pool, ws, "owner", Some(n)).await?;
    let text = |s: &Value, k: &str| s[k].as_str().unwrap_or_default().to_owned();
    let list = tree["schema"]["statuses"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok((list.iter())
        .map(|s| (text(s, "tone"), text(s, "light"), text(s, "dark")))
        .collect())
}

fn entry(tone: &str, light: &str, dark: &str) -> (String, String, String) {
    (tone.into(), light.into(), dark.into())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn migration_063_moves_the_tone_grey_to_blue_without_a_replay() -> Result<()> {
    let mut w = record_world::world(&["tests/x.rs > t1"]).await?;
    let write = async |ws: &str, command: Value| {
        let id = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_remote_input(&w.pool, ws, "owner", &id, &command).await
    };
    // W: an older default pair, and a dimension env, so dim_tones holds the tone too.
    let tree = joppa::store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut rules = tree["schema"]["test_results"].clone();
    let env = json!({"name":"env","type":"enum","values":["dev","prod"]});
    rules["fields"]
        .as_array_mut()
        .into_iter()
        .for_each(|f| f.push(env.clone()));
    let list = json!([{"tone":"red","light":"#bc5d52","dark":"#bc5d52"},
        {"tone":"blue","light":"#c8c3b8","dark":"#8a8f96"}]);
    let revision = tree["schema"]["revision"].as_u64().unwrap_or_default() as u32;
    write(
        &w.ws,
        json!({"op":"set_tree_schema","revision":revision,"filters":["env"],
        "test_results":rules,"statuses":list,"reason":"Colour the tests"}),
    )
    .await?;
    // S: the older status_colors map, which only the journal of the older release holds.
    let s = format!("older-map-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":"Older map"});
    joppa::store::execute_input(&w.pool, &s, "owner", "older-map-1", &create).await?;
    let current = joppa::store::tree_schema(&w.pool, &s, "owner", None).await?;
    let r = current["schema"]["revision"].as_u64().unwrap_or_default() as u32;
    let levels =
        json!({"op":"set_tree_schema","revision":r,"priorities":["p0","p1"],"reason":"Two levels"});
    write(&s, levels).await?;
    w.stop();
    let ws = w.ws.clone();
    let both = [ws.as_str(), s.as_str()];
    let map = "UPDATE joppa.events SET command=command||'{\"status_colors\":{\"blue\":\"#c8c3b8\"}}' WHERE workspace=$1 AND command->>'op'='set_tree_schema'; DELETE FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema'";
    let mut client = w.pool.get().await?;
    let tx = client.transaction().await?;
    tx.batch_execute("ALTER TABLE joppa.events DISABLE TRIGGER events_worm")
        .await?;
    for sql in map.split("; ") {
        tx.execute(sql, &[&s]).await?;
    }
    tx.batch_execute("ALTER TABLE joppa.events ENABLE TRIGGER events_worm")
        .await?;
    tx.commit().await?;
    drop(client);
    rebuild(&w, &[&s]).await?;
    store_older_form(&w, &both).await?;
    let older = rows(&w, &both).await?;
    ensure!(
        older.iter().any(|r| r.contains("\"grey\""))
            && older.iter().all(|r| !r.contains("\"blue\"")),
        "the older form: {older:#?}"
    );
    // Each place of 063 holds grey: the tones of objects, both status lists and the older map.
    let sql = "SELECT count(*) FILTER (WHERE signal->>'tone'='grey')>0 AND count(*) FILTER (WHERE test_status->>'status'='grey')>0 AND count(*) FILTER (WHERE dim_tones::text LIKE '%\"grey\"%')>0 AND (SELECT count(*) FROM joppa.board_cache WHERE workspace=ANY($1) AND (first_view->'statuses')::text LIKE '%\"#c8c3b8\"%')=2 AND (SELECT count(*) FROM joppa.state_entries WHERE workspace=ANY($1) AND section='meta' AND value::text LIKE '%\"status_colors\": {\"grey\"%')=1 FROM joppa.objects WHERE workspace=ANY($1)";
    let all: bool = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&&both[..]])
        .await?
        .get(0);
    ensure!(all, "a place of 063 holds no grey: {older:#?}");
    let kept = xmins(&w, &both, &KEPT).await?;
    // The serve start runs 063.
    w.start().await?;
    let moved = rows(&w, &both).await?;
    ensure!(
        moved.iter().all(|r| !r.contains("\"grey\"")),
        "a place of 063 holds grey: {moved:#?}"
    );
    ensure!(xmins(&w, &both, &KEPT).await? == kept, "a kept row changed");
    // A second start and `joppa migrate` write no row.
    let first = xmins(&w, &both, &MOVED).await?;
    w.stop();
    w.start().await?;
    w.stop();
    joppa::store::migrate(&w.pool).await?;
    ensure!(
        xmins(&w, &both, &MOVED).await? == first,
        "a second run writes a row"
    );
    // Each stored schema revision reads as blue.
    let new = ["#6fa8dc", "#8bbbe8"];
    let wanted = vec![
        entry("red", "#bc5d52", "#bc5d52"),
        entry("blue", new[0], new[1]),
        entry("purple", "#8061a9", "#9b7bc6"),
        entry("yellow", "#ae7a16", "#c8921f"),
        entry("orange", "#c4772e", "#c4772e"),
        entry("green", "#39835a", "#39835a"),
    ];
    let found = statuses(&w, &w.ws, revision + 1).await?;
    ensure!(found == wanted, "W: {found:?}");
    let found = statuses(&w, &w.ws, revision).await?;
    ensure!(
        found.contains(&entry("blue", new[0], new[1])),
        "W {revision}: {found:?}"
    );
    let found = statuses(&w, &s, r + 1).await?;
    ensure!(
        found.contains(&entry("blue", "#c8c3b8", "#c8c3b8")),
        "S: {found:?}"
    );
    // A rebuild from the journal writes the same rows as 063.
    rebuild(&w, &both).await?;
    let rebuilt = rows(&w, &both).await?;
    for (a, b) in moved.iter().zip(&rebuilt) {
        ensure!(a == b, "063 wrote\n{a}\nand the rebuild wrote\n{b}");
    }
    ensure!(moved.len() == rebuilt.len(), "{moved:#?} {rebuilt:#?}");
    Ok(())
}
