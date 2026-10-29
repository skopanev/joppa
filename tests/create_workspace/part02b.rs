// Part of tests/create_workspace/part02.rs: the tail of the test before it, included right after it.

/// Steps 2 to 7 after the refused calls: Billing is created, owned and refused as specified.
#[allow(clippy::too_many_arguments)]
async fn billing_is_created_owned_and_refused_as_specified(
    pool: &deadpool_postgres::Pool,
    id: &str,
    alex: &str,
    base: &str,
    hidden: &str,
    alex_key: &str,
    alex_cookie: &str,
    sam_cookie: &str,
    root: &str,
    client: &reqwest::Client,
) -> Result<()> {
    let get = |path: String, cookie: &str| browser_get(client, root, path, cookie);
    let create = |body: Value, cookie: Option<&str>, origin: Option<&str>| {
        browser_create(client, root, body, cookie, origin)
    };
    // Steps 3 and 5: one Workspace operation creates Billing; the reply names it.
    let response = create(json!({"title":"Billing"}), Some(alex_cookie), Some(root)).await?;
    assert_eq!(response.status(), 200);
    let created = response.json::<Value>().await?;
    let billing = created["id"].as_str().unwrap().to_owned();
    assert_eq!(created, json!({"id":billing,"title":"Billing"}));
    assert!(billing != base && billing != hidden);
    // The receipt holds the one submitted operation; internal migrations may follow it.
    let receipt = |workspace: String| {
        let pool = pool.clone();
        async move {
            let conn = pool.get().await?;
            let rows = conn
                .query(
                    "SELECT actor,request_id,input::text FROM joppa.command_receipts WHERE workspace=$1",
                    &[&workspace],
                )
                .await?;
            ensure!(rows.len() == 1, "one request per creation");
            let creations: i64 = conn
                .query_one(
                    "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='workspace'",
                    &[&workspace],
                )
                .await?
                .get(0);
            ensure!(creations == 1, "one workspace event per creation");
            let input: Value = serde_json::from_str(rows[0].get(2))?;
            anyhow::Ok((
                rows[0].get::<_, String>(0),
                rows[0].get::<_, String>(1),
                input,
            ))
        }
    };
    let (actor, request_id, input) = receipt(billing.clone()).await?;
    assert_eq!(actor, alex);
    assert_eq!(
        input,
        json!({"batch":false,"operations":[{"command":{"op":"workspace","title":"Billing"}}]})
    );
    // Step 2: a second call gets new IDs.
    let second = create(json!({"title":"Billing"}), Some(alex_cookie), Some(root))
        .await?
        .json::<Value>()
        .await?;
    let second_id = second["id"].as_str().unwrap().to_owned();
    assert_ne!(second_id, billing);
    assert_ne!(receipt(second_id.clone()).await?.1, request_id);

    // Step 6: alex owns Billing although the rules do not name it.
    let list = get("/api/workspaces".into(), alex_cookie)
        .await?
        .json::<Value>()
        .await?;
    let items = list.as_array().unwrap();
    assert!(items.contains(&json!({"id":billing,"title":"Billing"})));
    assert!(!items.iter().any(|item| item["id"] == hidden));
    let read = get(format!("/api/workspaces/{billing}"), alex_cookie).await?;
    assert_eq!(read.status(), 200);
    let read = read.json::<Value>().await?;
    // Step 7: the default read names the owner next to the unchanged members.
    assert_eq!(read["index"]["owner"], json!(alex));
    assert_eq!(read["index"]["members"], json!({alex.to_owned():"human"}));
    assert_eq!(
        get(format!("/api/workspaces/{billing}/board"), alex_cookie)
            .await?
            .status(),
        200
    );
    let base_read = get(format!("/api/workspaces/{base}"), alex_cookie)
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(base_read["index"]["owner"], "owner");
    // A member who does not own a workspace that the rules exclude still cannot see it.
    for path in [
        format!("/api/workspaces/{hidden}"),
        format!("/api/workspaces/{hidden}/board"),
    ] {
        assert_eq!(get(path, alex_cookie).await?.status(), 404);
    }
    // The second signed-in user is not a member of Billing.
    for path in [
        format!("/api/workspaces/{billing}"),
        format!("/api/workspaces/{billing}/board"),
    ] {
        let response = get(path, sam_cookie).await?;
        assert_eq!(response.status(), 404);
        assert_eq!(
            response.json::<Value>().await?,
            json!({"error":"Workspace not found"})
        );
    }
    let sam_list = get("/api/workspaces".into(), sam_cookie)
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(sam_list, json!([{"id":base,"title":base}]));
    // Ownership does not replace membership. No public command removes the owner's
    // membership, so the fixture deletes that row from the second owned workspace.
    let removed = pool
        .get()
        .await?
        .execute(
            "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2",
            &[&second_id, &alex],
        )
        .await?;
    assert_eq!(removed, 1);
    let list = get("/api/workspaces".into(), alex_cookie)
        .await?
        .json::<Value>()
        .await?;
    assert!(
        !list
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"] == second_id)
    );
    assert!(list.as_array().unwrap().iter().any(|w| w["id"] == billing));
    for path in [
        format!("/api/workspaces/{second_id}"),
        format!("/api/workspaces/{second_id}/board"),
    ] {
        let response = get(path, alex_cookie).await?;
        assert_eq!(response.status(), 404);
        assert_eq!(
            response.json::<Value>().await?,
            json!({"error":"Workspace not found"})
        );
    }

    // Step 3: the commands route keeps its refusals.
    let response = client
        .post(format!("{root}/api/workspaces/{billing}/commands"))
        .header("cookie", alex_cookie)
        .header("origin", root)
        .json(&json!({"request_id":"again","command":{"op":"workspace","title":"Again"}}))
        .send()
        .await?;
    assert_eq!(response.status(), 403, "a browser session stays read-only");
    let response = client
        .post(format!("{root}/api/workspaces/remote-{id}/commands"))
        .bearer_auth(alex_key)
        .json(&json!({"request_id":"remote","command":{"op":"workspace","title":"Remote"}}))
        .send()
        .await?;
    assert!(response.status().is_client_error());
    assert!(
        response
            .text()
            .await?
            .contains("workspace creation is available only through the server CLI")
    );

    // Step 4: an ID collision returns the existing error and rolls back.
    let unchanged = snapshot(pool).await?;
    let error = store::execute(
        pool,
        &billing,
        alex,
        "collision",
        &Command::Workspace {
            title: "Billing".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "workspace exists");
    assert_eq!(snapshot(pool).await?, unchanged);
    Ok(())
}
