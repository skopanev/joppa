//! requirement:edit-makes-revision over a real server: live criterion saves in a batch leave R
//! at its read revision, pre-epoch criterion saves replay with forced R revisions, and any other
//! revision gap stays stale.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{last_event, stored};
use save_world::{append_event, batch, head, ok, refused, replay, revisions};
use serde_json::{Value, json};

const R: &str = "requirement:r";
const STALE: &str = "stale object revision";

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

// ac:lz57zu7i
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_save_of_a_requirement_accepts_the_revisions_that_its_criterion_saves_forced()
-> Result<()> {
    // Live past own_requirement_revisions: A and B save without forcing R, so R goes from 5 to 6.
    let live = record_world::world(&[]).await?;
    let b = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":R,
        "data":{"title":"B","owner":"owner"},"reason":"Create"});
    ok(&live, b.clone()).await?;
    drive(&live, 5).await?;
    let live_cmds = [
        body(&live, "ac:a", "Given A, then A holds.").await?,
        body(&live, "ac:b", "Given B, then B holds.").await?,
        save_of(R, 5, json!({"body":"The new rule"})),
    ];
    let live_reply = batch(&live, &live_cmds)
        .await?
        .map_err(anyhow::Error::msg)?;
    let live_saved = &live_reply["results"][2]["object"];
    ensure!(
        live_saved["address"] == R && live_saved["revision"] == 6,
        "{live_reply}"
    );
    let live_event = last_event(&live).await?;
    ensure!(
        live_event["id"] == "r" && live_event["revision"] == 5,
        "live operation 3 event: {live_event}"
    );
    let live_history = live
        .mcp(
            &live.bot,
            "joppa_objects",
            json!({"address":R,"history":true}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(live_history["total"] == 6, "{live_history}");

    // Second fixture: replay a stored pre-epoch batch where R was at 5, A and B forced 6 and 7, and R saved at caught-up revision 7 -> 8.
    let pre = record_world::world(&[]).await?;
    pre.pool
        .get()
        .await?
        .execute(
            "UPDATE joppa.replay_epochs SET first_position=1000 WHERE workspace=$1 AND rule='own_requirement_revisions'",
            &[&pre.ws],
        )
        .await?;
    replay(&pre).await?;
    // After setup replay before epoch, R is at revision 2 (1 create + 1 forced by ac:a). Append ac:b (forces R -> 3) and 2 direct saves of R (3->4, 4->5).
    append_event(
        &pre,
        "bot",
        &json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":R,"data":{"title":"B","owner":"owner","body":""},"relations":{},"reason":"Create b"}),
    )
    .await?;
    for (rev, text) in [(3, "Rule 3"), (4, "Rule 4")] {
        append_event(
            &pre,
            "bot",
            &json!({"op":"save_object","kind":"requirement","id":"r","revision":rev,"parent":null,"data":{"title":"R","owner":"owner","body":text},"relations":{},"reason":"Drive"}),
        )
        .await?;
    }
    let a_rev = stored(&pre, "ac:a").await?.0;
    let b_rev = stored(&pre, "ac:b").await?.0;
    let five_data = pre.object(R).await?["data"].clone();
    let mut merged = five_data.clone();
    merged["body"] = json!("The new rule");
    for event in [
        json!({"op":"save_object","kind":"ac","id":"a","revision":a_rev,"parent":R,"data":{"title":"A","owner":"owner","body":"Given A, then A holds."},"relations":{},"reason":"Save A"}),
        json!({"op":"save_object","kind":"ac","id":"b","revision":b_rev,"parent":R,"data":{"title":"B","owner":"owner","body":"Given B, then B holds."},"relations":{},"reason":"Save B"}),
        json!({"op":"save_object","kind":"requirement","id":"r","revision":7,"parent":null,"data":merged,"relations":{},"reason":"Save R"}),
    ] {
        append_event(&pre, "bot", &event).await?;
    }
    let all = revisions(&pre, R).await?;
    let numbers: Vec<i64> = all.iter().map(|(n, _)| *n).collect();
    ensure!(numbers == (1..=8).collect::<Vec<_>>(), "{numbers:?}");
    let five = &all[4].1;
    for (number, forced) in &all[5..7] {
        for key in ["data", "parent", "relations", "present"] {
            ensure!(
                forced[key] == five[key],
                "revision {number} changes {key}: {forced} {five}"
            );
        }
        ensure!(
            forced["contract"] == json!([*number, 5]),
            "revision {number} contract: {forced}"
        );
    }
    ensure!(all[7].1["data"] == merged, "revision 8: {}", all[7].1);
    let event = last_event(&pre).await?;
    ensure!(
        event["id"] == "r" && event["revision"] == 7,
        "the replayed event of operation 3: {event}"
    );
    Ok(())
}

// ac:h68zt3sg
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn any_other_revision_gap_stays_stale() -> Result<()> {
    let w = record_world::world(&[]).await?;
    drive(&w, 8).await?;
    let unchanged = async |world: &World, commands: &[Value], error: &str| -> Result<()> {
        let start = head(world).await?;
        refused(batch(world, commands).await?, error)?;
        ensure!(head(world).await? == start, "{error}: the batch appended");
        Ok(())
    };
    let next_a = stored(&w, "ac:a").await?.0 + 1;
    let first = [
        save_of(R, 8, json!({"title":"R renamed"})),
        save_of("ac:a", next_a, json!({"body":"Given A, then A holds."})),
        save_of(R, 8, json!({"body":"The batch rule"})),
    ];
    unchanged(&w, &first, &format!("operation 3: {STALE}")).await?;
    ok(&w, save_of(R, 8, json!({"title":"R nine"}))).await?;
    ensure!(stored(&w, R).await?.0 == 9, "direct save gives R 9");
    let after = [
        body(&w, "ac:a", "Given A once more, then A holds.").await?,
        save_of(R, 8, json!({"body":"The late rule"})),
    ];
    unchanged(&w, &after, &format!("operation 2: {STALE}")).await?;
    ensure!(stored(&w, R).await?.0 == 9, "R stays at 9");

    // Second fixture before own_requirement_revisions: replayed criterion save forces R from 8 to 9, then live batch with R at 8 is refused.
    let pre = record_world::world(&[]).await?;
    pre.pool
        .get()
        .await?
        .execute(
            "UPDATE joppa.replay_epochs SET first_position=1000 WHERE workspace=$1 AND rule='own_requirement_revisions'",
            &[&pre.ws],
        )
        .await?;
    replay(&pre).await?;
    for rev in 2..8 {
        append_event(
            &pre,
            "bot",
            &json!({"op":"save_object","kind":"requirement","id":"r","revision":rev,"parent":null,"data":{"title":"R","owner":"owner","body":format!("Rule {rev}")},"relations":{},"reason":"Drive"}),
        )
        .await?;
    }
    let a_rev = stored(&pre, "ac:a").await?.0;
    append_event(
        &pre,
        "bot",
        &json!({"op":"save_object","kind":"ac","id":"a","revision":a_rev,"parent":R,"data":{"title":"A","owner":"owner","body":"Given A pre-epoch, then A holds."},"relations":{},"reason":"Pre-epoch A"}),
    )
    .await?;
    ensure!(
        stored(&pre, R).await?.0 == 9,
        "pre-epoch criterion save forces R 9"
    );
    let alone = [save_of(R, 8, json!({"body":"The late rule"}))];
    unchanged(&pre, &alone, &format!("operation 1: {STALE}")).await?;
    Ok(())
}
