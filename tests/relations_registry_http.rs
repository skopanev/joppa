//! requirement:q438uw0s over a real server: the relation registry of a workspace. Every
//! workspace has the system relations, the owner declares and removes flow relations, and a
//! used or system relation stays.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{change, declare, msg, refused, registry, saved, schema, stored, tree};
use serde_json::{Value, json};

/// The two system relations of every workspace.
fn system() -> Value {
    json!({"depends_on":{"title":"Depends on",
            "description":"The node depends on the target. No cycles.","system":true},
        "related":{"title":"Related","description":"A reference between nodes that blocks nothing.",
            "system":true}})
}

/// Give workspace `ws` the stored form of the older release: its schema revisions, its meta
/// row and its board state hold no relation registry, and it has no relations_reset epoch.
/// A stored schema revision cannot change, so a changed revision row is written again.
async fn older_registry(w: &World, ws: &str) -> Result<()> {
    let conn = w.pool.get().await?;
    let sql = "SELECT revision,value::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND value ? 'relations'";
    for row in conn.query(sql, &[&ws]).await? {
        let revision: i32 = row.get(0);
        let mut value: Value = serde_json::from_str(row.get(1))?;
        value.as_object_mut().map(|m| m.remove("relations"));
        let delete = "DELETE FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' AND revision=$2";
        conn.execute(delete, &[&ws, &revision]).await?;
        let insert = "INSERT INTO joppa.state_revisions(workspace,section,key,revision,value) VALUES($1,'tree_schema','',$2,$3::text::jsonb)";
        conn.execute(insert, &[&ws, &revision, &value.to_string()])
            .await?;
    }
    let strip = |column: &str| {
        format!(
            "jsonb_set({column}-'relations','{{tree_schema}}',COALESCE((SELECT jsonb_agg(t-'relations' ORDER BY n) FROM jsonb_array_elements({column}->'tree_schema') WITH ORDINALITY x(t,n)),'[]'::jsonb))"
        )
    };
    let meta = format!(
        "UPDATE joppa.state_entries SET value={} WHERE workspace=$1 AND section='meta' AND key='' AND jsonb_typeof(value->'tree_schema')='array'",
        strip("value")
    );
    conn.execute(&meta, &[&ws]).await?;
    let board = format!(
        "UPDATE joppa.board_cache SET state={} WHERE workspace=$1 AND jsonb_typeof(state->'tree_schema')='array'",
        strip("state")
    );
    conn.execute(&board, &[&ws]).await?;
    for sql in [
        "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='relations'",
        "DELETE FROM joppa.replay_epochs WHERE workspace=$1 AND rule='relations_reset'",
    ] {
        conn.execute(sql, &[&ws]).await?;
    }
    Ok(())
}

/// A tree reply has the system relations and no link_relations key.
fn has_system(tree: &Value, name: &str) -> Result<()> {
    ensure!(registry(tree) == system(), "{name}: {tree}");
    let none =
        tree.get("link_relations").is_none() && tree["schema"].get("link_relations").is_none();
    ensure!(none, "{name} gives link_relations: {tree}");
    Ok(())
}

// ac:esfpw74b
// test:mu4xp9ux
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_workspace_has_the_system_relations_depends_on_and_related() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    // Workspace V of the same owner was created before this change.
    let v = format!("older-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":"Older workspace"});
    joppa::store::execute_input(&w.pool, &v, "owner", "older-1", &create).await?;
    joppa::store::enroll_service(&w.pool, &v, "owner", "bot", "agent").await?;
    w.stop();
    older_registry(&w, &v).await?;
    w.start().await?;
    has_system(&tree(&w, &w.bot, None).await?, "W")?;
    let read = |revision: Option<u64>| {
        let mut args = json!({"workspace":v});
        if let Some(r) = revision {
            args["revision"] = json!(r);
        }
        w.mcp_raw(Some(&w.bot), "joppa_tree", args)
    };
    for (revision, name) in [(None, "V"), (Some(1), "V revision 1")] {
        let (code, reply) = read(revision).await?;
        let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("");
        ensure!(
            code == 200 && reply["result"]["isError"] != true,
            "{name}: {reply}"
        );
        has_system(&serde_json::from_str(text)?, name)?;
    }
    Ok(())
}

