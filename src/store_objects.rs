use super::*;
use crate::objects;

pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    events: &[Event],
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Result<()> {
    // Read each current projection once. History rows never change after insertion.
    let existing: std::collections::BTreeMap<String, Value> = client
        .query("SELECT address,jsonb_build_object('address',address,'kind',kind,'local_id',local_id,'revision',revision,'parent',parent,'present',present,'title',title,'owner',owner,'description',description,'document_at',document_at,'inbox_status',inbox_status,'document',document,'review_required',review_required,'work_ready',work_ready,'signal',signal,'process',process)::text FROM joppa.objects WHERE workspace=$1 AND ($2::text[] IS NULL OR address=ANY($2))", &[&workspace,&selected.map(|s|s.iter().cloned().collect::<Vec<_>>())])
        .await?.iter().map(|row| Ok((row.get(0),serde_json::from_str(row.get(1))?)))
        .collect::<Result<_>>()?;
    let prefix = state.reference_prefix.as_deref().unwrap_or(workspace);
    let registered: std::collections::BTreeSet<(String, String)> = client
        .query(
            "SELECT kind,local_id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2",
            &[&workspace, &prefix],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let schema_changed = events.iter().any(|e| {
        matches!(
            e.command,
            Command::SetTreeSchema { .. }
                | Command::MigrateSchema {}
                | Command::MigrateDocuments {}
        )
    });
    let mut docs = vec![];
    let mut revisions = vec![];
    let mut sources: Vec<String> = vec![];
    let mut links = vec![];
    let mut ids = vec![];
    for (address, obj) in state
        .objects
        .iter()
        .filter(|(key, _)| selected.is_none_or(|s| s.contains(*key)))
    {
        let r = obj.current();
        let previous = existing.get(address);
        let previous_revision = previous.and_then(|v| v["revision"].as_u64()).unwrap_or(0) as u32;
        if let Some(previous) = previous.filter(|_| !state.unloaded_objects.contains(address)) {
            let old = obj
                .revisions
                .iter()
                .find(|r| r.revision == previous_revision)
                .context("object revision history was shortened")?;
            ensure!(
                serde_json::to_value(old)? == previous["document"],
                "object history changed at {address}/{previous_revision}"
            );
        }
        let mut signal = objects::signal(state, address);
        if let Some(previous) = previous {
            signal["at"] = if events.is_empty()
                || (previous["signal"]["tone"] == signal["tone"]
                    && previous["signal"]["label"] == signal["label"]
                    && previous["signal"]["assessment"] == signal["assessment"]
                    && previous["signal"]["readiness_blocker"] == signal["readiness_blocker"])
            {
                previous["signal"]["at"].clone()
            } else {
                json!(
                    events
                        .last()
                        .map(|e| e.recorded_at.as_str())
                        .unwrap_or(&r.at)
                )
            };
        }
        let document = if state.unloaded_objects.contains(address) {
            previous.context("unloaded object projection is missing")?["document"].clone()
        } else {
            json!(r)
        };
        let doc = json!({"address":address,"kind":obj.kind,"local_id":obj.id,"revision":r.revision,"parent":r.parent,"present":r.present,"title":r.data["title"],"owner":r.data["owner"],"description":if state.unloaded_objects.contains(address) {previous.unwrap()["description"].clone()} else {r.data["description"].clone()},"document_at":r.at,"inbox_status":r.data["status"].as_str().unwrap_or("unprocessed"),"document":document,"review_required":!objects::review_links(state,address).is_empty(),"work_ready":objects::work_ready(state,address),"signal":signal,"process":objects::process(state,address)});
        if previous != Some(&doc) {
            docs.push(doc);
        }
        if r.revision > previous_revision {
            for revision in obj
                .revisions
                .iter()
                .filter(|r| r.revision > previous_revision)
            {
                revisions.push(
                    json!({"address":address,"revision":revision.revision,"document":revision}),
                );
            }
        }
        if r.revision > previous_revision || schema_changed {
            sources.push(address.clone());
            for link in &r.links {
                links.push(json!({"source":address,"target":link.target,"relation":link.relation,"revision":link.revision,"blocking":link.blocking}));
            }
            if state.schema_migrated
                && r.present
                && let Ok(targets) = objects::reference_targets(state, &obj.kind, &r.data)
            {
                for (field, target, _) in targets {
                    links.push(json!({"source":address,"target":target,"relation":format!("reference:{field}"),"revision":null,"blocking":false}));
                }
            }
        }
        if !state
            .object_aliases
            .values()
            .any(|target| target == address)
            && !registered.contains(&(obj.kind.clone(), obj.id.clone()))
        {
            ids.push(json!({"kind":obj.kind,"local_id":obj.id}));
        }
    }
    if !docs.is_empty() {
        client.execute("INSERT INTO joppa.objects(workspace,address,kind,local_id,revision,parent,present,title,owner,description,document_at,inbox_status,document,review_required,work_ready,signal,process) SELECT $1,x.address,x.kind,x.local_id,x.revision,x.parent,x.present,x.title,x.owner,x.description,x.document_at,x.inbox_status,x.document,x.review_required,x.work_ready,x.signal,x.process FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,kind text,local_id text,revision integer,parent text,present boolean,title text,owner text,description text,document_at text,inbox_status text,document jsonb,review_required boolean,work_ready boolean,signal jsonb,process jsonb) ON CONFLICT(workspace,address) DO UPDATE SET revision=EXCLUDED.revision,parent=EXCLUDED.parent,present=EXCLUDED.present,title=EXCLUDED.title,owner=EXCLUDED.owner,description=EXCLUDED.description,document_at=EXCLUDED.document_at,inbox_status=EXCLUDED.inbox_status,document=EXCLUDED.document,review_required=EXCLUDED.review_required,work_ready=EXCLUDED.work_ready,signal=EXCLUDED.signal,process=EXCLUDED.process", &[&workspace,&json!(docs).to_string()]).await?;
    }
    if !revisions.is_empty() {
        let body = json!(revisions).to_string();
        let conflicts=client.query_one("SELECT EXISTS (SELECT 1 FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,revision integer,document jsonb) JOIN joppa.object_revisions r ON r.workspace=$1 AND r.address=x.address AND r.revision=x.revision WHERE r.document IS DISTINCT FROM x.document)",&[&workspace,&body]).await?;
        ensure!(
            !conflicts.get::<_, bool>(0),
            "stored object revision differs from the journal"
        );
        client.execute("INSERT INTO joppa.object_revisions(workspace,address,revision,document) SELECT $1,x.address,x.revision,x.document FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,revision integer,document jsonb) ON CONFLICT(workspace,address,revision) DO NOTHING", &[&workspace,&body]).await?;
    }
    if !sources.is_empty() {
        client
            .execute(
                "DELETE FROM joppa.object_links WHERE workspace=$1 AND source=ANY($2)",
                &[&workspace, &sources],
            )
            .await?;
        client.execute("INSERT INTO joppa.object_links(workspace,source,target,relation,revision,blocking) SELECT $1,x.source,x.target,x.relation,x.revision,x.blocking FROM jsonb_to_recordset($2::text::jsonb) AS x(source text,target text,relation text,revision integer,blocking boolean)", &[&workspace,&json!(links).to_string()]).await?;
    }
    if !ids.is_empty() {
        client.execute("SELECT joppa.register_item($1,x.kind,x.local_id,$3) FROM jsonb_to_recordset($2::text::jsonb) AS x(kind text,local_id text)",&[&workspace,&json!(ids).to_string(),&prefix]).await?;
    }
    if !state.object_aliases.is_empty() {
        let aliases: Vec<Value> = state
            .object_aliases
            .iter()
            .map(|(address, target)| json!({"address":address,"target":target}))
            .collect();
        let body = json!(aliases).to_string();
        client.execute("INSERT INTO joppa.object_aliases(workspace,address,target) SELECT $1,x.address,x.target FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,target text) ON CONFLICT DO NOTHING", &[&workspace,&body]).await?;
        let conflict=client.query_one("SELECT EXISTS (SELECT 1 FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,target text) LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=x.address WHERE a.target IS DISTINCT FROM x.target)", &[&workspace,&body]).await?;
        ensure!(
            !conflict.get::<_, bool>(0),
            "object alias differs from the journal"
        );
    }
    Ok(())
}
async fn authorized(
    pool: &Pool,
    workspace: &str,
    actor: &str,
) -> Result<(deadpool_postgres::Object, String)> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    Ok((conn, workspace))
}
pub async fn tree_schema(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    revision: Option<u32>,
) -> Result<Value> {
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    let requested = revision.map(i64::from);
    let row=conn.query_one("SELECT state->>'admin',COALESCE((state->'tree_schema'->-1->>'revision')::integer,0),COALESCE(state->>'workspace_id',workspace),position,(SELECT value::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' AND revision=COALESCE($2::bigint,(state->'tree_schema'->-1->>'revision')::bigint)) FROM joppa.board_cache WHERE workspace=$1", &[&workspace,&requested]).await?;
    let current: i32 = row.get(1);
    let n = revision.unwrap_or(current as u32);
    let selected = if n == 0 {
        objects::SchemaRevision {
            revision: 0,
            types: objects::legacy_defaults(),
            actor: row.get(0),
            at: String::new(),
            position: 0,
            reason: "Default workspace tree".into(),
        }
    } else {
        serde_json::from_str::<objects::SchemaRevision>(
            &row.get::<_, Option<String>>(4)
                .context("unknown schema revision")?,
        )?
    };
    Ok(
        json!({"workspace":row.get::<_,String>(2),"position":row.get::<_,i64>(3),"current_revision":current,"schema":selected}),
    )
}

