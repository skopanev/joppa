// Part 6 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

/// Every read that the restart must keep on one transport: the Task fields, the
/// revision history of each object and, on HTTP, the test reply of criterion A.
async fn journal_snapshot(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
    addresses: &[String],
) -> Result<BTreeMap<String, Value>> {
    let get = |path: String, tool: &'static str, arguments: Value| {
        fetch(http, root, ws, token, mcp, (path, tool, arguments))
    };
    let mut out = BTreeMap::new();
    for task in ["work:t1", "work:t2"] {
        let t = get(
            format!("objects?address={task}"),
            "joppa_objects",
            json!({"address":task}),
        )
        .await?;
        // HTTP keeps comments as a map by ID; MCP gives a page of items. Both give the
        // same ID, text, actor and reply.
        let entries: Vec<(String, Value)> = match t["comments"]["items"].as_array() {
            Some(items) => items
                .iter()
                .map(|c| (c["id"].as_str().unwrap_or_default().to_owned(), c.clone()))
                .collect(),
            None => t["annotations"]["comments"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(id, c)| (id.clone(), c.clone()))
                .collect(),
        };
        let comments: serde_json::Map<String, Value> = entries
            .into_iter()
            .map(|(id, c)| {
                let reply_to = c.get("reply_to").cloned().unwrap_or(Value::Null);
                (
                    id,
                    json!({"text": c["text"], "actor": c["actor"], "reply_to": reply_to}),
                )
            })
            .collect();
        out.insert(
            format!("task {task}"),
            json!({
                "status": t["process"]["status"],
                "assignee": t["process"]["assignee"],
                // An MCP detail names the current revision `revision`.
                "current_revision": if mcp { &t["revision"] } else { &t["current_revision"] },
                "comments": comments,
            }),
        );
    }
    for address in addresses {
        let history = get(
            format!("objects?address={address}&history=true"),
            "joppa_objects",
            json!({"address":address,"history":true}),
        )
        .await?;
        let count = listed(&history, "revisions").as_array().map_or(0, Vec::len) as u64;
        ensure!(
            count > 0 && history["current_revision"] == count,
            "{address}: no revisions: {history}"
        );
        out.insert(
            format!("history {address}"),
            listed(&history, "revisions").clone(),
        );
        for n in 1..=count {
            let revision = get(
                format!("objects?address={address}&revision={n}"),
                "joppa_objects",
                json!({"address":address,"revision":n}),
            )
            .await?;
            ensure!(
                revision["document"]["revision"] == n,
                "{address} revision {n}: {revision}"
            );
            // The time of the last status event is display state that a rebuild takes from
            // the document; the AC compares documents, revisions, comments and test results.
            let mut revision = revision;
            if let Some(signal) = revision["signal"].as_object_mut() {
                signal.remove("at");
            }
            out.insert(format!("revision {address} {n}"), revision);
        }
    }
    if !mcp {
        let tests = get("tests?ac=criterion:a".into(), "", Value::Null).await?;
        out.insert("tests criterion:a".into(), tests);
    }
    Ok(out)
}

/// Add one line to `out` for each JSON path below `at` where `old` and `new` differ.
fn differences(at: &str, old: &Value, new: &Value, out: &mut Vec<String>) {
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            for key in o.keys().chain(n.keys().filter(|k| !o.contains_key(*k))) {
                let missing = Value::Null;
                let (a, b) = (
                    o.get(key).unwrap_or(&missing),
                    n.get(key).unwrap_or(&missing),
                );
                if o.contains_key(key) != n.contains_key(key) {
                    out.push(format!("{at}.{key}: before {a}, after {b}"));
                } else {
                    differences(&format!("{at}.{key}"), a, b, out);
                }
            }
        }
        (Value::Array(o), Value::Array(n)) if o.len() == n.len() => {
            for (i, (a, b)) in o.iter().zip(n).enumerate() {
                differences(&format!("{at}[{i}]"), a, b, out);
            }
        }
        _ if old != new => out.push(format!("{at}: before {old}, after {new}")),
        _ => {}
    }
}