// ac:8dcnlo3s
// test:ag3sm8mm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_declares_a_flow_relation_and_a_types_only_change_keeps_it() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    let n = tree(&w, &w.owner, None).await?["schema"]["revision"]
        .as_u64()
        .unwrap_or(0);
    let swap = json!({"swap":{"title":"Swap","description":"Client screens"}});
    schema(&w, json!({"put_relations":swap}))
        .await?
        .map_err(msg)?;
    let next = registry(&tree(&w, &w.owner, Some(n + 1)).await?)["swap"].clone();
    ensure!(
        next["title"] == "Swap"
            && next["description"] == "Client screens"
            && next["system"] != true,
        "revision {}: {next}",
        n + 1
    );
    let doc = relations_world::node("Documents", "document", &[], json!({}));
    schema(&w, json!({"put_types":{"doc":doc}}))
        .await?
        .map_err(msg)?;
    let kept = registry(&tree(&w, &w.owner, None).await?);
    ensure!(
        kept["swap"] == next,
        "the put_types change keeps swap: {kept}"
    );
    // A put replaces title and description, and the server keeps system.
    let blocks = json!({"depends_on":{"title":"Blocks"}});
    schema(&w, json!({"put_relations":blocks}))
        .await?
        .map_err(msg)?;
    let depends = registry(&tree(&w, &w.owner, None).await?)["depends_on"].clone();
    ensure!(
        depends["title"] == "Blocks"
            && depends["description"].as_str().unwrap_or("").is_empty()
            && depends["system"] == true,
        "{depends}"
    );
    let bad = schema(&w, json!({"put_relations":{"bad key":{}}})).await?;
    refused(bad, "invalid id: bad key")?;
    let flagged = schema(&w, json!({"put_relations":{"x":{"system":true}}})).await?;
    refused(flagged, "system is set by the server")
}

// ac:602nwv8y
// test:7m5o1x60
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_used_or_system_relation_cannot_be_removed() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    saved(&w, "doc:a", 0, json!({"relations":{"swap":{"edges":[]}}})).await?;
    let used = schema(&w, json!({"remove_relations":["swap"]})).await?;
    refused(used, "relation swap is used by a live node")?;
    let system = schema(&w, json!({"remove_relations":["related"]})).await?;
    refused(system, "a system relation cannot be removed")?;
    change(&w, "doc:a", json!({"relations":{"swap":null}})).await?;
    schema(&w, json!({"remove_relations":["swap"]}))
        .await?
        .map_err(msg)?;
    let left = registry(&tree(&w, &w.owner, None).await?);
    ensure!(
        left.get("swap").is_none() && left["related"]["system"] == true,
        "{left}"
    );
    Ok(())
}

// ac:9ubwij03
// test:b5s5p58g
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn restore_object_refuses_a_node_with_a_removed_relation() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    saved(&w, "doc:b", 0, json!({})).await?;
    let edge = json!({"swap":{"edges":[{"to":"doc:b"}]}});
    saved(&w, "doc:a", 0, json!({"relations":edge})).await?;
    let archive = json!({"op":"archive_object","target":"doc:a","revision":1,"reason":"Done"});
    w.command(&w.owner, archive).await?.map_err(msg)?;
    // No live node uses swap after the archive.
    schema(&w, json!({"remove_relations":["swap"]}))
        .await?
        .map_err(msg)?;
    let (revision, ..) = stored(&w, "doc:a").await?;
    let restore = json!({"op":"restore_object","target":"doc:a","revision":revision,
        "reason":"Use it again"});
    refused(
        w.command(&w.owner, restore).await?,
        "declare relation swap first",
    )?;
    let (_, present, _) = stored(&w, "doc:a").await?;
    ensure!(!present, "doc:a stays archived");
    Ok(())
}
