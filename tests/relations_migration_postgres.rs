//! requirement:q438uw0s: migration 062 moves the stored links of the older release to relations
//! without a replay. The test writes the rows as the older release wrote them (older_links.rs)
//! and starts the server twice. The deploy request gives the time of the migration.
#[path = "common/older_links.rs"]
mod older_links;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use older_links::{ROWS, doc, fixture, flat, remote_rows, snapshot, store_older_form, wanted};
use relations_world::{relations, saved};
use serde_json::{Value, json};
use std::collections::BTreeSet;

// ac:ajpnbe0g
// test:TBD
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
    write(
        &k,
        json!({"op":"save_object","kind":"domain","id":"api","revision":0,"parent":null,
        "data":{"title":"API","owner":"owner"},"reason":"Fixture"}),
    )
    .await?;
    let remote = json!({"workspace":k,"target":"domain:api","relation":"related","revision":1,"blocking":false});
    write(&w.ws, json!({"op":"set_remote_links","target":doc(30),"revision":0,"links":[remote],"reason":"Uses the API"})).await?;
    w.stop();
    store_older_form(&w).await?;
    let remote_before = remote_rows(&w).await?;
    ensure!(!remote_before.is_empty(), "W stores the remote link");
    let kept = ["events", "object_revisions", "state_revisions"];
    let before = snapshot(&w, &kept).await?;
    w.start().await?;
    for n in 1..=30 {
        let read = w.object(&doc(n)).await?;
        ensure!(
            flat(&relations(&read)) == wanted(&doc(n)),
            "{}: {read}",
            doc(n)
        );
    }
    let sql = "SELECT source,target,relation,revision,when_label IS NULL FROM joppa.object_links WHERE workspace=$1";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    let found: BTreeSet<(String, String, String, Option<i32>, bool)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
        .collect();
    let expected: BTreeSet<_> = fixture()
        .into_iter()
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
    for row in w.pool.get().await?.query(sql, &[&w.ws]).await? {
        let key: String = row.get(0);
        let mut documents = Vec::new();
        for column in [1, 2] {
            let value: Value =
                serde_json::from_str(row.get::<_, Option<&str>>(column).unwrap_or("{}"))?;
            documents.extend(value["revisions"].as_array().cloned().unwrap_or_default());
        }
        documents.push(serde_json::from_str(
            row.get::<_, Option<&str>>(3).unwrap_or("{}"),
        )?);
        for document in &documents {
            let right =
                flat(&document["relations"]) == wanted(&key) && document.get("links").is_none();
            ensure!(right, "{key}: {document}");
        }
    }
    ensure!(
        remote_rows(&w).await? == remote_before,
        "the remote_links rows stay"
    );
    let sql = "SELECT (SELECT first_position FROM joppa.replay_epochs WHERE workspace=$1 AND rule='relations_reset'),(SELECT max(position)+1 FROM joppa.events WHERE workspace=$1),(SELECT first_view->>'projection_version' FROM joppa.board_cache WHERE workspace=$1)";
    let row = w.pool.get().await?.query_one(sql, &[&w.ws]).await?;
    let (epoch, next, version): (Option<i64>, Option<i64>, Option<String>) =
        (row.get(0), row.get(1), row.get(2));
    ensure!(
        epoch.is_some() && epoch == next,
        "relations_reset {epoch:?}, next position {next:?}"
    );
    ensure!(
        version.as_deref() == Some("22"),
        "board projection version {version:?}"
    );
    ensure!(
        snapshot(&w, &kept).await? == before,
        "events, object_revisions and state_revisions keep every row"
    );
    // A second start writes no row.
    let first = snapshot(&w, &ROWS).await?;
    w.stop();
    w.start().await?;
    ensure!(
        snapshot(&w, &ROWS).await? == first,
        "the second start writes a row"
    );
    Ok(())
}
