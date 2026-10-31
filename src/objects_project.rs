//! The objects projection: rows of joppa.objects, revisions, links and aliases.
use super::*;

pub(in crate::store) async fn project(
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
            Command::SetTreeSchema(crate::commands::SetTreeSchemaOp { .. })
                | Command::MigrateSchema {}
                | Command::MigrateDocuments {}
                | Command::MigrateConsumers {}
                | Command::MigrateTestRules {}
                | Command::MigrateNoteParents {}
                | Command::MigrateBody {}
        )
    });
    // A redaction rewrites the stored revisions of its object; nothing else may.
    let redactions: Vec<(String, String)> = events
        .iter()
        .filter_map(|e| match &e.command {
            Command::RedactObject {
                target, fragment, ..
            } => Some((
                objects::resolve_address(state, target).to_owned(),
                fragment.clone(),
            )),
            _ => None,
        })
        .collect();
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
        if let Some(previous) = previous.filter(|_| {
            !state.unloaded_objects.contains(address)
                && !redactions.iter().any(|(a, _)| a == address)
        }) {
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
        let doc = json!({"address":address,"kind":obj.kind,"local_id":obj.id,"revision":r.revision,"parent":r.parent,"present":r.present,"title":r.data["title"],"owner":r.data["owner"],"description":if state.unloaded_objects.contains(address) {previous.unwrap()["description"].clone()} else {r.data["body"].clone()},"document_at":r.at,"inbox_status":r.data["status"].as_str().unwrap_or("unprocessed"),"document":document,"review_required":!objects::review_links(state,address).is_empty(),"work_ready":objects::work_ready(state,address),"signal":signal,"process":objects::process(state,address)});
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
                && let Ok(targets) = objects::stored_reference_targets(state, &obj.kind, &r.data)
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
    if !redactions.is_empty() {
        // A redaction only removes text, so the field length rule of migration 037 skips it.
        client
            .execute("SELECT set_config('joppa.replaying','1',true)", &[])
            .await?;
    }
    for (address, fragment) in &redactions {
        // The fragment as it appears inside a JSON string.
        let encoded = serde_json::to_string(fragment)?;
        let encoded = &encoded[1..encoded.len() - 1];
        client.execute("UPDATE joppa.object_revisions SET document=replace(document::text,$3,$4)::jsonb WHERE workspace=$1 AND address=$2 AND strpos(document::text,$3)>0", &[&workspace,address,&encoded,&crate::redact::MASK]).await?;
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
