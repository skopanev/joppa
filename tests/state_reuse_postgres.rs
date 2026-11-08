//! A write after a write of this process starts from the state in memory and stores the same
//! rows as a write that reads the current rows. Two equal workspaces get the same writes: A
//! keeps its state between writes, and in B an external change of the board row comes before
//! each write, so each write of B reads the current rows. Both get the same direct SQL change
//! of one current row before one write. The test reads process-wide probe
//! counters, so it is the only test of this binary.
#[path = "common/state_compare.rs"]
mod compare;
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Result, ensure};
use joppa::{probe, store};
use serde_json::{Value, json};

/// One measured write.
enum Step {
    Write(Value),
    Record(Value),
}

/// The change before a write in A: none, an external change of the board row, or a direct
/// SQL change of one current row that does not touch the board row.
#[derive(Clone, Copy, PartialEq)]
enum Before {
    Nothing,
    Board,
    Row,
}

/// The tree with a note type of the inbox, a requirement, two criteria, a test node and two
/// notes with a body. Each object is at revision 1.
async fn world(pool: &pg_plan::Pool, ws: &str) -> Result<()> {
    let op = json!({"op":"workspace","title":"State reuse"});
    store::execute_input(pool, ws, "owner", "workspace", &op).await?;
    store::enroll_service(pool, ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, ws, "owner", None).await?;
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400},
        "note":{"title":"Notes","role":"document","inbox":true,"status":"none","root":true,"parents":[]}},
        "test_results":tree["schema"]["test_results"]});
    let node = |kind: &str, id: &str, parent: Option<&str>, title: &str| {
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
            "data":{"title":title,"owner":"owner","body":format!("The body of {id}")},"reason":"Create"})
    };
    let commands = [
        schema,
        node("requirement", "r", None, "R"),
        node("ac", "a1", Some("requirement:r"), "A1"),
        node("ac", "a2", Some("requirement:r"), "A2"),
        node("test", "t1", Some("ac:a1"), "tests/reuse.rs > one"),
        node("note", "n1", None, "Note 1"),
        node("note", "n2", None, "Note 2"),
    ];
    for (i, command) in commands.iter().enumerate() {
        let request = format!("setup-{i}");
        store::execute_remote_input(pool, ws, "owner", &request, command).await?;
    }
    Ok(())
}

fn save(kind: &str, id: &str, revision: u32, parent: Option<&str>, body: &str) -> Step {
    Step::Write(
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,
        "data":{"title":id.to_uppercase(),"owner":"owner","body":body},"reason":"Change"}),
    )
}

/// The measured writes, and the change before each in A. Index 3 is the failed write: a save
/// with an old revision.
fn steps() -> Vec<(Step, Before)> {
    use Before::{Board, Nothing as No, Row};
    vec![
        (save("ac", "a1", 1, Some("requirement:r"), "New rule"), No),
        (save("ac", "a3", 0, Some("requirement:r"), "Third"), No),
        (save("note", "n1", 1, None, "The changed note"), No),
        (save("ac", "a1", 1, Some("requirement:r"), "Stale"), No),
        (
            Step::Record(
                json!({"commit":"c0ffee7","results":[{"test":"test:t1","status":"pass"}]}),
            ),
            No,
        ),
        (
            save("ac", "a2", 1, Some("requirement:r"), "Second rule"),
            No,
        ),
        (save("note", "n2", 1, None, "Another note"), Board),
        (save("ac", "a3", 1, Some("requirement:r"), "Third rule"), No),
        (save("ac", "a2", 2, Some("requirement:r"), "Hand rule"), Row),
        (save("ac", "a1", 2, Some("requirement:r"), "Final rule"), No),
    ]
}

/// A change of the title in the meta row, as by hand: the board row stays as it is.
async fn change_row(pool: &pg_plan::Pool, ws: &str) -> Result<()> {
    let sql = "UPDATE joppa.state_entries SET value=jsonb_set(value,'{title}','\"Changed by hand\"') WHERE workspace=$1 AND section='meta' AND key=''";
    ensure!(pool.get().await?.execute(sql, &[&ws]).await? == 1);
    Ok(())
}

async fn touch(pool: &pg_plan::Pool, ws: &str) -> Result<()> {
    let sql = "UPDATE joppa.board_cache SET position=position WHERE workspace=$1";
    ensure!(pool.get().await?.execute(sql, &[&ws]).await? == 1);
    Ok(())
}

