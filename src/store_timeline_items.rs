//! The items of journal rows: one entry for each event, in the form of the timeline read.
//! The timeline and the History read use it inside their own read transaction.
use super::*;

/// Build one item for each row. Each row holds position, recorded_at::text, actor and
/// command::text in its first four columns.
pub(super) async fn items(
    conn: &impl GenericClient,
    workspace: &str,
    rows: &[tokio_postgres::Row],
) -> Result<Vec<Value>> {
    let commands: Vec<Value> = rows
        .iter()
        .map(|row| serde_json::from_str(row.get(3)))
        .collect::<std::result::Result<_, _>>()?;
    let positions: Vec<i64> = rows.iter().map(|row| row.get(0)).collect();
    let addresses: Vec<Option<String>> = commands.iter().map(crate::timeline::address).collect();
    // The object, its revision at the event and the revision before that one.
    let objects = conn.query(
        "SELECT x.i, o.address, o.kind, o.title, r.revision, r.document::text, p.document::text
         FROM unnest($2::text[], $3::bigint[]) WITH ORDINALITY AS x(address, position, i)
         LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=x.address
         JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target, x.address)
         LEFT JOIN LATERAL (SELECT revision, document FROM joppa.object_revisions
             WHERE workspace=$1 AND address=o.address AND (document->>'position')::bigint<=x.position
             ORDER BY revision DESC LIMIT 1) r ON true
         LEFT JOIN joppa.object_revisions p ON p.workspace=$1 AND p.address=o.address AND p.revision=r.revision-1",
        &[&workspace, &addresses, &positions],
    ).await?;
    let mut found = std::collections::BTreeMap::new();
    for row in &objects {
        let document = |i| {
            row.get::<_, Option<&str>>(i)
                .map(serde_json::from_str::<Value>)
                .transpose()
        };
        found.insert(
            row.get::<_, i64>(0) as usize - 1,
            (
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                row.get::<_, String>(3),
                row.get::<_, Option<i32>>(4),
                document(5)?,
                document(6)?,
            ),
        );
    }
    // The journal keeps redacted fragments; the timeline masks them in every text it returns.
    let fragments: Vec<String> = conn
        .query(
            "SELECT command->>'fragment' FROM joppa.events WHERE workspace=$1 AND command->>'op'='redact_object' AND command->>'fragment'<>''",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let mask = |text: &str| {
        fragments.iter().fold(text.to_owned(), |text, fragment| {
            text.replace(fragment, crate::redact::MASK)
        })
    };
    let actors: Vec<String> = rows
        .iter()
        .map(|row| row.get::<_, String>(2))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let types = access::actor_types(conn, workspace, &actors).await?;
    let mut items = vec![];
    for (i, (row, command)) in rows.iter().zip(&commands).enumerate() {
        let position: i64 = row.get(0);
        let actor: String = row.get(2);
        let mut change = crate::timeline::change(command);
        let mut reason = crate::timeline::reason(command);
        let mut item = json!({
            "position": position,
            "at": row.get::<_, String>(1),
            "actor": actor,
            "actor_type": types[&actor],
            "op": command["op"],
            "address": addresses[i],
            "kind": null,
            "title": null,
            "revision": null,
        });
        if command["op"] == "record_report" {
            let commit = command["commit"].as_str().unwrap_or_default();
            item["title"] = json!(format!("Test results {}", &commit[..commit.len().min(12)]));
        }
        if let Some((address, kind, title, revision, saved, previous)) = found.get(&i) {
            item["address"] = json!(address);
            item["kind"] = json!(kind);
            item["revision"] = json!(revision);
            item["title"] = json!(
                saved
                    .as_ref()
                    .and_then(|d| d["data"]["title"].as_str())
                    .unwrap_or(title)
            );
            // A revision that this event made gives the reason in its stored, masked form.
            if let Some(saved) = saved.as_ref().filter(|d| d["position"] == position) {
                if let Some(text) = saved["reason"].as_str() {
                    reason = text.to_owned();
                }
                if command["op"] == "save_object"
                    && previous
                        .as_ref()
                        .is_some_and(|p| crate::timeline::moved(saved, p))
                {
                    change = "move";
                }
            }
        }
        item["change"] = json!(change);
        item["reason"] = json!(mask(&reason));
        if let Some(title) = item["title"].as_str() {
            item["title"] = json!(mask(title));
        }
        items.push(item);
    }
    Ok(items)
}
