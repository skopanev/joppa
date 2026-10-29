// Part 2 of tests/read_pages_http.rs, included there, so the test names stay unchanged.

// read-list-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn plain_and_fields_pages_share_address_cursors() -> Result<()> {
    let w = record_world::world(&[]).await?;
    save_all(&w, "read-pages-sweep", 30, |i| {
        json!({"op":"save_object","kind":"ac","id":format!("b{i:02}"),"revision":0,
            "parent":"requirement:r","data":{"title":format!("B{i:02}"),"owner":"owner"},
            "reason":"Sweep test"})
    })
    .await?;
    let plain = read(&w, "joppa_objects", json!({"parent":"requirement:r"})).await?;
    let first = addresses(&plain);
    ensure!(
        first.len() == 25 && plain["next_cursor"] == first[24].as_str(),
        "{plain}"
    );
    // A page with fields continues the plain sweep, from the same snapshot position.
    let named = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["revision"],"cursor":plain["next_cursor"]}),
    )
    .await?;
    let rest = addresses(&named);
    ensure!(
        rest.len() == 6
            && rest[0] > first[24]
            && named["items"][0]["revision"] == 1
            && named.get("next_cursor").is_none()
            && plain["position"].is_number()
            && named["position"] == plain["position"]
            && named["workspace"] == plain["workspace"]
            && plain["total"].as_u64() >= Some(30)
            && named["total"] == plain["total"],
        "{named}"
    );
    // And the other way: a fields page gives a bare address that a plain page takes.
    let named = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["revision"]}),
    )
    .await?;
    let cursor = named["next_cursor"].as_str().unwrap_or_default().to_owned();
    ensure!(
        cursor == first[24] && named["total"] == plain["total"],
        "{named}"
    );
    let plain = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","cursor":cursor}),
    )
    .await?;
    ensure!(addresses(&plain) == rest, "{plain}");
    // The position belongs to the snapshot: a detail read without a write between gives the
    // same position, and a write moves it.
    let detail = read(&w, "joppa_objects", json!({"address":"ac:b00"})).await?;
    ensure!(detail["position"] == plain["position"], "{detail} {plain}");
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"ac","id":"b99","revision":0,"parent":"requirement:r",
            "data":{"title":"B99","owner":"owner"},"reason":"Sweep test"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let moved = read(&w, "joppa_objects", json!({"parent":"requirement:r"})).await?;
    ensure!(
        moved["position"].as_i64() > plain["position"].as_i64(),
        "{moved} {plain}"
    );
    // A cursor that is not an address is refused, not read from the start.
    for args in [
        json!({"parent":"requirement:r","cursor":"25"}),
        json!({"parent":"requirement:r","fields":["revision"],"cursor":"25"}),
    ] {
        let refused = w.mcp(&w.bot, "joppa_objects", args).await?;
        ensure!(
            refused
                .err()
                .unwrap_or_default()
                .contains("invalid object cursor"),
            "the cursor 25 passed"
        );
    }
    Ok(())
}
