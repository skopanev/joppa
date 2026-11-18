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
/// The objects of one build step. A full build of a large workspace holds the documents of one
/// step only, so its memory stays small.
const STEP: usize = 500;

/// Build the search rows of `selected`, or of every object, and remove the rows of the other
/// selected objects (of every other object without `selected`).
pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    position: u64,
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Result<()> {
    let keys: Vec<&String> = state
        .objects
        .keys()
        .filter(|key| selected.is_none_or(|s| s.contains(*key)))
        .collect();
    let (mut kinds, mut ids) = (vec![], vec![]);
    for step in keys.chunks(STEP) {
        let documents = documents(client, workspace, state, step).await?;
        for d in &documents {
            kinds.push(d.kind.clone());
            ids.push(d.local_id.clone());
        }
        let payload = serde_json::to_string(&documents)?;
        drop(documents);
        client.execute("INSERT INTO joppa.search_documents(workspace,kind,local_id,req,revision,title,content,content_hash)
      SELECT $1,x.kind,x.local_id,x.req,x.revision,x.title,x.content,x.content_hash FROM jsonb_to_recordset($2::text::jsonb)
      AS x(kind text,local_id text,req text,revision int,title text,content text,content_hash text)
      ON CONFLICT(workspace,kind,local_id) DO UPDATE SET req=EXCLUDED.req,revision=EXCLUDED.revision,title=EXCLUDED.title,content=EXCLUDED.content,content_hash=EXCLUDED.content_hash,
        embedding=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.embedding ELSE NULL END,
        embedding_profile=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.embedding_profile ELSE NULL END,
        attempts=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.attempts ELSE 0 END,
        last_error=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.last_error ELSE NULL END
      WHERE (search_documents.req,search_documents.revision,search_documents.title,search_documents.content) IS DISTINCT FROM (EXCLUDED.req,EXCLUDED.revision,EXCLUDED.title,EXCLUDED.content)", &[&workspace,&payload]).await?;
    }
    // The kept rows are (kind, local_id) pairs, so the delete reads the primary key.
    let kept = "NOT EXISTS(SELECT 1 FROM unnest($2::text[],$3::text[]) AS x(kind,local_id) WHERE x.kind=d.kind AND x.local_id=d.local_id)";
    match selected {
        Some(selected) => {
            let selected: Vec<&String> = selected.iter().collect();
            client.execute(&format!("DELETE FROM joppa.search_documents d WHERE workspace=$1 AND (d.kind,d.local_id) IN (SELECT split_part(a,':',1),substr(a,strpos(a,':')+1) FROM unnest($4::text[]) a) AND {kept}"), &[&workspace,&kinds,&ids,&selected]).await?
        }
        None => {
            let sql = format!("DELETE FROM joppa.search_documents d WHERE workspace=$1 AND {kept}");
            client.execute(&sql, &[&workspace, &kinds, &ids]).await?
        }
    };
    client.execute("INSERT INTO joppa.search_state(workspace,position,canonical_id,reference_prefix) VALUES($1,$2,$3,$4)
      ON CONFLICT(workspace) DO UPDATE SET position=EXCLUDED.position,canonical_id=EXCLUDED.canonical_id,reference_prefix=EXCLUDED.reference_prefix", &[&workspace,&(position as i64),&state.workspace_id.as_deref().unwrap_or(workspace),&state.reference_prefix.as_deref().unwrap_or(workspace)]).await?;
    Ok(())
}

/// The documents of the present objects `keys` that no archived ancestor hides.
async fn documents(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    keys: &[&String],
) -> Result<Vec<Document>> {
    let mut addresses = std::collections::BTreeSet::new();
    for key in keys {
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
    // Only a selected object or an ancestor that is not loaded needs a copy of the workspace.
    let hydrated = if addresses.is_empty() {
        std::borrow::Cow::Borrowed(state)
    } else {
        let mut hydrated = state.clone();
        let addresses: Vec<String> = addresses.into_iter().collect();
        state_rows::hydrate_objects(client, workspace, &mut hydrated, &addresses).await?;
        std::borrow::Cow::Owned(hydrated)
    };
    let state = hydrated.as_ref();
    let configured = crate::objects::schema(state);
    let mut documents = vec![];
    for (key, obj) in keys
        .iter()
        .filter_map(|key| state.objects.get_key_value(*key))
        .filter(|(_, o)| o.current().present)
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
    Ok(documents)
}
pub(super) async fn current(pool: &Pool, workspace: &str, actor: Option<&str>) -> Result<String> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    if let Some(actor) = actor {
        access::check(&*tx, &workspace, actor, false).await?;
    }
    let row=tx.query_one("SELECT COALESCE((SELECT position FROM joppa.search_state WHERE workspace=$1),-1), COALESCE((SELECT max(position) FROM joppa.events WHERE workspace=$1),0), EXISTS(SELECT 1 FROM joppa.search_pending WHERE workspace=$1)", &[&workspace]).await?;
    // The objects that writes left for later come first (search_pending.rs).
    let (pending, behind) = (
        row.get::<_, bool>(2),
        row.get::<_, i64>(0) != row.get::<_, i64>(1),
    );
    if pending || behind {
        // The search lock keeps the search builders apart, and no write waits for it.
        tx.execute(super::search_pending::LOCK, &[&workspace])
            .await?;
        if !super::search_pending::build(&*tx, &workspace).await? && behind {
            let (state, position) = super::state_cache::read(&*tx, &workspace).await?;
            project(&*tx, &workspace, &state, position, None).await?;
        }
    }
    tx.commit().await?;
    Ok(workspace)
}

pub use super::search_query::{Flags, find, find_with};
