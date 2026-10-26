//! The Task queue projection: one row of joppa.task_queue for each current Task.
use super::*;

pub(in crate::store) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    selected: Option<&BTreeSet<String>>,
) -> Result<()> {
    let mut rows = vec![];
    for actor in state.members.keys() {
        for (_, created_at, req, id, data) in super::tasks::queue_entries(state, actor, selected)? {
            let address = data["address"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("task:{req}/{id}"));
            if selected.is_some_and(|s| !s.contains(&address)) {
                continue;
            }
            let mut targets = BTreeSet::new();
            let mut subjects = BTreeSet::new();
            let mut current = Some(address.as_str());
            let mut seen = BTreeSet::new();
            while let Some(key) = current {
                let key = crate::objects::resolve_address(state, key);
                if !seen.insert(key.to_owned()) {
                    break;
                }
                targets.insert(key.to_owned());
                let Some(o) = state.objects.get(key) else {
                    break;
                };
                for (_, target, subject) in
                    crate::objects::stored_reference_targets(state, &o.kind, &o.current().data)
                        .unwrap_or_default()
                {
                    targets.insert(target.clone());
                    if subject {
                        subjects.insert(target);
                    }
                }
                current = o.current().parent.as_deref();
            }
            if !state.schema_migrated {
                for p in data["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str())
                {
                    let key = format!("project:{p}");
                    subjects.insert(key.clone());
                    targets.insert(key);
                }
                if let Some(q) = state.requirements.get(&req) {
                    if let Some(cap) = &q.capability {
                        targets.insert(format!("capability:{cap}"));
                        if let Some(domain) =
                            state.capabilities.get(cap).and_then(|c| c.domain.as_ref())
                        {
                            targets.insert(format!("domain:{domain}"));
                        }
                    }
                    for c in &q.revisions.last().unwrap().consumers {
                        targets.insert(format!("consumer:{c}"));
                    }
                }
                if let Some(ac) = data["ac"].as_str() {
                    targets.insert(format!("ac:{ac}"));
                }
                targets.insert(format!("task:{}", id.rsplit('/').next().unwrap_or(&id)));
            }
            rows.push(json!({"address":address,"actor":actor,"created_at":created_at,"sort_req":req,"sort_id":id,"status":data["status"],"assignee":data["assignee"],"purpose":data["purpose"],"claimable":data["claimable"],"eligible":data.get("unclaimed_eligible").unwrap_or(&data["claimable"]),"claim_deadline_ms":data["claim"]["deadline_ms"],"targets":targets,"subjects":subjects,"data":data}));
        }
    }
    let selected = selected.map(|s| s.iter().cloned().collect::<Vec<_>>());
    client.execute("DELETE FROM joppa.task_queue WHERE workspace=$1 AND ($2::text[] IS NULL OR address=ANY($2))", &[&workspace,&selected]).await?;
    if !rows.is_empty() {
        client.execute("INSERT INTO joppa.task_queue(workspace,address,actor,created_at,sort_req,sort_id,status,assignee,purpose,claimable,eligible,claim_deadline_ms,targets,subjects,data) SELECT $1,x.address,x.actor,x.created_at,x.sort_req,x.sort_id,x.status,x.assignee,x.purpose,x.claimable,x.eligible,x.claim_deadline_ms,x.targets,x.subjects,x.data FROM jsonb_to_recordset($2::text::jsonb) x(address text,actor text,created_at bigint,sort_req text,sort_id text,status text,assignee text,purpose text,claimable boolean,eligible boolean,claim_deadline_ms bigint,targets text[],subjects text[],data jsonb)", &[&workspace,&json!(rows).to_string()]).await?;
    }
    Ok(())
}
