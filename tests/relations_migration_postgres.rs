//! requirement:q438uw0s: migration 062 moves the stored links of the older release to relations
//! without a replay and keeps a rollback form. The test writes the rows as the older release
//! wrote them (older_links.rs). `joppa migrate` runs as the install role, then the server
//! starts twice as the application role of server/install.sh, which may run no DDL. The test
//! does not build the older binary: the stored rows show that it can read them. The deploy
//! request gives the time of the migration.
#[path = "common/older_links.rs"]
mod older_links;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Context, Result, ensure};
use older_links::{ROWS, doc, fixture, flat, remote_rows, snapshot, store_older_form, wanted};
use record_world::{World, serve};
use relations_world::{relations, saved, without_key};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// One link of the older form: target, relation, blocking and pin.
type Link = (String, String, bool, Option<u64>);

/// Links of the older form, sorted.
fn links(value: &Value) -> Vec<Link> {
    let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let mut out: Vec<Link> = (value.as_array().into_iter().flatten())
        .map(|l| {
            (
                text(&l["target"]),
                text(&l["relation"]),
                l["blocking"] == true,
                l["revision"].as_u64(),
            )
        })
        .collect();
    out.sort();
    out
}

/// The rollback links of `address`: depends_on blocks, related does not, pins stay.
fn shadow(address: &str) -> Vec<Link> {
    let edges = wanted(address).into_iter();
    let mut out: Vec<Link> = edges
        .map(|(k, t, r)| (t, k.clone(), k == "depends_on", r))
        .collect();
    out.sort();
    out
}

/// The GRANT block of server/install.sh for `role` in database `db`.
fn install_grants(role: &str, db: &str) -> Result<String> {
    let script = include_str!("../server/install.sh");
    let start = script
        .find("GRANT CONNECT ON DATABASE joppa TO joppa_app;")
        .context("install.sh has no grant block")?;
    let end = start + script[start..].find("\nSQL\n").context("grant block end")?;
    Ok(script[start..end]
        .replace("DATABASE joppa ", &format!("DATABASE \"{db}\" "))
        .replace("joppa_app", role))
}

/// Start the server as the application role on the port of W, with the tokens of W.
async fn start_app(w: &World, url: &str, tokens: &std::path::Path) -> Result<serve::Server> {
    let port: u16 = w.root.rsplit(':').next().context("port")?.parse()?;
    let env = [("JOPPA_PUBLIC_URL".to_owned(), w.root.clone())];
    serve::start_with(url, tokens, port, &env).await
}

/// The reads and the rows of W after the migration.
async fn migrated(w: &World) -> Result<()> {
    for n in 1..=30 {
        let read = w.object(&doc(n)).await?;
        let right = flat(&relations(&read)) == wanted(&doc(n)) && without_key(&read, "links");
        ensure!(right, "{}: {read}", doc(n));
    }
    let sql = "SELECT source,target,relation,revision,blocking,when_label IS NULL FROM joppa.object_links WHERE workspace=$1";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    type Row = (String, String, String, Option<i32>, bool, bool);
    let found: BTreeSet<Row> = (rows.iter())
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
        .collect();
    let expected: BTreeSet<Row> = (fixture().into_iter())
        .flat_map(|(source, _, edges)| {
            edges.into_iter().map(move |(k, t, r)| {
                (
                    source.clone(),
                    t,
                    k.to_owned(),
                    r.map(|r| r as i32),
                    k == "depends_on",
                    true,
                )
            })
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
        // The state rows hold relations and the rollback links.
        for column in [1, 2] {
            let value: Value = serde_json::from_str(&text(column))?;
            for revision in value["revisions"].as_array().into_iter().flatten() {
                let right = flat(&revision["relations"]) == wanted(&key)
                    && links(&revision["links"]) == shadow(&key);
                ensure!(right, "state row {key}: {revision}");
            }
        }
        // A migrated node maps back exactly: its document holds only the shadow links.
        let document: Value = serde_json::from_str(&text(3))?;
        let right =
            links(&document["links"]) == shadow(&key) && document.get("relations").is_none();
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
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let db: String = w
        .pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    let role = format!("joppa_app_{}", uuid::Uuid::new_v4().simple());
    let grants = install_grants(&role, &db)?;
    let setup = format!("CREATE ROLE {role} NOLOGIN; GRANT {role} TO CURRENT_USER; {grants}");
    w.pool.get().await?.batch_execute(&setup).await?;
    let result = async {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_joppa"))
            .arg("migrate")
            .env("JOPPA_DATABASE_URL", &url)
            .output()?;
        ensure!(
            out.status.success(),
            "joppa migrate: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let separator = if url.contains('?') { '&' } else { '?' };
        let app = format!("{url}{separator}options=-c%20role%3D{role}");
        let tokens =
            std::env::temp_dir().join(format!("joppa-d19-{}.json", uuid::Uuid::new_v4().simple()));
        let names = json!({w.owner.clone():"owner",w.bot.clone():"bot",w.alex.clone():"alex"});
        std::fs::write(&tokens, names.to_string())?;
        // A serve start as the application role fails on DDL.
        let server = start_app(&w, &app, &tokens).await?;
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
        let server = start_app(&w, &app, &tokens).await?;
        ensure!(
            snapshot(&w, &ROWS).await? == first,
            "the second start writes a row"
        );
        drop(server);
        Ok(())
    }
    .await;
    let cleanup =
        format!("REVOKE {role} FROM CURRENT_USER; DROP OWNED BY {role}; DROP ROLE {role}");
    w.pool.get().await?.batch_execute(&cleanup).await?;
    result
}
