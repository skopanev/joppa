//! requirement:launch-gate: an older journal holds acceptances, the launch flag of a type and the
//! launch mark of a criterion. It replays, and none of them has an effect: no read gives an
//! acceptance. A live write refuses the flag and each acceptance operation.
mod common;
use common::*;
use joppa::objects;
use joppa::removed_ops::{LAUNCH_BLOCKER, refusal};
use serde_json::{Value, json};

// ac:gate-checks-subject
#[test]
fn an_old_journal_with_acceptances_and_a_launch_flag_replays_without_effect() {
    let mut w = World::new();
    assert_eq!(
        w.state.objects["ac:key"].current().data["before_launch"],
        true
    );
    let revision = w.state.objects["requirement:r"].current().revision;
    let before = objects::process(&w.state, "requirement:r");
    // An older schema event makes criteria launch conditions.
    let schema = objects::schema(&w.state);
    let mut types = serde_json::to_value(&schema.types).unwrap();
    types["ac"]["launch_blocker"] = json!(true);
    let put = json!({"op":"set_tree_schema","revision":schema.revision,"types":types,
        "reason":"Make criteria launch conditions"});
    w.legacy("owner", put.clone());
    for stored in [
        json!({"op":"accept_commit","target":"requirement:r","revision":revision,"commit":"c1c1c1c"}),
        json!({"op":"accept","req":"r","revision":1,"run":"run-1"}),
        json!({"op":"revoke","req":"r","revision":1,"reason":"Wrong run"}),
    ] {
        w.legacy("owner", stored);
    }
    // No state, process fact or schema read gives an acceptance, the commit or the flag.
    let process = objects::process(&w.state, "requirement:r");
    assert_eq!(process, before);
    assert!(process.get("acceptance").is_none(), "{process}");
    let state = serde_json::to_string(&w.state).unwrap();
    assert!(
        !state.contains("c1c1c1c") && !state.contains("\"acceptance"),
        "{state}"
    );
    let shown = serde_json::to_string(&objects::schema(&w.state).types).unwrap();
    assert!(!shown.contains("launch_blocker"), "{shown}");
    // A live write refuses the flag and each acceptance operation, and names the replacement.
    let mut put = put;
    put["revision"] = json!(objects::schema(&w.state).revision);
    assert_eq!(
        w.apply("owner", put).unwrap_err().to_string(),
        LAUNCH_BLOCKER
    );
    let live =
        json!({"op":"accept_commit","target":"requirement:r","revision":revision,"commit":"c2"});
    let error = w.apply("owner", live).unwrap_err().to_string();
    assert_eq!(error, refusal("accept_commit"));
    assert!(
        error.contains("record the owner decision as a Source"),
        "{error}"
    );
}

// ac:gate-checks-subject
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stored_commit_acceptance_replays_from_the_database_without_effect() -> anyhow::Result<()>
{
    let pool = joppa::store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    joppa::store::migrate(&pool).await?;
    let ws = format!("stored-commit-{}", uuid::Uuid::new_v4().simple());
    let write = async |command: Value| {
        let request = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_input(&pool, &ws, "owner", &request, &command).await
    };
    write(json!({"op":"workspace","title":"Stored acceptance"})).await?;
    let tree = joppa::store::tree_schema(&pool, &ws, "owner", None).await?;
    let revision = tree["schema"]["revision"].as_u64().unwrap_or(0);
    let types = json!({
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]}});
    write(json!({"op":"set_tree_schema","revision":revision,"types":types,"reason":"Configure"}))
        .await?;
    write(
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,
        "data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Agree"}),
    )
    .await?;
    // The criterion save gives requirement:r its revision 2.
    write(
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r",
        "data":{"title":"A","owner":"owner","before_launch":true},"reason":"Agree"}),
    )
    .await?;
    let scope = joppa::store::ObjectScope {
        address: Some("requirement:r"),
        ..Default::default()
    };
    let before = joppa::store::read_objects(&pool, &ws, "owner", &scope).await?;
    assert_eq!(before["current_revision"], 2, "{before}");
    // The older server stored the launch flag and the acceptance on a commit.
    let mut flagged = types.clone();
    flagged["ac"]["launch_blocker"] = json!(true);
    let stored = [
        json!({"op":"set_tree_schema","revision":revision + 1,"types":flagged,"reason":"Launch"}),
        json!({"op":"accept_commit","target":"requirement:r","revision":2,"commit":"c1c1c1c"}),
    ];
    for command in stored {
        pool.get().await?.execute(
            "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,MAX(position)+1,'owner',$2,$3::text::jsonb FROM joppa.events WHERE workspace=$1",
            &[&ws, &uuid::Uuid::new_v4().to_string(), &command.to_string()],
        ).await?;
    }
    joppa::store::backfill_board(&pool).await?;
    joppa::store::check_journal(&pool).await?;
    let read = joppa::store::read_objects(&pool, &ws, "owner", &scope).await?;
    assert_eq!(read["process"], before["process"], "{read}");
    assert!(!read.to_string().contains("\"acceptance"), "{read}");
    let board = joppa::store::read(&pool, &ws, "owner", &joppa::store::Scope::full()).await?;
    assert!(board.get("launch").is_none(), "{board}");
    assert!(!board.to_string().contains("\"acceptance"), "{board}");
    Ok(())
}
