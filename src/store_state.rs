use super::*;
use std::collections::BTreeMap;

type Entries = BTreeMap<(String, String), Value>;

pub(super) fn changed_summaries(
    state: &Workspace,
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .filter(|(req, q)| {
            selected.is_none_or(|s| {
                s.contains(&crate::objects::requirement_address(req))
                    || q.revisions.last().is_some_and(|r| {
                        r.acs
                            .iter()
                            .any(|ac| s.contains(&crate::objects::criterion_address(&ac.id)))
                    })
            })
        })
        .map(|(req, q)| {
            (
                req.clone(),
                crate::objects::requirement_summary(state, req, q.revisions.last().unwrap()),
            )
        })
        .collect();
    Value::Object(summaries)
}

pub(super) fn affected(
    state: &Workspace,
    previous: Option<&Workspace>,
) -> Result<Option<std::collections::BTreeSet<String>>> {
    use std::collections::{BTreeSet, VecDeque};
    let Some(previous) = previous else {
        return Ok(None);
    };
    if serde_json::to_value(crate::objects::schema(state))?
        != serde_json::to_value(crate::objects::schema(previous))?
        || state.members != previous.members
        || state.admin != previous.admin
    {
        return Ok(None);
    }
    let mut changed = BTreeSet::new();
    for (key, o) in &state.objects {
        if previous.objects.get(key).is_none_or(|old| {
            serde_json::to_value(old.current()).ok() != serde_json::to_value(o.current()).ok()
        }) {
            changed.insert(key.clone());
        }
    }
    for (req, q) in &state.requirements {
        if serde_json::to_value(q)?
            != previous
                .requirements
                .get(req)
                .map(serde_json::to_value)
                .transpose()?
                .unwrap_or(Value::Null)
        {
            changed.insert(crate::objects::requirement_address(req));
            for w in [previous, state] {
                for key in w.objects.keys() {
                    if crate::objects::verification_context(w, key)
                        .is_some_and(|(context, _)| context == req)
                    {
                        changed.insert(key.clone());
                    }
                }
            }
        }
    }
    for (key, acceptances) in &state.acceptances {
        if previous.acceptances.get(key) != Some(acceptances) {
            changed.insert(key.clone());
        }
    }
    for (key, t) in &state.task_process {
        if Some(serde_json::to_value(t)?)
            != previous
                .task_process
                .get(key)
                .map(serde_json::to_value)
                .transpose()?
        {
            changed.insert(key.clone());
        }
    }
    for key in state
        .planning_claims
        .keys()
        .chain(previous.planning_claims.keys())
    {
        if state.planning_claims.get(key) != previous.planning_claims.get(key) {
            changed.insert(key.clone());
        }
    }
    for (key, notes) in &state.notes {
        if Some(serde_json::to_value(notes)?)
            != previous
                .notes
                .get(key)
                .map(serde_json::to_value)
                .transpose()?
        {
            changed.insert(crate::objects::resolve_address(state, key).to_owned());
        }
    }
    let mut incoming: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for w in [previous, state] {
        for (key, o) in &w.objects {
            let r = o.current();
            let mut targets: Vec<String> = r
                .parent
                .iter()
                .cloned()
                .chain(
                    r.links
                        .iter()
                        .filter(|l| l.relation == "depends_on")
                        .map(|l| l.target.clone()),
                )
                .collect();
            if let Some(deps) = r.data["depends_on"].as_array() {
                targets.extend(
                    deps.iter()
                        .filter_map(|v| v.as_str())
                        .map(crate::objects::criterion_address),
                );
            }
            for target in targets {
                incoming
                    .entry(crate::objects::resolve_address(w, &target).to_owned())
                    .or_default()
                    .insert(key.clone());
            }
        }
    }
    let mut queue: VecDeque<String> = changed.iter().cloned().collect();
    while let Some(key) = queue.pop_front() {
        for source in incoming.get(&key).into_iter().flatten() {
            if changed.insert(source.clone()) {
                queue.push_back(source.clone());
            }
        }
    }
    Ok(Some(changed))
}

