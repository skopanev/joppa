use super::*;

#[test]
fn the_test_list_gives_each_position_once_and_then_nothing() {
    let list = "aaaaaaaa, bbbbbbbb,aaaaaaaa";
    let ids: Vec<_> = (0..4).map(|n| listed(list, n)).collect();
    assert_eq!(
        ids,
        [
            Some("aaaaaaaa".to_owned()),
            Some("bbbbbbbb".to_owned()),
            Some("aaaaaaaa".to_owned()),
            None
        ]
    );
}

// ac:gw4b0wmx
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn an_alias_or_an_item_id_without_an_object_makes_a_generated_address_taken() -> Result<()> {
    let pool = crate::store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    crate::store::migrate(&pool).await?;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let db: String = tx.query_one("SELECT current_database()", &[]).await?.get(0);
    ensure!(db.starts_with("joppa_test"), "test database required");
    let workspace = format!("taken-{}", &Uuid::new_v4().simple().to_string()[..8]);
    tx.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&workspace])
        .await?;
    // The item_ids row has no object, and its namespace is not the namespace of the workspace.
    tx.execute(
        "SELECT joppa.register_item($1,'ac','gggggggg','other')",
        &[&workspace],
    )
    .await?;
    let mut state = Workspace::default();
    state
        .object_aliases
        .insert("point:cccccccc".into(), "note:source-x".into());
    for (kind, id, expected) in [
        ("point", "cccccccc", true),
        ("ac", "gggggggg", true),
        ("point", "gggggggg", false),
        ("ac", "cccccccc", false),
    ] {
        let found = taken(&*tx, &workspace, &state, kind, id).await?;
        ensure!(found == expected, "{kind}:{id} taken={found}");
    }
    // The generation passes each taken address and takes the first free one.
    let mut ids = ["cccccccc", "gggggggg", "hhhhhhhh"].into_iter();
    let mut next = || ids.next().unwrap_or("zzzzzzzz").to_owned();
    let id = free_id(&*tx, &workspace, &state, "point", &mut next).await?;
    ensure!(id == "gggggggg", "point got {id}");
    let id = free_id(&*tx, &workspace, &state, "ac", &mut next).await?;
    ensure!(id == "hhhhhhhh", "ac got {id}");
    // After 16 taken generations the write is refused, and no 17th generation runs.
    let mut count = 0;
    let refused = free_id(&*tx, &workspace, &state, "ac", || {
        count += 1;
        "gggggggg".to_owned()
    })
    .await
    .expect_err("every generation is taken");
    ensure!(
        refused.to_string() == "no free local ID after 16 attempts; retry the write" && count == 16,
        "{refused} after {count} generations"
    );
    tx.rollback().await?;
    Ok(())
}
