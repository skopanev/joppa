//! requirement:workspace-prefix-from-title: create and rename set a slug prefix with a
//! number on a collision, the migration gives a UUID workspace a slug prefix, and each old
//! item ID still reads its object.
#[path = "common/resolve_id.rs"]
mod resolve_id;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn db() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok(pool)
}

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    store::execute_input(
        pool,
        ws,
        "owner",
        &uuid::Uuid::new_v4().to_string(),
        &command,
    )
    .await
}

/// The item ID of `domain:id` with `namespace`.
async fn item(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    id: &str,
    namespace: &str,
) -> Result<String> {
    Ok(pool.get().await?.query_opt("SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='domain' AND local_id=$2 AND namespace=$3", &[&ws, &id, &namespace]).await?
        .with_context(|| format!("no item ID of domain:{id} in {namespace}"))?.get(0))
}

async fn reads(pool: &deadpool_postgres::Pool, ws: &str, item: &str, id: &str) -> Result<()> {
    let found = resolve_id::resolve(pool, "owner", ws, item).await?;
    ensure!(found == format!("domain:{id}"), "{item}: {found}");
    Ok(())
}

async fn prefix(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Option<String>> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT state->>'reference_prefix' FROM joppa.board_cache WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0))
}

// prefix-create-rename
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn create_and_rename_set_a_slug_prefix_and_old_ids_still_resolve() -> Result<()> {
    let pool = db().await?;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let title = format!("Billing Team {tag}");
    let slug = format!("billing-team-{tag}");
    let first = format!("first-{tag}");
    write(&pool, &first, json!({"op":"workspace","title":title})).await?;
    ensure!(
        prefix(&pool, &first).await?.as_deref() == Some(slug.as_str()),
        "first prefix"
    );
    let second = format!("second-{tag}");
    write(&pool, &second, json!({"op":"workspace","title":title})).await?;
    let numbered = format!("{slug}-2");
    ensure!(
        prefix(&pool, &second).await?.as_deref() == Some(numbered.as_str()),
        "second prefix: {:?}",
        prefix(&pool, &second).await?
    );
    write(&pool, &second, json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Billing","owner":"owner"},"reason":"Prefix test"})).await?;
    let old = item(&pool, &second, "d", &numbered).await?;
    write(&pool, &second, json!({"op":"rename_workspace","title":format!("Billing Ops {tag}"),"reason":"New team name"})).await?;
    let renamed = format!("billing-ops-{tag}");
    ensure!(
        prefix(&pool, &second).await?.as_deref() == Some(renamed.as_str()),
        "renamed prefix"
    );
    write(&pool, &second, json!({"op":"save_object","kind":"domain","id":"e","revision":0,"parent":null,"data":{"title":"Ops","owner":"owner"},"reason":"Prefix test"})).await?;
    let new = item(&pool, &second, "e", &renamed).await?;
    ensure!(new.starts_with(&format!("{renamed}-")), "{new}");
    let alias = item(&pool, &second, "d", &renamed).await?;
    for (id, local) in [(&old, "d"), (&alias, "d"), (&new, "e")] {
        reads(&pool, &second, id, local).await?;
    }
    Ok(())
}

// prefix-migration
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_migration_gives_each_uuid_workspace_a_slug_prefix() -> Result<()> {
    let pool = db().await?;
    let ws = uuid::Uuid::new_v4().to_string();
    let tag = &ws[..8];
    // An older journal: a UUID workspace whose domain got an item ID with the UUID prefix.
    let mut events = vec![
        json!({"op":"workspace","title":format!("Mobile Apps {tag}")}),
        json!({"op":"domain","id":"d","title":"Billing","authority":"owner"}),
    ];
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_task_claims",
        "migrate_persistent_claims",
        "migrate_consumers",
        "migrate_consumer_drops",
        "migrate_test_rules",
        "migrate_note_parents",
    ] {
        events.push(json!({ "op": op }));
    }
    let conn = pool.get().await?;
    conn.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&ws])
        .await?;
    for (i, command) in events.iter().enumerate() {
        conn.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,'owner',$3,$4::text::jsonb)",
            &[&ws, &(i as i64 + 1), &uuid::Uuid::new_v4().to_string(), &command.to_string()]).await?;
    }
    drop(conn);
    let old = item(&pool, &ws, "d", &ws).await?;
    store::migrate(&pool).await?;
    let position = || async {
        Ok::<i64, anyhow::Error>(
            pool.get()
                .await?
                .query_one(
                    "SELECT max(position) FROM joppa.events WHERE workspace=$1",
                    &[&ws],
                )
                .await?
                .get(0),
        )
    };
    let first = position().await?;
    store::migrate(&pool).await?;
    let second = position().await?;
    let slug = format!("mobile-apps-{tag}");
    ensure!(
        prefix(&pool, &ws).await?.as_deref() == Some(slug.as_str()),
        "prefix: {:?}",
        prefix(&pool, &ws).await?
    );
    let new = item(&pool, &ws, "d", &slug).await?;
    reads(&pool, &ws, &old, "d").await?;
    reads(&pool, &ws, &new, "d").await?;
    ensure!(
        first == second,
        "the second start wrote {} events",
        second - first
    );
    Ok(())
}
