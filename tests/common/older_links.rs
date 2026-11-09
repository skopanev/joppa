//! The stored rows of the older release (9a3887e, the master before requirement:q438uw0s) for
//! the test of migration 062: documents, state rows and journal commands with links, rows of
//! object_links with blocking, and no relations_reset epoch. Also the row reads of the test.
#![allow(dead_code)]
use super::record_world::World;
use super::relations_world::link;
use anyhow::Result;
use serde_json::{Value, json};

/// One edge by the replay rule: relation key, target and pin.
pub type Edge = (&'static str, String, Option<u64>);

/// The tables of the rows of one workspace that a server start can write.
pub const ROWS: [&str; 8] = [
    "events",
    "object_revisions",
    "state_revisions",
    "state_entries",
    "objects",
    "object_links",
    "replay_epochs",
    "board_cache",
];

pub fn doc(n: u32) -> String {
    format!("doc:n{n:02}")
}

/// The old links of each linked node, and its relations by the replay rule as (key, to, pin).
pub fn fixture() -> Vec<(String, Value, Vec<Edge>)> {
    let mut out = vec![
        (
            doc(1),
            json!([
                link(&doc(2), "related", false, None),
                link(&doc(3), "depends_on", false, None),
                link(&doc(4), "related", false, None),
                link(&doc(5), "depends_on", true, Some(1))
            ]),
            vec![
                ("depends_on", doc(5), Some(1)),
                ("related", doc(2), None),
                ("related", doc(3), None),
                ("related", doc(4), None),
            ],
        ),
        // A related X and a non-blocking depends_on X merge into one related edge.
        (
            doc(6),
            json!([
                link(&doc(7), "related", false, Some(1)),
                link(&doc(7), "depends_on", false, None)
            ]),
            vec![("related", doc(7), Some(1))],
        ),
        (
            doc(8),
            json!([link(&doc(9), "depends_on", true, None)]),
            vec![("depends_on", doc(9), None)],
        ),
        (
            doc(10),
            json!([link(&doc(11), "depends_on", false, Some(1))]),
            vec![("related", doc(11), Some(1))],
        ),
    ];
    for n in 12..=20 {
        out.push((
            doc(n),
            json!([link(&doc(1), "related", false, None)]),
            vec![("related", doc(1), None)],
        ));
    }
    out
}

/// Relations as (key, to, pin) in key order and edge order. Keys without edges drop.
pub fn flat(relations: &Value) -> Vec<(String, String, Option<u64>)> {
    let mut out = Vec::new();
    for (key, entry) in relations.as_object().into_iter().flatten() {
        for e in entry["edges"].as_array().into_iter().flatten() {
            out.push((
                key.clone(),
                e["to"].as_str().unwrap_or("").to_owned(),
                e["revision"].as_u64(),
            ));
        }
    }
    out
}

pub fn wanted(address: &str) -> Vec<(String, String, Option<u64>)> {
    let found = fixture().into_iter().find(|(a, ..)| a == address);
    let edges = found.map(|f| f.2).unwrap_or_default();
    edges
        .into_iter()
        .map(|(k, t, r)| (k.to_owned(), t, r))
        .collect()
}

/// A document in the older form: links and no relations.
pub fn older(document: &mut Value, links: &Value) {
    document.as_object_mut().map(|m| m.remove("relations"));
    document["links"] = links.clone();
}

/// Rewrite the rows of W as the older release wrote them.
pub async fn store_older_form(w: &World) -> Result<()> {
    let links_of = |address: &str| {
        let found = fixture().into_iter().find(|(a, ..)| a == address);
        found.map_or(json!([]), |f| f.1)
    };
    let mut client = w.pool.get().await?;
    let conn = client.transaction().await?;
    // The journal is write-once. The test gives its commands the older form with the guard
    // off, in this transaction only.
    conn.batch_execute("ALTER TABLE joppa.events DISABLE TRIGGER events_worm")
        .await?;
    // The older table has the blocking column.
    let column = "ALTER TABLE joppa.object_links ADD COLUMN IF NOT EXISTS blocking boolean NOT NULL DEFAULT false";
    conn.batch_execute(column).await?;
    let sql =
        "SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1";
    for row in conn.query(sql, &[&w.ws]).await? {
        let (address, revision): (String, i32) = (row.get(0), row.get(1));
        let mut document: Value = serde_json::from_str(row.get(2))?;
        older(&mut document, &links_of(&address));
        let update = "UPDATE joppa.object_revisions SET document=$4::text::jsonb WHERE workspace=$1 AND address=$2 AND revision=$3";
        conn.execute(update, &[&w.ws, &address, &revision, &document.to_string()])
            .await?;
    }
    for row in conn
        .query(
            "SELECT address,document::text FROM joppa.objects WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
    {
        let address: String = row.get(0);
        let mut document: Value = serde_json::from_str(row.get(1))?;
        older(&mut document, &links_of(&address));
        let update =
            "UPDATE joppa.objects SET document=$3::text::jsonb WHERE workspace=$1 AND address=$2";
        conn.execute(update, &[&w.ws, &address, &document.to_string()])
            .await?;
    }
    let sql = "SELECT key,value::text,COALESCE(compact,value)::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects'";
    for row in conn.query(sql, &[&w.ws]).await? {
        let key: String = row.get(0);
        let mut both: Vec<Value> = vec![
            serde_json::from_str(row.get(1))?,
            serde_json::from_str(row.get(2))?,
        ];
        for value in &mut both {
            for revision in value["revisions"].as_array_mut().into_iter().flatten() {
                older(revision, &links_of(&key));
            }
        }
        let update = "UPDATE joppa.state_entries SET value=$3::text::jsonb,compact=$4::text::jsonb WHERE workspace=$1 AND section='objects' AND key=$2";
        conn.execute(
            update,
            &[&w.ws, &key, &both[0].to_string(), &both[1].to_string()],
        )
        .await?;
    }
    let sql = "SELECT position,command::text FROM joppa.events WHERE workspace=$1 AND command->>'op'='save_object'";
    for row in conn.query(sql, &[&w.ws]).await? {
        let position: i64 = row.get(0);
        let mut command: Value = serde_json::from_str(row.get(1))?;
        let address = format!(
            "{}:{}",
            command["kind"].as_str().unwrap_or(""),
            command["id"].as_str().unwrap_or("")
        );
        older(&mut command, &links_of(&address));
        let update =
            "UPDATE joppa.events SET command=$3::text::jsonb WHERE workspace=$1 AND position=$2";
        conn.execute(update, &[&w.ws, &position, &command.to_string()])
            .await?;
    }
    conn.execute(
        "DELETE FROM joppa.object_links WHERE workspace=$1",
        &[&w.ws],
    )
    .await?;
    for (source, links, _) in fixture() {
        for l in links.as_array().into_iter().flatten() {
            let insert = "INSERT INTO joppa.object_links(workspace,source,target,relation,revision,blocking) VALUES($1,$2,$3,$4,$5,$6)";
            let revision = l["revision"].as_i64().map(|r| r as i32);
            let row: (&str, &str) = (
                l["target"].as_str().unwrap_or(""),
                l["relation"].as_str().unwrap_or(""),
            );
            conn.execute(
                insert,
                &[
                    &w.ws,
                    &source,
                    &row.0,
                    &row.1,
                    &revision,
                    &(l["blocking"] == true),
                ],
            )
            .await?;
        }
    }
    let epoch = "DELETE FROM joppa.replay_epochs WHERE workspace=$1 AND rule='relations_reset'";
    conn.execute(epoch, &[&w.ws]).await?;
    conn.batch_execute("ALTER TABLE joppa.events ENABLE TRIGGER events_worm")
        .await?;
    conn.commit().await?;
    Ok(())
}

/// The row count and the xmin of each row of W in `table`.
pub async fn xmins(w: &World, table: &str) -> Result<Vec<String>> {
    let sql = format!("SELECT xmin::text FROM joppa.{table} WHERE workspace=$1 ORDER BY 1");
    let rows = w.pool.get().await?.query(&sql, &[&w.ws]).await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// The value and the xmin of each remote_links row of W.
pub async fn remote_rows(w: &World) -> Result<Vec<String>> {
    let sql = "SELECT value::text||' '||xmin::text FROM joppa.state_entries WHERE workspace=$1 AND section='remote_links' ORDER BY key";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

pub async fn snapshot(w: &World, tables: &[&str]) -> Result<Vec<Vec<String>>> {
    let mut out = Vec::new();
    for table in tables {
        out.push(xmins(w, table).await?);
    }
    Ok(out)
}
