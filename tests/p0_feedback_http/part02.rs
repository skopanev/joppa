// Part 2 of tests/p0_feedback_http.rs: ac:1w9zqzg9.

// ac:1w9zqzg9
// test:cnc6rzdj
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p0_4_contract_hash_reads_and_source_updates() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_p0_schema(&w).await?;
    for cmd in [
        json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Domain D","owner":"owner","body":"d-v1"},"reason":"Create D"}),
        json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:d","data":{"title":"Cap C","owner":"owner","body":"c-v1"},"reason":"Create C"}),
        json!({"op":"save_object","kind":"source","id":"s","revision":0,"parent":null,"data":{"title":"Source S","owner":"owner","body":"s-v1"},"reason":"Create S"}),
        json!({"op":"save_object","kind":"ac","id":"ac_src","revision":0,"parent":"capability:c","data":{"title":"AC A","owner":"owner","body":"a-v1","sources":["source:s"]},"reason":"Create A"}),
        json!({"op":"save_object","kind":"note","id":"n","revision":0,"parent":null,"data":{"title":"Inbox N","owner":"owner","body":"inbox"},"reason":"Create N"}),
    ] {
        ok(&w, cmd).await?;
    }

    let plain = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"domain:d"})).await?;
    ensure!(plain.get("contract_hash").is_none(), "plain card: {plain}");

    let obj = |args: Value| mcp_ok(&w, &w.bot, "joppa_objects", args);
    let d1 = obj(json!({"address":"domain:d","fields":["contract_hash"]})).await?;
    let c1 = obj(json!({"address":"capability:c","fields":["contract_hash","parent_hash_current"]})).await?;
    let a1 = obj(json!({"address":"ac:ac_src","fields":["contract_hash"]})).await?;
    let n1 = obj(json!({"address":"note:n","fields":["contract_hash"]})).await?;
    ensure!(
        is_sha256_hex(&d1["contract_hash"])
            && d1["contract_hash"] == c1["parent_hash_current"]
            && is_sha256_hex(&a1["contract_hash"])
            && n1["contract_hash"].is_null(),
        "initial hashes: d={d1} c={c1} a={a1} n={n1}"
    );

    let list_d = obj(json!({"kind":"domain","fields":["contract_hash"]})).await?;
    ensure!(list_d["items"][0]["contract_hash"] == d1["contract_hash"], "{list_d}");
    let many_dc = obj(json!({"address":"domain:d,capability:c","fields":["contract_hash"]})).await?;
    ensure!(many_dc["items"][0]["contract_hash"] == d1["contract_hash"], "{many_dc}");
    ensure!(http_get(&w, "address=domain:d").await?["contract_hash"] == d1["contract_hash"]);
    let http_many = http_get(&w, "address=domain:d,capability:c&fields=contract_hash").await?;
    ensure!(http_many["items"][0]["contract_hash"] == d1["contract_hash"], "{http_many}");

    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":1,"data":{"owner":"alex"},"reason":"Owner"})).await?;
    ensure!(obj(json!({"address":"domain:d","fields":["contract_hash"]})).await?["contract_hash"] == d1["contract_hash"]);

    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":2,"data":{"body":"d-v2"},"reason":"Body D"})).await?;
    let d_body = obj(json!({"address":"domain:d","fields":["contract_hash"]})).await?;
    let c_after = obj(json!({"address":"capability:c","fields":["parent_hash_current"]})).await?;
    ensure!(
        is_sha256_hex(&d_body["contract_hash"])
            && d_body["contract_hash"] != d1["contract_hash"]
            && d_body["contract_hash"] == c_after["parent_hash_current"],
        "after D body: d={d_body} c={c_after}"
    );

    ok(&w, json!({"op":"save_object","kind":"source","id":"s","revision":1,"data":{"body":"s-v2"},"reason":"Body S"})).await?;
    let a2 = obj(json!({"address":"ac:ac_src","fields":["contract_hash"]})).await?;
    ensure!(is_sha256_hex(&a2["contract_hash"]) && a2["contract_hash"] != a1["contract_hash"], "{a1} -> {a2}");
    Ok(())
}

// ac:1w9zqzg9
// test:qkmwmdhb
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p0_4_contract_hash_backfill_archived_and_refusals() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    setup_p0_schema(&w).await?;
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"D","owner":"owner","body":"body"},"reason":"Create D"})).await?;
    ok(&w, json!({"op":"save_object","kind":"capability","id":"leaf","revision":0,"parent":"domain:d","data":{"title":"Leaf","owner":"owner","body":"leaf"},"reason":"Create leaf"})).await?;
    let d_hash = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"domain:d","fields":["contract_hash"]})).await?["contract_hash"].clone();

    w.command(&w.owner, json!({"op":"archive_object","target":"capability:leaf","revision":1,"reason":"Archive leaf"})).await?.map_err(anyhow::Error::msg)?;
    let archived = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:leaf","fields":["contract_hash"]})).await?;
    ensure!(archived["contract_hash"].is_null(), "archived leaf: {archived}");

    let bad_save = save(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":1,"data":{"contract_hash":"abc"},"reason":"Bad field"})).await?;
    refused(bad_save, "contract_hash is computed by the server")?;

    let tree = mcp_ok(&w, &w.bot, "joppa_tree", json!({})).await?;
    let bad_schema = w.command(&w.owner, json!({
        "op": "set_tree_schema",
        "revision": tree["schema"]["revision"],
        "put_types": {"domain":{"title":"Domain","role":"group","root":true,"status":"children","parents":[],"fields":{"contract_hash":{"type":"string"}}}},
        "reason": "Reserved field"
    })).await?;
    refused(bad_schema, "field contract_hash is computed by the server")?;

    let pos_before = head(&w).await?;
    w.stop();
    let conn = w.pool.get().await?;
    conn.execute("UPDATE joppa.objects SET contract_hash=NULL WHERE workspace=$1", &[&w.ws]).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{parent_hash_version}','\"1\"'::jsonb) WHERE workspace=$1", &[&w.ws]).await?;
    drop(conn);
    w.start().await?;

    let conn = w.pool.get().await?;
    let row = conn.query_one(
        "SELECT first_view->>'parent_hash_version', (SELECT contract_hash FROM joppa.objects WHERE workspace=$1 AND address='domain:d') FROM joppa.board_cache WHERE workspace=$1",
        &[&w.ws],
    ).await?;
    let (ver, filled): (Option<String>, Option<String>) = (row.get(0), row.get(1));
    ensure!(ver.as_deref() == Some("2") && filled.as_deref() == d_hash.as_str() && head(&w).await? == pos_before, "ver={ver:?} filled={filled:?}");
    Ok(())
}
