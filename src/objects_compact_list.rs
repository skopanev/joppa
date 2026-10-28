//! The lean list of requirement:read-contract: the current objects under one node, or the
//! roots, or every node with parent "*", in address order. Each row gives only the address,
//! the title, the status tone and the number of present children, with no item ID, process
//! or priority lookup. `total` counts every matching object. The tone takes the stored rollup,
//! as in every other read, so a status rule (tests, children, links) shows here too.
use super::*;

pub async fn compact_list(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    parent: Option<&str>,
    kind: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Value> {
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    let rows = conn
        .query(
            "SELECT x.address,x.title,x.signal::text,x.total,
                (SELECT count(*) FROM joppa.objects c WHERE c.workspace=$1 AND c.parent=x.address AND c.present)
            FROM (SELECT o.address,o.title,o.signal,count(*) OVER() AS total FROM joppa.objects o
                WHERE o.workspace=$1 AND o.present AND ($2::text IS NULL OR o.kind=$2)
                    AND (($3::text IS NULL AND o.parent IS NULL) OR $3='*'
                        OR o.parent=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$3),$3))) x
            WHERE x.address>$4 ORDER BY x.address LIMIT $5",
            &[&workspace, &kind, &parent, &cursor.unwrap_or(""), &((limit + 1) as i64)],
        )
        .await?;
    let total: i64 = rows.first().map_or(0, |r| r.get(3));
    let mut rolled: Vec<Value> = rows
        .iter()
        .take(limit)
        .map(|r| {
            let signal = r
                .get::<_, Option<String>>(2)
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or_else(|| json!({}));
            json!({"address": r.get::<_, String>(0), "signal": signal})
        })
        .collect();
    apply_rollups(&**conn, &workspace, &mut rolled).await?;
    let mut items: Vec<Value> = rows
        .iter()
        .zip(&rolled)
        .map(|(r, rolled)| {
            let mut item =
                json!({"address": r.get::<_, String>(0), "title": r.get::<_, String>(1)});
            if let Some(tone) = rolled["signal"]["tone"].as_str().filter(|t| !t.is_empty()) {
                item["status"] = json!(tone);
            }
            let children: i64 = r.get(4);
            if children > 0 {
                item["children"] = json!(children);
            }
            item
        })
        .collect();
    let mut reply = json!({"total": total});
    if rows.len() > limit {
        reply["next_cursor"] = items.last().map_or(Value::Null, |i| i["address"].clone());
    }
    reply["items"] = Value::Array(std::mem::take(&mut items));
    Ok(reply)
}
