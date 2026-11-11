//! The list read, part 2: the list query and the decoration of its rows.
use super::*;

// The objects linked to $8 in either direction, and $8 itself as parent.
const RELATED: &str = "o.parent=$8 OR EXISTS (SELECT 1 FROM joppa.object_links l WHERE l.workspace=o.workspace
                AND (l.relation IN ('depends_on','related') OR l.relation LIKE 'reference:%') AND ((l.source=o.address AND l.target IN (SELECT $8 UNION SELECT address FROM joppa.object_aliases WHERE workspace=o.workspace AND target=$8)) OR (l.target=o.address AND l.source=$8)))";
// The holders of the relation key $19 and the targets of its edges.
const MEMBER: &str = "o.document->'relations' ? $19
                OR EXISTS (SELECT 1 FROM joppa.object_links l JOIN joppa.objects s ON s.workspace=l.workspace AND s.address=l.source AND s.present
                    WHERE l.workspace=o.workspace AND l.relation=$19 AND l.target IN (SELECT o.address UNION SELECT address FROM joppa.object_aliases WHERE workspace=o.workspace AND target=o.address))";

// The body moved from read_objects unchanged, so it keeps `&*conn` on a borrowed transaction.
#[allow(clippy::too_many_arguments, clippy::borrow_deref_ref)]
pub(super) async fn list_rows(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: String,
    public: String,
    position: i64,
    prefix: String,
    selected: std::collections::BTreeMap<String, objects::TreeType>,
    scope: &ObjectScope<'_>,
    plan: super::object_list::ListPlan<'_>,
) -> Result<Value> {
    let super::object_list::ListPlan {
        related_to,
        limit,
        inbox,
        after_processed,
        after,
        bound,
        after_filter,
        order,
        in_relation,
    } = plan;
    let filter = bound.clause_at([3, 4, 11, 12, 13, 14, 15, 16, 17]);
    let f = bound.params();
    // A status list with the archived tone also keeps the current archived nodes.
    let archived = scope.filter.archived();
    // A member read ($19) keeps the holders of the key and the targets of its edges. The
    // document of a holder has the key in its relations, also a key with no edges. A list
    // without related_to or a member key reads no edge (tac:jogrtcra).
    let related = match related_to.is_some() {
        true => RELATED,
        false => "$8::text IS NULL",
    };
    let member = match in_relation.is_some() {
        true => MEMBER,
        false => "$19::text IS NULL",
    };
    let query = format!(
        r#"
        SELECT o.address,o.kind,o.local_id,o.revision,o.parent,o.title,o.owner,o.work_ready,
            (SELECT id FROM joppa.item_ids WHERE workspace=o.workspace AND kind=COALESCE(split_part(a.address,':',1),o.kind)
                AND local_id=COALESCE(split_part(a.address,':',2),o.local_id) AND namespace=$6),
            o.document_at,o.description,o.signal::text,o.present,o.inbox_status,o.process::text,
            CASE WHEN $10::boolean THEN {data}::text END,o.total,
            (SELECT count(*) FROM joppa.objects c WHERE c.workspace=o.workspace AND c.parent=o.address AND c.present),
            CASE WHEN $10::boolean THEN (o.document->'relations')::text END,
            o.document->'data'->>'assignee'
        FROM (SELECT o.*,count(*) OVER() AS total FROM joppa.objects o
        WHERE o.workspace=$1 AND (o.present OR $18)
            AND {filter}
            AND ($7::text IS NULL OR o.inbox_status=$7)
            AND ({related})
            AND ({member})) o
        LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.target=o.address
        WHERE {after_filter}
        ORDER BY {order} LIMIT $5
    "#,
        data = crate::computed_fields::DATA_SQL
    );
    let rows = conn
        .query(
            &query,
            &[
                &workspace,
                &after,
                f[0],
                f[1],
                &((limit + 1) as i64),
                &prefix,
                &scope.note_status,
                &related_to,
                &after_processed,
                &scope.data,
                f[2],
                f[3],
                f[4],
                f[5],
                f[6],
                f[7],
                f[8],
                &archived,
                &in_relation,
            ],
        )
        .await?;
    // `total` counts every matching object, also the objects before the cursor.
    let total: i64 = rows.first().map_or(0, |r| r.get(16));
    let more = rows.len() > limit;
    let mut items:Vec<Value>=rows.iter().take(limit).map(|r|json!({"address":r.get::<_,String>(0),"kind":r.get::<_,String>(1),"id":r.get::<_,String>(2),"revision":r.get::<_,i32>(3),"parent":r.get::<_,Option<String>>(4),"title":r.get::<_,String>(5),"owner":r.get::<_,Option<String>>(6),"work_ready":r.get::<_,bool>(7),"item_id":r.get::<_,Option<String>>(8),"at":r.get::<_,Option<String>>(9),"body":r.get::<_,Option<String>>(10),"signal":serde_json::from_str::<Value>(r.get(11)).unwrap_or_else(|_|json!({})),"present":r.get::<_,bool>(12),"note_status":if selected.get(&r.get::<_,String>(1)).is_some_and(|t| t.inbox.unwrap_or(false)) {json!(r.get::<_,String>(13))}else{Value::Null}})).collect();
    for (item, row) in items.iter_mut().zip(rows.iter()) {
        item["process"] = serde_json::from_str::<Value>(row.get(14))?;
        if let Some(data) = row.get::<_, Option<String>>(15) {
            item["data"] = serde_json::from_str::<Value>(&data)?;
        }
        if !item["note_status"].is_null() {
            item["status"] = item["note_status"].clone();
        }
        item["children"] = json!(row.get::<_, i64>(17));
        // Who holds the work: a member ID or an agent alias.
        if let Some(assignee) = row.get::<_, Option<String>>(19).filter(|a| !a.is_empty()) {
            item["assignee"] = json!(assignee);
        }
        // A read with data also gives the stored relations: {key: {title?, edges}}.
        if let Some(relations) = row.get::<_, Option<String>>(18) {
            item["relations"] = serde_json::from_str::<Value>(&relations)?;
        }
    }
    // The decoration comes from the status projection of the last write.
    apply_rollups(&*conn, &workspace, &mut items).await?;
    // A dimension value of the data filter gives each row its tone for that combination.
    if let Some(key) = &bound.combination {
        let set: fn(&mut Value, &str) = |item, tone| item["signal"]["tone"] = json!(tone);
        super::dimensions::read_tones(&*conn, &workspace, key, &mut items, set).await?;
    }
    let next = more.then(|| {
        let last = items.last().unwrap();
        let address = last["address"].as_str().unwrap();
        if inbox {
            format!(
                "{position}:{}:{address}",
                u8::from(last["note_status"] == "processed")
            )
        } else {
            format!("{position}:{address}")
        }
    });
    Ok(
        json!({"workspace":public,"position":position,"total":total,"items":items,"next_cursor":next}),
    )
}
