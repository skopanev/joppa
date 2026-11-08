// Part 17 of tests/reports_http.rs, included there, so the test names stay unchanged.

// ac:acceptance-needs-green
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_command_acceptance_is_refused_with_the_replacement() -> Result<()> {
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    for command in [
        json!({"op":"accept_commit","target":"requirement:key","revision":1,"commit":"c1c1c1c"}),
        json!({"op":"accept","req":"key","revision":1,"run":"run-1"}),
    ] {
        let reply = w
            .http
            .post(format!("{}/commands", w.base))
            .bearer_auth(&w.owner)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        let code = reply.status().as_u16();
        let body: Value = reply.json().await?;
        let expected = joppa::removed_ops::refusal(command["op"].as_str().unwrap_or(""));
        ensure!(
            code != 200 && body["error"] == expected.as_str(),
            "{code} {body}"
        );
        ensure!(expected.contains("as a Source"), "{expected}");
    }
    // No read gives an acceptance.
    let read: Value = w
        .http
        .get(format!("{}/objects?address=requirement:key", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(read["process"].get("acceptance").is_none(), "{read}");
    Ok(())
}
