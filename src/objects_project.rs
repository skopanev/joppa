//! The objects projection: rows of joppa.objects, revisions, links and aliases. A large
//! workspace has tens of thousands of objects, so the projection walks them in steps of PART:
//! a step reads the stored rows and IDs of its objects only, and the rows to write leave in
//! parts of PART as soon as a part is full.
use super::*;
use crate::store::stream::{PART, each};
#[path = "objects_project_parts.rs"]
mod parts;
use parts::{Pending, delete_links, existing_of, redact, registered_of, stale_sources};

pub(in crate::store) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    events: &[Event],
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Result<()> {
    let prefix = state.reference_prefix.as_deref().unwrap_or(workspace);
    let schema_changed = events
        .iter()
        .any(|e| crate::store::schema_event(&e.command));
    // A redaction rewrites the stored revisions of its object. Nothing else may. It runs first,
    // so the revision check of each part compares with the redacted rows.
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
    redact(client, workspace, &redactions).await?;
    let keys: Vec<&String> = state
        .objects
        .keys()
        .filter(|key| selected.is_none_or(|s| s.contains(*key)))
        .collect();
    // The old links of the sources leave once. With several steps the sources are known before
    // the walk, so the links of each step can leave in parts.
    let early = if schema_changed {
        delete_links(client, workspace, &keys).await?;
        true
    } else if keys.len() > PART {
        let sources = stale_sources(client, workspace, state, &keys).await?;
        delete_links(client, workspace, &sources).await?;
        true
    } else {
        false
    };
    let mut pending = Pending::default();
    let mut sources: Vec<String> = vec![];
    for step in keys.chunks(PART) {
        // Read each current projection once. History rows never change after insertion.
        let existing = existing_of(client, workspace, step).await?;
        let registered = registered_of(client, workspace, prefix, step).await?;
        let (docs, revisions, links, ids) = (
            &mut pending.docs,
            &mut pending.revisions,
            &mut pending.links,
            &mut pending.ids,
        );
        for &address in step {
            let obj = &state.objects[address];
            let r = obj.current();
            let previous = existing.get(address);
            let previous_revision =
                previous.and_then(|v| v["revision"].as_u64()).unwrap_or(0) as u32;
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
                    crate::relations::stored_document(old)? == previous["document"],
                    "object history changed at {address}/{previous_revision}"
                );
            }
            let mut signal = objects::signal(state, address);
            if let Some(previous) = previous {
                signal["at"] = if events.is_empty()
                    || (previous["signal"]["tone"] == signal["tone"]
                        && previous["signal"]["label"] == signal["label"])
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
                crate::relations::stored_document(r)?
            };
            let doc = json!({"address":address,"kind":obj.kind,"local_id":obj.id,"revision":r.revision,"parent":r.parent,"present":r.present,"title":r.data["title"],"owner":r.data["owner"],"description":if state.unloaded_objects.contains(address) {previous.unwrap()["description"].clone()} else {r.data["body"].clone()},"document_at":r.at,"inbox_status":r.data["status"].as_str().unwrap_or("unprocessed"),"document":document,"signal":signal,"process":objects::process(state,address),"parent_hash_current":objects::parent_hash_current(state,address),"contract_hash":objects::contract_hash(state,address)});
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
                for (relation, edge) in r.edges() {
                    links.push(json!({"source":address,"target":edge.to,"relation":relation,"revision":edge.revision,"when_label":edge.when}));
                }
                if state.schema_migrated
                    && r.present
                    && let Ok(targets) =
                        objects::stored_reference_targets(state, &obj.kind, &r.data)
                {
                    for (field, target, _) in targets {
                        links.push(json!({"source":address,"target":target,"relation":format!("reference:{field}"),"revision":null,"when_label":null}));
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
        pending
            .flush(client, workspace, prefix, early, false)
            .await?;
    }
    if !early && !sources.is_empty() {
        delete_links(client, workspace, &sources).await?;
    }
    pending.flush(client, workspace, prefix, true, true).await?;
    drop((pending, sources, keys));
    if !state.object_aliases.is_empty() {
        let aliases: Vec<Value> = state
            .object_aliases
            .iter()
            .map(|(address, target)| json!({"address":address,"target":target}))
            .collect();
        for part in aliases.chunks(PART) {
            let body = json!(part).to_string();
            client.execute("INSERT INTO joppa.object_aliases(workspace,address,target) SELECT $1,x.address,x.target FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,target text) ON CONFLICT DO NOTHING", &[&workspace, &body]).await?;
            let conflict = client.query_one("SELECT EXISTS (SELECT 1 FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,target text) LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=x.address WHERE a.target IS DISTINCT FROM x.target)", &[&workspace, &body]).await?;
            ensure!(
                !conflict.get::<_, bool>(0),
                "object alias differs from the journal"
            );
        }
    }
    // A full projection frees the rows of every object.
    if selected.is_none() {
        crate::store::stream::trim_heap();
    }
    Ok(())
}
