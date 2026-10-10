use super::*;
use std::collections::BTreeSet;

pub(super) async fn project(
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
                    crate::objects::reference_targets(state, &o.kind, &o.current().data)
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
            rows.push(json!({"address":address,"actor":actor,"created_at":created_at,"sort_req":req,"sort_id":id,"status":data["status"],"assignee":data["assignee"],"purpose":data["purpose"],"claimable":data["claimable"],"targets":targets,"subjects":subjects,"data":data}));
        }
    }
    let selected = selected.map(|s| s.iter().cloned().collect::<Vec<_>>());
    client.execute("DELETE FROM joppa.task_queue WHERE workspace=$1 AND ($2::text[] IS NULL OR address=ANY($2))", &[&workspace,&selected]).await?;
    if !rows.is_empty() {
        client.execute("INSERT INTO joppa.task_queue(workspace,address,actor,created_at,sort_req,sort_id,status,assignee,purpose,claimable,targets,subjects,data) SELECT $1,x.address,x.actor,x.created_at,x.sort_req,x.sort_id,x.status,x.assignee,x.purpose,x.claimable,x.targets,x.subjects,x.data FROM jsonb_to_recordset($2::text::jsonb) x(address text,actor text,created_at bigint,sort_req text,sort_id text,status text,assignee text,purpose text,claimable boolean,targets text[],subjects text[],data jsonb)", &[&workspace,&json!(rows).to_string()]).await?;
    }
    Ok(())
}

async fn selector(client: &impl GenericClient, workspace: &str, key: &str) -> Result<String> {
    let (kind, id) = key.split_once(':').context("priority must be kind:id")?;
    ensure!(!id.is_empty(), "priority ID is empty");
    let rows=client.query("SELECT address FROM joppa.objects WHERE workspace=$1 AND present AND (address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2) OR (kind=$3 AND regexp_replace(local_id,'^.*/','')=$4)) ORDER BY (address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2)) DESC LIMIT 2", &[&workspace,&key,&kind,&id]).await?;
    if let Some(row) = rows.first() {
        let found: String = row.get(0);
        if rows.len() == 1 || found == key || client.query_one("SELECT EXISTS(SELECT 1 FROM joppa.object_aliases WHERE workspace=$1 AND address=$2 AND target=$3)", &[&workspace,&key,&found]).await?.get::<_,bool>(0) {
            return Ok(found);
        }
    }
    anyhow::bail!("unknown or ambiguous priority target: {key}")
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn read(
    client: &impl GenericClient,
    workspace: &str,
    actor: &str,
    view: &str,
    limit: usize,
    cursor: Option<&str>,
    project: Option<&str>,
    purpose: Option<&str>,
    priorities: &[String],
) -> Result<Value> {
    let row=client.query_one("SELECT position,COALESCE(state->>'workspace_id',workspace),(first_view->'object_types')::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?;
    let position: i64 = row.get(0);
    let public: String = row.get(1);
    let mut targets = vec![];
    for priority in priorities {
        targets.push(selector(client, workspace, priority).await?);
    }
    let project = if let Some(project) = project {
        if project.contains(':') {
            Some(selector(client, workspace, project).await?)
        } else {
            let types: Value = serde_json::from_str(row.get(2))?;
            let kinds: BTreeSet<String> = types
                .as_object()
                .into_iter()
                .flat_map(|t| t.values())
                .flat_map(|t| {
                    t["references"]
                        .as_object()
                        .into_iter()
                        .flat_map(|refs| refs.values())
                })
                .filter(|r| r["verification_subject"] == true)
                .filter_map(|r| r["target"].as_str().map(str::to_owned))
                .collect();
            let kinds: Vec<String> = kinds.into_iter().collect();
            let matches=client.query("SELECT address FROM joppa.objects WHERE workspace=$1 AND local_id=$2 AND present AND (kind=ANY($3) OR '*'=ANY($3)) LIMIT 2", &[&workspace,&project,&kinds]).await?;
            ensure!(matches.len() == 1, "unknown project");
            Some(matches[0].get::<_, String>(0))
        }
    } else {
        None
    };
    ensure!(
        purpose.is_none_or(|p| !p.trim().is_empty()),
        "task purpose must be nonempty"
    );
    let offset = if let Some(cursor) = cursor {
        let (snapshot, offset) = cursor.split_once(':').context("invalid task cursor")?;
        ensure!(
            snapshot.parse::<i64>()? == position,
            "task queue changed; start again without cursor"
        );
        offset.parse::<i64>()?
    } else {
        0
    };
    ensure!(offset >= 0, "invalid task cursor offset");
    let held = client.query_opt("SELECT address FROM joppa.task_queue WHERE workspace=$1 AND actor=$2 AND assignee=$2 AND status='in_progress' AND EXISTS(SELECT 1 FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2 AND value='\"agent\"'::jsonb) ORDER BY created_at,sort_req,sort_id LIMIT 1", &[&workspace,&actor]).await?.map(|r| r.get::<_,String>(0));
    let filter = "workspace=$1 AND actor=$2 AND ($3::text IS NULL OR $3=ANY(subjects)) AND ($4::text IS NULL OR purpose=$4) AND ($5='all' OR ($5='available' AND claimable AND $6::text IS NULL) OR ($5='mine' AND assignee=$2 AND status='in_progress') OR ($5='waiting' AND status='open' AND (NOT claimable OR $6::text IS NOT NULL)))";
    let total: i64 = client
        .query_one(
            &format!("SELECT count(*) FROM joppa.task_queue WHERE {filter}"),
            &[&workspace, &actor, &project, &purpose, &view, &held],
        )
        .await?
        .get(0);
    ensure!(offset <= total, "invalid task cursor offset");
    let rows=client.query(&format!("SELECT data::text FROM joppa.task_queue q WHERE {filter} ORDER BY COALESCE((SELECT min(p.ordinality) FROM unnest($7::text[]) WITH ORDINALITY p(target,ordinality) WHERE p.target=ANY(q.targets)),cardinality($7)+1),created_at,sort_req,sort_id LIMIT $8 OFFSET $9"), &[&workspace,&actor,&project,&purpose,&view,&held,&targets,&(limit as i64),&offset]).await?;
    let mut items: Vec<Value> = rows
        .iter()
        .map(|r| serde_json::from_str(r.get(0)))
        .collect::<std::result::Result<_, _>>()?;
    if let Some(held) = held {
        for item in &mut items {
            if item["claimable"] == true {
                item["claimable"] = json!(false);
                item["waiting_reason"] = json!(format!("agent already holds task {held}"));
            }
        }
    }
    let next = (offset + (items.len() as i64) < total)
        .then(|| format!("{position}:{}", offset + items.len() as i64));
    Ok(
        json!({"workspace":public,"position":position,"view":view,"total":total,"tasks":items,"next_cursor":next}),
    )
}
