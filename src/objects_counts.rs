//! The aggregate counts of a list read (src/node_counts.rs). One statement counts the nodes
//! that the node filters keep, with the read rights and the archived rule of the list: the
//! present nodes, and the current archived nodes too when `status` names the archived tone.
//! Without a filter, the list and the counts keep the roots.
use super::*;
use crate::node_counts::{Aggregate, Key};

pub async fn count_objects(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    filter: &crate::node_filter::NodeFilter,
    aggregate: &Aggregate,
) -> Result<Value> {
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let archived = filter.archived();
    let bound = crate::node_filter::bind(&*conn, &workspace, filter, true, archived).await?;
    // $1 is the workspace, $2 the archived flag, $3 to $10 the clause, $11 on the data fields.
    let keys: &[Key] = match aggregate {
        Aggregate::Count => &[],
        Aggregate::GroupBy(keys) => keys,
    };
    let mut fields: Vec<&str> = vec![];
    let values: Vec<String> = keys
        .iter()
        .map(|key| match key {
            Key::Kind => "to_jsonb(o.kind)".to_owned(),
            Key::Status => format!("to_jsonb({})", tone(&bound)),
            Key::Owner => "to_jsonb(o.owner)".to_owned(),
            // An empty assignee is no assignee, as in the assignee filter.
            Key::Assignee => format!("to_jsonb(NULLIF({},''))", crate::node_filter::ASSIGNEE),
            // A JSON null is a missing value.
            Key::Data { field, .. } => {
                fields.push(field);
                format!(
                    "NULLIF({}->${}::text,'null'::jsonb)",
                    crate::computed_fields::DATA_SQL,
                    10 + fields.len()
                )
            }
        })
        .collect();
    let rows = format!(
        "FROM joppa.objects o WHERE o.workspace=$1 AND (o.present OR $2) AND {}",
        bound.clause(3)
    );
    let query = if values.is_empty() {
        format!("SELECT count(*) {rows}")
    } else {
        let n = values.len();
        let named: Vec<String> = (0..n).map(|i| format!("{} AS k{i}", values[i])).collect();
        let read: Vec<String> = (0..n).map(|i| format!("k{i}::text")).collect();
        let order: Vec<String> = (0..n).map(|i| format!("k{i} ASC NULLS LAST")).collect();
        let by: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
        format!(
            "SELECT {},n FROM (SELECT {},count(*) AS n {rows} GROUP BY {}) g ORDER BY {}",
            read.join(","),
            named.join(","),
            by.join(","),
            order.join(",")
        )
    };
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![&workspace, &archived];
    params.extend(bound.params());
    params.extend(
        fields
            .iter()
            .map(|f| f as &(dyn tokio_postgres::types::ToSql + Sync)),
    );
    let found = conn.query(&query, &params).await?;
    // The read-only transaction ends here, so no ROLLBACK runs later on drop.
    conn.commit().await?;
    if keys.is_empty() {
        let total: i64 = found.first().map_or(0, |r| r.get(0));
        return Ok(json!({"total": total}));
    }
    let mut total = 0;
    let mut groups = vec![];
    for row in &found {
        let mut group = serde_json::Map::new();
        for (i, key) in keys.iter().enumerate() {
            let value = row.get::<_, Option<&str>>(i).map(serde_json::from_str);
            group.insert(
                key.sent().to_owned(),
                value.transpose()?.unwrap_or(Value::Null),
            );
        }
        let count: i64 = row.get(keys.len());
        total += count;
        group.insert("count".into(), json!(count));
        groups.push(Value::Object(group));
    }
    Ok(json!({"total": total, "groups": groups}))
}

/// The status tone of a node, as the list gives it: the tone of the combination that the
/// data filter names, else the stored tone.
fn tone(bound: &crate::node_filter::Bound) -> String {
    let tone = crate::node_filter::TONE;
    match bound.combination {
        // $10 is the data parameter of the clause: the envelope with the combination key.
        Some(_) => format!("COALESCE(o.dim_tones->>($10::text::jsonb->>'key'),{tone})"),
        None => tone.to_owned(),
    }
}
