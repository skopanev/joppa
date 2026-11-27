// Part 4 of tests/depends_sort_http.rs: ac:jqc25hpf (test:8sx7wota and test:arvhlxdp).

// ac:jqc25hpf
// test:8sx7wota
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn browser_uuid_workspaces_receive_unique_short_slugs_preserved_on_rename() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let cookie = session(&w, "alex").await?;
    let auth = [Auth::Cookie(&cookie), Auth::Origin(&w.root)];

    let tag = &uuid::Uuid::new_v4().simple().to_string()[..6];
    let title = format!("Billing Team {tag}");
    let slug = format!("billing-team-{tag}");
    let slug2 = format!("{slug}-2");
    let slug3 = format!("{slug}-3");

    // Occupy base slug with a CLI workspace first so browser UUID workspaces get -2 and -3.
    let create_cli: Operation = serde_json::from_value(json!({"op":"workspace","title":&title}))?;
    store::execute(&w.pool, &slug, "alex", &format!("cli-{tag}"), &create_cli).await?;

    let (code1, b1) = call(&w.http, &w.root, "POST", "/api/workspaces", &auth, Some(&json!({"title":&title}))).await?;
    ensure!(code1 == 200 && b1 == json!({"id":&slug2,"title":&title}), "b1: {code1} {b1}");

    let (code2, b2) = call(&w.http, &w.root, "POST", "/api/workspaces", &auth, Some(&json!({"title":&title}))).await?;
    ensure!(code2 == 200 && b2 == json!({"id":&slug3,"title":&title}), "b2: {code2} {b2}");

    // Find internal UUID of slug2 and verify both slug2 and UUID resolve to the same workspace!
    let uuid2: String = w
        .pool
        .get()
        .await?
        .query_one("SELECT workspace FROM joppa.workspace_aliases WHERE name=$1", &[&slug2])
        .await?
        .get(0);
    ensure!(uuid2.len() == 36 && uuid::Uuid::parse_str(&uuid2).is_ok(), "uuid2: {uuid2}");

    for target in [slug2.as_str(), uuid2.as_str()] {
        let (rc, rb) = call(&w.http, &w.root, "GET", &format!("/api/workspaces/{target}/board"), &auth, None).await?;
        ensure!(rc == 200 && rb["workspace"] == slug2, "board {target}: {rc} {rb}");
    }

    // Rename title -> workspace_id stays slug2.
    let (rnc, rnb) = call(
        &w.http,
        &w.root,
        "POST",
        &format!("/api/workspaces/{slug2}/rename"),
        &auth,
        Some(&json!({"title":"New Finance Name"})),
    )
    .await?;
    ensure!(rnc == 200 && rnb == json!({"id":&slug2,"title":"New Finance Name"}), "rename: {rnc} {rnb}");

    read_key::purge(&w.pool, &[&slug, &uuid2]).await?;
    let uuid3: String = w
        .pool
        .get()
        .await?
        .query_one("SELECT workspace FROM joppa.workspace_aliases WHERE name=$1", &[&slug3])
        .await?
        .get(0);
    read_key::purge(&w.pool, &[&uuid3]).await?;
    Ok(())
}

// ac:jqc25hpf
// test:arvhlxdp
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn non_uuid_existing_id_and_non_latin_titles_keep_their_ids() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let cookie = session(&w, "alex").await?;
    let auth = [Auth::Cookie(&cookie), Auth::Origin(&w.root)];

    // 1. Non-UUID CLI workspace ("custom-key-ws") with title "Alpha Project" keeps id "custom-key-ws".
    let create_cli: Operation = serde_json::from_value(json!({"op":"workspace","title":"Alpha Project"}))?;
    store::execute(&w.pool, "custom-key-ws", "alex", "cli-alpha", &create_cli).await?;
    ensure!(
        store::public_id(&w.pool, "custom-key-ws").await? == "custom-key-ws",
        "non-UUID key must not get a slug workspace_id"
    );

    // 2. UUID workspace with an existing explicit workspace_id ("explicit-slug") keeps it on startup migrate_notes.
    let explicit_uuid = uuid::Uuid::new_v4().to_string();
    let create_exp: Operation = serde_json::from_value(json!({"op":"workspace","title":"Beta Project"}))?;
    store::execute(&w.pool, &explicit_uuid, "alex", "exp-1", &create_exp).await?;
    let set_id: Operation = serde_json::from_value(json!({"op":"update_workspace","workspace_id":"explicit-slug","reason":"Explicit"}))?;
    store::execute(&w.pool, &explicit_uuid, "alex", "exp-2", &set_id).await?;
    store::migrate_notes(&w.pool).await?;
    ensure!(
        store::public_id(&w.pool, &explicit_uuid).await? == "explicit-slug",
        "existing workspace_id must stay explicit-slug"
    );

    // 3. Browser UUID workspace with Cyrillic-only title ("Отдел кадров") has empty slug -> keeps its UUID!
    let (cc, cb) = call(&w.http, &w.root, "POST", "/api/workspaces", &auth, Some(&json!({"title":"Отдел кадров"}))).await?;
    let cyr_id = cb["id"].as_str().context("cyr id")?.to_owned();
    ensure!(
        cc == 200 && cyr_id.len() == 36 && uuid::Uuid::parse_str(&cyr_id).is_ok(),
        "Cyrillic-only title must keep its UUID: {cc} {cb}"
    );

    let auto_slug_uuid: String = w
        .pool
        .get()
        .await?
        .query_opt("SELECT workspace FROM joppa.workspace_aliases WHERE name='beta-project'", &[])
        .await?
        .map(|r| r.get(0))
        .unwrap_or_default();
    read_key::purge(&w.pool, &["custom-key-ws", &explicit_uuid, &cyr_id]).await?;
    if !auto_slug_uuid.is_empty() {
        read_key::purge(&w.pool, &[&auto_slug_uuid]).await?;
    }
    Ok(())
}
