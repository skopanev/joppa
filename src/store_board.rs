use super::*;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;

fn stale_result(
    events: &[Event],
    req: &str,
    revision: u32,
    run: &str,
    check: &str,
) -> Option<DateTime<Utc>> {
    let at = events.iter().rev().find(|event| matches!(&event.command,
        Command::Result { req: event_req, revision: event_revision, run: event_run, evidence }
        if event_req == req && *event_revision == revision && event_run == run && evidence.check == check));
    let at = at?;
    let mut date = at.recorded_at.replace(' ', "T");
    if date.len() >= 3
        && (date.as_bytes()[date.len() - 3] == b'+' || date.as_bytes()[date.len() - 3] == b'-')
    {
        date.push_str(":00");
    }
    DateTime::parse_from_rfc3339(&date)
        .ok()
        .map(|at| at.with_timezone(&Utc) + Duration::days(7))
}

fn stale_requirement(
    id: &str,
    q: &crate::model::Requirement,
    summary: &Value,
    events: &[Event],
) -> Option<DateTime<Utc>> {
    let r = q.revisions.last().expect("requirement has a revision");
    let Some((run_id, run)) = r
        .runs
        .iter()
        .filter(|(_, run)| run.finished.is_some())
        .max_by_key(|(_, run)| run.started)
    else {
        return None;
    };
    r.acs
        .iter()
        .filter(|ac| summary["acs"][&ac.id]["verification"] == "pass")
        .flat_map(|ac| {
            run.plan
                .iter()
                .filter(move |check| check.ac == ac.id && run.results.contains_key(&check.id))
        })
        .filter_map(|check| stale_result(events, id, r.number, run_id, &check.id))
        .min()
}

fn event_time(events: &[Event], req: &str, revision: u32) -> Option<String> {
    events.iter().rev().find_map(|event| {
        let command = serde_json::to_value(&event.command).ok()?;
        let relevant = matches!(
            command["op"].as_str().unwrap_or(""),
            "revise"
                | "assess"
                | "confirm"
                | "task"
                | "review_task"
                | "retire_task"
                | "complete_task"
                | "define_verification_scope"
                | "plan"
                | "review_coverage"
                | "result"
                | "finish"
                | "issue"
                | "resolve"
                | "withdraw_decision"
                | "accept"
                | "revoke"
        );
        if relevant
            && command["req"] == req
            && (command["revision"] == revision
                || command["op"] == "revise" && command["expected_revision"] == revision - 1)
        {
            Some(event.recorded_at.clone())
        } else {
            None
        }
    })
}

fn requirement_signal(id: &str, q: &crate::model::Requirement, events: &[Event]) -> Value {
    let r = q.revisions.last().expect("requirement has a revision");
    let s = checks::summary(r);
    let acs = s["acs"].as_object();
    let failed = acs.is_some_and(|a| a.values().any(|v| v["verification"] == "fail"));
    let unready = acs.is_some_and(|a| a.values().any(|v| v["ready"] != true));
    let stale_after = stale_requirement(id, q, &s, events);
    let stale = stale_after
        .as_ref()
        .is_some_and(|deadline| *deadline <= Utc::now());
    let (tone, label, status) = if r.acs.is_empty() {
        ("grey", "No acceptance criteria", "waiting")
    } else if failed {
        ("red", "Checks failed", "fail")
    } else if stale {
        ("orange", "Checks stale", "pass")
    } else if s["ready_for_acceptance"] == true {
        ("green", "All ACs green", "pass")
    } else if !r.confirmed || unready {
        ("yellow", "Waiting for decision", "waiting")
    } else if r.tasks.values().any(|task| task.review_required) {
        ("purple", "Task review required", "review")
    } else if !r.runs.is_empty()
        || r.tasks
            .values()
            .any(|t| t.retired.is_none() && t.purpose != "decision")
    {
        ("yellow", "In progress", "progress")
    } else {
        ("grey", "Ready to work", "available")
    };
    json!({"tone":tone,"label":label,"status":status,"at":event_time(events,id,r.number),"stale_after":stale_after.map(|at|at.to_rfc3339())})
}

