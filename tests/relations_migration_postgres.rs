//! requirement:q438uw0s: migration 062 moves the stored links of the older release to relations
//! without a replay, and migration 064 then stores relations only. The test writes the rows as
//! the older release wrote them (older_links.rs). `joppa migrate` runs as the install role, then
//! the server starts twice as the application role of server/install.sh, which may run no DDL.
//! The deploy request gives the time of the migration.
#[path = "common/app_role.rs"]
mod app_role;
#[path = "common/older_links.rs"]
mod older_links;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use app_role::{AppRole, migrate};
use older_links::{ROWS, doc, fixture, flat, remote_rows, snapshot, store_older_form, wanted};
use record_world::World;
use relations_world::{relations, saved, without_key};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The reads and the rows of W after the migration.
async fn migrated(w: &World) -> Result<()> {
    for n in 1..=30 {
        let read = w.object(&doc(n)).await?;
        let right = flat(&relations(&read)) == wanted(&doc(n)) && without_key(&read, "links");
        ensure!(right, "{}: {read}", doc(n));
    }
    let sql = "SELECT source,target,relation,revision,when_label IS NULL FROM joppa.object_links WHERE workspace=$1";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    type Row = (String, String, String, Option<i32>, bool);
    let found: BTreeSet<Row> = (rows.iter())
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
        .collect();
    let expected: BTreeSet<Row> = (fixture().into_iter())
        .flat_map(|(source, _, edges)| {
            edges
                .into_iter()
                .map(move |(k, t, r)| (source.clone(), t, k.to_owned(), r.map(|r| r as i32), true))
        })
        .collect();
    ensure!(
        rows.len() == expected.len() && found == expected,
        "object_links: {found:?}"
    );
    let sql = "SELECT key,value::text,compact::text,(SELECT document::text FROM joppa.objects o WHERE o.workspace=s.workspace AND o.address=s.key) FROM joppa.state_entries s WHERE workspace=$1 AND section='objects' AND key LIKE 'doc:%'";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    ensure!(rows.len() == 30, "{} object rows", rows.len());
    for row in rows {
        let key: String = row.get(0);
        let text = |i: usize| row.get::<_, Option<&str>>(i).unwrap_or("{}").to_owned();
        // After migration 064 the state rows and the document hold relations and no links.
        for column in [1, 2] {
            let value: Value = serde_json::from_str(&text(column))?;
            for revision in value["revisions"].as_array().into_iter().flatten() {
                let right =
                    flat(&revision["relations"]) == wanted(&key) && revision.get("links").is_none();
                ensure!(right, "state row {key}: {revision}");
            }
        }
        let document: Value = serde_json::from_str(&text(3))?;
        let right = document["relations"].is_object()
            && flat(&document["relations"]) == wanted(&key)
            && document.get("links").is_none();
        ensure!(right, "document {key}: {document}");
    }
    let sql = "SELECT (SELECT first_position FROM joppa.replay_epochs WHERE workspace=$1 AND rule='relations_reset'),(SELECT max(position)+1 FROM joppa.events WHERE workspace=$1),(SELECT first_view->>'projection_version' FROM joppa.board_cache WHERE workspace=$1)";
    let row = w.pool.get().await?.query_one(sql, &[&w.ws]).await?;
    let (epoch, next, version): (Option<i64>, Option<i64>, Option<String>) =
        (row.get(0), row.get(1), row.get(2));
    ensure!(
        epoch.is_some() && epoch == next,
        "relations_reset {epoch:?}, next {next:?}"
    );
    ensure!(
        version.as_deref() == Some("22"),
        "projection version {version:?}"
    );
    Ok(())
}

// ac:ajpnbe0g
// test:56hv9nbj
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn migration_062_moves_the_stored_links_without_a_replay() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    for n in 1..=30 {
        saved(&w, &doc(n), 0, json!({})).await?;
    }
    // doc:n30 has a remote link to workspace K.
    let k = format!("kept-{}", uuid::Uuid::new_v4().simple());
    let write = async |ws: &str, command: Value| {
        let request = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_remote_input(&w.pool, ws, "owner", &request, &command).await
    };
    let create = json!({"op":"workspace","title":"Kept"});
    joppa::store::execute_input(&w.pool, &k, "owner", "kept-1", &create).await?;
    let api = json!({"op":"save_object","kind":"domain","id":"api","revision":0,"parent":null,
        "data":{"title":"API","owner":"owner"},"reason":"Fixture"});
    write(&k, api).await?;
    let remote = json!({"workspace":k,"target":"domain:api","relation":"related","revision":1,"blocking":false});
    write(&w.ws, json!({"op":"set_remote_links","target":doc(30),"revision":0,"links":[remote],"reason":"Uses the API"})).await?;
    w.stop();
    store_older_form(&w).await?;
    let remote_before = remote_rows(&w).await?;
    ensure!(!remote_before.is_empty(), "W stores the remote link");
    let kept = ["events", "object_revisions", "state_revisions"];
    let before = snapshot(&w, &kept).await?;
    // The application role has exactly the grants of server/install.sh.
    let app = AppRole::create(&w).await?;
    let result = async {
        migrate()?;
        // A serve start as the application role fails on DDL.
        let server = app.start(&w).await?;
        migrated(&w).await?;
        ensure!(
            remote_rows(&w).await? == remote_before,
            "the remote_links rows stay"
        );
        let rows = "events, object_revisions and state_revisions keep every row";
        ensure!(snapshot(&w, &kept).await? == before, "{rows}");
        // A second start writes no row.
        let first = snapshot(&w, &ROWS).await?;
        drop(server);
        let server = app.start(&w).await?;
        ensure!(
            snapshot(&w, &ROWS).await? == first,
            "the second start writes a row"
        );
        drop(server);
        Ok(())
    }
    .await;
    app.remove(&w).await?;
    result
}
