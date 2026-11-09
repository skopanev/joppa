//! requirement:owner-manages-workspace over a real server: a workspace that the owner hid
//! before the hidden flag became per member stays hidden only for the owner after the server
//! rebuilds it from its journal.
#[path = "common/hidden_lists.rs"]
mod hidden_lists;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use hidden_lists::{listed, web};
use record_world::World;
use serde_json::json;

/// The board projection version of the release before the per-member hidden flag.
const OLDER_VERSION: &str = "22";

/// joppa_list gives W with hidden: true to O and without hidden to alex. GET /api/workspaces of
/// alex lists W, and GET /api/workspaces of O omits it.
async fn hidden_only_for_the_owner(w: &World, moment: &str) -> Result<()> {
    let owner = listed(w, &w.owner).await?;
    ensure!(
        owner["hidden"] == true,
        "{moment}: joppa_list of O: {owner}"
    );
    let alex = listed(w, &w.alex).await?;
    ensure!(
        alex.get("hidden").is_none(),
        "{moment}: joppa_list of alex: {alex}"
    );
    let lists = web(w, &w.alex, "/api/workspaces").await?;
    ensure!(
        lists.contains(&w.ws),
        "{moment}: GET /api/workspaces of alex"
    );
    let lists = web(w, &w.owner, "/api/workspaces").await?;
    ensure!(!lists.contains(&w.ws), "{moment}: GET /api/workspaces of O");
    Ok(())
}

// ac:yaqk6paj
// test:bzpi5m0v
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_workspace_hidden_by_the_owner_before_the_change_stays_hidden_only_for_the_owner()
-> Result<()> {
    let mut w = record_world::world(&[]).await?;
    // The journal of W gets update_workspace {hidden: true} from O, as the older release wrote
    // it. Its board projection keeps the older version with the hidden flag for every member.
    let event = json!({"op":"update_workspace","hidden":true}).to_string();
    let request = uuid::Uuid::new_v4().to_string();
    w.pool.get().await?.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,'owner',$2,$3::text::jsonb FROM joppa.events WHERE workspace=$1",
        &[&w.ws, &request, &event],
    ).await?;
    let older = json!({"hidden":true,"projection_version":OLDER_VERSION}).to_string();
    let sql = format!(
        "UPDATE joppa.board_cache SET first_view=first_view||'{older}'::jsonb, position=(SELECT MAX(position) FROM joppa.events WHERE workspace=$1) WHERE workspace=$1"
    );
    // The server starts and rebuilds W.
    hidden_lists::restart_after(&mut w, &sql).await?;
    hidden_only_for_the_owner(&w, "after the first rebuild").await?;
    hidden_lists::rebuild(&mut w).await?;
    hidden_only_for_the_owner(&w, "after the second rebuild").await?;
    Ok(())
}
