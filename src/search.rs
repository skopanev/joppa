//! Authorized search over rebuildable current-state projections, never evidence.
use super::*;
use crate::embeddings::{self, Embedder, PROFILE};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Duration;

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
fn embedding_input(title: &str, content: &str) -> String {
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
    let content_hash = format!("{:x}", Sha256::digest(embedding_input(title, &content)));
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
) -> Result<()> {
    let configured = crate::objects::schema(state);
    let mut documents = vec![];
    for (key, obj) in state.objects.iter().filter(|(_, o)| o.current().present) {
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
        embedding_profile=CASE WHEN search_documents.content_hash=EXCLUDED.content_hash THEN search_documents.embedding_profile ELSE NULL END
      WHERE (search_documents.req,search_documents.revision,search_documents.title,search_documents.content) IS DISTINCT FROM (EXCLUDED.req,EXCLUDED.revision,EXCLUDED.title,EXCLUDED.content)", &[&workspace,&payload]).await?;
    client.execute("DELETE FROM joppa.search_documents d WHERE workspace=$1 AND NOT EXISTS(SELECT 1 FROM jsonb_to_recordset($2::text::jsonb) AS x(kind text,local_id text) WHERE x.kind=d.kind AND x.local_id=d.local_id)", &[&workspace,&payload]).await?;
    client.execute("INSERT INTO joppa.search_state(workspace,position,canonical_id,reference_prefix) VALUES($1,$2,$3,$4)
      ON CONFLICT(workspace) DO UPDATE SET position=EXCLUDED.position,canonical_id=EXCLUDED.canonical_id,reference_prefix=EXCLUDED.reference_prefix", &[&workspace,&(position as i64),&state.workspace_id.as_deref().unwrap_or(workspace),&state.reference_prefix.as_deref().unwrap_or(workspace)]).await?;
    Ok(())
}
async fn current(pool: &Pool, workspace: &str, actor: Option<&str>) -> Result<String> {
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
        let (state, events) = load(&*tx, &workspace).await?;
        project(&*tx, &workspace, &state, events.len() as u64).await?;
    }
    tx.commit().await?;
    Ok(workspace)
}

pub async fn find(
    pool: &Pool,
    actor: &str,
    workspace: &str,
    query: &str,
    kind: Option<&str>,
    limit: i64,
    embedder: Option<&Embedder>,
) -> Result<Value> {
    ensure!(
        !query.trim().is_empty() && query.chars().count() <= 2000,
        "query must contain 1–2000 characters"
    );
    ensure!((1..=50).contains(&limit), "limit must be 1–50");
    ensure!(
        kind.is_none_or(|v| !v.is_empty() && v.len() <= 100),
        "unknown search kind"
    );
    // Authorization precedes any query embedding or provider traffic.
    let workspace = current(pool, workspace, Some(actor)).await?;
    let (vector, mut semantic) = if let Some(embedder) = embedder {
        match embedder.embed(&[format!("Instruct: Given a product question, retrieve product requirements and acceptance criteria that answer it.\nQuery: {query}")]).await {
            Ok(v)=>(Some(embeddings::literal(&v[0])?),"ready"),
            Err(_)=>(None,"unavailable"),
        }
    } else {
        (None, "not_configured")
    };
    let mut conn = pool.get().await?;
    let tx = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    // Recheck after network I/O: access may have been revoked while embedding.
    access::check(&*tx, &workspace, actor, false).await?;
    let state=tx.query_one("SELECT canonical_id,position,reference_prefix FROM joppa.search_state WHERE workspace=$1", &[&workspace]).await?;
    let canonical: String = state.get(0);
    let position: i64 = state.get(1);
    let prefix: String = state.get(2);
    let counts=tx.query_one("SELECT count(*), count(*) FILTER(WHERE embedding IS NOT NULL AND embedding_profile=$3) FROM joppa.search_documents WHERE workspace=$1 AND ($2::text IS NULL OR kind=$2)", &[&workspace,&kind,&PROFILE]).await?;
    let total: i64 = counts.get(0);
    let embedded: i64 = counts.get(1);
    if semantic == "ready" && embedded < total {
        semantic = "pending";
    }
    let mut scores = BTreeMap::<(String, String), f64>::new();
    let mut exact = BTreeMap::<(String, String), bool>::new();
    let rows=tx.query("WITH q AS (SELECT websearch_to_tsquery('simple',$2) || websearch_to_tsquery('english',$2) || websearch_to_tsquery('russian',$2) AS terms), matches AS (
      SELECT d.kind,d.local_id,false AS exact,ts_rank_cd(d.terms,q.terms) AS rank FROM joppa.search_documents d,q
      WHERE d.workspace=$1 AND ($3::text IS NULL OR d.kind=$3) AND d.terms @@ q.terms
      UNION ALL
      SELECT d.kind,d.local_id,true,0::real FROM joppa.item_ids i LEFT JOIN joppa.object_aliases a ON a.workspace=i.workspace AND a.address=i.kind||':'||i.local_id JOIN joppa.search_documents d
      ON d.workspace=i.workspace AND d.kind=COALESCE(split_part(a.target,':',1),i.kind) AND d.local_id=COALESCE(split_part(a.target,':',2),i.local_id)
      WHERE i.id=$2 AND d.workspace=$1 AND ($3::text IS NULL OR d.kind=$3))
      SELECT kind,local_id,bool_or(exact) AS exact FROM matches GROUP BY kind,local_id
      ORDER BY exact DESC,max(rank) DESC,kind,local_id LIMIT 100", &[&workspace,&query,&kind]).await?;
    for (rank, row) in rows.iter().enumerate() {
        let key = (row.get(0), row.get(1));
        let is_exact: bool = row.get(2);
        scores.insert(
            key.clone(),
            if is_exact {
                1.0
            } else {
                1.0 / (61.0 + rank as f64)
            },
        );
        exact.insert(key, is_exact);
    }
    if let Some(vector) = &vector {
        // Restrict the candidate set by workspace before exact cosine ranking.
        let rows=tx.query("SELECT kind,local_id FROM joppa.search_documents WHERE workspace=$1 AND ($2::text IS NULL OR kind=$2) AND embedding IS NOT NULL AND embedding_profile=$3 ORDER BY embedding <=> $4::text::vector,kind,local_id LIMIT 100", &[&workspace,&kind,&PROFILE,&vector]).await?;
        for (rank, row) in rows.iter().enumerate() {
            *scores.entry((row.get(0), row.get(1))).or_default() += 1.0 / (61.0 + rank as f64);
        }
    }
    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let selected = ranked
        .into_iter()
        .take(limit as usize)
        .map(|((kind, id), score)| {
            let exact = exact
                .get(&(kind.clone(), id.clone()))
                .copied()
                .unwrap_or(false);
            json!({"kind":kind,"id":id,"score":score,"exact":exact})
        })
        .collect::<Vec<_>>();
    let rows = tx.query("SELECT d.kind,d.local_id,d.title,left(d.content,600),d.req,d.revision,i.id,k.score,k.exact
      FROM jsonb_to_recordset($3::text::jsonb) AS k(kind text,id text,score double precision,exact boolean)
      JOIN joppa.search_documents d ON d.workspace=$1 AND d.kind=k.kind AND d.local_id=k.id
      LEFT JOIN joppa.object_aliases a ON a.workspace=d.workspace AND a.target=d.kind||':'||d.local_id
      JOIN joppa.item_ids i ON i.workspace=d.workspace AND i.kind=COALESCE(split_part(a.address,':',1),d.kind) AND i.local_id=COALESCE(split_part(a.address,':',2),d.local_id) AND i.namespace=$2
      ORDER BY k.score DESC,d.kind,d.local_id", &[&workspace,&prefix,&serde_json::to_string(&selected)?]).await?;
    let items = rows.iter().map(|row|json!({"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"system_id":row.get::<_,String>(6),"title":row.get::<_,String>(2),"excerpt":row.get::<_,String>(3),"req":row.get::<_,Option<String>>(4),"revision":row.get::<_,Option<i32>>(5),"score":row.get::<_,f64>(7),"exact_id":row.get::<_,bool>(8)})).collect::<Vec<_>>();
    tx.commit().await?;
    Ok(
        json!({"workspace":canonical,"position":position,"mode":if vector.is_some() && embedded>0 {"hybrid"} else {"full_text"},"semantic_status":semantic,"indexed":total,"embedded":embedded,"items":items}),
    )
}

/// One bounded batch. Hash compare-and-set prevents a late vector from replacing
/// current content after an edit or archive while the provider was running.
pub async fn index_batch(pool: &Pool, embedder: &Embedder) -> Result<usize> {
    let rows=pool.get().await?.query("SELECT workspace,kind,local_id,title,content,content_hash FROM joppa.search_documents WHERE embedding IS NULL OR embedding_profile IS DISTINCT FROM $1 ORDER BY workspace,kind,local_id LIMIT 16", &[&PROFILE]).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let inputs = rows
        .iter()
        .map(|r| embedding_input(r.get(3), r.get(4)))
        .collect::<Vec<_>>();
    let vectors = embedder.embed(&inputs).await?;
    let conn = pool.get().await?;
    for (row, vector) in rows.iter().zip(vectors) {
        conn.execute("UPDATE joppa.search_documents SET embedding=$5::text::vector,embedding_profile=$6 WHERE workspace=$1 AND kind=$2 AND local_id=$3 AND content_hash=$4", &[&row.get::<_,String>(0),&row.get::<_,String>(1),&row.get::<_,String>(2),&row.get::<_,String>(5),&embeddings::literal(&vector)?,&PROFILE]).await?;
    }
    Ok(rows.len())
}
pub async fn worker(pool: Pool, embedder: Embedder) {
    loop {
        let result = async {
            for row in pool
                .get()
                .await?
                .query("SELECT id FROM joppa.workspaces", &[])
                .await?
            {
                current(&pool, row.get(0), None).await?;
            }
            index_batch(&pool, &embedder).await
        }
        .await;
        let delay = match result {
            Ok(0) => 30,
            Ok(_) => 1,
            Err(_) => {
                eprintln!("joppa: semantic indexing unavailable; pending work retained for retry");
                30
            }
        };
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}
