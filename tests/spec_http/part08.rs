// Part 8 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_comment_records_its_author_and_server_time() -> Result<()> {
    // comments-on-object-ac1
    let w = world().await?;
    let clock = w.pool.get().await?;
    let server_time = || async {
        let time: String = clock
            .query_one("SELECT clock_timestamp()::text", &[])
            .await?
            .get(0);
        Ok::<_, anyhow::Error>(
            chrono::DateTime::parse_from_str(&time, "%Y-%m-%d %H:%M:%S%.f%#z")?
                .with_timezone(&chrono::Utc),
        )
    };
    w.note("input", 0, "Input", "Context", json!([])).await?;
    for address in [
        "domain:security",
        "capability:restrict",
        "requirement:key",
        "ac:key-rights",
        "note:input",
    ] {
        let before = server_time().await?;
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(&w.alex)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"note","target":address,"id":"n1","text":"Clarify the boundary"}}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let object = w.get(&format!("address={address}")).await?;
        let comment = &object["annotations"]["comments"]["n1"];
        ensure!(comment["author"] == "alex", "{comment}");
        ensure!(comment.get("actor_type").is_none(), "{comment}");
        ensure!(comment["actor"] == comment["author"], "{comment}");
        ensure!(comment["at"] == comment["created_at"], "{comment}");
        let created = chrono::DateTime::parse_from_str(
            comment["created_at"].as_str().unwrap_or(""),
            "%Y-%m-%d %H:%M:%S%.f%#z",
        )?
        .with_timezone(&chrono::Utc);
        let after = server_time().await?;
        ensure!(
            created >= before && created <= after,
            "{comment}; server interval {before} to {after}"
        );
        ensure!(
            (created - before).num_milliseconds() <= 5000,
            "comment time exceeds the five second write interval: {comment}"
        );
        let response = w
            .http
            .post(format!("{}/mcp", w.root))
            .bearer_auth(&w.alex)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":address}}}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let reply: Value = response.json().await?;
        let object: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        // MCP gives the same comment in its page, without the repeated author and created_at.
        let item = object["comments"]["items"]
            .as_array()
            .and_then(|items| items.iter().find(|c| c["id"] == "n1"))
            .cloned()
            .unwrap_or_default();
        ensure!(
            item["actor"] == comment["author"]
                && item.get("actor_type").is_none()
                && item["at"] == comment["created_at"]
                && item["text"] == comment["text"]
                && item.get("author").is_none(),
            "{reply}"
        );
    }
    Ok(())
}
