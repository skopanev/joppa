// Part 2 of tests/read_statements_postgres.rs, included there, so the test names stay unchanged.

async fn ok(w: &record_world::World, args: Value) -> Result<Value> {
    w.mcp(&w.bot, "joppa_objects", args)
        .await?
        .map_err(anyhow::Error::msg)
}

/// The public item IDs of the first `n` criteria of the fixture, in their order.
async fn public_ids(w: &record_world::World, n: usize) -> Result<Vec<String>> {
    let sql = "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='ac' AND local_id=$2";
    let conn = w.pool.get().await?;
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let local = format!("c{i:03}");
        ids.push(conn.query_one(sql, &[&w.ws, &local]).await?.get(0));
    }
    Ok(ids)
}

// tac:m7qjm2lj
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn two_and_25_addresses_run_the_same_statements_in_one_transaction() -> Result<()> {
    let _one = ONE.lock().await;
    let w = world(true).await?;
    let ids = public_ids(&w, 25).await?;
    let lists = [
        ("addresses", addresses(2), addresses(25)),
        ("public item IDs", ids[..2].join(","), ids.join(",")),
    ];
    for (name, two, all) in lists {
        for fields in [None, Some(json!(["title", "status"]))] {
            let mut found = vec![];
            for address in [&two, &all] {
                let mut args = json!({"address": address});
                if let Some(fields) = &fields {
                    args["fields"] = fields.clone();
                }
                let reply = measure(&w, "joppa_objects", args).await?;
                let statements = reply.map_err(anyhow::Error::msg)?;
                // One read-only snapshot for all addresses: one START TRANSACTION.
                ensure!(
                    statements.starting("start transaction") == 1,
                    "{name} {fields:?}: {statements}"
                );
                found.push(statements);
            }
            ensure!(
                found[0].total() == found[1].total(),
                "{name} {fields:?}: 2 give {} and 25 give {}",
                found[0],
                found[1]
            );
        }
    }
    Ok(())
}

// tac:m7qjm2lj
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_of_many_addresses_keeps_order_duplicates_aliases_and_unknown_objects() -> Result<()>
{
    let w = world(false).await?;
    let alias =
        "INSERT INTO joppa.object_aliases(workspace,address,target) VALUES ($1,'ac:old','ac:c003')";
    w.pool.get().await?.execute(alias, &[&w.ws]).await?;
    let p = public_ids(&w, 5).await?.remove(4);
    let mut asked = vec!["ac:c002".to_owned(), "ac:old".into(), p, "ac:c002".into()];
    let mut expected: Vec<String> = ["ac:c002", "ac:c003", "ac:c004", "ac:c002"]
        .map(str::to_owned)
        .to_vec();
    asked.push("requirement:r".into());
    expected.push("requirement:r".into());
    for i in 5..25 {
        asked.push(format!("ac:c{i:03}"));
        expected.push(format!("ac:c{i:03}"));
    }
    for fields in [None, Some(json!(["title", "status", "comments_count"]))] {
        let with = |mut args: Value| {
            if let Some(fields) = &fields {
                args["fields"] = fields.clone();
            }
            args
        };
        let reply = ok(&w, with(json!({"address": asked.join(",")}))).await?;
        let items = reply["items"].as_array().cloned().unwrap_or_default();
        let order: Vec<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
        ensure!(order == expected, "{fields:?}: {order:?}");
        // Each item equals the read of its one address, plus the address.
        for (item, address) in items.iter().zip(&expected) {
            let mut one = ok(&w, with(json!({"address": address}))).await?;
            one["address"] = json!(address);
            ensure!(*item == one, "{fields:?} {address}: {item} {one}");
        }
        ensure!(items[0] == items[3], "{reply}");
    }
    for address in ["ac:c001,ac:nope", "ac:c001,nope:x"] {
        let reply = w
            .mcp(&w.bot, "joppa_objects", json!({"address":address}))
            .await?;
        ensure!(
            reply == Err("unknown object".into()),
            "{address}: {reply:?}"
        );
    }
    Ok(())
}

// tac:m7qjm2lj
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_save_between_two_statements_changes_no_item_of_the_reply() -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let w = world(false).await?;
    let list = json!({"parent":"requirement:r","limit":100,"fields":["revision"]});
    let list = ok(&w, list).await?;
    let mut revisions: std::collections::BTreeMap<String, u64> = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| Some((i["address"].as_str()?.to_owned(), i["revision"].as_u64()?)))
        .collect();
    // One joppa_batch saves the title G<n> on all 25 criteria in one transaction.
    let mut generation = async |n: usize| -> Result<()> {
        let operations: Vec<Value> = (0..25)
            .map(|i| {
                let address = format!("ac:c{i:03}");
                let revision = revisions.get(&address).copied().unwrap_or(1);
                revisions.insert(address, revision + 1);
                json!({"command":{"op":"save_object","kind":"ac","id":format!("c{i:03}"),
                "revision":revision,"data":{"title":format!("G{n}")},"reason":"Snapshot test"}})
            })
            .collect();
        batch(&w, &format!("generation-{n}"), operations).await
    };
    generation(0).await?;
    let done = AtomicBool::new(false);
    // The writer stops the reader also after a failed write.
    let writer = async {
        let mut written = Ok(());
        for n in 1..=20 {
            written = generation(n).await;
            if written.is_err() {
                break;
            }
        }
        done.store(true, Ordering::SeqCst);
        written
    };
    let reader = async {
        let mut reads = 0;
        while !done.load(Ordering::SeqCst) || reads == 0 {
            for fields in [None, Some(json!(["title"]))] {
                let mut args = json!({"address": addresses(25)});
                if let Some(fields) = fields {
                    args["fields"] = fields;
                }
                let reply = ok(&w, args).await?;
                let mut titles: Vec<&Value> = reply["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|i| {
                        if i["title"].is_null() {
                            &i["data"]["title"]
                        } else {
                            &i["title"]
                        }
                    })
                    .collect();
                titles.dedup();
                ensure!(
                    titles.len() == 1,
                    "two generations in one reply: {titles:?}"
                );
            }
            reads += 1;
        }
        Ok::<_, anyhow::Error>(reads)
    };
    let (written, reads) = tokio::join!(writer, reader);
    written?;
    ensure!(reads? > 0, "no read");
    Ok(())
}
