// Part 12 of tests/reports_http.rs, included there, so the test names stay unchanged.

async fn post(w: &World, token: &str, path: &str, body: Value) -> Result<(u16, Value)> {
    let reply = w
        .http
        .post(format!("{}{path}", w.base))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    Ok((
        reply.status().as_u16(),
        reply.json().await.unwrap_or(Value::Null),
    ))
}
