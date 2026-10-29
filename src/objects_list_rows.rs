//! The list read, part 2: the list query and the decoration of its rows.
use super::*;

// The body moved from read_objects unchanged, so it keeps `&*conn` on a borrowed transaction.
#[allow(clippy::too_many_arguments, clippy::borrow_deref_ref)]
pub(super) async fn list_rows(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: String,
    public: String,
    position: i64,
    prefix: String,
    selected: std::collections::BTreeMap<String, objects::TreeType>,
    task_kinds: Vec<String>,
    criterion_kinds: Vec<String>,
    specification_kinds: Vec<String>,
    unstarted: bool,
    scope: &ObjectScope<'_>,
    plan: super::object_list::ListPlan<'_>,
) -> Result<Value> {
    let super::object_list::ListPlan {
        related_to,
        limit,
        inbox,
        after_processed,
        after,
        parent,
        after_filter,
        order,
        values,
        green,
    } = plan;
    let query = format!(
        r#"
        WITH RECURSIVE task_parents(task,address,status,verification,blocked) AS (
            SELECT t.address,COALESCE(a.target,t.parent),COALESCE(t.process->>'status','open'),COALESCE(t.process->>'verification','unverified'),
                COALESCE(jsonb_typeof(t.process->'blocked'),'null')='object'
            FROM joppa.objects t
            LEFT JOIN joppa.object_aliases a ON a.workspace=t.workspace AND a.address=t.parent
            WHERE $9::boolean AND t.workspace=$1 AND t.present AND t.parent IS NOT NULL
                AND t.kind=ANY($10::text[])
            UNION
            SELECT t.task,COALESCE(a.target,p.parent),t.status,t.verification,t.blocked
            FROM task_parents t JOIN joppa.objects p ON p.workspace=$1 AND p.address=t.address
            LEFT JOIN joppa.object_aliases a ON a.workspace=p.workspace AND a.address=p.parent
            WHERE p.parent IS NOT NULL AND p.kind<>ALL($11::text[]) AND p.kind<>ALL($12::text[])
        ), work_counts AS (
            SELECT address,COUNT(*) FILTER (WHERE status NOT IN ('done','retired') AND NOT blocked) AS active,
                COUNT(*) FILTER (WHERE status='done') AS done,COUNT(*) FILTER (WHERE status='retired') AS retired
            FROM task_parents GROUP BY address
        )
        SELECT o.address,o.kind,o.local_id,o.revision,o.parent,o.title,o.owner,o.review_required,o.work_ready,
            (SELECT id FROM joppa.item_ids WHERE workspace=o.workspace AND kind=COALESCE(split_part(a.address,':',1),o.kind)
                AND local_id=COALESCE(split_part(a.address,':',2),o.local_id) AND namespace=$6),
            o.document_at,left(o.description,300),o.signal::text,o.present,o.inbox_status,
            jsonb_build_object('active',COALESCE(w.active,0),'done',COALESCE(w.done,0),'retired',COALESCE(w.retired,0))::text,o.process::text,
            CASE WHEN (o.document->'contract'->>0)::int=o.revision THEN (o.document->'contract'->>1)::int ELSE o.revision END,
            CASE WHEN $16::boolean THEN (o.document->'data')::text END
        FROM joppa.objects o
        LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.target=o.address
        LEFT JOIN work_counts w ON w.address=o.address
        WHERE o.workspace=$1 AND o.present
            AND {after_filter} AND ($3::text IS NULL OR o.kind=$3)
            AND (($4::text IS NULL AND ($9::boolean OR $8::text IS NOT NULL OR o.parent IS NULL)) OR o.parent=$4 OR $4='*')
            AND ($7::text IS NULL OR o.inbox_status=$7)
            AND ($8::text IS NULL OR o.parent=$8 OR EXISTS (SELECT 1 FROM joppa.object_links l WHERE l.workspace=o.workspace
                AND ((l.source=o.address AND l.target IN (SELECT $8 UNION SELECT address FROM joppa.object_aliases WHERE workspace=o.workspace AND target=$8)) OR (l.target=o.address AND l.source=$8))))
            AND (NOT $9::boolean OR (o.present AND o.kind=ANY($11::text[])
                AND o.process->'assessment'->>'state'='ready' AND o.work_ready AND NOT o.review_required
                AND COALESCE(jsonb_typeof(o.process->'readiness_blocker'),'null')<>'object'
                AND COALESCE(o.process->>'verification','unverified')<>'pass'
                AND NOT o.address=ANY($14::text[])
                AND COALESCE(w.active,0)=0
                AND COALESCE(jsonb_typeof(o.process->'planning_claim'),'null')<>'object'
                AND NOT COALESCE(o.rollup ? 'waits_for',false)))
        ORDER BY {order} LIMIT $5
    "#
    );
    let rows = conn
        .query(
            &query,
            &[
                &workspace,
                &after,
                &scope.kind,
                &parent,
                &if unstarted {
                    i64::MAX
                } else {
                    (limit + 1) as i64
                },
                &prefix,
                &scope.note_status,
                &related_to,
                &unstarted,
                &task_kinds,
                &criterion_kinds,
                &specification_kinds,
                &after_processed,
                &green,
                &values,
                &scope.data,
            ],
        )
        .await?;
    let rows = if unstarted {
        let start = if after.is_empty() {
            0
        } else {
            rows.iter()
                .position(|r| r.get::<_, String>(0) == after)
                .context("object list changed; start again without cursor")?
                + 1
        };
        rows.into_iter().skip(start).take(limit + 1).collect()
    } else {
        rows
    };
    let more = rows.len() > limit;
    let mut items:Vec<Value>=rows.iter().take(limit).map(|r|json!({"address":r.get::<_,String>(0),"kind":r.get::<_,String>(1),"id":r.get::<_,String>(2),"revision":r.get::<_,i32>(3),"contract_revision":r.get::<_,i32>(17),"parent":r.get::<_,Option<String>>(4),"title":r.get::<_,String>(5),"owner":r.get::<_,Option<String>>(6),"review_required":r.get::<_,bool>(7),"work_ready":r.get::<_,bool>(8),"item_id":r.get::<_,Option<String>>(9),"at":r.get::<_,Option<String>>(10),"description":r.get::<_,Option<String>>(11),"signal":serde_json::from_str::<Value>(r.get(12)).unwrap_or_else(|_|json!({})),"present":r.get::<_,bool>(13),"note_status":if selected.get(&r.get::<_,String>(1)).is_some_and(|t| t.inbox.unwrap_or(false)) {json!(r.get::<_,String>(14))}else{Value::Null}})).collect();
    for (item, row) in items.iter_mut().zip(rows.iter()) {
        if criterion_kinds
            .iter()
            .chain(&task_kinds)
            .any(|kind| item["kind"] == kind.as_str())
        {
            item["process"] = serde_json::from_str::<Value>(row.get(16))?;
        }
        if let Some(data) = row.get::<_, Option<String>>(18) {
            item["data"] = serde_json::from_str(&data)?;
        }
        if !item["note_status"].is_null() {
            item["status"] = item["note_status"].clone();
        }
    }
    if unstarted {
        for (item, row) in items.iter_mut().zip(rows.iter()) {
            item["work_summary"] = serde_json::from_str(row.get(15))?;
        }
    }
    // The decoration comes from the status projection of the last write.
    apply_rollups(&*conn, &workspace, &mut items).await?;
    add_priorities(&*conn, &workspace, &mut items).await?;
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
    Ok(json!({"workspace":public,"position":position,"items":items,"next_cursor":next}))
}
