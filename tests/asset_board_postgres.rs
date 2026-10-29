//! An asset board field: the result names an object of the type asset, each criterion
//! lists the assets that apply to it or the explicit value all, and each asset gets its
//! own status. A new asset starts grey.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

const X: &str = "tests/app.rs > shared";
const Y: &str = "tests/app.rs > pcos_only";

async fn pool() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok(pool)
}

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    store::execute_remote_input(pool, ws, "bot", &uuid::Uuid::new_v4().to_string(), &command).await
}

fn save(kind: &str, id: &str, parent: Option<&str>, data: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Create"})
}

/// Assets life, pcos and wield. Criterion A applies to all assets, criterion B to pcos.
async fn fixture(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("asset-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Assets"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    let mut block = tree["schema"]["test_results"].clone();
    block["fields"].as_array_mut().unwrap().push(json!({"name":"asset","title":"Asset","type":"ref","target":"asset","board":true,"applies":"applies_to"}));
    let applies = json!({"applies_to":{"target":"asset","title":"Assets","required":true,"multiple":true,"allow_all":true}});
    write(pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Assets","test_results":block,"types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"],"references":applies},
        "asset":{"title":"Assets","role":"document","status":"none","root":true,"parents":[]}
    }})).await?;
    for id in ["life", "pcos", "wield"] {
        write(
            pool,
            &ws,
            save("asset", id, None, json!({"title":id,"owner":"owner"})),
        )
        .await?;
    }
    write(
        pool,
        &ws,
        save(
            "requirement",
            "r",
            None,
            json!({"title":"R","owner":"owner","body":"The rule"}),
        ),
    )
    .await?;
    write(
        pool,
        &ws,
        save(
            "ac",
            "a",
            Some("requirement:r"),
            json!({"title":"A","owner":"owner","applies_to":"all","tests":[X]}),
        ),
    )
    .await?;
    write(
        pool,
        &ws,
        save(
            "ac",
            "b",
            Some("requirement:r"),
            json!({"title":"B","owner":"owner","applies_to":["pcos"],"tests":[Y]}),
        ),
    )
    .await?;
    Ok(ws)
}

async fn record(pool: &deadpool_postgres::Pool, ws: &str, results: Value) -> Result<Value> {
    store::record_results(
        pool,
        ws,
        "bot",
        &json!({"commit":"c0ffee8","results":results}),
    )
    .await
}

async fn status(pool: &deadpool_postgres::Pool, ws: &str, ac: &str) -> Result<Value> {
    store::criterion_tests(pool, ws, "bot", ac).await
}

// asset-board-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_asset_that_applies_gets_its_own_status() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool).await?;
    // A shared test without an asset counts for every asset, the pcos test only for pcos.
    record(
        &pool,
        &ws,
        json!([{"test":X,"status":"pass"},{"test":Y,"status":"pass","asset":"asset:pcos"}]),
    )
    .await?;
    let a = status(&pool, &ws, "ac:a").await?;
    ensure!(
        a["status"] == "green" && a["values"]["asset"]["life"]["status"] == "green",
        "{a}"
    );
    let b = status(&pool, &ws, "ac:b").await?;
    ensure!(
        b["status"] == "green"
            && b["values"]["asset"]["pcos"]["status"] == "green"
            && b["values"]["asset"]["life"]["status"] == "n/a",
        "{b}"
    );
    let board = store::board_read(&pool, &ws, "bot").await?;
    ensure!(
        board["result_filters"]
            == json!([{"field":"asset","title":"Asset","values":["life","pcos","wield"]}]),
        "{board}"
    );
    // A value that is not an asset is refused.
    let error = record(
        &pool,
        &ws,
        json!([{"test":Y,"status":"pass","asset":"web"}]),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(
        error.contains("result field asset needs an object of the type asset"),
        "{error}"
    );
    // A new asset starts grey on B's test, which names only pcos: B does not apply to it,
    // so B stays green. A has a shared result, which counts for the new asset too.
    write(
        &pool,
        &ws,
        save(
            "asset",
            "next",
            None,
            json!({"title":"next","owner":"owner"}),
        ),
    )
    .await?;
    let a = status(&pool, &ws, "ac:a").await?;
    ensure!(a["values"]["asset"]["next"]["status"] == "green", "{a}");
    let b = status(&pool, &ws, "ac:b").await?;
    ensure!(
        b["status"] == "green" && b["values"]["asset"]["next"]["status"] == "n/a",
        "{b}"
    );
    // A criterion for all assets whose result names one asset is grey for the new one.
    write(&pool, &ws, save("ac", "c", Some("requirement:r"), json!({"title":"C","owner":"owner","applies_to":"all","tests":["tests/app.rs > per_asset"]}))).await?;
    record(
        &pool,
        &ws,
        json!([
            {"test":"tests/app.rs > per_asset","status":"pass","asset":"life"},
            {"test":"tests/app.rs > per_asset","status":"pass","asset":"pcos"},
            {"test":"tests/app.rs > per_asset","status":"pass","asset":"wield"}
        ]),
    )
    .await?;
    let c = status(&pool, &ws, "ac:c").await?;
    ensure!(
        c["status"] == "grey" && c["values"]["asset"]["next"]["status"] == "grey",
        "the new asset has no result: {c}"
    );
    Ok(())
}

// asset-board-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_names_its_assets_or_the_explicit_value_all() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool).await?;
    let error = write(
        &pool,
        &ws,
        save(
            "ac",
            "c",
            Some("requirement:r"),
            json!({"title":"C","owner":"owner","tests":[X]}),
        ),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(
        error.contains("reference applies_to is required"),
        "{error}"
    );
    let error = write(
        &pool,
        &ws,
        save(
            "ac",
            "c",
            Some("requirement:r"),
            json!({"title":"C","owner":"owner","applies_to":[],"tests":[X]}),
        ),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(
        error.contains("reference applies_to is required"),
        "{error}"
    );
    let error = write(
        &pool,
        &ws,
        save(
            "ac",
            "c",
            Some("requirement:r"),
            json!({"title":"C","owner":"owner","applies_to":["web"],"tests":[X]}),
        ),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(error.contains("unknown reference target"), "{error}");
    Ok(())
}
