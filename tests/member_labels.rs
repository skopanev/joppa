use anyhow::Result;
use joppa::store;
use serde_json::json;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn labels_use_email_names_without_changing_ids_or_exposing_other_members() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let db: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let ws = format!("labels-{suffix}");
    let owner = format!("owner-{suffix}");
    let actor = format!("user-{suffix}");
    let outsider = format!("outsider-{suffix}");
    for (id, email) in [
        (&actor, format!("rita.petrov@{suffix}.example")),
        (&outsider, format!("other@{suffix}.example")),
    ] {
        conn.execute(
            "INSERT INTO joppa.google_identities(actor,google_sub,email) VALUES($1,$1,$2)",
            &[id, &email],
        )
        .await?;
    }
    drop(conn);
    for (i, command) in [
        json!({"op":"workspace","title":"Member labels"}),
        json!({"op":"member","id":actor,"role":"human"}),
    ]
    .iter()
    .enumerate()
    {
        store::execute_input(&pool, &ws, &owner, &format!("setup-{i}"), command).await?;
    }
    let full = store::read(&pool, &ws, &owner, &store::Scope::full()).await?;
    assert_eq!(full["member_labels"], json!({&actor: "rita.petrov"}));
    assert_eq!(full["state"]["members"][&actor], "human");
    assert!(full["state"]["members"].get("rita.petrov").is_none());
    assert!(
        store::read(&pool, &ws, &outsider, &store::Scope::full())
            .await
            .is_err()
    );
    let index = store::read(&pool, &ws, &owner, &store::Scope::from_args(&json!({}))?).await?;
    assert!(index.get("member_labels").is_none());
    Ok(())
}
