// Part 3 of tests/read_statements_postgres.rs, included there, so the test names stay unchanged.

/// The fields that each add at most one query to a read.
const FIELDS: [&str; 8] = [
    "children",
    "child_counts",
    "comments",
    "comments_count",
    "files",
    "files_count",
    "references",
    "relation_groups",
];

/// The read `args` with `["title","status"]` reads no comment, file, edge, reference or remote
/// link. Each name of FIELDS added to it adds at most one statement. A refused name, such as a
/// detail key on a list, is skipped. Gives the names that the read takes.
async fn each_field_adds_one(w: &record_world::World, args: Value) -> Result<Vec<&'static str>> {
    let with = |fields: Value| {
        let mut args = args.clone();
        args["fields"] = fields;
        args
    };
    let base = measure(w, "joppa_objects", with(json!(["title", "status"]))).await?;
    let base = base.map_err(anyhow::Error::msg)?;
    for table in ["joppa.state_entries", "joppa.object_links", "joppa.remote_links"] {
        ensure!(base.naming(table) == 0, "{args} reads {table}: {base}");
    }
    let mut taken = vec![];
    for field in FIELDS {
        let fields = json!(["title", "status", field]);
        let Ok(found) = measure(w, "joppa_objects", with(fields)).await? else {
            continue;
        };
        ensure!(
            found.total() <= base.total() + 1,
            "{args} with {field}: {found} after {base}"
        );
        taken.push(field);
    }
    Ok(taken)
}

// tac:jogrtcra
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn a_list_of_100_with_fields_runs_only_the_queries_its_fields_need() -> Result<()> {
    let _one = ONE.lock().await;
    let w = world(true).await?;
    let list = json!({"parent":"requirement:r","limit":100});
    let mut page = list.clone();
    page["fields"] = json!(["title", "status"]);
    let page = ok(&w, page).await?;
    ensure!(
        page["items"].as_array().map(Vec::len) == Some(100),
        "{page}"
    );
    let taken = each_field_adds_one(&w, list).await?;
    ensure!(
        taken.contains(&"children") && taken.contains(&"child_counts"),
        "{taken:?}"
    );
    Ok(())
}

// tac:jogrtcra
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn twenty_five_addresses_with_fields_run_only_the_queries_their_fields_need() -> Result<()> {
    let _one = ONE.lock().await;
    let w = world(true).await?;
    let taken = each_field_adds_one(&w, json!({"address": addresses(25)})).await?;
    ensure!(
        taken == FIELDS,
        "a read by address takes each field: {taken:?}"
    );
    Ok(())
}

// tac:jogrtcra
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn joppa_attachments_reads_the_files_without_the_card_read() -> Result<()> {
    let _one = ONE.lock().await;
    let w = world(true).await?;
    // A card and a read with ["title","status"] sign no Spaces URL.
    let reads = [
        json!({"address":"ac:a"}),
        json!({"address": addresses(25)}),
        json!({"address": addresses(25),"fields":["title","status"]}),
        json!({"parent":"requirement:r","limit":100,"fields":["title","status"]}),
    ];
    for args in reads {
        let reply = ok(&w, args.clone()).await?.to_string();
        ensure!(!reply.contains("X-Amz-"), "{args}: {reply}");
    }
    // The card read counts the incoming edges of the target in joppa.object_links.
    // joppa_attachments reads only the files.
    let listed = measure(&w, "joppa_attachments", json!({"target":"ac:c000"})).await?;
    let listed = listed.map_err(anyhow::Error::msg)?;
    ensure!(listed.naming("joppa.object_links") == 0, "{listed}");
    Ok(())
}
