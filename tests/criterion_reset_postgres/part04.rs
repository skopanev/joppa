// Part 4 of tests/criterion_reset_postgres.rs: any change of a node resets its criteria.

/// Set the epoch of any_change_reset, replay the journal and give the status of A and D.
async fn replay_from(pool: &deadpool_postgres::Pool, ws: &str, epoch: i64) -> Result<(Value, Value)> {
    let sql = "UPDATE joppa.replay_epochs SET first_position=$2 WHERE workspace=$1 AND rule='any_change_reset'";
    pool.get().await?.execute(sql, &[&ws, &epoch]).await?;
    let replayed = replay(pool, ws).await?;
    Ok((replayed["criterion:a"]["status"]["status"].clone(), replayed["criterion:d"]["status"]["status"].clone()))
}

/// The current revision of `address`.
async fn revision(pool: &deadpool_postgres::Pool, ws: &str, address: &str) -> Result<Value> {
    Ok(read(pool, ws, address).await?["current_revision"].clone())
}

// ac:lkcs7om2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_data_value_or_link_resets_the_criteria_below_and_a_sibling_save_does_not() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let green = |a: &str, d: &str| (json!(a), json!(d));
    // Only data.env of A changes: A turns grey, D stays green.
    let a = revision(&pool, &ws, "criterion:a").await?;
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":"a","revision":a,"parent":"spec:r","data":{"title":"a","owner":"owner","env":"dev"},"reason":"Name the environment"})).await?;
    ensure!(status(&pool, &ws, "criterion:a").await?["status"] == "grey", "env change");
    ensure!(status(&pool, &ws, "criterion:d").await?["status"] == "green", "D after the env change");
    // Before its epoch, the same change kept A green in a replay. From its epoch, it resets A.
    let after = events(&pool, &ws).await? + 1;
    ensure!(replay_from(&pool, &ws, after).await? == green("green", "green"), "replay before the epoch");
    ensure!(replay_from(&pool, &ws, 1).await? == green("grey", "green"), "replay from the epoch");
    upload_c1(&pool, &ws).await?;
    ensure!(status(&pool, &ws, "criterion:a").await?["status"] == "green", "a new record");
    // A new link of spec R resets A below it.
    let r = revision(&pool, &ws, "spec:r").await?;
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"spec","id":"r","revision":r,"parent":"area:g","data":{"title":"r","body":"Implement the rule","owner":"owner"},"links":[{"target":"spec:rh","relation":"related"}],"reason":"Relate the rules"})).await?;
    ensure!(status(&pool, &ws, "criterion:a").await?["status"] == "grey", "link change");
    upload_c1(&pool, &ws).await?;
    // A new sibling C under R gives R a new revision with the same parent, links and data:
    // A stays green.
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":"c","revision":0,"parent":"spec:r","data":{"title":"c","owner":"owner"},"reason":"Create the criterion"})).await?;
    ensure!(status(&pool, &ws, "criterion:a").await?["status"] == "green", "sibling save");
    let live = snapshot(&pool, &ws).await?;
    ensure!(replay(&pool, &ws).await? == live, "replay");
    Ok(())
}
