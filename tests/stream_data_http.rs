//! Black-box check of streamed POST /api/data walk, item-by-item serialization, query pool safety, and peak RSS (ac:0jsmrkim).
#[path = "common/data_tree.rs"]
mod data_tree;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use data_tree::{node, put_types, save};
use read_key::{Auth, call, create_key, purge, session};
use serde_json::{Value, json};

fn linux_rss_kb() -> Option<i64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    text.lines().find_map(|l| {
        l.strip_prefix("VmRSS:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

async fn wait_indexed(w: &record_world::World, query: &str, least: usize) -> Result<()> {
    for _ in 0..300 {
        let page = w
            .mcp(&w.bot, "joppa_search", json!({"query": query, "limit": 25}))
            .await?;
        let items = page.map_err(anyhow::Error::msg)?["items"]
            .as_array()
            .map_or(0, Vec::len);
        if items >= least {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    anyhow::bail!("search index did not reach {least} items for {query}")
}

async fn raw_post(
    http: &reqwest::Client,
    root: &str,
    key: &str,
    body: &Value,
) -> Result<(u16, String)> {
    let res = http
        .post(format!("{root}/api/data"))
        .bearer_auth(key)
        .json(body)
        .send()
        .await?;
    Ok((res.status().as_u16(), res.text().await?))
}

// ac:0jsmrkim
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn post_api_data_streams_walk_fetches_only_selected_items_and_bounds_rss() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let result = async {
        let mut item_type = node("Items", "none", &["box"], json!({"env": {"type": "text"}}));
        item_type["field_max_length"] = json!(4096);
        put_types(
            &w,
            json!({
                "box": node("Boxes", "children", &[], json!({})),
                "item": item_type,
            }),
        )
        .await?;
        save(&w, "box:root", None, json!({"title": "Root"})).await?;

        let long_body = "d".repeat(2000);
        for chunk in 0..50 {
            let ops: Vec<Value> = (0..100)
                .map(|j| {
                    let idx = chunk * 100 + j;
                    let env = if idx % 2 == 0 { "a" } else { "b" };
                    let marker = if matches!(idx, 0 | 10 | 20 | 11) { " quaggastar" } else { "" };
                    let body = format!("{}{}", &long_body[..2000 - marker.len()], marker);
                    json!({"command": {
                        "op": "save_object",
                        "kind": "item",
                        "id": format!("i{idx:04}"),
                        "revision": 0,
                        "parent": "box:root",
                        "data": {"title": format!("Item {idx:04}"), "owner": "owner", "body": body, "env": env},
                        "reason": "Seed"
                    }})
                })
                .collect();
            w.mcp(&w.bot, "joppa_batch", json!({"request_id": format!("b-{chunk}"), "operations": ops}))
                .await?
                .map_err(anyhow::Error::msg)?;
        }
        wait_indexed(&w, "quaggastar", 4).await?;

        let cookie = session(&w, "alex").await?;
        let key = create_key(&w, &cookie, &w.ws).await?;
        let rss_before = linux_rss_kb();

        let conn = w.pool.get().await?;
        conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[]).await?;
        conn.execute("SELECT pg_stat_statements_reset()", &[]).await?;

        let req_ok = json!({"filter": {"env": ["a"]}, "kind": "item", "fields": ["title", "body", "env"], "limit": 50, "max_chars": 200000});
        let (code_ok, text_ok) = raw_post(&w.http, &w.root, &key, &req_ok).await?;
        ensure!(code_ok == 200, "expected 200, got {code_ok}: {text_ok}");
        let parsed_ok: Value = serde_json::from_str(&text_ok)?;
        let expected_text = serde_json::to_string(&json!({
            "items": parsed_ok["items"],
            "position": parsed_ok["position"],
            "workspace": parsed_ok["workspace"],
        }))?;
        ensure!(text_ok == expected_text, "reply JSON must match serde key order byte for byte");
        let items = parsed_ok["items"].as_array().context("items array")?;
        ensure!(items.len() == 50, "expected 50 items, got {}", items.len());
        ensure!(items[0]["address"] == "item:i0000" && items[49]["address"] == "item:i0098");
        ensure!(items.iter().all(|i| i["env"] == "a" && i["body"].as_str().is_some_and(|b| b.chars().count() == 2000)));

        let mut req_422 = req_ok.clone();
        req_422["max_chars"] = json!(1000);
        let (code_422, body_422) = call(&w.http, &w.root, "POST", "/api/data", &[Auth::Bearer(&key)], Some(&req_422)).await?;
        ensure!(code_422 == 422, "expected 422, got {code_422}: {body_422}");
        let wanted_err = format!("response is {} characters, max_chars is 1000", text_ok.chars().count());
        ensure!(body_422["error"] == wanted_err, "422 text mismatch: {} vs {wanted_err}", body_422["error"]);

        // Run 16 concurrent query calls to prove pool connections release before search_query::find_within.
        let req_q = json!({"filter": {"env": ["a"]}, "kind": "item", "fields": ["title", "env"], "query": "quaggastar", "limit": 25});
        let mut tasks = vec![];
        for _ in 0..16 {
            let (http, root, key, body) = (w.http.clone(), w.root.clone(), key.clone(), req_q.clone());
            tasks.push(tokio::spawn(async move { raw_post(&http, &root, &key, &body).await }));
        }
        for t in tasks {
            let (code, text) = t.await??;
            ensure!(code == 200, "query failed: {code} {text}");
            let v: Value = serde_json::from_str(&text)?;
            let found: Vec<&str> = v["items"].as_array().context("q items")?.iter().filter_map(|i| i["address"].as_str()).collect();
            ensure!(found == ["item:i0000", "item:i0010", "item:i0020"], "unexpected query hits: {found:?}");
            ensure!(v["items"][0].get("body").is_none(), "body must not appear when omitted from fields");
        }

        if let (Some(before), Some(after)) = (rss_before, linux_rss_kb()) {
            let delta_kb = after - before;
            println!("server VmRSS before={before} kB, after={after} kB, delta={delta_kb} kB");
            ensure!(delta_kb < 16384, "server VmRSS grew by {delta_kb} kB (>= 16 MB) across 18 /api/data calls on 10 MB dataset");
        }

        let walk_sql: String = conn.query_one(
            "SELECT query FROM pg_stat_statements WHERE query LIKE '%SELECT o.address,o.kind,o.local_id%' LIMIT 1",
            &[],
        ).await?.get(0);
        ensure!(!walk_sql.contains("description") && !walk_sql.contains("o.title"), "walk read heavy columns: {walk_sql}");

        let selected_rows: i64 = conn.query_one(
            "SELECT COALESCE(sum(rows)::bigint, 0) FROM pg_stat_statements WHERE query LIKE '%FROM unnest($2::text[]) WITH ORDINALITY s(address,ord)%'",
            &[],
        ).await?.get(0);
        println!("selected_rows={selected_rows} across 18 calls on 5,001 live nodes");
        ensure!(selected_rows == 148, "expected 50 + 50 + 16*3 = 148 selected rows, got {selected_rows}");
        Ok(())
    }
    .await;
    purge(&w.pool, &[&w.ws]).await?;
    result
}
