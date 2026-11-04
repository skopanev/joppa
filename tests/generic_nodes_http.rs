//! requirement:generic-nodes over a real server: every node takes the same fields and
//! operations. A Task is a node: data.assignee, assess, archive_object and the node filters.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{listed, save, world};
use serde_json::json;

// generic-nodes-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn assess_marks_any_node_and_the_ready_filter_follows_the_mark() -> Result<()> {
    let w = world(&[]).await?;
    let data = json!({"title":"Call the API","owner":"owner","body":"Call it","assignee":"alex"});
    save(&w, "task", "t", "ac:a", data, json!([])).await?;
    let mark = |target: &str, ready: bool, reason: &str| json!({"op":"assess","target":target,"ready":ready,"reason":reason});
    for target in ["task:t", "requirement:r"] {
        w.command(&w.bot, mark(target, false, "Which API version?"))
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    let task = w.object("task:t").await?;
    let mut assessment = task["process"]["assessment"].clone();
    let position = assessment["position"].take();
    ensure!(
        assessment
            == json!({"state":"waiting","reason":"Which API version?","actor":"bot","position":null})
            && position.as_u64() > Some(0),
        "{task}"
    );
    let filter = |ready: bool| json!({"kind":["task","requirement"],"assignee":null,"ready":ready});
    ensure!(
        listed(&w, filter(false)).await? == ["requirement:r", "task:t"],
        "both wait"
    );
    let alex = json!({"kind":"task","assignee":"alex","ready":false});
    ensure!(
        listed(&w, alex).await? == ["task:t"],
        "alex holds the waiting task"
    );
    w.command(&w.bot, mark("task:t", true, "Version 2"))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        listed(&w, filter(true)).await? == ["task:t"],
        "the task is ready again"
    );
    ensure!(
        w.object("task:t").await?["process"]["assessment"]["state"] == "ready",
        "ready"
    );
    // The older form of a criterion assessment and an unknown node are refused.
    let older = json!({"op":"assess","req":"r","revision":1,"ac":"a",
        "assessment":{"ready":true,"reason":"Known","owner":null,"decision":null}});
    ensure!(
        w.command(&w.bot, older).await?
            == Err(
                "assess with req, revision and ac was removed; use assess {target, ready, reason}"
                    .into()
            ),
        "the older form is refused"
    );
    let unknown = w.command(&w.bot, mark("task:none", true, "Known")).await?;
    ensure!(unknown == Err("unknown object".into()), "{unknown:?}");
    Ok(())
}

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
        ("block_task", "assess"),
        ("assess_signal", "assess"),
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
