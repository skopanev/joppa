// Part 35 of tests/reports_http.rs: the test nodes of a criterion over HTTP.

/// The direct test nodes of `parent` in the workspace at `base`, read with `token`.
async fn test_nodes(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    parent: &str,
) -> Result<Vec<Value>> {
    let url = format!("{base}/objects?parent={parent}&kind=test&depth=1&limit=100");
    let page: Value = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(page["items"].as_array().cloned().unwrap_or_default())
}

/// The commands that make the test nodes directly under `parent` name exactly `tests`:
/// an archive of each node whose name left the list and a save of each new name.
async fn bind_commands(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    parent: &str,
    tests: &[&str],
) -> Result<Vec<Value>> {
    let nodes = test_nodes(http, base, token, parent).await?;
    let mut commands = vec![];
    for node in &nodes {
        if !tests.iter().any(|t| node["title"] == *t) {
            let address = node["address"].as_str().unwrap_or_default();
            let url = format!("{base}/objects?address={address}");
            let read: Value = http
                .get(url)
                .bearer_auth(token)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            commands.push(json!({"op":"archive_object","target":address,"revision":read["current_revision"],"reason":"Unbind"}));
        }
    }
    for test in tests {
        if !nodes.iter().any(|n| n["title"] == *test) {
            commands.push(
                json!({"op":"save_object","kind":"test","revision":0,"parent":parent,
                "data":{"title":test,"owner":"owner"},"reason":"Bind"}),
            );
        }
    }
    Ok(commands)
}

/// Send the commands of bind_commands one by one with `token`.
async fn bind_at(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    parent: &str,
    tests: &[&str],
) -> Result<()> {
    for command in bind_commands(http, base, token, parent, tests).await? {
        let body = json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command});
        let reply = http
            .post(format!("{base}/commands"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "bind {parent}: {}",
            reply.text().await?
        );
    }
    Ok(())
}

/// The displayed tone of `address` in the HTTP object read of the workspace at `base`:
/// the HTTP read gives it as signal.tone.
async fn tone_at(http: &reqwest::Client, base: &str, token: &str, address: &str) -> Result<Value> {
    let url = format!("{base}/objects?address={address}");
    let read: Value = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(read["signal"]["tone"].clone())
}

impl World {
    /// The tone of `address` in the joppa_objects read of this world with `token`.
    async fn mcp_tone(&self, token: &str, address: &str) -> Result<Value> {
        let args = json!({"workspace":self.root_ws().1,"address":address});
        Ok(mcp_read(self, token, "joppa_objects", args).await?["status"]["tone"].clone())
    }
}

/// The test names of one test status read, sorted: the nodes come in address order.
fn listed(status: &Value) -> Vec<&str> {
    let mut names: Vec<&str> = status["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["address"].as_str())
        .collect();
    names.sort_unstable();
    names
}

/// The test names of one test status read as JSON values, sorted.
fn addresses(status: &Value) -> Vec<Value> {
    listed(status).into_iter().map(Value::from).collect()
}

/// The entry of the test `name` in one test status read, or null.
fn test_of<'a>(status: &'a Value, name: &str) -> &'a Value {
    status["tests"]
        .as_array()
        .and_then(|tests| tests.iter().find(|t| t["address"] == name))
        .unwrap_or(&Value::Null)
}