fn aggregate(children: impl Iterator<Item = Value>, noun: &str) -> Value {
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut latest: Option<String> = None;
    let mut stale_after: Option<String> = None;
    let mut count = 0;
    for child in children {
        count += 1;
        let tone = match child["tone"].as_str().unwrap_or("grey") {
            "red" => "red",
            "orange" => "orange",
            "yellow" => "yellow",
            "purple" => "purple",
            "green" => "green",
            _ => "grey",
        };
        *counts.entry(tone).or_default() += 1;
        if let Some(at) = child["at"].as_str() {
            if latest.as_deref().is_none_or(|old| at > old) {
                latest = Some(at.to_owned());
            }
        }
        if let Some(deadline) = child["stale_after"].as_str() {
            if stale_after.as_deref().is_none_or(|old| deadline < old) {
                stale_after = Some(deadline.to_owned());
            }
        }
    }
    if count == 0 {
        return json!({"tone":"grey","label":format!("No {noun}"),"at":null});
    }
    let tone = ["red", "orange", "purple", "yellow", "grey", "green"]
        .into_iter()
        .find(|tone| counts.get(tone).copied().unwrap_or(0) > 0)
        .unwrap_or("grey");
    let parts: Vec<_> = [
        ("green", "complete"),
        ("orange", "stale"),
        ("yellow", "pending"),
        ("purple", "needs review"),
        ("red", "failed"),
        ("grey", "unchecked or empty"),
    ]
    .into_iter()
    .filter_map(|(tone, label)| {
        counts
            .get(tone)
            .filter(|n| **n > 0)
            .map(|n| format!("{n} {label}"))
    })
    .collect();
    json!({"tone":tone,"label":format!("Current {noun}: {}",parts.join(" · ")),"at":latest,"stale_after":stale_after})
}

pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    events: &[Event],
) -> Result<()> {
    let mut reqs = serde_json::Map::new();
    let mut caps = serde_json::Map::new();
    let mut domains = serde_json::Map::new();
    for (id, q) in &state.requirements {
        reqs.insert(id.clone(), requirement_signal(id, q, events));
    }
    for (id, _) in &state.capabilities {
        caps.insert(
            id.clone(),
            aggregate(
                state
                    .requirements
                    .iter()
                    .filter(|(_, q)| q.capability == *id)
                    .filter_map(|(rid, _)| reqs.get(rid).cloned()),
                "requirements",
            ),
        );
    }
    for (id, _) in &state.domains {
        domains.insert(
            id.clone(),
            aggregate(
                state
                    .capabilities
                    .iter()
                    .filter(|(_, c)| c.domain == *id)
                    .filter_map(|(cid, _)| caps.get(cid).cloned()),
                "capabilities",
            ),
        );
    }
    let summary = summaries(state);
    let configured = state.requirements.values().any(|q| {
        q.revisions
            .last()
            .is_some_and(|r| r.acs.iter().any(|a| a.before_launch))
    });
    let mut blockers = vec![];
    for (id, q) in &state.requirements {
        let r = q.revisions.last().expect("requirement has a revision");
        if summary[id]["accepted"] != true {
            for a in r.acs.iter().filter(|a| a.before_launch) {
                blockers.push(json!({"req":id,"revision":r.number,"ac":a.id}));
            }
        }
    }
    let blocker_count = blockers.len();
    let launch = json!({"ready":configured && blockers.is_empty(),"configured":configured,"blockers":blockers});
    let first_view = json!({"title":&state.title,"members":&state.members,"domains":&state.domains,
        "order":state.tree_order.get("domain").cloned().unwrap_or_default(),"signals":&domains,
        "launch":{"ready":launch["ready"],"configured":configured,"blocker_count":blocker_count},"workspace_id":state.workspace_id.as_deref().unwrap_or(workspace),
        "counts":{"consumers":state.consumers.len(),"intake":state.intake.len()}});
    client.execute("INSERT INTO joppa.board_cache(workspace,position,state,summary,signals,launch,first_view) VALUES($1,$2,$3::text::jsonb,$4::text::jsonb,$5::text::jsonb,$6::text::jsonb,$7::text::jsonb) ON CONFLICT (workspace) DO UPDATE SET position=EXCLUDED.position,state=EXCLUDED.state,summary=EXCLUDED.summary,signals=EXCLUDED.signals,launch=EXCLUDED.launch,first_view=EXCLUDED.first_view",
        &[&workspace,&(events.len() as i64),&serde_json::to_string(state)?,&summary.to_string(),&json!({"requirements":reqs,"capabilities":caps,"domains":domains}).to_string(),&launch.to_string(),&first_view.to_string()]).await?;
    Ok(())
}

fn parsed(row: &tokio_postgres::Row, index: usize) -> Result<Value> {
    Ok(serde_json::from_str(row.get::<_, &str>(index))?)
}

async fn ids(
    client: &impl GenericClient,
    workspace: &str,
    kind: &str,
    local: &[String],
) -> Result<Value> {
    let mut out = json!({});
    for row in client.query("SELECT local_id,id FROM joppa.item_ids WHERE workspace=$1 AND namespace=(SELECT COALESCE(state->>'reference_prefix',workspace) FROM joppa.board_cache WHERE workspace=$1) AND kind=$2 AND local_id=ANY($3)", &[&workspace,&kind,&local]).await? {
        let key: String = row.get(0);
        out[&key] = json!(row.get::<_, String>(1));
    }
    Ok(out)
}

