//! Links and child nodes that name a deleted workspace read as "no access" and never
//! satisfy work. The owner can remove them.
#[path = "common/purge_world.rs"]
mod purge_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use purge_world::{pool, workspace, write};
use serde_json::json;

// owner-manages-workspace-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn links_to_a_deleted_workspace_read_as_no_access_and_never_satisfy_work() -> Result<()> {
    let pool = pool().await?;
    let (p, _) = workspace(&pool, "product", "p").await?;
    let (c, _) = workspace(&pool, "component", "c").await?;
    write(
        &pool,
        &c,
        "owner",
        json!({"op":"save_object","kind":"domain","id":"api","revision":0,"parent":null,
        "data":{"title":"API contract","owner":"owner"},"reason":"Fixture"}),
    )
    .await?;
    write(
        &pool,
        &p,
        "owner",
        json!({"op":"save_object","kind":"domain","id":"backend","revision":0,"parent":null,
        "data":{"title":"Backend","owner":"owner","workspace":c},"reason":"Fixture"}),
    )
    .await?;
    let note = store::read_objects(
        &pool,
        &p,
        "owner",
        &store::ObjectScope {
            address: Some("note:n"),
            ..Default::default()
        },
    )
    .await?;
    write(&pool, &p, "owner", json!({"op":"set_remote_links","target":"note:n","revision":note["remote_links_revision"],"reason":"Uses the API",
        "links":[{"workspace":c,"target":"domain:api","relation":"related","revision":1,"blocking":false},
                 {"workspace":c,"target":"domain:api","relation":"depends_on","revision":1,"blocking":true}]})).await?;
    write(
        &pool,
        &c,
        "owner",
        json!({"op":"delete_workspace","reason":"Owner order"}),
    )
    .await?;
    let read = async |address: &str| {
        store::read_objects(
            &pool,
            &p,
            "owner",
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await
    };
    let note = read("note:n").await?;
    let links = note["remote_links"].as_array().context("remote links")?;
    ensure!(links.len() == 2, "{note}");
    for link in links {
        ensure!(
            link["status"]["access"] == false && link["status"]["satisfied"] == false,
            "{link}"
        );
    }
    let node = read("domain:backend").await?;
    ensure!(
        node["child_workspace"]["access"] == false && node["child_workspace"]["status"] == "grey",
        "{node}"
    );
    // The owner removes the links to the deleted workspace.
    write(&pool, &p, "owner", json!({"op":"set_remote_links","target":"note:n","revision":note["remote_links_revision"],"reason":"The component is deleted","links":[]})).await?;
    ensure!(
        read("note:n").await?["remote_links"] == json!([]),
        "the links stay"
    );
    Ok(())
}
