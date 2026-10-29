//! Part of `store_state`: `project` and the items after it, moved from store_state.rs unchanged.
use super::*;

/// Store only changed current rows and changed verification revisions.
pub(crate) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    previous: Option<&Workspace>,
) -> Result<()> {
    let current = entries(state)?;
    let previous_entries = previous.map(entries).transpose()?.unwrap_or_default();
    let changed: Vec<Value> = current
        .iter()
        .filter(|(key, value)| previous_entries.get(*key) != Some(*value))
        .map(|((section, key), value)| json!({"section":section,"key":key,"value":value}))
        .collect();
    if !changed.is_empty() {
        client.execute("INSERT INTO joppa.state_entries(workspace,section,key,value,kind,compact) SELECT $1,x.section,x.key,COALESCE(x.value,'null'::jsonb),CASE WHEN x.section='objects' THEN x.value->>'kind' END,CASE WHEN x.section='objects' THEN jsonb_set(x.value,'{revisions,0,data}',(x.value->'revisions'->0->'data')-'body'-'description') END FROM jsonb_to_recordset($2::text::jsonb) x(section text,key text,value jsonb) ON CONFLICT(workspace,section,key) DO UPDATE SET value=EXCLUDED.value,kind=EXCLUDED.kind,compact=EXCLUDED.compact", &[&workspace,&json!(changed).to_string()]).await?;
    }
    for (section, key) in previous_entries
        .keys()
        .filter(|key| !current.contains_key(*key))
    {
        client
            .execute(
                "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section=$2 AND key=$3",
                &[&workspace, section, key],
            )
            .await?;
    }
    let mut revisions = vec![];
    for (key, q) in &state.requirements {
        for r in &q.revisions {
            let value = serde_json::to_value(r)?;
            let old = previous
                .and_then(|p| p.requirements.get(key))
                .and_then(|q| q.revisions.iter().find(|old| old.number == r.number))
                .map(serde_json::to_value)
                .transpose()?;
            if old.as_ref() != Some(&value) {
                revisions.push(
                    json!({"section":"requirements","key":key,"revision":r.number,"value":value}),
                );
            }
        }
    }
    for r in &state.tree_schema {
        let value = serde_json::to_value(r)?;
        let old = previous
            .and_then(|p| p.tree_schema.iter().find(|old| old.revision == r.revision))
            .map(serde_json::to_value)
            .transpose()?;
        if old.as_ref() != Some(&value) {
            revisions.push(
                json!({"section":"tree_schema","key":"","revision":r.revision,"value":value}),
            );
        }
    }
    if !revisions.is_empty() {
        let owners: Vec<Value> = revisions
            .iter()
            .filter(|r| r["section"] == "requirements")
            .flat_map(|r| {
                r["value"]["acs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |ac| json!({"criterion":ac["id"],"requirement":r["key"]}))
            })
            .collect();
        let owners = json!(owners).to_string();
        let conflicts = client.query_one("SELECT EXISTS(SELECT 1 FROM jsonb_to_recordset($2::text::jsonb) x(criterion text,requirement text) JOIN joppa.ac_owners a ON a.workspace=$1 AND a.criterion=x.criterion WHERE a.requirement<>x.requirement)", &[&workspace,&owners]).await?;
        ensure!(!conflicts.get::<_, bool>(0), "AC id belongs to another REQ");
        client.execute("INSERT INTO joppa.ac_owners(workspace,criterion,requirement) SELECT DISTINCT $1,x.criterion,x.requirement FROM jsonb_to_recordset($2::text::jsonb) x(criterion text,requirement text) ON CONFLICT DO NOTHING", &[&workspace,&owners]).await?;
        client.execute("INSERT INTO joppa.state_revisions(workspace,section,key,revision,value) SELECT $1,x.section,x.key,x.revision,x.value FROM jsonb_to_recordset($2::text::jsonb) x(section text,key text,revision integer,value jsonb) ON CONFLICT(workspace,section,key,revision) DO UPDATE SET value=EXCLUDED.value WHERE state_revisions.value IS DISTINCT FROM EXCLUDED.value", &[&workspace,&json!(revisions).to_string()]).await?;
    }
    Ok(())
}

pub(crate) async fn hydrate_revision(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    command: &Value,
) -> Result<()> {
    let Some(req) = command["req"].as_str() else {
        return Ok(());
    };
    let Some(q) = state.requirements.get_mut(req) else {
        return Ok(());
    };
    let requested = command["assessment"]["decision"]
        .as_str()
        .and_then(|id| q.comments.get(id))
        .map(|c| c.revision as i32)
        .or_else(|| command["revision"].as_u64().map(|n| n as i32));
    let carry_history = command["carry_tasks"] == true && !state.tasks_migrated;
    let withdraw = command["op"] == "withdraw_decision";
    if carry_history
        || withdraw
        || requested.is_some_and(|n| q.revisions.last().is_none_or(|r| r.number != n as u32))
    {
        let all = carry_history || withdraw;
        for row in client.query("SELECT value::text FROM joppa.state_revisions WHERE workspace=$1 AND section='requirements' AND key=$2 AND ($3::boolean OR revision=$4) ORDER BY revision", &[&workspace,&req,&all,&requested]).await? {
            let r: crate::model::Revision = serde_json::from_str(row.get(0))?;
            if !q.revisions.iter().any(|old| old.number == r.number) { q.revisions.push(r); }
        }
        q.revisions.sort_by_key(|r| r.number);
    }
    Ok(())
}

pub(crate) async fn read_requirement(
    client: &impl GenericClient,
    workspace: &str,
    requested: &str,
    scope: &Scope,
) -> Result<Value> {
    let address = crate::objects::requirement_address(requested);
    let row = client.query_opt("SELECT s.key,s.value::text,b.position,COALESCE(b.state->>'workspace_id',b.workspace),COALESCE(b.state->>'reference_prefix',b.workspace),(SELECT value::text FROM joppa.state_entries WHERE workspace=s.workspace AND section='summaries' AND key=s.key),b.launch::text FROM joppa.state_entries s JOIN joppa.board_cache b ON b.workspace=s.workspace WHERE s.workspace=$1 AND s.section='requirements' AND s.key IN ($2,$3) ORDER BY (s.key=$2) DESC LIMIT 1", &[&workspace,&requested,&address]).await?.with_context(|| format!("unknown requirement {requested}"))?;
    let req: String = row.get(0);
    let q: crate::model::Requirement = serde_json::from_str(row.get(1))?;
    let current = q.revisions.last().context("requirement has no revisions")?;
    let mut out = json!({"workspace":row.get::<_,String>(3),"position":row.get::<_,i64>(2),"launch":serde_json::from_str::<Value>(row.get(6))?,"requirement":{"id":req,"capability":q.capability,"revision_count":current.number,"current_revision":current.number}});
    if scope.history && scope.revision.is_none() {
        return Ok(out);
    }
    let revision: crate::model::Revision = if scope.revision.is_none_or(|n| n == current.number) {
        current.clone()
    } else {
        let n = scope.revision.unwrap() as i32;
        serde_json::from_str(client.query_opt("SELECT value::text FROM joppa.state_revisions WHERE workspace=$1 AND section='requirements' AND key=$2 AND revision=$3", &[&workspace,&req,&n]).await?.context("unknown requirement revision")?.get(0))?
    };
    if revision.acs.is_empty() {
        out["status"] = json!("no criteria");
        out["verification"] = json!("unverified");
    }
    out["requirement"]["capability"] =
        json!(revision.capability.as_deref().or(q.capability.as_deref()));
    out["requirement"]["revisions"] = json!([revision]);
    let comments: serde_json::Map<String, Value> = q
        .comments
        .iter()
        .filter(|(_, c)| c.revision == revision.number)
        .map(|(id, c)| (id.clone(), json!(c)))
        .collect();
    out["requirement"]["comments"] = json!(comments);
    let mut addresses = vec![crate::objects::requirement_address(&req)];
    for ac in &revision.acs {
        addresses.push(crate::objects::criterion_address(&ac.id));
        if !ac.id.contains(':') {
            addresses.push(format!("ac:{req}/{}", ac.id));
        }
    }
    mark_present(client, workspace, &addresses, &revision, &mut out).await?;
    let mut notes = json!({});
    for row in client.query("SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='notes' AND key=ANY($2)", &[&workspace,&addresses]).await? {
        notes[row.get::<_,String>(0)] = serde_json::from_str(row.get(1))?;
    }
    out["notes"] = notes;
    out["summary"] = if revision.number == current.number {
        serde_json::from_str::<Value>(row.get(5))?
    } else {
        checks::summary(&revision)
    };
    if scope.revision.is_none() {
        let mut tasks = revision.tasks.clone();
        for row in client.query("SELECT p.key,p.value::text FROM joppa.state_entries p JOIN joppa.state_entries c ON c.workspace=p.workspace AND c.section='task_context' AND c.key=p.key WHERE p.workspace=$1 AND p.section='task_process' AND c.value->>0=$2", &[&workspace,&req]).await? {
            tasks.insert(row.get(0),serde_json::from_str(row.get(1))?);
        }
        out["requirement"]["revisions"][0]["tasks"] = json!(tasks);
    }
    let prefix: String = row.get(4);
    let mut ids = json!({});
    for row in client.query("SELECT kind,local_id,id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2 AND kind||':'||local_id=ANY($3)", &[&workspace,&prefix,&addresses]).await? {
        ids[row.get::<_,String>(0)][row.get::<_,String>(1)] = json!(row.get::<_,String>(2));
    }
    out["system_ids"] = ids;
    Ok(out)
}

/// An archived requirement or criterion stays in the requirement record, so a read
/// marks each one with `present` from the object tree, and an archived requirement
/// with `archived: true`.
pub(crate) async fn mark_present(
    client: &impl GenericClient,
    workspace: &str,
    addresses: &[String],
    revision: &crate::model::Revision,
    out: &mut Value,
) -> Result<()> {
    let present: std::collections::HashMap<String, bool> = client
        .query(
            "SELECT address,present FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
            &[&workspace, &addresses],
        )
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    let req_present = present.get(&addresses[0]).copied().unwrap_or(true);
    out["requirement"]["present"] = json!(req_present);
    if !req_present {
        out["requirement"]["archived"] = json!(true);
    }
    for (i, ac) in revision.acs.iter().enumerate() {
        let known = [
            crate::objects::criterion_address(&ac.id),
            format!(
                "ac:{}/{}",
                addresses[0].trim_start_matches("requirement:"),
                ac.id
            ),
        ];
        let live = known
            .iter()
            .find_map(|a| present.get(a).copied())
            .unwrap_or(true);
        out["requirement"]["revisions"][0]["acs"][i]["present"] = json!(live);
    }
    Ok(())
}