pub async fn board_read(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    level: Option<&str>,
    id: Option<&str>,
    revision: Option<u32>,
) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    match level {
        None => {
            let row = conn
                .query_one(
                    "SELECT position,first_view::text FROM joppa.board_cache WHERE workspace=$1",
                    &[&workspace],
                )
                .await?;
            let first = parsed(&row, 1)?;
            let domains = &first["domains"];
            let members = &first["members"];
            let domain_keys: Vec<String> = domains
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys().cloned())
                .collect();
            let humans: Vec<String> = members
                .as_object()
                .into_iter()
                .flat_map(|o| o.iter())
                .filter(|(_, role)| role.as_str() == Some("human"))
                .map(|(id, _)| id.clone())
                .collect();
            let mut labels = json!({});
            for label in conn.query("SELECT actor,split_part(email,'@',1) FROM joppa.google_identities WHERE actor=ANY($1)",&[&humans]).await? {
                let actor:String=label.get(0); labels[&actor]=json!(label.get::<_,String>(1));
            }
            Ok(
                json!({"workspace":first["workspace_id"],"position":row.get::<_,i64>(0),"lazy":true,
                "state":{"title":first["title"],"members":members,"domains":domains,"tree_order":{"domain":first["order"]},"capabilities":{},"requirements":{},"consumers":{},"intake":{},"notes":{},"projects":{}},
                "signals":{"domains":first["signals"],"capabilities":{},"requirements":{}},"summary":{},"history":[],"member_labels":labels,
                "launch":first["launch"],"system_ids":{"domain":ids(&**conn,&workspace,"domain",&domain_keys).await?,"capability":{},"requirement":{},"ac":{},"consumer":{}},
                "counts":first["counts"]}),
            )
        }
        Some("domain") => {
            let id = id.context("domain id required")?;
            let row=conn.query_one("SELECT COALESCE((SELECT jsonb_object_agg(key,value) FROM jsonb_each(state->'capabilities') WHERE value->>'domain'=$2),'{}'::jsonb)::text, COALESCE(signals->'capabilities','{}'::jsonb)::text, COALESCE(state->'tree_order'->($3 || $2),'[]'::jsonb)::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace,&id,&"capability:"]).await?;
            let caps = parsed(&row, 0)?;
            let keys: Vec<String> = caps
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys().cloned())
                .collect();
            let all = parsed(&row, 1)?;
            let signals: serde_json::Map<_, _> = keys
                .iter()
                .filter_map(|k| all.get(k).cloned().map(|v| (k.clone(), v)))
                .collect();
            Ok(
                json!({"capabilities":caps,"signals":signals,"order":parsed(&row,2)?,"system_ids":ids(&**conn,&workspace,"capability",&keys).await?}),
            )
        }
        Some("capability") => {
            let id = id.context("capability id required")?;
            let row=conn.query_one("SELECT COALESCE((SELECT jsonb_object_agg(key,jsonb_build_object('capability',value->'capability','revisions',jsonb_build_array(jsonb_build_object('number',value->'revisions'->-1->'number','title',value->'revisions'->-1->'title','body',value->'revisions'->-1->'body')),'revision_count',jsonb_array_length(value->'revisions'))) FROM jsonb_each(state->'requirements') WHERE value->>'capability'=$2),'{}'::jsonb)::text, COALESCE(signals->'requirements','{}'::jsonb)::text, COALESCE(state->'tree_order'->($3 || $2),'[]'::jsonb)::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace,&id,&"requirement:"]).await?;
            let reqs = parsed(&row, 0)?;
            let keys: Vec<String> = reqs
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys().cloned())
                .collect();
            let all = parsed(&row, 1)?;
            let signals: serde_json::Map<_, _> = keys
                .iter()
                .filter_map(|k| all.get(k).cloned().map(|v| (k.clone(), v)))
                .collect();
            Ok(
                json!({"requirements":reqs,"signals":signals,"order":parsed(&row,2)?,"system_ids":ids(&**conn,&workspace,"requirement",&keys).await?}),
            )
        }
        Some("requirement") => {
            let id = id.context("requirement id required")?;
            let row=conn.query_one("SELECT jsonb_build_object('capability',state->'requirements'->$2->'capability','revisions',jsonb_build_array(state->'requirements'->$2->'revisions'->COALESCE($3::integer-1,-1)),'comments',state->'requirements'->$2->'comments')::text, (summary->$2)::text, (state->'consumers')::text, COALESCE((SELECT jsonb_object_agg(key,value) FROM jsonb_each(state->'notes') WHERE key='requirement:'||$2 OR key LIKE 'ac:'||$2||'/%'),'{}'::jsonb)::text, (state->'projects')::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace,&id,&revision.map(|v|v as i32)]).await?;
            let q = parsed(&row, 0)?;
            ensure!(
                !q["revisions"][0].is_null(),
                "unknown requirement or revision"
            );
            let number = q["revisions"][0]["number"]
                .as_u64()
                .context("invalid revision")? as i32;
            let mut events = vec![];
            for event in conn.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 AND command->>'req'=$2 AND ((command->>'revision')::integer=$3 OR command->>'op'='revise' AND (command->>'expected_revision')::integer=$3-1) ORDER BY position", &[&workspace,&id,&number]).await? {
                events.push(json!({"position":event.get::<_,i64>(0),"actor":event.get::<_,String>(1),"recorded_at":event.get::<_,String>(2),"command":serde_json::from_str::<Value>(event.get::<_,&str>(3))?}));
            }
            let ac_ids: Vec<String> = q["revisions"][0]["acs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["id"].as_str().map(str::to_owned))
                .collect();
            Ok(
                json!({"requirement":q,"summary":parsed(&row,1)?,"consumers":parsed(&row,2)?,"notes":parsed(&row,3)?,"projects":parsed(&row,4)?,"status_events":events,"system_ids":{"ac":ids(&**conn,&workspace,"ac",&ac_ids).await?}}),
            )
        }
        Some("consumers") => {
            let row = conn
                .query_one(
                    "SELECT (state->'consumers')::text FROM joppa.board_cache WHERE workspace=$1",
                    &[&workspace],
                )
                .await?;
            let consumers = parsed(&row, 0)?;
            let keys: Vec<String> = consumers
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys().cloned())
                .collect();
            Ok(
                json!({"consumers":consumers,"system_ids":ids(&**conn,&workspace,"consumer",&keys).await?}),
            )
        }
        Some("intake") => {
            let row = conn
                .query_one(
                    "SELECT (state->'intake')::text FROM joppa.board_cache WHERE workspace=$1",
                    &[&workspace],
                )
                .await?;
            Ok(json!({"intake":parsed(&row,0)?}))
        }
        Some("filter") => {
            let row=conn.query_one("SELECT (state->'capabilities')::text, COALESCE((SELECT jsonb_object_agg(key,jsonb_build_object('capability',value->'capability','revisions',jsonb_build_array(value->'revisions'->-1),'revision_count',jsonb_array_length(value->'revisions'))) FROM jsonb_each(state->'requirements')),'{}'::jsonb)::text,summary::text,signals::text,(state->'tree_order')::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?;
            let caps = parsed(&row, 0)?;
            let reqs = parsed(&row, 1)?;
            let cap_ids: Vec<String> = caps
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys().cloned())
                .collect();
            let req_ids: Vec<String> = reqs
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys().cloned())
                .collect();
            Ok(
                json!({"capabilities":caps,"requirements":reqs,"summary":parsed(&row,2)?,"signals":parsed(&row,3)?,"tree_order":parsed(&row,4)?,
                "system_ids":{"capability":ids(&**conn,&workspace,"capability",&cap_ids).await?,"requirement":ids(&**conn,&workspace,"requirement",&req_ids).await?}}),
            )
        }
        Some("notes") => {
            let target = id.context("notes target required")?;
            let row=conn.query_one(r#"SELECT COALESCE(state->'notes'->$2,'{"description":"","comments":{},"attachments":{}}'::jsonb)::text FROM joppa.board_cache WHERE workspace=$1"#, &[&workspace,&target]).await?;
            Ok(json!({"target":target,"notes":parsed(&row,0)?}))
        }
        Some("history") => {
            let mut events = vec![];
            for event in conn.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position", &[&workspace]).await? {
                events.push(json!({"position":event.get::<_,i64>(0),"actor":event.get::<_,String>(1),"recorded_at":event.get::<_,String>(2),"command":serde_json::from_str::<Value>(event.get::<_,&str>(3))?}));
            }
            let requirement = if let Some(id) = id {
                conn.query_opt("SELECT (state->'requirements'->$2)::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace,&id]).await?
                    .and_then(|r|r.get::<_,Option<String>>(0)).map(|v|serde_json::from_str::<Value>(&v)).transpose()?
            } else {
                None
            };
            Ok(json!({"history":events,"requirement":requirement}))
        }
        Some(other) => anyhow::bail!("unknown board level {other}"),
    }
}
