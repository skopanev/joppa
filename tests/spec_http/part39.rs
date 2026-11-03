// Part 39 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// Make the test nodes directly under `parent` name exactly `tests`, as `token`: save a node
/// for each new name and archive each node whose name left the list.
async fn bind_tests(w: &World, token: &str, parent: &str, tests: &[&str]) -> Result<()> {
    let nodes = w
        .get_as(token, &format!("parent={parent}&kind=test&depth=1&limit=100"))
        .await?;
    let items = nodes["items"].as_array().cloned().unwrap_or_default();
    for item in &items {
        if !tests.iter().any(|t| item["title"] == *t) {
            let address = item["address"].as_str().unwrap_or_default();
            let revision = w.get_as(token, &format!("address={address}")).await?["current_revision"].clone();
            w.command_as(token, json!({"op":"archive_object","target":address,"revision":revision,"reason":"Unbind"}))
                .await?;
        }
    }
    for test in tests {
        if !items.iter().any(|i| i["title"] == *test) {
            w.command_as(token, json!({"op":"save_object","kind":"test","revision":0,"parent":parent,
                "data":{"title":test,"owner":"owner"},"reason":"Bind"}))
                .await?;
        }
    }
    Ok(())
}
