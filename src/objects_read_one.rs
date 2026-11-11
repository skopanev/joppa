//! The read of one object by its address. Its references give the own and the computed
//! reference values with the titles of their objects.
use super::*;

/// One object by its address, at its current or an older revision.
// The body moved from read_objects unchanged, so it keeps `&*conn` on a borrowed transaction.
#[allow(clippy::too_many_arguments, clippy::borrow_deref_ref)]
pub(super) async fn read_one(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: String,
    public: String,
    position: i64,
    prefix: String,
    scope: &ObjectScope<'_>,
    key: &str,
) -> Result<Value> {
    let key = conn
        .query_opt(
            "SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2",
            &[&workspace, &key],
        )
        .await?
        .map(|row| row.get::<_, String>(0))
        .unwrap_or_else(|| key.to_owned());
    ensure!(
        scope.filter == crate::node_filter::NodeFilter::default()
            && scope.cursor.is_none()
            && scope.note_status.is_none()
            && scope.related_to.is_none(),
        "address cannot be combined with list filters"
    );
    let row=conn.query_opt(&format!("SELECT o.kind,o.local_id,o.revision,jsonb_set(o.document,'{{data}}',{data})::text,o.work_ready,(SELECT id FROM joppa.item_ids WHERE workspace=o.workspace AND kind=COALESCE(split_part(a.address,':',1),o.kind) AND local_id=COALESCE(split_part(a.address,':',2),o.local_id) AND namespace=$3),o.signal::text,o.process::text FROM joppa.objects o LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.target=o.address WHERE o.workspace=$1 AND o.address=$2", data = crate::computed_fields::DATA_SQL), &[&workspace,&key,&prefix]).await?.context("unknown object")?;
    let current: i32 = row.get(2);
    if scope.history {
        ensure!(
            scope.revision.is_none(),
            "history gives the revision headers; read one revision separately"
        );
        // The revision headers: number, time, actor and reason, without the bodies.
        let revisions: Vec<Value> = conn
            .query("SELECT revision,document->>'at',document->>'actor',document->>'reason',document->>'alias' FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 ORDER BY revision", &[&workspace, &key])
            .await?
            .iter()
            .map(|r| {
                let mut header = json!({"revision": r.get::<_, i32>(0), "at": r.get::<_, Option<String>>(1),
                    "actor": r.get::<_, Option<String>>(2), "reason": r.get::<_, Option<String>>(3)});
                crate::actor_alias::put(&mut header, r.get::<_, Option<&str>>(4));
                header
            })
            .collect();
        return Ok(
            json!({"workspace":public,"address":key,"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"current_revision":current,"position":position,"revisions":revisions}),
        );
    }
    let n = scope.revision.unwrap_or(current as u32);
    ensure!(n > 0, "object revision must be positive");
    let document: Value = if n == current as u32 {
        serde_json::from_str(row.get(3))?
    } else {
        serde_json::from_str(conn.query_opt("SELECT document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 AND revision=$3", &[&workspace,&key,&(n as i32)]).await?.context("unknown object revision")?.get(0))?
    };
    // A revision stored before migration 062 reads with relations by the replay rule.
    let document = crate::relations::adapt_document(document)?;
    let mut relations = document["relations"].clone();
    let targets: Vec<String> = relations
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .flat_map(|r| r["edges"].as_array().into_iter().flatten())
        .filter_map(|e| e["to"].as_str().map(str::to_owned))
        .collect();
    let found:std::collections::BTreeMap<String,Value>=conn.query("SELECT t.address,o.title,o.kind,o.revision,o.present FROM unnest($2::text[]) t(address) LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=t.address JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target,t.address)",&[&workspace,&targets]).await?.iter().map(|r|(r.get(0),json!({"title":r.get::<_,String>(1),"current_revision":r.get::<_,i32>(3),"present":r.get::<_,bool>(4)}))).collect();
    // Each edge with the title, the current revision and the presence of its target.
    for relation in relations
        .as_object_mut()
        .into_iter()
        .flat_map(|m| m.values_mut())
    {
        for edge in relation["edges"].as_array_mut().into_iter().flatten() {
            let target = found.get(edge["to"].as_str().unwrap_or(""));
            for (key, value) in target.and_then(Value::as_object).into_iter().flatten() {
                edge[key] = value.clone();
            }
            if target.is_none() {
                edge["present"] = json!(false);
            }
        }
    }
    let mut references: Vec<Value> = if n == current as u32 {
        conn.query("SELECT l.target,l.relation,o.title,o.revision,o.present FROM joppa.object_links l JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.target WHERE l.workspace=$1 AND l.source=$2 AND l.relation LIKE 'reference:%' ORDER BY l.relation,l.target", &[&workspace,&key]).await?.iter().map(|r| json!({"target":r.get::<_,String>(0),"field":r.get::<_,String>(1).strip_prefix("reference:").unwrap(),"title":r.get::<_,String>(2),"current_revision":r.get::<_,i32>(3),"present":r.get::<_,bool>(4)})).collect()
    } else {
        vec![]
    };
    if n == current as u32 {
        let kind = row.get::<_, String>(0);
        references.extend(computed_references(conn, &workspace, &kind, &document).await?);
        references.sort_by(|a, b| {
            (a["field"].as_str(), a["target"].as_str())
                .cmp(&(b["field"].as_str(), b["target"].as_str()))
        });
    }
    // The edges come as counts by group; relations_of gives them in pages.
    let relation_groups = if n == current as u32 {
        super::link_groups::groups(&*conn, &workspace, &key).await?
    } else {
        json!([])
    };
    let projected_process: Value = serde_json::from_str(row.get(7))?;
    let annotation = projected_process["annotation_address"]
        .as_str()
        .unwrap_or(&key);
    let mut annotations:Value=serde_json::from_str(conn.query_one("SELECT COALESCE((SELECT value FROM joppa.state_entries WHERE workspace=$1 AND section='notes' AND key=$2),'{}'::jsonb)::text", &[&workspace,&annotation]).await?.get(0))?;
    if let Some(comments) = annotations["comments"].as_object_mut() {
        for comment in comments.values_mut() {
            comment["author"] = comment["actor"].clone();
            comment["created_at"] = comment["at"].clone();
        }
    }
    let mut item = json!({"workspace":public,"position":position,"address":key,"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"item_id":row.get::<_,Option<String>>(5),"current_revision":current,"revision_count":current,"document":document,"historical":n != current as u32,"work_ready":if n == current as u32 {json!(row.get::<_,bool>(4))}else{Value::Null},"relations":relations,"references":references,"relation_groups":relation_groups,"annotations":if n == current as u32 {annotations}else{json!({})},"metadata_scope":"current","signal":if n == current as u32 {serde_json::from_str::<Value>(row.get(6))?}else{Value::Null},"process":if n == current as u32 {serde_json::from_str::<Value>(row.get(7))?}else{Value::Null}});
    if n == current as u32 {
        // Each file has a presigned GET. An object without files needs no file storage.
        let mut files = crate::file_list::present(&item["annotations"]["attachments"]);
        crate::file_list::sign(&workspace, &mut files)?;
        item["files"] = json!(files);
        // The decoration comes from the status projection of the last write.
        apply_rollups(&*conn, &workspace, std::slice::from_mut(&mut item)).await?;
        // A test node gives the deciding result of its test: status, commit and message.
        let kind = item["kind"].as_str().unwrap_or_default().to_owned();
        let role: Option<String> = conn.query_opt("SELECT value->'types'->$2->>'role' FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1", &[&workspace, &kind]).await?.and_then(|r| r.get(0));
        if role.as_deref() == Some(crate::test_nodes::ROLE) {
            let mut stored =
                stored_statuses(&*conn, &workspace, std::slice::from_ref(&key)).await?;
            if let Some(result) = stored.remove(&key).and_then(|s| s["tests"].get(0).cloned()) {
                item["result"] = result;
            }
        }
    }
    Ok(item)
}

/// The items of the references of `kind` with `from` children: each address of the computed
/// value in `document` with the title of its object, as the own references give them.
async fn computed_references(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    kind: &str,
    document: &Value,
) -> Result<Vec<Value>> {
    let sql = "SELECT (value->'types'->$2->'references')::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1";
    let rules = conn.query_opt(sql, &[&workspace, &kind]).await?;
    let rules: Option<String> = rules.and_then(|r| r.get(0));
    let rules: Value = serde_json::from_str(rules.as_deref().unwrap_or("null"))?;
    let mut wanted: Vec<(String, String)> = vec![];
    for (field, _) in rules
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, r)| r["from"].is_string())
    {
        for target in document["data"][field].as_array().into_iter().flatten() {
            wanted.extend(target.as_str().map(|t| (field.clone(), t.to_owned())));
        }
    }
    if wanted.is_empty() {
        return Ok(vec![]);
    }
    let targets: Vec<&String> = wanted.iter().map(|(_, t)| t).collect();
    let sql = "SELECT address,title,revision,present FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)";
    let found: std::collections::BTreeMap<String, Value> = conn.query(sql, &[&workspace, &targets]).await?.iter().map(|r| (r.get(0), json!({"title":r.get::<_,String>(1),"current_revision":r.get::<_,i32>(2),"present":r.get::<_,bool>(3)}))).collect();
    let items = wanted.into_iter().filter_map(|(field, target)| {
        let mut item = found.get(&target)?.clone();
        item["target"] = json!(target);
        item["field"] = json!(field);
        Some(item)
    });
    Ok(items.collect())
}
