// Part 1 of tests/incremental_projection.rs, included there, so the test names stay unchanged.

/// Derived tables that a rebuild must reproduce exactly.
const TABLES: [&str; 8] = [
    "objects",
    "object_revisions",
    "object_links",
    "ac_owners",
    "state_entries",
    "state_revisions",
    "task_queue",
    "search_documents",
];

/// Columns that differ between an incremental write and a rebuild by contract.
/// Everything else must match, so this list stays short and explicit.
fn dropped(table: &str) -> &'static [&'static str] {
    match table {
        // terms is generated from title and content, so comparing those is
        // enough. embedding is filled asynchronously and may be absent.
        "search_documents" => &["terms", "embedding"],
        _ => &[],
    }
}

type Snapshot = BTreeMap<String, Vec<String>>;

async fn snapshot(pool: &deadpool_postgres::Pool, workspace: &str) -> anyhow::Result<Snapshot> {
    let conn = pool.get().await?;
    let mut out = Snapshot::new();
    for table in TABLES {
        let statement = format!("SELECT to_jsonb(t)::text FROM joppa.{table} t WHERE workspace=$1");
        let mut rows = vec![];
        for row in conn.query(&statement, &[&workspace]).await? {
            let mut value: Value = serde_json::from_str(row.get(0))?;
            if let Some(object) = value.as_object_mut() {
                for key in dropped(table) {
                    object.remove(*key);
                }
                // The status moment is kept from the previous row while tone
                // and label hold. A rebuild has no previous row, so only the
                // tone and the label are comparable here.
                if let Some(signal) = object.get_mut("signal").and_then(Value::as_object_mut) {
                    signal.remove("at");
                }
            }
            rows.push(value.to_string());
        }
        // Order by content: the tables have different keys and a stable row
        // order is all the comparison needs.
        rows.sort();
        out.insert(table.into(), rows);
    }
    Ok(out)
}

async fn rebuild(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    wipe: bool,
) -> anyhow::Result<Snapshot> {
    {
        let conn = pool.get().await?;
        if wipe {
            for table in TABLES {
                let statement = format!("DELETE FROM joppa.{table} WHERE workspace=$1");
                conn.execute(&statement, &[&workspace]).await?;
            }
        }
        conn.execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    }
    store::backfill_board(pool).await?;
    snapshot(pool, workspace).await
}

async fn step(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    counter: &mut usize,
    label: &str,
    command: Value,
) -> anyhow::Result<()> {
    *counter += 1;
    let request = format!("op-{counter}");
    if common::is_removed(&command) {
        common::append_legacy(pool, workspace, "owner", &request, &command).await?;
    } else {
        store::execute_input(pool, workspace, "owner", &request, &command)
            .await
            .map_err(|e| anyhow::anyhow!("{label}: {command}: {e:#}"))?;
    }
    let incremental = snapshot(pool, workspace).await?;
    for wipe in [false, true] {
        let rebuilt = rebuild(pool, workspace, wipe).await?;
        for table in TABLES {
            anyhow::ensure!(
                incremental[table] == rebuilt[table],
                "after {label} the incremental joppa.{table} differs from a rebuild with wipe={wipe}\nincremental: {:#?}\nrebuilt: {:#?}",
                incremental[table],
                rebuilt[table]
            );
        }
    }
    Ok(())
}
