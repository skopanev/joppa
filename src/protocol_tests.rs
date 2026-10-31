//! The compact MCP read of joppa_tests (requirement:read-contract): one criterion, a list of
//! criteria, the criteria of a parent, a Task, or the overview.
use anyhow::{Context, ensure};
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, read_shapes, store};
use serde_json::{Value, json};

/// The most criteria that one joppa_tests call reads.
const TESTS_MAX: usize = 100;

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
                contains: None,
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

pub async fn tests(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
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
        let mut page = rc::page(items, a["cursor"].as_str())?;
        for key in ["workspace", "position"] {
            page[key] = overview[key].clone();
        }
        return Ok(rc::strip_reply(page));
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
