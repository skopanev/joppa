// Part 3 of tests/depends_sort_http.rs: ac:tzshgbmy (test:fo6z8upz and test:m9gxpivu).

// ac:tzshgbmy
// test:fo6z8upz
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn generic_order_sorts_built_ins_enums_numbers_and_computed_fields() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_dep_schema(&w).await?;

    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"rcore","revision":0,"parent":null,"data":{"title":"Core","owner":"owner","body":"B","lane":"core"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"redge","revision":0,"parent":null,"data":{"title":"Edge","owner":"owner","body":"B","lane":"edge"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"ac_core","revision":0,"parent":"requirement:rcore","data":{"title":"AC Core","owner":"owner","stage":"to_implement","score":2},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"ac_edge","revision":0,"parent":"requirement:redge","data":{"title":"AC Edge","owner":"owner","stage":"to_decide","score":10},"reason":"Init"})).await?;

    // Create tasks in creation order: t_old (low, points 10, under ac_edge), t_mid (urgent, points 2, under ac_core),
    // t_unprio (no priority, points 100, under ac_core), t_waiter (depends on t_old so t_old gets dependents=1).
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_old","revision":0,"parent":"ac:ac_edge","data":{"title":"Old","owner":"owner","stage":"doing","priority":"low","points":10},"reason":"1"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_mid","revision":0,"parent":"ac:ac_core","data":{"title":"Mid","owner":"owner","stage":"todo","priority":"urgent","points":2},"reason":"2"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_unprio","revision":0,"parent":"ac:ac_core","data":{"title":"Unprio","owner":"owner","stage":"todo"},"reason":"3"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_waiter","revision":0,"parent":"ac:ac_core","data":{"title":"Waiter","owner":"owner","stage":"todo","priority":"high","points":5},"relations":{"depends_on":{"edges":[{"to":"task:t_old"}]}},"reason":"4"})).await?;

    // 1. Work-selector order: blocked=false, order=[dependents desc, data.priority asc, age asc].
    // t_old (dependents=1) comes first, then t_mid (dependents=0, priority=urgent), then t_unprio (dependents=0, priority=null last).
    let sel = mcp_ok(
        &w,
        &w.bot,
        "joppa_objects",
        json!({"kind":"task","blocked":false,"order":["dependents desc","data.priority asc","age asc"]}),
    )
    .await?;
    ensure!(
        item_addrs(&sel) == ["task:t_old", "task:t_mid", "task:t_unprio"],
        "selector order: {sel}"
    );
    let http_sel = http_get(&w, "kind=task&blocked=false&order=dependents%20desc,priority%20asc,age%20asc").await?;
    ensure!(item_addrs(&http_sel) == ["task:t_old", "task:t_mid", "task:t_unprio"], "http selector: {http_sel}");

    // 2. Numeric field sort (10 > 5 > 2, absent last on both desc and asc!).
    let num_desc = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":"points desc"})).await?;
    ensure!(
        item_addrs(&num_desc) == ["task:t_old", "task:t_waiter", "task:t_mid", "task:t_unprio"],
        "numeric desc (10 > 5 > 2 > null): {num_desc}"
    );
    let num_asc = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":"data.points asc"})).await?;
    ensure!(
        item_addrs(&num_asc) == ["task:t_mid", "task:t_waiter", "task:t_old", "task:t_unprio"],
        "numeric asc (2 < 5 < 10 < null): {num_asc}"
    );

    // 3. Schema enum order (stage values: ["todo", "doing", "done"] -> todo comes before doing even though "t" > "d"!).
    let stg = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":["stage asc","age asc"]})).await?;
    ensure!(
        item_addrs(&stg) == ["task:t_mid", "task:t_unprio", "task:t_waiter", "task:t_old"],
        "enum order (todo before doing): {stg}"
    );

    // 4. Computed field order (lane from parent: ["core", "edge"]) + age desc tie-break.
    let comp = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":["lane asc","age desc"]})).await?;
    ensure!(
        item_addrs(&comp) == ["task:t_waiter", "task:t_unprio", "task:t_mid", "task:t_old"],
        "computed lane + age desc: {comp}"
    );

    // 5. Snapshot pagination (<position>:@<offset>) across pages of size 2.
    let p1 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":"age asc","limit":2})).await?;
    ensure!(item_addrs(&p1) == ["task:t_old", "task:t_mid"], "p1: {p1}");
    let cur = p1["next_cursor"].as_str().context("next_cursor")?;
    ensure!(cur.contains(":@2"), "snapshot cursor shape: {cur}");
    let p2 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":"age asc","limit":2,"cursor":cur})).await?;
    ensure!(item_addrs(&p2) == ["task:t_unprio", "task:t_waiter"] && p2.get("next_cursor").is_none(), "p2: {p2}");
    Ok(())
}

// ac:tzshgbmy
// test:m9gxpivu
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn order_refuses_stale_cursors_bad_clauses_and_non_list_modes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_dep_schema(&w).await?;
    for id in ["k1", "k2", "k3"] {
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"ac:a","data":{"title":id,"owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    }

    let p1 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","order":"age asc","limit":1})).await?;
    let cur = p1["next_cursor"].as_str().context("cursor")?.to_owned();
    // Mutate workspace so snapshot advances -> cursor must be refused!
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"k1","revision":1,"data":{"stage":"doing"},"reason":"Bump"})).await?;
    let stale = w.mcp(&w.bot, "joppa_objects", json!({"kind":"task","order":"age asc","limit":1,"cursor":cur})).await?;
    ensure!(
        stale == Err("object list changed, start again without cursor".into()),
        "stale cursor: {stale:?}"
    );

    // Bad clauses: empty, 5 clauses, bad direction, duplicate canonical key, invalid identifier.
    for bad in [
        json!(""),
        json!([]),
        json!(["age","title","revision","status","dependents"]),
        json!("dependents sideways"),
        json!(["priority asc","data.priority desc"]),
        json!("bad$field"),
    ] {
        let r = w.mcp(&w.bot, "joppa_objects", json!({"kind":"task","order":bad})).await?;
        ensure!(r.is_err(), "expected refusal for {bad}: {r:?}");
    }

    // Non-list modes refused even when order is "address".
    for args in [
        json!({"address":"ac:a","order":"address"}),
        json!({"relations_of":"ac:a","order":"address"}),
        json!({"in_relation":"depends_on","order":"address"}),
        json!({"count":true,"order":"address"}),
        json!({"group_by":["kind"],"order":"dependents desc"}),
    ] {
        let r = w.mcp(&w.bot, "joppa_objects", args.clone()).await?;
        ensure!(
            r == Err("order applies only to object list reads".into()),
            "expected non-list refusal for {args}: {r:?}"
        );
    }
    let (_, http_msg) = http_err(&w, "count=true&order=age").await?;
    ensure!(http_msg.contains("order applies only to object list reads"), "http err: {http_msg}");
    Ok(())
}
