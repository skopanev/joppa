// Part 14 of tests/reports_http.rs, included there, so the test names stay unchanged.

// settings-in-name-menu-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn settings_in_name_menu_ac2_access_lists_people_by_name_and_no_agents() -> Result<()> {
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let ws = w.base.rsplit('/').next().context("workspace")?.to_owned();
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let person = format!("person-{}", uuid::Uuid::new_v4().simple());
    let op: Operation = serde_json::from_value(json!({"op":"member","id":person,"role":"human"}))?;
    store::execute(&pool, &ws, "owner", "add-person", &op).await?;
    let email = format!("{person}@example.com");
    pool.get()
        .await?
        .execute(
            "INSERT INTO joppa.google_identities(actor,google_sub,email,display_name) VALUES($1,$1,$2,'Maria Lopez')",
            &[&person, &email],
        )
        .await?;
    // The grant of a member is not listed again; a pending grant is.
    let pending = format!("pending-{person}@example.com");
    store::grant_email(&pool, &ws, "owner", &email).await?;
    store::grant_email(&pool, &ws, "owner", &pending).await?;
    let read = |token: String| {
        let http = w.http.clone();
        let url = format!("{root}/api/access");
        async move {
            let reply = http.get(url).bearer_auth(token).send().await?;
            anyhow::Ok((reply.status().as_u16(), reply.json::<Value>().await?))
        }
    };
    let (code, owned) = read(w.owner.clone()).await?;
    ensure!(code == 200, "{code} {owned}");
    let item = owned["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == ws.as_str()))
        .context("owned workspace")?;
    let people: Vec<&str> = item["people"]
        .as_array()
        .context("people")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    // A person with a Google identity shows by name and email; the agent member is absent.
    ensure!(
        people.contains(&format!("Maria Lopez <{email}>").as_str()) && !people.contains(&"agent"),
        "{people:?}"
    );
    ensure!(item["emails"] == json!([pending]), "{}", item["emails"]);
    // A member who does not own the workspace gets no entry for it.
    let (code, other) = read(w.other.clone()).await?;
    ensure!(code == 200, "{code} {other}");
    ensure!(
        other["items"]
            .as_array()
            .is_some_and(|items| items.iter().all(|i| i["id"] != ws.as_str())),
        "{other}"
    );
    Ok(())
}
