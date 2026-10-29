// Part of tests/oauth/part02.rs: the later steps of the test before it, included right after it.

/// Refresh rotation, the restart of the server and the revocation of the token family. The
/// restarted server stays up while the caller holds the returned handle.
#[allow(clippy::too_many_arguments)]
async fn rotation_restart_and_revocation(
    url: &str,
    ws: &str,
    other: &str,
    tokens: &std::path::Path,
    port: u16,
    base: &str,
    resource: &str,
    server: Server,
    http: &reqwest::Client,
    probe: &Value,
    id: &str,
    cookie: &str,
    grant: &Value,
    access: &str,
    refresh: &str,
) -> Result<Server> {
    assert_eq!(exchange(http, base, grant).await?.status(), 400);
    assert_eq!(
        http.post(resource)
            .header("Cookie", cookie)
            .json(&probe)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.post(resource)
            .bearer_auth(refresh)
            .json(&probe)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.get(format!("{base}/api/me"))
            .bearer_auth(access)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.post(resource)
            .bearer_auth(access)
            .header("Origin", "https://attacker.example")
            .json(&probe)
            .send()
            .await?
            .status(),
        403
    );
    drop(server);
    let server = start(url, tokens, port).await?;
    let r: Value = http
        .post(resource)
        .bearer_auth(access)
        .json(&probe)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert!(
        r["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("existing OAuth login")
    );
    assert_eq!(
        http.post(resource)
            .bearer_auth(access)
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await?
            .status(),
        202
    );
    for (target, denied) in [(ws, false), (other, true)] {
        let r:Value=http.post(resource).bearer_auth(access).json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"joppa_tree","arguments":{"workspace":target}}})).send().await?.json().await?;
        assert_eq!(r["result"]["isError"] == true, denied);
    }
    let rotation = json!({"grant_type":"refresh_token","client_id":id,"refresh_token":refresh,"resource":resource});
    let next: Value = exchange(http, base, &rotation).await?.json().await?;
    let next_access = next["access_token"].as_str().unwrap();
    assert_ne!(next_access, access);
    assert_eq!(
        http.post(resource)
            .bearer_auth(next_access)
            .json(&probe)
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(exchange(http, base, &rotation).await?.status(), 400);
    for key in [access, next_access] {
        assert_eq!(
            http.post(resource)
                .bearer_auth(key)
                .json(&probe)
                .send()
                .await?
                .status(),
            401
        );
    }
    let r=exchange(http,base,&json!({"grant_type":"refresh_token","client_id":id,"refresh_token":next["refresh_token"],"resource":resource})).await?;
    assert_eq!(r.status(), 400);
    Ok(server)
}
