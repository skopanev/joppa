// Part 36 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// One note save command with the given links.
fn note_save(id: &str, revision: u32, body: &str, links: Value) -> Value {
    json!({"op":"save_object","kind":"note","id":id,"revision":revision,"parent":null,"data":{"title":id,"owner":"owner","body":body},"links":links,"reason":"Save"})
}

/// Bot sends a write over HTTP. The write must fail with exactly `error` and change no row.
async fn bot_refused_over_http(w: &World, command: Value, error: &str) -> Result<()> {
    let before = saved_rows(w).await?;
    let (ok, body) = send_as(w, &w.bot, &command).await?;
    ensure!(!ok && body["error"] == error, "{command}: {body}");
    ensure!(
        saved_rows(w).await? == before,
        "{command}: a refusal changed state"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_makes_a_blocking_dependency_cycle_over_http_and_is_refused() -> Result<()> {
    // blocking-pins-revision-ac2: A blocks B, so B has a blocking dependency on A.
    let w = world().await?;
    w.command(note_save("a", 0, "A", json!([]))).await?;
    let blocks_a = json!([{"target":"note:a","relation":"depends_on","blocking":true}]);
    w.command(note_save("b", 0, "B", blocks_a)).await?;
    let before = w.get("address=note:a").await?;
    ensure!(before["current_revision"] == 1, "{before}");
    // Bot makes B block A. The link names no revision, as in blocking-pins-revision-ac1.
    let blocks_b = json!([{"target":"note:b","relation":"depends_on","blocking":true}]);
    bot_refused_over_http(&w, note_save("a", 1, "A", blocks_b), "dependency cycle").await?;
    let after = w.get("address=note:a").await?;
    ensure!(
        after["current_revision"] == 1
            && after["document"]["links"] == json!([])
            && after["links"] == json!([]),
        "{after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_saves_an_object_over_http_and_adds_exactly_one_revision() -> Result<()> {
    // edit-makes-revision-ac1: the object is at revision R=2 before the save.
    let w = world().await?;
    w.command(note_save("a", 0, "First", json!([]))).await?;
    w.command(note_save("a", 1, "Second", json!([]))).await?;
    let read = w.get_as(&w.bot, "address=note:a").await?;
    let r = read["current_revision"]
        .as_u64()
        .context("current revision")?;
    ensure!(r == 2 && read["document"]["revision"] == r, "{read}");
    // Bot changes the data that it read and saves it with revision=R. The read data
    // also holds the note status that the server sets.
    let mut data = read["document"]["data"].clone();
    ensure!(data["status"] == "unprocessed", "{read}");
    data["title"] = json!("Changed title");
    data["body"] = json!("Changed body");
    let (ok, body) = send_as(
        &w,
        &w.bot,
        &json!({"op":"save_object","kind":"note","id":"a","revision":r,"parent":null,"data":data,"links":[],"reason":"Change"}),
    )
    .await?;
    ensure!(ok, "{body}");
    // The API read gives revision R+1, and the data equals the saved data.
    let after = w.get_as(&w.bot, "address=note:a").await?;
    ensure!(
        after["current_revision"] == r + 1
            && after["document"]["revision"] == r + 1
            && after["document"]["actor"] == "bot"
            && after["document"]["data"] == data,
        "{after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_saves_on_a_stale_revision_over_http_and_is_refused() -> Result<()> {
    // edit-makes-revision-ac2: the object is at R+1=2, and bot saves with revision=R=1.
    let w = world().await?;
    w.command(note_save("a", 0, "First", json!([]))).await?;
    w.command(note_save("a", 1, "Second", json!([]))).await?;
    let before = w.get_as(&w.bot, "address=note:a").await?;
    ensure!(before["current_revision"] == 2, "{before}");
    bot_refused_over_http(
        &w,
        note_save("a", 1, "Stale change", json!([])),
        "stale object revision; read the object again",
    )
    .await?;
    let after = w.get_as(&w.bot, "address=note:a").await?;
    ensure!(
        after["current_revision"] == 2
            && after["document"]["revision"] == 2
            && after["document"]["data"] == before["document"]["data"],
        "{after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_saves_a_test_list_on_a_task_over_http_and_is_refused() -> Result<()> {
    // foreign-check-ignored-ac1: task T is under ac:key-rights.
    let w = world().await?;
    let task = ready_task(&w, "t", "key-rights").await?;
    bot_refused_over_http(
        &w,
        json!({"op":"set_ac_tests","target":task,"tests":["checks.rs > key rights"],"reason":"Wrong target"}),
        "tests are listed on criteria, not on tasks",
    )
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_saves_a_link_with_an_unknown_relation_over_http_and_is_refused() -> Result<()> {
    // typed-links-ac2: two notes, and bot links them with relation "likes".
    let w = world().await?;
    w.command(note_save("a", 0, "A", json!([]))).await?;
    w.command(note_save("b", 0, "B", json!([]))).await?;
    let likes = json!([{"target":"note:a","relation":"likes"}]);
    bot_refused_over_http(&w, note_save("b", 1, "B", likes), "unknown link relation").await?;
    // No link is stored in either direction.
    for address in ["note:a", "note:b"] {
        let read = w.get(&format!("address={address}")).await?;
        ensure!(
            read["current_revision"] == 1
                && read["document"]["links"] == json!([])
                && read["links"] == json!([])
                && read["incoming"] == json!([]),
            "{read}"
        );
    }
    Ok(())
}
