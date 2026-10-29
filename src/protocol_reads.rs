//! The compact MCP reads of joppa_objects and joppa_tests (requirement:read-contract).
use anyhow::{Context, ensure};
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, read_shapes, store};
use serde_json::{Value, json};

/// The most criteria that one joppa_tests call reads.
const TESTS_MAX: usize = 100;

pub async fn compact(pool: &Pool, actor: &str, name: &str, a: &Value) -> anyhow::Result<Value> {
    if name == "joppa_objects" {
        objects(pool, actor, a).await
    } else {
        tests(pool, actor, a).await
    }
}

fn number(a: &Value, key: &str) -> anyhow::Result<Option<u64>> {
    a.get(key)
        .map(|v| v.as_u64().with_context(|| format!("invalid {key}")))
        .transpose()
}

async fn objects(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
    let workspace = a["workspace"].as_str().context("missing workspace")?;
    let fields = read_shapes::fields_argument(&a["fields"])?;
    if let Some(fields) = &fields {
        rc::check_fields(fields)?;
    }
    let revision = number(a, "revision")?
        .map(u32::try_from)
        .transpose()
        .context("invalid object revision")?;
    let limit = number(a, "limit")?.map_or(rc::PAGE, |n| (n as usize).min(rc::PAGE));
    if let Some(address) = a["links_of"].as_str() {
        let text = |name: &str| {
            a[name]
                .as_str()
                .with_context(|| format!("links_of needs {name}"))
        };
        let reply = store::link_page(
            pool,
            workspace,
            actor,
            address,
            text("direction")?,
            text("relation")?,
            text("kind")?,
            limit,
            a["cursor"].as_str(),
        )
        .await?;
        return Ok(rc::strip_reply(reply));
    }
    let inbox =
        a["address"].is_null() && inbox_kind(pool, workspace, actor, a["kind"].as_str()).await?;
    let plain = a["address"].is_null()
        && fields.is_none()
        && ["work_filter", "related_to", "note_status"]
            .iter()
            .all(|k| a[*k].is_null())
        && !inbox;
    if plain {
        return store::compact_list(
            pool,
            workspace,
            actor,
            a["parent"].as_str(),
            a["kind"].as_str(),
            limit,
            a["cursor"].as_str(),
        )
        .await;
    }
    let history = a["history"].as_bool().unwrap_or(false);
    let reply = store::read_objects(
        pool,
        workspace,
        actor,
        &store::ObjectScope {
            address: a["address"].as_str(),
            parent: a["parent"].as_str(),
            kind: a["kind"].as_str(),
            revision,
            history,
            limit,
            // The cursor of a history read pages the revision headers here.
            cursor: a["cursor"].as_str().filter(|_| !history),
            note_status: a["note_status"].as_str(),
            related_to: a["related_to"].as_str(),
            work_filter: a["work_filter"].as_str(),
            data: fields
                .iter()
                .flatten()
                .any(|f| f == "data" || f.starts_with("data.")),
        },
    )
    .await?;
    match a["address"].as_str() {
        None => {
            let mut reply = reply;
            // A list in address order gives the bare address of its last item as the cursor,
            // as a plain list does, so one sweep may mix plain pages and pages with fields.
            if !inbox
                && a["work_filter"].is_null()
                && let Some((p, address)) = reply["next_cursor"]
                    .as_str()
                    .and_then(|c| c.split_once(':'))
                && p.parse::<i64>().is_ok()
            {
                reply["next_cursor"] = json!(address);
            }
            Ok(rc::list(reply, fields.as_deref()))
        }
        Some(_) if history => history_page(reply, a["cursor"].as_str()),
        Some(_) if revision.is_some() => Ok(rc::revision(reply)),
        Some(_) => rc::detail(
            reply,
            a["comments_cursor"].as_str(),
            a["links_cursor"].as_str(),
        ),
    }
}

