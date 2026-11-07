//! requirement:generic-nodes over a real server: every node takes the same fields and
//! operations. A Task is a node: data.assignee, depends_on, archive_object and the node filters.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{listed, save, world};
use serde_json::json;

// generic-nodes-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_ready_filter_follows_a_dependency_wait() -> Result<()> {
    let w = world(&[]).await?;
    let data = json!({"title":"Call the API","owner":"owner","body":"Call it","assignee":"alex"});
    // The task waits for the grey criterion ac:a.
    let link = json!([{"target":"ac:a","relation":"depends_on","blocking":true}]);
    save(&w, "task", "t", "requirement:r", data.clone(), link).await?;
    let task = w.object("task:t").await?;
    let mark = task["process"].get("assessment");
    ensure!(task["work_ready"] == false && mark.is_none(), "{task}");
    let filter = |ready: bool| json!({"kind":["task","requirement"],"assignee":null,"ready":ready});
    // The task has the rule none, so requirement:r does not count it and stays ready.
    let waiting = listed(&w, filter(false)).await?;
    ensure!(waiting == ["task:t"], "{waiting:?}");
    let ready = listed(&w, filter(true)).await?;
    ensure!(ready == ["requirement:r"], "{ready:?}");
    let alex = json!({"kind":"task","assignee":"alex","ready":false});
    ensure!(
        listed(&w, alex).await? == ["task:t"],
        "alex holds the waiting task"
    );
    // Without the link, the task is ready again.
    let save = json!({"op":"save_object","kind":"task","id":"t","revision":task["revision"],
        "parent":"requirement:r","data":data,"links":[],"reason":"No order"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    ensure!(
        listed(&w, filter(true)).await? == ["requirement:r", "task:t"],
        "the task is ready again"
    );
    // The readiness mark operation is refused, in the current and in the older form.
    let text = "assess was removed; use save_object with a typed field such as data.stage (fields in set_tree_schema)";
    let mark = json!({"op":"assess","target":"task:t","ready":true,"reason":"Known"});
    let older = json!({"op":"assess","req":"r","revision":1,"ac":"a",
        "assessment":{"ready":true,"reason":"Known","owner":null,"decision":null}});
    for command in [mark, older] {
        let reply = w.command(&w.bot, command).await?;
        ensure!(reply == Err(text.into()), "{reply:?}");
    }
    Ok(())
}

const STAGE: &str = "save_object with a typed field such as data.stage (fields in set_tree_schema)";

// generic-nodes-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_task_operations_and_reads_are_refused_with_their_replacement() -> Result<()> {
    let w = world(&[]).await?;
    let task = json!({"title":"Ship","owner":"owner","body":"Ship it"});
    save(&w, "task", "t", "ac:a", task, json!([])).await?;
    for (op, replacement) in [
        ("claim_task", "save_object with data.assignee"),
        ("claim_next_task", "save_object with data.assignee"),
        ("release_task", "save_object with data.assignee"),
        ("claim_planning", "save_object with data.assignee"),
        ("block_task", STAGE),
        ("unblock_task", STAGE),
        ("assess_signal", STAGE),
        ("complete_task", "archive_object"),
        ("retire_task", "archive_object"),
        ("set_priority", "save_object with data.priority"),
    ] {
        let reply = w
            .command(&w.bot, json!({"op":op,"id":"task:t","revision":1}))
            .await?;
        ensure!(
            reply == Err(format!("{op} was removed; use {replacement}")),
            "{op}: {reply:?}"
        );
    }
    let tasks = w.mcp(&w.bot, "joppa_tasks", json!({})).await?;
    let refusal = "joppa_tasks was removed; use joppa_objects with kind task, for example {\"kind\":\"task\",\"assignee\":\"alex\",\"ready\":true}";
    ensure!(tasks == Err(refusal.into()), "{tasks:?}");
    let unstarted = w
        .mcp(&w.bot, "joppa_objects", json!({"work_filter":"unstarted"}))
        .await?;
    let filter = "work_filter was removed; use the node filters, for example kind and ready";
    ensure!(unstarted == Err(filter.into()), "{unstarted:?}");
    let base = format!("{}/api/workspaces/{}", w.root, w.ws);
    let get = async |path: &str| -> Result<(u16, String)> {
        let reply = w
            .http
            .get(format!("{base}/{path}"))
            .bearer_auth(&w.bot)
            .send()
            .await?;
        Ok((reply.status().as_u16(), reply.text().await?))
    };
    let (code, body) = get("objects?work_filter=unstarted").await?;
    ensure!(code != 200 && body.contains(filter), "{code} {body}");
    ensure!(get("tasks").await?.0 == 404, "the Task queue route is gone");
    Ok(())
}

// generic-nodes-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_dependency_on_a_task_waits_until_the_task_is_archived() -> Result<()> {
    let w = world(&[]).await?;
    let task = |title: &str| json!({"title":title,"owner":"owner","body":"Do it"});
    save(&w, "task", "first", "ac:a", task("First"), json!([])).await?;
    let link = json!([{"target":"task:first","relation":"depends_on","blocking":true}]);
    save(&w, "task", "second", "ac:a", task("Second"), link).await?;
    let waiting = json!({"kind":"task","ready":false});
    ensure!(
        listed(&w, waiting.clone()).await? == ["task:second"],
        "second waits"
    );
    let second = w.object("task:second").await?;
    ensure!(second["waits_for"]["address"] == "task:first", "{second}");
    // A priority is one value of the schema list.
    let mut wrong = task("Third");
    wrong["priority"] = json!("p9");
    let reply = w
        .command(
            &w.bot,
            json!({"op":"save_object","kind":"task","revision":0,
        "parent":"ac:a","data":wrong,"reason":"Plan"}),
        )
        .await?;
    ensure!(
        reply == Err("priority is one of p0, p1, p2".into()),
        "{reply:?}"
    );
    let mut third = task("Third");
    third["priority"] = json!("p1");
    save(&w, "task", "third", "ac:a", third, json!([])).await?;
    ensure!(
        w.object("task:third").await?["data"]["priority"] == "p1",
        "p1"
    );
    let archive = json!({"op":"archive_object","target":"task:first","revision":1,"reason":"Done"});
    w.command(&w.bot, archive)
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        listed(&w, waiting).await?.is_empty(),
        "nothing waits after the archive"
    );
    Ok(())
}
