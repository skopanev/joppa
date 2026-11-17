//! requirement:q438uw0s over a real server: a flow relation of the registry can name a group,
//! the server trims it, omits an empty group, refuses more than 128 characters, refuses the
//! reserved dash and refuses a group on a system relation, and the help and the guide name group.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use relations_world::{msg, registry, schema, tree};
use serde_json::json;

// ac:qbjop7m1
// test:iziaxlld
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_trimmed_flow_group_persists_across_type_saves_and_the_docs_name_group() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    let put = json!({
        "swap": {"title": "Swap", "group": " Money "},
        "chain": {"title": "Chain", "group": ""}
    });
    schema(&w, json!({"put_relations": put}))
        .await?
        .map_err(msg)?;
    let reg = registry(&tree(&w, &w.owner, None).await?);
    ensure!(reg["swap"]["group"] == "Money", "{reg}");
    ensure!(reg["chain"].get("group").is_none(), "{reg}");

    let extra = relations_world::node("Extra", "document", &[], json!({}));
    schema(&w, json!({"put_types": {"extra": extra}}))
        .await?
        .map_err(msg)?;
    let after = registry(&tree(&w, &w.owner, None).await?);
    ensure!(after["swap"]["group"] == "Money", "{after}");
    ensure!(after["chain"].get("group").is_none(), "{after}");

    let help = w
        .mcp(&w.bot, "joppa_help", json!({"op": "set_tree_schema"}))
        .await?
        .map_err(msg)?;
    let put_help = help["operations"]["set_tree_schema"]["optional"]["put_relations"]
        .as_str()
        .unwrap_or_default();
    ensure!(put_help.contains("group"), "{help}");
    for (name, text) in [
        ("docs/schema.md", include_str!("../docs/schema.md")),
        ("docs/links.md", include_str!("../docs/links.md")),
    ] {
        ensure!(text.contains("group"), "{name} omits group");
    }
    let guide = w
        .http
        .get(format!("{}/guide", w.root))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    ensure!(guide.contains("group"), "/guide omits group");
    Ok(())
}

// ac:qbjop7m1
// test:yf78ulld
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_group_over_128_characters_a_reserved_dash_or_a_system_group_is_refused() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    let max_ok = "g".repeat(128);
    schema(
        &w,
        json!({"put_relations": {"swap": {"title": "Swap", "group": max_ok}}}),
    )
    .await?
    .map_err(msg)?;
    let before = tree(&w, &w.owner, None).await?["schema"]["revision"].clone();

    let too_long = "g".repeat(129);
    for (put, expected) in [
        (
            json!({"swap": {"title": "Swap", "group": too_long}}),
            "group holds at most 128 characters",
        ),
        (
            json!({"depends_on": {"title": "Depends on", "group": "Money"}}),
            "a system relation takes no group",
        ),
        (
            json!({"related": {"group": "Links"}}),
            "a system relation takes no group",
        ),
        (
            json!({"swap": {"title": "Swap", "group": " - "}}),
            "group - names the flows without a group",
        ),
    ] {
        let err = schema(&w, json!({"put_relations": put}))
            .await?
            .expect_err("invalid group must be refused");
        ensure!(err.contains(expected), "got {err:?}, wanted {expected:?}");
    }
    let after = tree(&w, &w.owner, None).await?["schema"]["revision"].clone();
    ensure!(after == before, "refused writes changed schema revision");
    Ok(())
}