/// Run each step in `ws`. Returns the normalized reply and the (state_loads, state_reuses)
/// of each step.
async fn run(pool: &pg_plan::Pool, ws: &str, always_touch: bool) -> Result<Vec<(Value, u64, u64)>> {
    let names = compare::names(pool, ws).await?;
    let mut out = vec![];
    for (i, (step, before)) in steps().into_iter().enumerate() {
        if before == Before::Row {
            change_row(pool, ws).await?;
        }
        if always_touch || before == Before::Board {
            touch(pool, ws).await?;
        }
        let (loads, reuses) = (
            probe::read(&probe::STATE_LOADS),
            probe::read(&probe::STATE_REUSES),
        );
        let request = format!("step-{i}");
        let reply = match step {
            Step::Write(command) => {
                store::execute_remote_input(pool, ws, "owner", &request, &command).await
            }
            Step::Record(args) => store::record_results(pool, ws, "bot", &args).await,
        };
        let loads = probe::read(&probe::STATE_LOADS) - loads;
        let reuses = probe::read(&probe::STATE_REUSES) - reuses;
        let reply = match reply {
            Ok(value) => json!({"ok":value}),
            Err(e) => json!({"error":format!("{e:#}")}),
        };
        out.push((reply, loads, reuses));
    }
    // The item IDs of the objects that the steps created.
    let names = [compare::names(pool, ws).await?, names].concat();
    out.into_iter()
        .map(|(reply, l, r)| Ok((compare::normalize(&reply, &names)?, l, r)))
        .collect()
}

// ac:0y087jhf
// test:pd4p7t8q
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_write_after_a_write_reuses_the_state_in_memory_and_stores_the_same_rows() -> Result<()> {
    let pool = pg_plan::db().await?;
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let (a, b) = (format!("reuse-{run_id}-a"), format!("reuse-{run_id}-b"));
    world(&pool, &a).await?;
    world(&pool, &b).await?;
    let in_a = run(&pool, &a, false).await?;
    let in_b = run(&pool, &b, true).await?;
    let counts = |steps: &[(Value, u64, u64)]| -> Vec<Value> {
        steps
            .iter()
            .map(|(_, l, r)| json!({"state_loads":l,"state_reuses":r}))
            .collect()
    };
    let evidence = json!({"a":counts(&in_a),"b":counts(&in_b)});
    println!("{evidence}");
    for (i, ((reply_a, ..), (reply_b, ..))) in in_a.iter().zip(&in_b).enumerate() {
        ensure!(reply_a == reply_b, "step {i}:\nA: {reply_a}\nB: {reply_b}");
    }
    ensure!(
        in_a[3].0.get("error").is_some(),
        "the stale save passed: {}",
        in_a[3].0
    );
    for (i, (reply, loads, reuses)) in in_a.iter().enumerate() {
        ensure!(i == 3 || reply.get("ok").is_some(), "step {i}: {reply}");
        match i {
            // The first write may start from the state of the setup writes of this process.
            0 => {}
            // The write after the failed write and the writes after each external change.
            4 | 6 | 8 => ensure!(*loads >= 1 && *reuses == 0, "A step {i}: {evidence}"),
            _ => ensure!(*loads == 0 && *reuses == 1, "A step {i}: {evidence}"),
        }
    }
    for (i, (_, loads, reuses)) in in_b.iter().enumerate() {
        ensure!(*loads >= 1 && *reuses == 0, "B step {i}: {evidence}");
    }
    let (rows_a, rows_b) = (
        compare::rows(&pool, &a).await?,
        compare::rows(&pool, &b).await?,
    );
    for ((table, x), (_, y)) in rows_a.iter().zip(&rows_b) {
        ensure!(!x.is_empty(), "no {table} rows");
        if let Some(diff) = compare::difference(x, y) {
            anyhow::bail!("{table} differs:\n{diff}");
        }
    }
    // The writes after the direct change keep it: none of them wrote the old state back.
    let sql = "SELECT value->>'title' FROM joppa.state_entries WHERE workspace=$1 AND section='meta' AND key=''";
    let title: Option<String> = pool.get().await?.query_one(sql, &[&a]).await?.get(0);
    ensure!(title.as_deref() == Some("Changed by hand"), "{title:?}");
    Ok(())
}
