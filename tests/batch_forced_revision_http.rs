//! requirement:edit-makes-revision over a real server: a batch save of a requirement accepts
//! the revisions that earlier criterion saves of the same batch forced. Any other gap is stale.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{last_event, stored};
use save_world::{batch, head, ok, refused, replay, revisions};
use serde_json::{Value, json};

const R: &str = "requirement:r";
const STALE: &str = "stale object revision; read the object again";

/// A save_object of `address` at `revision` with the data keys of `data`.
fn save_of(address: &str, revision: u64, data: Value) -> Value {
    let (kind, id) = address.split_once(':').unwrap_or_default();
    json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"data":data,
        "reason":"Forced revision test"})
}

/// A new body of `address` at its current revision.
async fn body(w: &World, address: &str, body: &str) -> Result<Value> {
    let revision = stored(w, address).await?.0;
    Ok(save_of(address, revision, json!({"body":body})))
}

/// Save new bodies of R until R is at `target`.
async fn drive(w: &World, target: u64) -> Result<()> {
    loop {
        let revision = stored(w, R).await?.0;
        ensure!(revision <= target, "R is at {revision}, above {target}");
        if revision == target {
            return Ok(());
        }
        ok(w, body(w, R, &format!("The rule at {revision}")).await?).await?;
    }
}

// deploy-24 AC-4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_save_of_a_requirement_accepts_the_revisions_that_its_criterion_saves_forced()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    let b = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":R,
        "data":{"title":"B","owner":"owner"},"reason":"Create"});
    ok(&w, b).await?;
    drive(&w, 5).await?;
    let commands = [
        body(&w, "ac:a", "Given A, then A holds.").await?,
        body(&w, "ac:b", "Given B, then B holds.").await?,
        save_of(R, 5, json!({"body":"The new rule"})),
    ];
    let reply = batch(&w, &commands).await?.map_err(anyhow::Error::msg)?;
    let saved = &reply["results"][2]["object"];
    ensure!(saved["address"] == R && saved["revision"] == 8, "{reply}");
    let all = revisions(&w, R).await?;
    let numbers: Vec<i64> = all.iter().map(|(n, _)| *n).collect();
    ensure!(numbers == (1..=8).collect::<Vec<_>>(), "{numbers:?}");
    let five = &all[4].1;
    for (number, forced) in &all[5..7] {
        for key in ["data", "parent", "relations", "contract"] {
            ensure!(
                forced[key] == five[key],
                "revision {number} changes {key}: {forced} {five}"
            );
        }
    }
    let mut merged = five["data"].clone();
    merged["body"] = json!("The new rule");
    ensure!(all[7].1["data"] == merged, "revision 8: {}", all[7].1);
    let event = last_event(&w).await?;
    ensure!(
        event["id"] == "r" && event["revision"] == 7,
        "the journal event of operation 3: {event}"
    );
    let history = json!({"address":R,"history":true});
    let history = w.mcp(&w.bot, "joppa_objects", history).await?;
    let history = history.map_err(anyhow::Error::msg)?;
    ensure!(history["total"] == 8, "{history}");
    // The replay gives R the same 8 revisions.
    replay(&w).await?;
    ensure!(revisions(&w, R).await? == all, "the replay changed R");
    Ok(())
}

// deploy-24 AC-5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn any_other_revision_gap_stays_stale() -> Result<()> {
    let w = record_world::world(&[]).await?;
    drive(&w, 8).await?;
    let unchanged = async |commands: &[Value], error: &str| -> Result<()> {
        let start = head(&w).await?;
        refused(batch(&w, commands).await?, error)?;
        ensure!(head(&w).await? == start, "{error}: the batch appended");
        Ok(())
    };
    // Revision 9 is a direct save of R in the same batch.
    let first = [
        save_of(R, 8, json!({"title":"R renamed"})),
        body(&w, "ac:a", "Given A, then A holds.").await?,
        save_of(R, 8, json!({"body":"The batch rule"})),
    ];
    unchanged(&first, &format!("operation 3: {STALE}")).await?;
    // Revision 9 comes from an earlier write.
    ok(&w, body(&w, "ac:a", "Given A again, then A holds.").await?).await?;
    ensure!(stored(&w, R).await?.0 == 9, "the criterion save forces R 9");
    let alone = [save_of(R, 8, json!({"body":"The late rule"}))];
    unchanged(&alone, &format!("operation 1: {STALE}")).await?;
    let after = [
        body(&w, "ac:a", "Given A once more, then A holds.").await?,
        save_of(R, 8, json!({"body":"The late rule"})),
    ];
    unchanged(&after, &format!("operation 2: {STALE}")).await?;
    ensure!(stored(&w, R).await?.0 == 9, "R stays at 9");
    Ok(())
}
