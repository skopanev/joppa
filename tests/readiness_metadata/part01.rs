// Part 1 of tests/readiness_metadata.rs, included there, so the test names stay unchanged.

// ac:gate-checks-subject
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn backfill_reprojects_work_ready_from_an_older_projection_version() -> anyhow::Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("readiness-{}", uuid::Uuid::new_v4().simple());
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    store::execute_input(&pool, &ws, "owner", "other", &json!({"op":"save_object","kind":"requirement","id":"other","revision":0,
        "parent":"capability:c","data":{"title":"Other scope","body":"Scope","owner":"owner","consumers":["customer"]},"reason":"Define"})).await?;
    let conn = pool.get().await?;
    let object = "SELECT revision,document::text,work_ready,process::text,signal::text FROM joppa.objects WHERE workspace=$1 AND address=$2";
    let (requirement, other) = (
        conn.query_one(object, &[&ws, &"requirement:r"]).await?,
        conn.query_one(object, &[&ws, &"requirement:other"]).await?,
    );
    let data = serde_json::from_str::<Value>(requirement.get(1))?["data"].clone();
    store::execute_input(&pool, &ws, "owner", "link", &json!({"op":"save_object","kind":"requirement","id":"r",
        "revision":requirement.get::<_,i32>(0),"parent":"capability:c","data":data,
        "links":[{"target":"requirement:other","relation":"depends_on","revision":other.get::<_,i32>(0),"blocking":true}],
        "reason":"Order after the other requirement"})).await?;
    let current = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    let full = (
        current.get::<_, bool>(2),
        serde_json::from_str::<Value>(current.get(3))?,
        serde_json::from_str::<Value>(current.get(4))?,
    );
    // An older projection stored the other readiness value.
    conn.execute("UPDATE joppa.objects SET work_ready=NOT work_ready WHERE workspace=$1 AND address='ac:ios'", &[&ws]).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{projection_version}','\"8\"') WHERE workspace=$1", &[&ws]).await?;
    let stale = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    assert_eq!(stale.get::<_, bool>(2), !full.0);
    store::backfill_board(&pool).await?;
    let repaired = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    assert_eq!(
        (
            repaired.get::<_, bool>(2),
            serde_json::from_str::<Value>(repaired.get(3))?,
            serde_json::from_str::<Value>(repaired.get(4))?
        ),
        full
    );
    let snapshot = "SELECT jsonb_build_array(b.xmin::text,b.first_view->>'projection_version',(SELECT jsonb_agg(jsonb_build_array(o.address,o.xmin::text) ORDER BY o.address) FROM joppa.objects o WHERE o.workspace=b.workspace))::text FROM joppa.board_cache b WHERE b.workspace=$1";
    let before: String = conn.query_one(snapshot, &[&ws]).await?.get(0);
    assert_eq!(serde_json::from_str::<Value>(&before)?[1], "22");
    store::backfill_board(&pool).await?;
    assert_eq!(
        conn.query_one(snapshot, &[&ws]).await?.get::<_, String>(0),
        before,
        "a current projection must not be rewritten"
    );
    Ok(())
}
