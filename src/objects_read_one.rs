//! The read of one object by its address.
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
        scope.parent.is_none()
            && scope.kind.is_none()
            && scope.cursor.is_none()
            && scope.note_status.is_none()
            && scope.related_to.is_none()
            && scope.work_filter.is_none(),
        "address cannot be combined with list filters"
    );
    let row=conn.query_opt("SELECT o.kind,o.local_id,o.revision,o.document::text,o.review_required,o.work_ready,(SELECT id FROM joppa.item_ids WHERE workspace=o.workspace AND kind=COALESCE(split_part(a.address,':',1),o.kind) AND local_id=COALESCE(split_part(a.address,':',2),o.local_id) AND namespace=$3),o.signal::text,o.process::text FROM joppa.objects o LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.target=o.address WHERE o.workspace=$1 AND o.address=$2", &[&workspace,&key,&prefix]).await?.context("unknown object")?;
    let current: i32 = row.get(2);
    if scope.history {
        ensure!(
            scope.revision.is_none(),
            "history gives the revision headers; read one revision separately"
        );
        // The revision headers: number, time, actor and reason, without the bodies.
        let revisions: Vec<Value> = conn
            .query("SELECT revision,document->>'at',document->>'actor',document->>'reason' FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 ORDER BY revision", &[&workspace, &key])
            .await?
            .iter()
            .map(|r| json!({"revision": r.get::<_, i32>(0), "at": r.get::<_, Option<String>>(1),
                "actor": r.get::<_, Option<String>>(2), "reason": r.get::<_, Option<String>>(3)}))
            .collect();
        return Ok(
            json!({"workspace":public,"address":key,"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"current_revision":current,"position":position,"revisions":revisions}),
        );
    }
    let n = scope.revision.unwrap_or(current as u32);
    ensure!(n > 0, "object revision must be positive");
    let mut document: Value = if n == current as u32 {
        serde_json::from_str(row.get(3))?
    } else {
        serde_json::from_str(conn.query_opt("SELECT document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 AND revision=$3", &[&workspace,&key,&(n as i32)]).await?.context("unknown object revision")?.get(0))?
    };
    let targets: Vec<String> = document["links"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["target"].as_str().map(str::to_owned))
        .collect();
    let found:std::collections::BTreeMap<String,Value>=conn.query("SELECT t.address,o.title,o.kind,o.revision,o.present FROM unnest($2::text[]) t(address) LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=t.address JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target,t.address)",&[&workspace,&targets]).await?.iter().map(|r|(r.get(0),json!({"title":r.get::<_,String>(1),"kind":r.get::<_,String>(2),"current_revision":r.get::<_,i32>(3),"present":r.get::<_,bool>(4)}))).collect();
    let outgoing: Vec<Value> = document["links"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|link| {
            let mut item = found
                .get(link["target"].as_str().unwrap_or(""))
                .cloned()
                .unwrap_or_else(|| json!({}));
            item["link"] = link.clone();
            item["blocking"] = json!(link["blocking"].as_bool().unwrap_or(false));
            // An ordering dependency stores no revision field, so its link item shows none.
            if let Some(revision) = link.get("revision") {
                item["revision"] = revision.clone();
            }
            item["metadata_scope"] = json!("current");
            item
        })
        .collect();
    let references: Vec<Value> = if n == current as u32 {
        conn.query("SELECT l.target,l.relation,o.title,o.revision,o.present FROM joppa.object_links l JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.target WHERE l.workspace=$1 AND l.source=$2 AND l.relation LIKE 'reference:%' ORDER BY l.relation,l.target", &[&workspace,&key]).await?.iter().map(|r| json!({"target":r.get::<_,String>(0),"field":r.get::<_,String>(1).strip_prefix("reference:").unwrap(),"title":r.get::<_,String>(2),"current_revision":r.get::<_,i32>(3),"present":r.get::<_,bool>(4)})).collect()
    } else {
        vec![]
    };
    // The incoming links come as counts by group; a group read gives them in pages.
    let link_groups = if n == current as u32 {
        super::link_groups::groups(&*conn, &workspace, &key).await?
    } else {
        json!([])
    };
    let projected_process: Value = serde_json::from_str(row.get(8))?;
    let annotation = projected_process["annotation_address"]
        .as_str()
        .unwrap_or(&key);
    let mut annotations:Value=serde_json::from_str(conn.query_one("SELECT COALESCE((SELECT value FROM joppa.state_entries WHERE workspace=$1 AND section='notes' AND key=$2),'{}'::jsonb)::text", &[&workspace,&annotation]).await?.get(0))?;
    let mut authors = std::collections::BTreeSet::new();
    if let Some(actor) = document["actor"].as_str() {
        authors.insert(actor.to_owned());
    }
    if n == current as u32
        && let Some(comments) = annotations["comments"].as_object()
    {
        authors.extend(
            comments
                .values()
                .filter_map(|comment| comment["actor"].as_str().map(str::to_owned)),
        );
    }
    let types =
        access::actor_types(&*conn, &workspace, &authors.into_iter().collect::<Vec<_>>()).await?;
    if let Some(actor) = document["actor"].as_str() {
        document["actor_type"] = json!(types[actor]);
    }
    if let Some(comments) = annotations["comments"].as_object_mut() {
        for comment in comments.values_mut() {
            comment["author"] = comment["actor"].clone();
            comment["created_at"] = comment["at"].clone();
            if n == current as u32
                && let Some(actor) = comment["actor"].as_str()
            {
                comment["actor_type"] = json!(types[actor]);
            }
        }
    }
    let mut item = json!({"workspace":public,"position":position,"address":key,"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"item_id":row.get::<_,Option<String>>(6),"current_revision":current,"revision_count":current,"document":document,"historical":n != current as u32,"review_required":if n == current as u32 {json!(row.get::<_,bool>(4))}else{Value::Null},"work_ready":if n == current as u32 {json!(row.get::<_,bool>(5))}else{Value::Null},"links":outgoing,"references":references,"link_groups":link_groups,"annotations":if n == current as u32 {annotations}else{json!({})},"metadata_scope":"current","signal":if n == current as u32 {serde_json::from_str::<Value>(row.get(7))?}else{Value::Null},"process":if n == current as u32 {serde_json::from_str::<Value>(row.get(8))?}else{Value::Null}});
    // The contract revision: a revision that changes only the test list keeps the contract
    // of the revision before it. A pin is current from this revision to current_revision.
    let contract = document["contract"]
        .as_array()
        .filter(|pair| pair.first().and_then(Value::as_u64) == Some(u64::from(n)))
        .and_then(|pair| pair.get(1).and_then(Value::as_u64))
        .unwrap_or(u64::from(n));
    item["contract_revision"] = json!(contract);
    let decision = &document["data"]["decision"];
    if decision["outcome"] == "needs_information"
        && let (Some(recipient), Some(text)) =
            (decision["asked"].as_str(), decision["question"].as_str())
    {
        item["question"] = json!({"recipient":recipient,"text":text});
    }
    if n == current as u32 {
        let mut files: Vec<_> = item["annotations"]["attachments"]
            .as_object()
            .into_iter()
            .flat_map(|attachments| attachments.iter())
            .filter(|(_, file)| file["removed"] != true)
            .map(|(id, file)| {
                let mut file = file.clone();
                file["id"] = json!(id);
                file["created_at"] = file["at"].clone();
                let at = chrono::DateTime::parse_from_str(
                    file["at"].as_str().unwrap_or(""),
                    "%Y-%m-%d %H:%M:%S%.f%#z",
                )
                .ok();
                (at, id.clone(), file)
            })
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        // Each file has a presigned GET. An object without files needs no file storage.
        let mut files: Vec<Value> = files.into_iter().map(|(_, _, file)| file).collect();
        for file in &mut files {
            let text = |key: &str| file[key].as_str().unwrap_or("").to_owned();
            let key = crate::spaces::Spaces::key(&workspace, &text("hash"))?;
            let url =
                crate::spaces::storage()?.download_url(&key, &text("name"), &text("content_type"));
            file["url"] = json!(url);
        }
        item["files"] = json!(files);
        // The decoration comes from the status projection of the last write.
        apply_rollups(&*conn, &workspace, std::slice::from_mut(&mut item)).await?;
        add_priorities(&*conn, &workspace, std::slice::from_mut(&mut item)).await?;
    }
    Ok(item)
}