fn entries(state: &Workspace) -> Result<Entries> {
    let value = serde_json::to_value(state)?;
    let mut entries = Entries::new();
    let mut meta = json!({});
    for (section, value) in value.as_object().unwrap() {
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                let mut value = value.clone();
                if ["objects", "requirements"].contains(&section.as_str()) {
                    value["revisions"] = json!([value["revisions"]
                        .as_array()
                        .and_then(|r| r.last())
                        .context("state entry has no revision")?]);
                }
                entries.insert((section.clone(), key.clone()), value);
            }
        } else if section == "tree_schema" {
            meta[section] = json!(
                value
                    .as_array()
                    .unwrap()
                    .last()
                    .into_iter()
                    .collect::<Vec<_>>()
            );
        } else {
            meta[section] = value.clone();
        }
    }
    entries.insert(("meta".into(), String::new()), meta);
    Ok(entries)
}

pub(super) fn metadata(state: &Workspace) -> Result<String> {
    Ok(entries(state)?
        .remove(&("meta".into(), String::new()))
        .unwrap()
        .to_string())
}

/// Read current rows. Historical revisions and journal payloads stay in PostgreSQL.
pub(super) async fn current(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, u64)> {
    let position = client
        .query_opt(
            "SELECT position FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let Some(position) = position else {
        ensure!(
            !client
                .query_one(
                    "SELECT EXISTS(SELECT 1 FROM joppa.workspaces WHERE id=$1)",
                    &[&workspace]
                )
                .await?
                .get::<_, bool>(0),
            "current projection is missing; run backfill-board"
        );
        return Ok((Workspace::default(), 0));
    };
    let mut value = serde_json::to_value(Workspace::default())?;
    let mut unloaded = std::collections::BTreeSet::new();
    for row in client.query("SELECT s.section,s.key,(CASE WHEN s.section='objects' AND b.first_view->'object_types'->s.kind->>'inbox'='true' AND b.first_view->'object_types'->s.kind->>'role' IN ('group','document') THEN COALESCE(s.compact,s.value) ELSE s.value END)::text,(s.section='objects' AND b.first_view->'object_types'->s.kind->>'inbox'='true' AND b.first_view->'object_types'->s.kind->>'role' IN ('group','document')) FROM joppa.state_entries s JOIN joppa.board_cache b ON b.workspace=s.workspace WHERE s.workspace=$1 ORDER BY s.section,s.key", &[&workspace]).await? {
        let section: String = row.get(0);
        let key: String = row.get(1);
        let entry: Value = serde_json::from_str(row.get(2))?;
        if row.get::<_,Option<bool>>(3) == Some(true) { unloaded.insert(key.clone()); }
        if section == "meta" {
            for (key, value_entry) in entry.as_object().unwrap() {
                value[key] = value_entry.clone();
            }
        } else {
            if !value[&section].is_object() { value[&section] = json!({}); }
            value[&section][key] = entry;
        }
    }
    let mut state: Workspace = serde_json::from_value(value)?;
    state.unloaded_objects = unloaded;
    ensure!(
        !state.admin.is_empty(),
        "current projection is missing; run backfill-board"
    );
    Ok((state, position.get::<_, i64>(0) as u64))
}

pub(super) async fn hydrate_objects(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    addresses: &[String],
) -> Result<()> {
    let addresses: Vec<String> = addresses
        .iter()
        .filter(|key| state.unloaded_objects.contains(*key))
        .cloned()
        .collect();
    if addresses.is_empty() {
        return Ok(());
    }
    for row in client.query("SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key=ANY($2)", &[&workspace,&addresses]).await? {
        let key: String=row.get(0);
        state.objects.insert(key.clone(),serde_json::from_str(row.get(1))?);
        state.unloaded_objects.remove(&key);
    }
    Ok(())
}

