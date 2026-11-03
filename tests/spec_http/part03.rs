// Part 3 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_upload_a_file() -> Result<()> {
    // files-keep-versions-ac2
    let w = world().await?;
    w.note(
        "file-target",
        0,
        "Private files",
        "Workspace members only",
        json!([]),
    )
    .await?;
    let bytes = format!("Unaccepted file {}", uuid::Uuid::new_v4());
    let hash = joppa::digest_hex::sha256_hex(bytes.as_bytes());
    let before = w.get("address=note:file-target").await?;
    ensure!(before["files"] == json!([]), "{before}");
    let mut replies = Vec::new();
    for workspace in [
        w.ws.clone(),
        format!("absent-{}", uuid::Uuid::new_v4().simple()),
    ] {
        let response = w
            .http
            .post(format!("{}/api/workspaces/{workspace}/blobs", w.root))
            .bearer_auth(&w.eve)
            .body(bytes.clone())
            .send()
            .await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await?;
        ensure!(
            status == 404 && body == json!({"error":"Workspace not found"}),
            "{status}: {body}"
        );
        replies.push((status, body));
        let after = w.get("address=note:file-target").await?;
        ensure!(
            after == before,
            "A refused upload changed the object: {after}"
        );
        ensure!(
            !fake_spaces::start().any(&hash),
            "A refused upload stored file content"
        );
    }
    ensure!(
        replies[0] == replies[1],
        "Upload replies expose workspace existence: {replies:?}"
    );
    Ok(())
}
