use super::*;
use std::collections::BTreeSet;

#[path = "queue_project.rs"]
mod project_rows;
pub(super) use project_rows::project;

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
    priorities: &[String],
) -> Result<Value> {
    let row=client.query_one("SELECT position,COALESCE(state->>'workspace_id',workspace),(first_view->'object_types')::text,floor(extract(epoch FROM clock_timestamp())*1000)::bigint FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?;
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
    let server_now: i64 = row.get(3);
    let (now, offset) = if let Some(cursor) = cursor {
        let parts: Vec<_> = cursor.split(':').collect();
        ensure!(parts.len() == 3, "invalid task cursor");
        ensure!(
            parts[0].parse::<i64>()? == position,
            "task queue changed; start again without cursor"
        );
        let time = parts[1].parse::<i64>()?;
        ensure!(time > 0 && time <= server_now, "invalid task cursor time");
        (time, parts[2].parse::<i64>()?)
    } else {
        (server_now, 0)
    };
    ensure!(offset >= 0, "invalid task cursor offset");
    // A Task under waiting work is not available; the wait comes from the status projection.
    let waits = super::objects::projected_waits(client, workspace).await?;
    let waiting: Vec<String> = waits.keys().cloned().collect();
    let filter = "workspace=$1 AND actor=$2 AND ($3::text IS NULL OR $3=ANY(subjects)) AND ($4='all' OR ($4='available' AND claimable AND NOT address=ANY($5)) OR ($4='mine' AND assignee=$2 AND status='in_progress') OR ($4='waiting' AND status='open' AND (NOT claimable OR address=ANY($5))))";
    let total: i64 = client
        .query_one(
            &format!("SELECT count(*) FROM joppa.task_queue WHERE {filter}"),
            &[&workspace, &actor, &project, &view, &waiting],
        )
        .await?
        .get(0);
    ensure!(offset <= total, "invalid task cursor offset");
    // Explicit priorities first, then the priority chain of each Task from the top.
    let values = super::objects::priority_values(client, workspace).await?;
    let chain = super::objects::priority_chain_sql("q.workspace", "q.address", "$9::text[]");
    let rows=client.query(&format!("SELECT data::text FROM joppa.task_queue q WHERE {filter} ORDER BY COALESCE((SELECT min(p.ordinality) FROM unnest($6::text[]) WITH ORDINALITY p(target,ordinality) WHERE p.target=ANY(q.targets)),cardinality($6)+1),{chain},created_at,sort_req,sort_id LIMIT $7 OFFSET $8"), &[&workspace,&actor,&project,&view,&waiting,&targets,&(limit as i64),&offset,&values]).await?;
    let mut items: Vec<Value> = rows
        .iter()
        .map(|r| serde_json::from_str(r.get(0)))
        .collect::<std::result::Result<_, _>>()?;
    for item in &mut items {
        item.as_object_mut().unwrap().remove("unclaimed_eligible");
        item.as_object_mut().unwrap().remove("unclaimed_error");
        let address = item["address"].as_str().map(str::to_owned).or_else(|| {
            Some(format!(
                "task:{}/{}",
                item["req"].as_str()?,
                item["id"].as_str()?
            ))
        });
        if item["claimable"] == true
            && let Some(target) = address.and_then(|a| waits.get(&a))
        {
            item["claimable"] = json!(false);
            item["waiting_reason"] = json!(crate::dependencies::reason(target));
        }
    }
    // A done Task under a criterion with a saved test list takes the criterion status as its
    // verification, as in joppa_objects. An unreadable status keeps the stored verdict.
    let parents: Vec<String> = items
        .iter()
        .filter(|i| i["status"] == "done")
        .filter_map(|i| i["parent"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !parents.is_empty() {
        let optional = parents.iter().cloned().collect();
        let statuses =
            super::reports::criterion_test_statuses(client, workspace, &parents, &optional).await?;
        for item in &mut items {
            let Some(status) = item["parent"]
                .as_str()
                .filter(|_| item["status"] == "done")
                .and_then(|p| statuses.get(p))
                .filter(|s| !s["tests_revision"].is_null())
            else {
                continue;
            };
            // After migrate_test_rules a Task has no verification.
            if item["verification"].is_null() {
                continue;
            }
            let Some(verification) = (match status["status"].as_str() {
                Some("green") => Some("pass"),
                Some("red") => Some("fail"),
                Some("grey") => Some("unverified"),
                _ => None,
            }) else {
                continue;
            };
            let process = |item: &Value| json!({"status":item["status"],"verification":item["verification"],"readiness_blocker":item["readiness_blocker"]});
            // Earlier signal rules, such as retirement or review, keep their signal.
            let rule = crate::objects::task_signal(&process(item));
            let decided = item["signal"]["tone"] == rule.0 && item["signal"]["label"] == rule.1;
            item["verification"] = json!(verification);
            if decided {
                let (tone, label) = crate::objects::task_signal(&process(item));
                item["signal"]["tone"] = json!(tone);
                item["signal"]["label"] = json!(label);
            }
        }
    }
    // A finished Task waits for nothing.
    for item in &mut items {
        if matches!(item["status"].as_str(), Some("done" | "retired")) {
            item.as_object_mut().map(|m| m.remove("waiting_reason"));
        }
    }
    let next = (offset + (items.len() as i64) < total)
        .then(|| format!("{position}:{now}:{}", offset + items.len() as i64));
    Ok(
        json!({"workspace":public,"position":position,"view":view,"total":total,"tasks":items,"next_cursor":next}),
    )
}
