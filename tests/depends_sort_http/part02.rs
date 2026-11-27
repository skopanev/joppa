// Part 2 of tests/depends_sort_http.rs: ac:h7pk3d60 (test:k079cw00 and test:lca9ucg3).

// ac:h7pk3d60
// test:k079cw00
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn transitive_dependents_and_chain_stop_on_satisfied_nodes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_dep_schema(&w).await?;
    let cookie = session(&w, "alex").await?;
    let rkey = create_key(&w, &cookie, &w.ws).await?;

    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"rq","revision":0,"parent":null,"data":{"title":"Req","owner":"owner","body":"Rule"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"ac0","revision":0,"parent":"requirement:rq","data":{"title":"AC0","owner":"owner"},"reason":"Init"})).await?;
    for id in ["c", "b", "a", "d"] {
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"ac:ac0","data":{"title":id.to_uppercase(),"owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    }
    // Chain: a -> b -> c, and d -> c.
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"b","revision":1,"relations":{"depends_on":{"edges":[{"to":"task:c"}]}},"reason":"b->c"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"a","revision":1,"relations":{"depends_on":{"edges":[{"to":"task:b"}]}},"reason":"a->b"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"d","revision":1,"relations":{"depends_on":{"edges":[{"to":"task:c"}]}},"reason":"d->c"})).await?;

    let fields = json!(["blocked", "dependents"]);
    // 1. List read with fields.
    let list = mcp_ok(&w, &w.bot, "joppa_objects", json!({"parent":"ac:ac0","fields":fields})).await?;
    ensure!(
        list["items"] == json!([
            {"address":"task:a","blocked":true,"dependents":0},
            {"address":"task:b","blocked":true,"dependents":1},
            {"address":"task:c","blocked":false,"dependents":3},
            {"address":"task:d","blocked":true,"dependents":0}
        ]),
        "list facts: {list}"
    );
    // 2. Single-address read with fields (MCP + HTTP).
    let one_c = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:c","fields":fields})).await?;
    ensure!(one_c == json!({"blocked":false,"dependents":3}), "one_c: {one_c}");
    let http_c = http_get(&w, "address=task:c").await?;
    ensure!(http_c["blocked"] == false && http_c["dependents"] == 3, "http_c: {http_c}");
    // 3. Multi-address read (MCP + HTTP) and POST /api/data.
    let many = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:c,task:b,task:a","fields":fields})).await?;
    ensure!(many["items"][0]["dependents"] == 3 && many["items"][1]["dependents"] == 1 && many["items"][2]["dependents"] == 0, "many: {many}");
    let http_many = http_get(&w, "address=task:c,task:a&fields=blocked,dependents").await?;
    ensure!(http_many["items"][0]["dependents"] == 3 && http_many["items"][1]["blocked"] == true, "http_many: {http_many}");
    let api = data_read(&w, &rkey, json!({"root":"ac:ac0","kind":"task","fields":["blocked","dependents"]})).await?;
    let c_item = api["items"].as_array().context("items")?.iter().find(|i| i["address"] == "task:c").context("c")?;
    ensure!(c_item["blocked"] == false && c_item["dependents"] == 3, "api c: {c_item}");

    // Mark intermediate node b done -> b has 0 dependents, and a stops chaining to c so c drops from 3 to 1 (only d).
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"b","revision":2,"data":{"stage":"done"},"reason":"b done"})).await?;
    let after_b = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:c,task:b,task:a","fields":fields})).await?;
    ensure!(
        after_b["items"] == json!([
            {"address":"task:c","blocked":false,"dependents":1},
            {"address":"task:b","blocked":true,"dependents":0},
            {"address":"task:a","blocked":false,"dependents":0}
        ]),
        "after b done: {after_b}"
    );
    Ok(())
}

// ac:h7pk3d60
// test:lca9ucg3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn cycle_terminates_and_plain_and_card_shapes_stay_compact() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_dep_schema(&w).await?;

    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"rq","revision":0,"parent":null,"data":{"title":"Req","owner":"owner","body":"Rule"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"ac0","revision":0,"parent":"requirement:rq","data":{"title":"AC0","owner":"owner"},"reason":"Init"})).await?;
    for id in ["e", "f", "plain"] {
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"ac:ac0","data":{"title":id,"owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    }
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"e","revision":1,"relations":{"depends_on":{"edges":[{"to":"task:f"}]}},"reason":"e->f"})).await?;
    // Simulate a historical replayed cycle f -> e directly in object_links.
    w.pool
        .get()
        .await?
        .execute(
            "INSERT INTO joppa.object_links(workspace,source,target,relation) VALUES($1,'task:f','task:e','depends_on')",
            &[&w.ws],
        )
        .await?;

    let started = std::time::Instant::now();
    let cyc = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:e,task:f","fields":["blocked","dependents"]})).await?;
    ensure!(started.elapsed().as_millis() < 500, "cycle query slow: {} ms", started.elapsed().as_millis());
    ensure!(
        cyc["items"] == json!([
            {"address":"task:e","blocked":true,"dependents":1},
            {"address":"task:f","blocked":true,"dependents":1}
        ]),
        "cycle facts: {cyc}"
    );

    // Plain list without fields keeps only address, title, status (no blocked or dependents keys).
    let plain_list = mcp_ok(&w, &w.bot, "joppa_objects", json!({"parent":"ac:ac0"})).await?;
    for item in plain_list["items"].as_array().context("items")? {
        ensure!(
            item.get("blocked").is_none() && item.get("dependents").is_none(),
            "plain list leaked dependency keys: {item}"
        );
    }

    // Card read without fields omits blocked=false and dependents=0, and includes them when true / >0.
    let card_plain = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:plain"})).await?;
    ensure!(
        card_plain.get("blocked").is_none() && card_plain.get("dependents").is_none(),
        "unblocked card leaked false/0: {card_plain}"
    );
    let card_e = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:e"})).await?;
    ensure!(
        card_e["blocked"] == true && card_e["dependents"] == 1,
        "blocked card missing active facts: {card_e}"
    );
    Ok(())
}
