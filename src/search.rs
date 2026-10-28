//! Authorized search over rebuildable current-state projections, never evidence.
use super::*;
use serde::Serialize;

#[derive(Serialize)]
struct Document {
    kind: String,
    local_id: String,
    req: Option<String>,
    revision: Option<i32>,
    title: String,
    content: String,
    content_hash: String,
}
pub(super) fn embedding_input(title: &str, content: &str) -> String {
    format!("{title}\n{content}").chars().take(8000).collect()
}
fn document(
    kind: &str,
    id: &str,
    req: Option<&str>,
    revision: Option<u32>,
    title: &str,
    content: String,
) -> Document {
    let content: String = content.chars().take(60_000).collect();
    let content_hash = crate::digest_hex::sha256_hex(embedding_input(title, &content));
    Document {
        kind: kind.into(),
        local_id: id.into(),
        req: req.map(str::to_owned),
        revision: revision.map(|n| n as i32),
        title: title.into(),
        content,
        content_hash,
    }
}
pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    position: u64,
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Result<()> {
    let mut hydrated = state.clone();
    let mut addresses = std::collections::BTreeSet::new();
    for key in state
        .objects
        .keys()
        .filter(|key| selected.is_none_or(|s| s.contains(*key)))
    {
        let mut current = Some(key.as_str());
        let mut seen = std::collections::BTreeSet::new();
        while let Some(key) = current {
            if !seen.insert(key) {
                break;
            }
            if state.unloaded_objects.contains(key) {
                addresses.insert(key.to_owned());
            }
            current = state
                .objects
                .get(key)
                .and_then(|o| o.current().parent.as_deref());
        }
    }
    state_rows::hydrate_objects(
        client,
        workspace,
        &mut hydrated,
        &addresses.into_iter().collect::<Vec<_>>(),
    )
    .await?;
    let state = &hydrated;
    let configured = crate::objects::schema(state);
    let mut documents = vec![];
    for (key, obj) in state
        .objects
        .iter()
        .filter(|(key, o)| o.current().present && selected.is_none_or(|s| s.contains(*key)))
    {
        let mut context = vec![];
        let mut current = Some(key.as_str());
        let mut seen = std::collections::BTreeSet::new();
        let mut archived = false;
        while let Some(address) = current {
            if !seen.insert(address) {
                archived = true;
                break;
            }
            let Some(ancestor) = state.objects.get(address) else {
                break;
            };
            let r = ancestor.current();
            if r.data["archived"] == true {
                archived = true;
                break;
            }
            let title = configured
                .types
                .get(&ancestor.kind)
                .map_or(ancestor.kind.as_str(), |t| t.title.as_str());
            let mut searchable = r.data.clone();
            if searchable["archived"].is_boolean() {
                searchable.as_object_mut().unwrap().remove("archived");
            }
            context.push(format!("{title}: {}", serde_json::to_string(&searchable)?));
            current = r.parent.as_deref();
        }
        if archived {
            continue;
        }
        context.reverse();
        let process = crate::objects::verification_context(state, key);
        if let Some((req, r)) = process {
            for comment in state.requirements[req]
                .comments
                .values()
                .filter(|c| c.revision == r.number)
            {
                context.push(format!(
                    "{}{}: {}",
                    if comment.withdrawn { "Withdrawn " } else { "" },
                    comment.kind,
                    comment.text
                ));
            }
        }
        let annotation = crate::objects::annotation_address(state, key);
        if let Some(notes) = state.notes.get(&annotation) {
            context.push(serde_json::to_string(notes)?);
        }
        documents.push(document(
            &obj.kind,
            &obj.id,
            process.map(|(req, _)| req),
            Some(process.map_or(obj.current().revision, |(_, r)| r.number)),
            obj.current().data["title"].as_str().unwrap_or(&obj.id),
            context.join("\n"),
        ));
    }
    let payload = serde_json::to_string(&documents)?;
    client.execute("INSERT INTO joppa.search_documents(workspace,kind,local_id,req,revision,title,content,content_hash)
      SELECT $1,x.kind,x.local_id,x.req,x.revision,x.title,x.content,x.content_hash FROM jsonb_to_recordset($2::text::jsonb)
      AS x(kind text,local_id text,req text,revision int,title text,content text,content_hash text)
      ON CONFLICT(workspace,kind,local_id) DO UPDATE SET req=EXCLUDED.req,revision=EXCLUDED.revision,title=EXCLUDED.title,content=EXCLUDED.content,content_hash=EXCLUDED.content_hash,
        embedding=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.embedding ELSE NULL END,
        embedding_profile=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.embedding_profile ELSE NULL END,
        attempts=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.attempts ELSE 0 END,
        last_error=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.last_error ELSE NULL END
      WHERE (search_documents.req,search_documents.revision,search_documents.title,search_documents.content) IS DISTINCT FROM (EXCLUDED.req,EXCLUDED.revision,EXCLUDED.title,EXCLUDED.content)", &[&workspace,&payload]).await?;
    client.execute("DELETE FROM joppa.search_documents d WHERE workspace=$1 AND ($3::text[] IS NULL OR d.kind||':'||d.local_id=ANY($3)) AND NOT EXISTS(SELECT 1 FROM jsonb_to_recordset($2::text::jsonb) AS x(kind text,local_id text) WHERE x.kind=d.kind AND x.local_id=d.local_id)", &[&workspace,&payload,&selected.map(|s|s.iter().cloned().collect::<Vec<_>>())]).await?;
    client.execute("INSERT INTO joppa.search_state(workspace,position,canonical_id,reference_prefix) VALUES($1,$2,$3,$4)
      ON CONFLICT(workspace) DO UPDATE SET position=EXCLUDED.position,canonical_id=EXCLUDED.canonical_id,reference_prefix=EXCLUDED.reference_prefix", &[&workspace,&(position as i64),&state.workspace_id.as_deref().unwrap_or(workspace),&state.reference_prefix.as_deref().unwrap_or(workspace)]).await?;
    Ok(())
}
pub(super) async fn current(pool: &Pool, workspace: &str, actor: Option<&str>) -> Result<String> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    if let Some(actor) = actor {
        access::check(&*tx, &workspace, actor, false).await?;
    }
    let row=tx.query_one("SELECT COALESCE((SELECT position FROM joppa.search_state WHERE workspace=$1),-1), COALESCE((SELECT max(position) FROM joppa.events WHERE workspace=$1),0)", &[&workspace]).await?;
    if row.get::<_, i64>(0) != row.get::<_, i64>(1) {
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[&workspace],
        )
        .await?;
        let (state, position) = state_rows::current(&*tx, &workspace).await?;
        project(&*tx, &workspace, &state, position, None).await?;
    }
    tx.commit().await?;
    Ok(workspace)
}

pub use super::search_query::{Flags, find, find_with};
