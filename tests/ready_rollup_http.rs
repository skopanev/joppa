//! requirement:ready-rolls-up over a real server: the `ready` node filter rolls up along the
//! status rule of the type, as the status tone does.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{World, listed, save, world};
use serde_json::{Value, json};

const TEST: &str = "tests/rollup.rs > one";

/// The present nodes that the `ready` filter lists, in address order.
async fn ready(w: &World, value: bool) -> Result<Vec<String>> {
    listed(w, json!({"parent":"*","ready":value})).await
}

async fn ok(w: &World, command: Value) -> Result<()> {
    w.command(&w.bot, command)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

// ready-rolls-up-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_parent_is_ready_only_when_every_counted_child_and_linking_object_is_ready() -> Result<()>
{
    // requirement:r > ac:a (one test) and ac:b. requirement:q > ac:c. ac:b relates to consumer:u.
    let w = world(&[TEST]).await?;
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let consumer =
        json!({"title":"Users","role":"document","status":"links","root":true,"parents":[]});
    let change = json!({"op":"set_tree_schema","revision":revision,"put_types":{"consumer":consumer},"reason":"Add users"});
    w.command(&w.owner, change)
        .await?
        .map_err(anyhow::Error::msg)?;
    let user = json!({"op":"save_object","kind":"consumer","id":"u","revision":0,"parent":null,
        "data":{"title":"U","owner":"owner","body":"A user"},"reason":"Create"});
    ok(&w, user).await?;
    let node = |title: &str| json!({"title":title,"owner":"owner"});
    let related = json!({"related":{"edges":[{"to":"consumer:u"}]}});
    save(&w, "ac", "b", "requirement:r", node("B"), related).await?;
    let root = json!({"op":"save_object","kind":"requirement","id":"q","revision":0,"parent":null,
        "data":{"title":"Q","owner":"owner","body":"The second rule"},"reason":"Create"});
    ok(&w, root).await?;
    save(&w, "ac", "c", "requirement:q", node("C"), json!({})).await?;
    let all = |test: &str| {
        let mut all = vec![
            "ac:a",
            "ac:b",
            "ac:c",
            "consumer:u",
            "requirement:q",
            "requirement:r",
        ];
        all.push(test);
        all.sort();
        all.into_iter().map(String::from).collect::<Vec<_>>()
    };
    let test = listed(&w, json!({"parent":"ac:a"})).await?;
    ensure!(test.len() == 1, "{test:?}");
    ensure!(
        ready(&w, true).await? == all(&test[0]),
        "every node is ready"
    );
    // B waits for the grey criterion A: its parent R and the consumer U that B relates to wait
    // too. The save keeps the related edge and adds a depends_on edge.
    let wait = json!({"op":"save_object","kind":"ac","id":"b","revision":1,"parent":"requirement:r",
        "data":node("B"),"relations":{"depends_on":{"edges":[{"to":"ac:a"}]}},"reason":"Order"});
    ok(&w, wait).await?;
    ensure!(
        ready(&w, false).await? == ["ac:b", "consumer:u", "requirement:r"],
        "{:?}",
        ready(&w, false).await?
    );
    let r = w.object("requirement:r").await?;
    ensure!(r["work_ready"] == false, "{r}");
    // C waits for A too: Q waits too, until a pass makes A green.
    let link = json!({"op":"save_object","kind":"ac","id":"c","revision":1,"parent":"requirement:q",
        "data":{"title":"C","owner":"owner"},"relations":{"depends_on":{"edges":[{"to":"ac:a"}]}},"reason":"Order"});
    ok(&w, link).await?;
    let waiting = ready(&w, false).await?;
    ensure!(
        waiting
            == [
                "ac:b",
                "ac:c",
                "consumer:u",
                "requirement:q",
                "requirement:r"
            ],
        "{waiting:?}"
    );
    w.recorded("c0ffee6", json!([{"test":TEST,"status":"pass"}]))
        .await?;
    ensure!(
        ready(&w, false).await?.is_empty(),
        "every node is ready again"
    );
    ensure!(
        ready(&w, true).await? == all(&test[0]),
        "the same nodes are ready"
    );
    Ok(())
}
