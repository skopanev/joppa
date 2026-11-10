//! The application role of server/install.sh can run the Settings writes: email and domain
//! grants and their removal, a member removal, a first and a later Google sign-in, and a
//! redaction, and a serve start with the data pass of migration 062. The test makes a role with
//! exactly the install.sh grants and runs as that role.
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command, store};
use serde_json::{Value, json};
use std::collections::BTreeSet;

async fn execute(pool: &deadpool_postgres::Pool, ws: &str, actor: &str, v: Value) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute(pool, ws, actor, &id, &serde_json::from_value::<Command>(v)?).await
}

/// The GRANT block of server/install.sh for `role` in database `db`.
fn install_grants(role: &str, db: &str) -> Result<String> {
    let script = include_str!("../server/install.sh");
    let start = script
        .find("GRANT CONNECT ON DATABASE joppa TO joppa_app;")
        .context("install.sh has no grant block")?;
    let end = start + script[start..].find("\nSQL\n").context("grant block end")?;
    Ok(script[start..end]
        .replace("DATABASE joppa ", &format!("DATABASE \"{db}\" "))
        .replace("joppa_app", role))
}

// owner-manages-workspace-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL and a role that may create roles"]
async fn owner_manages_workspace_ac3_the_install_role_runs_the_settings_writes() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let admin = store::pool(&url)?;
    let conn = admin.get().await?;
    let db: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(db.starts_with("joppa_test"), "test database required");
    drop(conn);
    store::migrate(&admin).await?;
    let role = format!("joppa_app_{}", uuid::Uuid::new_v4().simple());
    let conn = admin.get().await?;
    conn.batch_execute(&format!(
        "CREATE ROLE {role} NOLOGIN; GRANT {role} TO CURRENT_USER; {}",
        install_grants(&role, &db)?
    ))
    .await?;
    drop(conn);
    let separator = if url.contains('?') { '&' } else { '?' };
    let app = store::pool(&format!("{url}{separator}options=-c%20role%3D{role}"))?;
    let result = async {
        let current: String = app.get().await?.query_one("SELECT current_user", &[]).await?.get(0);
        ensure!(current == role, "the pool runs as {current}");
        let ws = format!("app-role-{}", uuid::Uuid::new_v4().simple());
        execute(&app, &ws, "owner", json!({"op":"workspace","title":"App role"})).await?;
        // A first sign-in without hd, then a later one with hd: the second fills hosted_domain.
        let person = format!("person-{}", uuid::Uuid::new_v4().simple());
        let email = format!("{person}@example.org");
        let joined = BTreeSet::from([ws.clone()]);
        store::enroll(&app, &person, &person, &email, None, &joined).await?;
        store::enroll(&app, &person, &person, &email, Some("example.org"), &joined).await?;
        store::update_identity_name(&app, &person, &person, "Maria Lopez").await?;
        // Grants and their removal.
        store::grant_email(&app, &ws, "owner", &email).await?;
        store::grant_domain(&app, &ws, "owner", "example.net").await?;
        store::enroll_granted(&app, &person, None, Some("example.org")).await?;
        let owned = store::list_owned_access(&app, "owner").await?;
        let item = owned["items"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["id"] == ws.as_str()))
            .context("owned workspace")?;
        ensure!(
            item["members"].as_array().is_some_and(|m| m.len() == 2),
            "{item}"
        );
        store::revoke_domain(&app, &ws, "owner", "example.net").await?;
        store::remove_member(&app, &ws, "owner", &person).await?;
        let owned = store::list_owned_access(&app, "owner").await?;
        let item = owned["items"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["id"] == ws.as_str()))
            .context("owned workspace")?;
        ensure!(
            item["emails"] == json!([]) && item["domains"] == json!([]),
            "{item}"
        );
        ensure!(
            item["members"].as_array().is_some_and(|m| m.len() == 1),
            "{item}"
        );
        store::grant_email(&app, &ws, "owner", "lena@example.org").await?;
        store::revoke_email(&app, &ws, "owner", "lena@example.org").await?;
        // A redaction rewrites stored revisions of the note.
        execute(&app, &ws, "owner", json!({"op":"save_object","kind":"note","id":"secret","revision":0,"parent":null,"data":{"title":"Secret note","owner":"owner","body":"Token s3cr3t"},"reason":"Save"})).await?;
        execute(&app, &ws, "owner", json!({"op":"redact_object","target":"note:secret","fragment":"s3cr3t","reason":"Remove the secret"})).await?;
        // A serve start as the application role runs no DDL. The data pass of migration 062
        // runs for a workspace without the relations_reset epoch or a row without relations.
        store::migrate_notes(&app).await?;
        admin.get().await?.batch_execute(&format!("DELETE FROM joppa.replay_epochs WHERE workspace='{ws}' AND rule='relations_reset'; UPDATE joppa.state_entries SET value=jsonb_set(value #- '{{revisions,0,relations}}','{{revisions,0,links}}','[]') WHERE workspace='{ws}' AND section='objects' AND key='note:secret'")).await?;
        store::migrate_notes(&app).await?;
        let moved: bool = admin.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.replay_epochs WHERE workspace=$1 AND rule='relations_reset') AND (SELECT value->'revisions'->0 ? 'relations' FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key='note:secret')", &[&ws]).await?.get(0);
        ensure!(moved, "the serve start did not move the pending workspace");
        // owner-manages-workspace-ac2: the deletion removes every row of the workspace.
        // The role itself cannot delete a journal row, also with the purge setting.
        let conn = app.get().await?;
        let refused = conn.batch_execute(&format!("BEGIN; SELECT set_config('joppa.purge_workspace','{ws}',true); DELETE FROM joppa.events WHERE workspace='{ws}'")).await;
        conn.batch_execute("ROLLBACK").await?;
        let refused = refused.err().context("the application role deleted journal rows")?;
        ensure!(format!("{refused:?}").contains("permission denied"), "{refused:?}");
        drop(conn);
        execute(&app, &ws, "owner", json!({"op":"delete_workspace","reason":"Owner order"})).await?;
        let left: i64 = admin.get().await?.query_one("SELECT (SELECT count(*) FROM joppa.events WHERE workspace=$1)+(SELECT count(*) FROM joppa.workspaces WHERE id=$1)+(SELECT count(*) FROM joppa.state_entries WHERE workspace=$1)", &[&ws]).await?.get(0);
        ensure!(left == 0, "{left} rows of the deleted workspace stay");
        Ok(())
    }
    .await;
    drop(app);
    let conn = admin.get().await?;
    conn.batch_execute(&format!(
        "REVOKE {role} FROM CURRENT_USER; DROP OWNED BY {role}; DROP ROLE {role}"
    ))
    .await?;
    result
}
