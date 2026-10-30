//! The owner deletes a workspace, and no row of it stays in the database. Other workspaces
//! keep every row. A workspace that was deleted before the purge leaves at the migration.
#[path = "common/purge_world.rs"]
mod purge_world;
use anyhow::{Result, ensure};
use joppa::store;
use purge_world::{blob, empty, pool, rows, workspace, write};
use serde_json::json;

// owner-manages-workspace-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn owner_manages_workspace_ac2_a_deleted_workspace_leaves_no_row_and_admits_nobody()
-> Result<()> {
    let pool = pool().await?;
    let shared = format!("shared {}", uuid::Uuid::new_v4());
    let (w, own) = workspace(&pool, "doomed", &shared).await?;
    let (k, kept) = workspace(&pool, "kept", &shared).await?;
    let before = rows(&pool, &w).await?;
    // The fixture writes every table, except the tables of Tasks, aliases and report receipts.
    let unwritten = [
        "ac_owners",
        "object_aliases",
        "task_queue",
        "test_report_receipts",
    ];
    ensure!(
        before
            .iter()
            .all(|(t, n)| *n > 0 || unwritten.contains(&t.as_str())),
        "the fixture misses a table: {before:?}"
    );
    let other = rows(&pool, &k).await?;
    // A member who is not the owner cannot delete the workspace.
    let delete = json!({"op":"delete_workspace","reason":"Owner order"});
    ensure!(
        write(&pool, &w, "alex", delete.clone()).await.is_err(),
        "a member deleted the workspace"
    );
    ensure!(
        rows(&pool, &w).await? == before,
        "a refused deletion changed rows"
    );
    write(&pool, &w, "owner", delete).await?;
    let after = rows(&pool, &w).await?;
    ensure!(
        empty(&after),
        "rows of the deleted workspace stay: {after:?}"
    );
    // A file that only W names leaves Spaces. A file that K also names stays.
    ensure!(!blob(&own), "the file of W stays");
    ensure!(blob(&kept), "the file of K left");
    let shared_hash = joppa::digest_hex::sha256_hex(shared.as_bytes());
    ensure!(blob(&shared_hash), "the shared file left");
    ensure!(
        rows(&pool, &k).await? == other,
        "the deletion changed rows of K"
    );
    // Nobody reads W, and no later command or grant is accepted.
    for actor in ["owner", "alex"] {
        let read = store::read_objects(
            &pool,
            &w,
            actor,
            &store::ObjectScope {
                address: Some("note:n"),
                ..Default::default()
            },
        )
        .await;
        ensure!(read.is_err(), "{actor} read the deleted workspace");
    }
    ensure!(
        write(
            &pool,
            &w,
            "owner",
            json!({"op":"member","id":"late","role":"human"})
        )
        .await
        .is_err(),
        "a deleted workspace accepted a command"
    );
    ensure!(
        store::grant_email(&pool, &w, "owner", "late@example.com")
            .await
            .is_err(),
        "a deleted workspace accepted a grant"
    );
    // An access rule that still names W admits nobody and does not stop the sign-in.
    let person = format!("person-{}", uuid::Uuid::new_v4().simple());
    store::enroll(
        &pool,
        &person,
        &person,
        &format!("{person}@example.org"),
        None,
        &[w.clone()].into(),
    )
    .await?;
    ensure!(empty(&rows(&pool, &w).await?), "a refused write added rows");
    store::check_journal(&pool).await?;
    Ok(())
}

// owner-manages-workspace-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_workspace_deleted_before_the_purge_leaves_at_the_migration() -> Result<()> {
    let pool = pool().await?;
    let (w, _) = workspace(
        &pool,
        "old-deleted",
        &format!("shared {}", uuid::Uuid::new_v4()),
    )
    .await?;
    let (k, _) = workspace(
        &pool,
        "old-kept",
        &format!("shared {}", uuid::Uuid::new_v4()),
    )
    .await?;
    // The first migration gives K the epochs of the rules that a new workspace has no row for.
    store::migrate(&pool).await?;
    let other = rows(&pool, &k).await?;
    // An older server only appended the deletion event and kept every row.
    pool.get().await?.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1, max(position)+1, 'owner', $2, '{\"op\":\"delete_workspace\",\"reason\":\"Old deletion\"}'::jsonb FROM joppa.events WHERE workspace=$1", &[&w, &uuid::Uuid::new_v4().to_string()]).await?;
    store::migrate(&pool).await?;
    ensure!(
        empty(&rows(&pool, &w).await?),
        "the migration kept rows of the deleted workspace"
    );
    let now = rows(&pool, &k).await?;
    ensure!(
        now == other,
        "the migration changed rows of K: {other:?} -> {now:?}"
    );
    store::check_journal(&pool).await?;
    Ok(())
}