#[derive(Default)]
pub struct ObjectScope<'a> {
    pub address: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub revision: Option<u32>,
    pub history: bool,
    pub limit: usize,
    pub cursor: Option<&'a str>,
    pub note_status: Option<&'a str>,
    pub related_to: Option<&'a str>,
}
pub async fn read_objects(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
) -> Result<Value> {
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let row=conn.query_one("SELECT position,COALESCE(state->>'workspace_id',workspace),COALESCE(state->>'reference_prefix',workspace) FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?;
    let position: i64 = row.get(0);
    let public: String = row.get(1);
    let prefix: String = row.get(2);
    if let Some(key) = scope.address {
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
                && scope.related_to.is_none(),
            "address cannot be combined with list filters"
        );
        let row=conn.query_opt("SELECT o.kind,o.local_id,o.revision,o.document::text,o.review_required,o.work_ready,(SELECT id FROM joppa.item_ids WHERE workspace=o.workspace AND kind=COALESCE(split_part(a.address,':',1),o.kind) AND local_id=COALESCE(split_part(a.address,':',2),o.local_id) AND namespace=$3),o.signal::text,o.process::text FROM joppa.objects o LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.target=o.address WHERE o.workspace=$1 AND o.address=$2", &[&workspace,&key,&prefix]).await?.context("unknown object")?;
        let current: i32 = row.get(2);
        if scope.history {
            ensure!(
                scope.revision.is_none(),
                "history reports counts; read one revision separately"
            );
            return Ok(
                json!({"workspace":public,"address":key,"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"current_revision":current,"revision_count":current,"position":position}),
            );
        }
        let n = scope.revision.unwrap_or(current as u32);
        ensure!(n > 0, "object revision must be positive");
        let document: Value = if n == current as u32 {
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
                item["metadata_scope"] = json!("current");
                item
            })
            .collect();
        let references: Vec<Value> = if n == current as u32 {
            conn.query("SELECT l.target,l.relation,o.title,o.revision,o.present FROM joppa.object_links l JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.target WHERE l.workspace=$1 AND l.source=$2 AND l.relation LIKE 'reference:%' ORDER BY l.relation,l.target", &[&workspace,&key]).await?.iter().map(|r| json!({"target":r.get::<_,String>(0),"field":r.get::<_,String>(1).strip_prefix("reference:").unwrap(),"title":r.get::<_,String>(2),"current_revision":r.get::<_,i32>(3),"present":r.get::<_,bool>(4)})).collect()
        } else {
            vec![]
        };
        let incoming:Vec<Value>=conn.query("SELECT l.source,o.title,l.relation,l.revision,l.blocking FROM joppa.object_links l JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source WHERE l.workspace=$1 AND l.target IN (SELECT $2 UNION SELECT address FROM joppa.object_aliases WHERE workspace=$1 AND target=$2) AND o.present ORDER BY l.source", &[&workspace,&key]).await?.iter().map(|r|json!({"source":r.get::<_,String>(0),"title":r.get::<_,String>(1),"relation":r.get::<_,String>(2),"revision":r.get::<_,Option<i32>>(3),"blocking":r.get::<_,bool>(4)})).collect();
        let projected_process: Value = serde_json::from_str(row.get(8))?;
        let annotation = projected_process["annotation_address"]
            .as_str()
            .unwrap_or(&key);
        let annotations:Value=serde_json::from_str(conn.query_one("SELECT COALESCE((SELECT value FROM joppa.state_entries WHERE workspace=$1 AND section='notes' AND key=$2),'{}'::jsonb)::text", &[&workspace,&annotation]).await?.get(0))?;
        return Ok(
            json!({"workspace":public,"position":position,"address":key,"kind":row.get::<_,String>(0),"id":row.get::<_,String>(1),"item_id":row.get::<_,Option<String>>(6),"current_revision":current,"revision_count":current,"document":document,"historical":n != current as u32,"review_required":if n == current as u32 {json!(row.get::<_,bool>(4))}else{Value::Null},"work_ready":if n == current as u32 {json!(row.get::<_,bool>(5))}else{Value::Null},"links":outgoing,"references":references,"incoming":if n == current as u32 {json!(incoming)}else{json!([])},"annotations":if n == current as u32 {annotations}else{json!({})},"metadata_scope":"current","signal":if n == current as u32 {serde_json::from_str::<Value>(row.get(7))?}else{Value::Null},"process":if n == current as u32 {serde_json::from_str::<Value>(row.get(8))?}else{Value::Null}}),
        );
    }
    ensure!(
        scope.revision.is_none() && !scope.history,
        "revision and history require an object address"
    );
    ensure!(
        scope
            .note_status
            .is_none_or(|status| ["unprocessed", "processed"].contains(&status)),
        "unknown Note status"
    );
    let selected: std::collections::BTreeMap<String, objects::TreeType> = serde_json::from_str(
        conn.query_one(
            "SELECT (first_view->'object_types')::text FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .get(0),
    )?;
    ensure!(
        scope.note_status.is_none()
            || scope
                .kind
                .is_some_and(|kind| selected.get(kind).is_some_and(|t| t.inbox.unwrap_or(false))),
        "note_status requires an inbox type"
    );
    let related_to = if let Some(key) = scope.related_to {
        let row = conn.query_opt("SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND o.address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2)", &[&workspace,&key]).await?.context("unknown related object")?;
        Some(row.get::<_, String>(0))
    } else {
        None
    };
    let limit = if scope.limit == 0 { 50 } else { scope.limit };
    ensure!((1..=100).contains(&limit), "limit must be 1 to 100");
    let after = if let Some(cursor) = scope.cursor {
        let (p, key) = cursor.split_once(':').context("invalid object cursor")?;
        ensure!(
            p.parse::<i64>()? == position,
            "object list changed; start again without cursor"
        );
        key
    } else {
        ""
    };
    let parent = if let Some(parent) = scope.parent {
        Some(
            conn.query_opt(
                "SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2",
                &[&workspace, &parent],
            )
            .await?
            .map(|row| row.get::<_, String>(0))
            .unwrap_or_else(|| parent.to_owned()),
        )
    } else {
        None
    };
    let rows=conn.query("SELECT o.address,o.kind,o.local_id,o.revision,o.parent,o.title,o.owner,o.review_required,o.work_ready,(SELECT id FROM joppa.item_ids WHERE workspace=o.workspace AND kind=COALESCE(split_part(a.address,':',1),o.kind) AND local_id=COALESCE(split_part(a.address,':',2),o.local_id) AND namespace=$6),o.document_at,left(o.description,300),o.signal::text,o.present,o.inbox_status FROM joppa.objects o LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.target=o.address WHERE o.workspace=$1 AND (o.present OR EXISTS (SELECT 1 FROM joppa.objects child WHERE child.workspace=o.workspace AND child.parent=o.address AND child.present)) AND o.address>$2 AND ($3::text IS NULL OR o.kind=$3) AND (($4::text IS NULL AND ($8::text IS NOT NULL OR o.parent IS NULL)) OR o.parent=$4 OR $4='*') AND ($7::text IS NULL OR o.inbox_status=$7) AND ($8::text IS NULL OR o.parent=$8 OR EXISTS (SELECT 1 FROM joppa.object_links l WHERE l.workspace=o.workspace AND ((l.source=o.address AND l.target IN (SELECT $8 UNION SELECT address FROM joppa.object_aliases WHERE workspace=o.workspace AND target=$8)) OR (l.target=o.address AND l.source=$8)))) ORDER BY o.address LIMIT $5", &[&workspace,&after,&scope.kind,&parent,&((limit+1) as i64),&prefix,&scope.note_status,&related_to]).await?;
    let more = rows.len() > limit;
    let items:Vec<Value>=rows.iter().take(limit).map(|r|json!({"address":r.get::<_,String>(0),"kind":r.get::<_,String>(1),"id":r.get::<_,String>(2),"revision":r.get::<_,i32>(3),"parent":r.get::<_,Option<String>>(4),"title":r.get::<_,String>(5),"owner":r.get::<_,Option<String>>(6),"review_required":r.get::<_,bool>(7),"work_ready":r.get::<_,bool>(8),"item_id":r.get::<_,Option<String>>(9),"at":r.get::<_,Option<String>>(10),"description":r.get::<_,Option<String>>(11),"signal":serde_json::from_str::<Value>(r.get(12)).unwrap_or_else(|_|json!({})),"present":r.get::<_,bool>(13),"note_status":if selected.get(&r.get::<_,String>(1)).is_some_and(|t| t.inbox.unwrap_or(false)) {json!(r.get::<_,String>(14))}else{Value::Null}})).collect();
    let next = more.then(|| {
        format!(
            "{position}:{}",
            items.last().unwrap()["address"].as_str().unwrap()
        )
    });
    Ok(json!({"workspace":public,"position":position,"items":items,"next_cursor":next}))
}