pub(super) async fn hydrate_command(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    previous: &mut Workspace,
    command: &Value,
) -> Result<()> {
    let mut addresses = vec![];
    if command["op"] == "set_tree_schema" {
        addresses.extend(state.unloaded_objects.iter().cloned());
    }
    if let (Some(kind), Some(id)) = (command["kind"].as_str(), command["id"].as_str()) {
        addresses.push(format!("{kind}:{id}"));
    }
    if matches!(
        command["op"].as_str(),
        Some(
            "claim_task"
                | "release_task"
                | "block_task"
                | "unblock_task"
                | "complete_task"
                | "review_task"
                | "retire_task"
                | "amend_task"
        )
    ) && let Some(id) = command["id"].as_str()
    {
        addresses.push(crate::objects::resolve_address(state, id).to_owned());
    }
    if matches!(
        command["op"].as_str(),
        Some("claim_planning" | "finish_planning" | "release_planning")
    ) && let Some(ac) = command["ac"].as_str()
    {
        let ac = crate::objects::resolve_address(state, ac).to_owned();
        addresses.push(ac.clone());
        if command["op"] == "release_planning" {
            addresses.extend(
                state
                    .task_process
                    .keys()
                    .filter(|key| {
                        state
                            .objects
                            .get(*key)
                            .is_some_and(|o| o.current().parent.as_deref() == Some(ac.as_str()))
                    })
                    .cloned(),
            );
        }
    }
    if let Some(target) = command["target"].as_str() {
        addresses.push(crate::objects::resolve_address(state, target).to_owned());
    }
    if command["op"] == "finish_planning"
        && let Some(ac) = command["ac"].as_str()
    {
        let ac = crate::objects::resolve_address(state, ac);
        if let Some(claim) = state.planning_claims.get(ac) {
            let keys: Vec<String> = state
                .task_process
                .keys()
                .filter(|key| {
                    state.objects.get(*key).is_some_and(|o| {
                        o.current()
                            .parent
                            .as_deref()
                            .map(|p| crate::objects::resolve_address(state, p))
                            == Some(ac)
                    })
                })
                .cloned()
                .collect();
            // A blocked Task also needs its document revision at the block position.
            let (blocked, positions): (Vec<String>, Vec<i64>) = keys
                .iter()
                .filter_map(|key| {
                    let block = state.task_process[key].blocked.as_ref()?;
                    Some((key.clone(), block.position as i64))
                })
                .unzip();
            for row in client.query("SELECT address,document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=ANY($2) AND ((document->>'position')::bigint>$3 OR (address,(document->>'position')::bigint) IN (SELECT * FROM unnest($4::text[],$5::bigint[]))) ORDER BY address,revision", &[&workspace, &keys, &(claim.position as i64), &blocked, &positions]).await? {
                    let key: String = row.get(0);
                    let revision: crate::objects::ObjectRevision = serde_json::from_str(row.get(1))?;
                    let object = state.objects.get_mut(&key).context("unknown Task")?;
                    if !object.revisions.iter().any(|r| r.revision == revision.revision) { object.revisions.push(revision); }
                    object.revisions.sort_by_key(|r| r.revision);
                }
        }
    }
    hydrate_objects(client, workspace, state, &addresses).await?;
    hydrate_objects(client, workspace, previous, &addresses).await?;
    Ok(())
}

/// Store only changed current rows and changed verification revisions.
pub(super) async fn project(
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

pub(super) async fn hydrate_revision(
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

pub(super) async fn read_requirement(
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

/// Explicit full snapshots include history without executing journal commands.
pub(super) async fn full(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, Vec<Event>)> {
    let (mut state, _) = current(client, workspace).await?;
    let unloaded: Vec<String> = state.unloaded_objects.iter().cloned().collect();
    hydrate_objects(client, workspace, &mut state, &unloaded).await?;
    for row in client.query("SELECT section,key,value::text FROM joppa.state_revisions WHERE workspace=$1 ORDER BY section,key,revision", &[&workspace]).await? {
        let section: String = row.get(0);
        let key: String = row.get(1);
        match section.as_str() {
            "requirements" => {
                let r: crate::model::Revision = serde_json::from_str(row.get(2))?;
                if let Some(q) = state.requirements.get_mut(&key) {
                    q.revisions.retain(|old| old.number != r.number);
                    q.revisions.push(r);
                    q.revisions.sort_by_key(|r| r.number);
                }
            }
            "tree_schema" => {
                let r: crate::objects::SchemaRevision = serde_json::from_str(row.get(2))?;
                state.tree_schema.retain(|old| old.revision != r.revision);
                state.tree_schema.push(r);
                state.tree_schema.sort_by_key(|r| r.revision);
            }
            _ => {}
        }
    }
    for row in client.query("SELECT address,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision", &[&workspace]).await? {
        let key: String = row.get(0);
        let r: crate::objects::ObjectRevision = serde_json::from_str(row.get(1))?;
        if let Some(o) = state.objects.get_mut(&key) {
            o.revisions.retain(|old| old.revision != r.revision);
            o.revisions.push(r);
            o.revisions.sort_by_key(|r| r.revision);
        }
    }
    let mut events = vec![];
    for row in client.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position", &[&workspace]).await? {
        let actor: String = row.get(1);
        let actor_type = crate::inbox::actor_type(&state, &actor).to_owned();
        events.push(Event{position:row.get::<_,i64>(0) as u64,actor,actor_type,recorded_at:row.get(2),command:serde_json::from_str(row.get(3))?});
    }
    Ok((state, events))
}
