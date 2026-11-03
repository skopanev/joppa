//! requirement:generic-nodes over a real server: accept_commit accepts any node over the test
//! nodes at and below it.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{save, world};
use serde_json::{Value, json};

// generic-nodes-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn accept_commit_accepts_any_node_over_the_test_nodes_below_it() -> Result<()> {
    const NEEDS: &str = "acceptance needs records on the commit where every test below passes";
    let w = world(&["tests/x.rs > one"]).await?;
    let task = json!({"title":"Plan","owner":"owner","body":"Plan it"});
    save(&w, "task", "t", "requirement:r", task, json!([])).await?;
    let accept = async |token: &str, target: &str, commit: &str| -> Result<Result<Value, String>> {
        let revision = w.object(target).await?["revision"].clone();
        w.command(
            token,
            json!({"op":"accept_commit","target":target,"revision":revision,"commit":commit}),
        )
        .await
    };
    w.recorded(
        "c1c1c1c",
        json!([{"test":"tests/x.rs > one","status":"fail"}]),
    )
    .await?;
    ensure!(
        accept(&w.owner, "ac:a", "c1c1c1c").await? == Err(NEEDS.into()),
        "a failure refuses"
    );
    w.recorded(
        "c2c2c2c",
        json!([{"test":"tests/x.rs > one","status":"pass"}]),
    )
    .await?;
    for target in ["ac:a", "requirement:r"] {
        accept(&w.owner, target, "c2c2c2c")
            .await?
            .map_err(anyhow::Error::msg)?;
        let acceptance = &w.object(target).await?["process"]["acceptance"];
        ensure!(
            acceptance["commit"] == "c2c2c2c" && acceptance["actor"] == "owner",
            "{acceptance}"
        );
    }
    ensure!(
        accept(&w.owner, "task:t", "c2c2c2c").await? == Err(NEEDS.into()),
        "no test below"
    );
    let other = accept(&w.alex, "ac:a", "c2c2c2c").await?;
    ensure!(
        other == Err("only the owner of the node accepts".into()),
        "{other:?}"
    );
    Ok(())
}
