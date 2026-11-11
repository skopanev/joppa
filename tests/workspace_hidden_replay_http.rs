//! requirement:owner-manages-workspace over a real server: a workspace that the owner hid
//! before the hidden flag became per member stays hidden only for the owner after the server
//! starts and migrates it. The migration replays no journal.
#[path = "common/hidden_lists.rs"]
mod hidden_lists;
#[path = "common/http_command.rs"]
mod http_command;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use hidden_lists::{hide, listed, web};
// MCP hides update_workspace, member and remove_member: O sends them over HTTP.
use http_command::ok;
use record_world::World;
use serde_json::json;

/// The board projection version of the release before the per-member hidden flag (ea2c146).
/// The migration of the hidden flag keeps it.
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

/// Store the meta row and the board row of `ws` as the older binary (ea2c146) stored them. The
/// meta value has "hidden": true only when `hidden`, and no hidden_by. first_view has "hidden",
/// no hidden_by and the older projection version. The board row is at the journal end.
async fn store_older_form(w: &World, ws: &str, hidden: bool) -> Result<()> {
    let conn = w.pool.get().await?;
    let meta = "UPDATE joppa.state_entries SET value=(value-'hidden_by')||CASE WHEN $2 THEN '{\"hidden\":true}'::jsonb ELSE '{}'::jsonb END WHERE workspace=$1 AND section='meta' AND key=''";
    let rows = conn.execute(meta, &[&ws, &hidden]).await?;
    ensure!(rows == 1, "meta row of {ws}");
    let board = "UPDATE joppa.board_cache SET first_view=(first_view-'hidden_by')||jsonb_build_object('hidden',$2::bool,'projection_version',$3::text), position=(SELECT MAX(position) FROM joppa.events WHERE workspace=$1) WHERE workspace=$1";
    let rows = conn.execute(board, &[&ws, &hidden, &OLDER_VERSION]).await?;
    ensure!(rows == 1, "board row of {ws}");
    Ok(())
}

/// The board row of `ws`: its xmin, first_view.projection_version and the xmin of its meta row.
/// The test does not read the older "hidden" key: the fix can keep it next to hidden_by.
async fn rows_of(w: &World, ws: &str) -> Result<(String, String, String)> {
    let sql = "SELECT b.xmin::text,COALESCE(b.first_view->>'projection_version',''),(SELECT s.xmin::text FROM joppa.state_entries s WHERE s.workspace=b.workspace AND s.section='meta' AND s.key='') FROM joppa.board_cache b WHERE b.workspace=$1";
    let row = w.pool.get().await?.query_one(sql, &[&ws]).await?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

// ac:yaqk6paj
// test:bzpi5m0v
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_workspace_hidden_by_the_owner_before_the_change_stays_hidden_only_for_the_owner()
-> Result<()> {
    let mut w = record_world::world(&[]).await?;
    // Workspace V of the same owner and alex never hid.
    let v = hidden_lists::second_workspace(&w).await?;
    // The journal of W gets several events from O. The last update_workspace {hidden: true}
    // wins, and O removes alex and adds alex again.
    for command in [
        hide(true),
        hide(false),
        json!({"op":"remove_member","id":"alex","reason":"Left the team"}),
        json!({"op":"member","id":"alex","role":"human"}),
        hide(true),
    ] {
        ok(&w, &w.owner, command).await?;
    }
    w.stop();
    // W and V keep the older stored form with the older projection version.
    store_older_form(&w, &w.ws, true).await?;
    store_older_form(&w, &v, false).await?;
    let stored_v = rows_of(&w, &v).await?;
    // The server starts and migrates.
    w.start().await?;
    hidden_only_for_the_owner(&w, "after the first start").await?;
    let first = (rows_of(&w, &w.ws).await?, rows_of(&w, &v).await?);
    eprintln!("after the first start: W {:?}, V {:?}", first.0, first.1);
    ensure!(
        first.0.1 == OLDER_VERSION && first.1.1 == OLDER_VERSION,
        "the board projection version stays {OLDER_VERSION}: W {:?}, V {:?}",
        first.0,
        first.1
    );
    ensure!(
        first.1 == stored_v,
        "the board row and the meta row of V keep their xmin: before {stored_v:?}, after {:?}",
        first.1
    );
    // A second start changes no row.
    w.stop();
    w.start().await?;
    hidden_only_for_the_owner(&w, "after the second start").await?;
    let second = (rows_of(&w, &w.ws).await?, rows_of(&w, &v).await?);
    eprintln!("after the second start: W {:?}, V {:?}", second.0, second.1);
    ensure!(
        second == first,
        "a second start changes no row: first {first:?}, second {second:?}"
    );
    // A full replay of the journal of W gives the same lists as the migration.
    hidden_lists::rebuild(&mut w).await?;
    hidden_only_for_the_owner(&w, "after a full rebuild").await?;
    Ok(())
}
