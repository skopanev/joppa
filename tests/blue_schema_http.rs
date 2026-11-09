//! requirement:vqlbur46 over a real server: a stored status list of an older release reads
//! with the tone blue and the colour rule, and a live schema write refuses the tone grey.
//! The older lists are journal events of the older release: a live write refuses them.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use blue_world::{NEW_PAIR, OLD_PAIRS, blue, blue_entry, old_schema, statuses, workspace};
use serde_json::{Value, json};

/// An older saved list with the tone grey in `light` and `dark` after the tone green.
fn older_list(light: &str, dark: &str) -> Value {
    // The older release stored the tone grey: this list keeps it on purpose.
    json!([{"tone":"green","light":"#22a559","dark":"#2fbf6a"},
        {"tone":"grey","light":light,"dark":dark}])
}

// ac:17ses87u
// test:lst7mjh6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_stored_status_lists_give_blue() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    let names = ["V", "P", "U", "S", "Q"];
    let mut ws = Vec::new();
    for name in names {
        ws.push(workspace(&w, name).await?);
    }
    // The older release wrote these events. The older map names the tone grey too.
    let changes = [
        json!({"statuses":older_list(OLD_PAIRS[0].0, OLD_PAIRS[0].1)}),
        json!({"statuses":older_list(OLD_PAIRS[1].0, OLD_PAIRS[1].1)}),
        json!({"statuses":older_list("#aaaaaa", "#555555")}),
        json!({"status_colors":{"grey":"#c8c3b8"}}),
        json!({"status_colors":{"grey":"#123456"}}),
    ];
    w.stop();
    let mut revisions = Vec::new();
    for (ws, change) in ws.iter().zip(changes) {
        revisions.push(old_schema(&w, ws, change).await?);
    }
    w.start().await?;
    let wanted = [
        blue(NEW_PAIR.0, NEW_PAIR.1),
        blue(NEW_PAIR.0, NEW_PAIR.1),
        blue("#aaaaaa", "#555555"),
        blue("#c8c3b8", "#c8c3b8"),
        blue("#123456", "#123456"),
    ];
    // The new workspace W has the default list.
    let own = blue_entry(&statuses(&w, &w.ws, None).await?)?;
    ensure!(own == blue(NEW_PAIR.0, NEW_PAIR.1), "W: {own}");
    for ((name, ws), want) in names.iter().zip(&ws).zip(&wanted) {
        let list = statuses(&w, ws, None).await?;
        let found = blue_entry(&list)?;
        ensure!(found == *want, "{name}: {list}");
        // The blue entry keeps the place of grey: the older list put it after green.
        if !name.starts_with(['S', 'Q']) {
            ensure!(list[1] == *want, "{name}: the place of grey: {list}");
        }
    }
    // The stored schema revisions of V read the same: revision 1 (the default list) and the
    // revision of the older list.
    for revision in [1, revisions[0]] {
        let list = statuses(&w, &ws[0], Some(revision)).await?;
        ensure!(
            blue_entry(&list)? == wanted[0],
            "V revision {revision}: {list}"
        );
    }
    Ok(())
}

// ac:yvzerac1
// test:bqa6eth5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_live_schema_write_refuses_the_tone_grey() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let revision = || async {
        let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
        Ok::<_, anyhow::Error>(tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone())
    };
    let before = revision().await?;
    let grey = json!([{"tone":"grey","light":"#112233","dark":"#445566"}]);
    let command = json!({"op":"set_tree_schema","revision":before,"statuses":grey,
        "reason":"Colour the waiting tone"});
    let reply = w.command(&w.owner, command).await?;
    let error = "status tones are red, purple, yellow, orange, blue and green";
    ensure!(
        reply.as_ref().is_err_and(|e| e.contains(error)),
        "grey: {reply:?}"
    );
    ensure!(
        revision().await? == before,
        "the grey command stored a revision"
    );
    let list = json!([blue("#112233", "#445566")]);
    let command = json!({"op":"set_tree_schema","revision":before,"statuses":list,
        "reason":"Colour the waiting tone"});
    w.command(&w.owner, command)
        .await?
        .map_err(anyhow::Error::msg)?;
    let after = revision().await?;
    ensure!(after.as_u64() == before.as_u64().map(|r| r + 1), "{after}");
    let list = statuses(&w, &w.ws, None).await?;
    ensure!(list[0] == blue("#112233", "#445566"), "{list}");
    ensure!(blue_entry(&list)? == list[0], "{list}");
    Ok(())
}