/// The revision headers in pages of 25, oldest first: items, total and next_cursor.
fn history_page(mut reply: Value, cursor: Option<&str>) -> anyhow::Result<Value> {
    let revisions = match reply["revisions"].take() {
        Value::Array(revisions) => revisions,
        _ => vec![],
    };
    reply.as_object_mut().map(|m| m.remove("revisions"));
    let page = rc::page(revisions, cursor)?;
    let mut out = rc::revision(reply);
    for key in ["items", "total", "next_cursor"] {
        if !page[key].is_null() {
            out[key] = page[key].clone();
        }
    }
    Ok(out)
}

/// An Inbox type lists unprocessed inputs first, so its list keeps the order of read_objects.
async fn inbox_kind(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    kind: Option<&str>,
) -> anyhow::Result<bool> {
    let Some(kind) = kind else {
        return Ok(false);
    };
    let tree = store::tree_schema(pool, workspace, actor, None).await?;
    Ok(tree["schema"]["types"][kind]["inbox"] == true)
}

/// The criteria of a parent (direct children) or of every node with `parent: "*"`, of
/// `kind` (default ac), in address order: one page of TESTS_MAX, next_cursor and total.
async fn criteria(
    pool: &Pool,
    actor: &str,
    workspace: &str,
    a: &Value,
) -> anyhow::Result<(Vec<String>, Value)> {
    let mut all = vec![];
    let mut cursor: Option<String> = None;
    loop {
        let page = store::read_objects(
            pool,
            workspace,
            actor,
            &store::ObjectScope {
                address: None,
                parent: Some(a["parent"].as_str().unwrap_or("*")),
                kind: Some(a["kind"].as_str().unwrap_or("ac")),
                revision: None,
                history: false,
                limit: 100,
                cursor: cursor.as_deref(),
                note_status: None,
                related_to: None,
                work_filter: None,
                data: false,
            },
        )
        .await?;
        for item in page["items"].as_array().into_iter().flatten() {
            all.extend(item["address"].as_str().map(str::to_owned));
        }
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    let start: usize = a["cursor"]
        .as_str()
        .map_or(Ok(0), str::parse)
        .context("invalid cursor")?;
    let total = all.len();
    let selected: Vec<String> = all.into_iter().skip(start).take(TESTS_MAX).collect();
    let mut meta = json!({"total": total});
    if start + TESTS_MAX < total {
        meta["next_cursor"] = json!((start + TESTS_MAX).to_string());
    }
    Ok((selected, meta))
}

async fn tests(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
    let workspace = a["workspace"].as_str().context("missing workspace")?;
    if let Some(task) = a["task"].as_str() {
        return Ok(rc::strip_reply(
            store::task_status(pool, workspace, actor, task).await?,
        ));
    }
    let (targets, meta, single) = if let Some(ac) = a["ac"].as_str() {
        let targets: Vec<String> = ac
            .split(',')
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect();
        ensure!(
            targets.len() <= TESTS_MAX,
            "at most {TESTS_MAX} criteria in one call"
        );
        let single = targets.len() == 1;
        (targets, json!({}), single)
    } else if a["parent"].is_string() || a["kind"].is_string() {
        let (targets, meta) = criteria(pool, actor, workspace, a).await?;
        (targets, meta, false)
    } else {
        // The overview comes in pages of 25, in address order.
        let overview = store::tests_overview(pool, workspace, actor).await?;
        let items = overview["items"].as_array().cloned().unwrap_or_default();
        return Ok(rc::strip_reply(rc::page(items, a["cursor"].as_str())?));
    };
    let summary = a["summary"].as_bool().unwrap_or(false);
    let mut items = vec![];
    for target in &targets {
        let item = store::criterion_tests(pool, workspace, actor, target).await?;
        items.push(if summary {
            read_shapes::test_summary(item)
        } else {
            item
        });
    }
    if single {
        return Ok(rc::strip_reply(items.remove(0)));
    }
    let mut reply = meta;
    reply["items"] = Value::Array(items);
    Ok(rc::strip_reply(reply))
}
