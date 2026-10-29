// Part 2 of tests/incremental_projection.rs, included there, so the test names stay unchanged.

/// One sequence, ordered by the seed. The seed only permutes steps that do not
/// depend on each other, so every seed must reach the same final projection.
fn sequence(seed: usize) -> Vec<(&'static str, Value)> {
    let scope = "One agreed build set";
    let subject =
        json!({"app":"sha-a","backend":"sha-b","infra":"config-v1","environment":"staging"});
    let note = |id: &str, title: &str, parent: Option<&str>, links: Value| {
        json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":parent,
            "data":{"title":title,"owner":"owner","body":"Recorded input"},"links":links,
            "reason":"Record an input"})
    };
    let mut steps: Vec<(&'static str, Value)> = vec![
        ("confirm", json!({"op":"confirm","req":"r","revision":1})),
        ("assess ios", assess("ios", true)),
        ("assess key", assess("key", true)),
    ];
    // Three linked inputs: a note has no parent, so each input links to the one above it.
    let nesting = vec![
        ("note top", note("top", "Top input", None, json!([]))),
        (
            "note middle",
            note(
                "middle",
                "Middle input",
                None,
                json!([{"target":"note:top","relation":"related"}]),
            ),
        ),
        (
            "note leaf",
            note(
                "leaf",
                "Leaf input",
                None,
                json!([{"target":"note:middle","relation":"related"}]),
            ),
        ),
    ];
    // Two inputs that point at each other: an ordinary cycle is allowed and
    // must not make the closure loop.
    let cycle = vec![
        ("note left", note("left", "Left input", None, json!([]))),
        (
            "note right",
            note(
                "right",
                "Right input",
                None,
                json!([{"target":"note:left","relation":"related"}]),
            ),
        ),
        (
            "note left points back",
            json!({"op":"save_object","kind":"note","id":"left","revision":1,"parent":null,
                "data":{"title":"Left input","owner":"owner","body":"Recorded input"},
                "links":[{"target":"note:right","relation":"related"}],"reason":"Close the cycle"}),
        ),
    ];
    if seed.is_multiple_of(2) {
        steps.extend(nesting);
        steps.extend(cycle);
    } else {
        steps.extend(cycle);
        steps.extend(nesting);
    }
    steps.extend(vec![
        (
            "empty group",
            json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"}),
        ),
        (
            "archive group",
            json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive the empty group"}),
        ),
        (
            "write while archived",
            json!({"op":"note","target":"domain:d","id":"archive-context","text":"Keep the group archived"}),
        ),
        (
            "restore group",
            json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore the group"}),
        ),
        (
            "edit restored group",
            json!({"op":"edit_domain","id":"unused","title":"Restored","owner":"owner","reason":"Edit the restored group"}),
        ),
        (
            "verification scope",
            json!({"op":"define_verification_scope","req":"r","revision":1,
                "subject_keys":["app","backend","infra","environment"],"scope":scope,
                "reason":"Agree the components"}),
        ),
        (
            "plan",
            json!({"op":"plan","req":"r","revision":1,"run":"r1","subject":subject,"scope":scope,
                "discovery_complete":true,
                "checks":[{"id":"t-ios[ios]","ac":"ios","method":"automated","expected":["access survives"]},
                          {"id":"t-key[key]","ac":"key","method":"automated","expected":["read and verify only"]}]}),
        ),
        // Evidence only: no document changes at all, yet readiness moves.
        (
            "result ios",
            json!({"op":"result","req":"r","revision":1,"run":"r1",
                "evidence":{"check":"t-ios[ios]","outcome":"pass","observed":["access survives"],
                "note":"Recorded by the suite"}}),
        ),
        (
            "result key",
            json!({"op":"result","req":"r","revision":1,"run":"r1",
                "evidence":{"check":"t-key[key]","outcome":"pass","observed":["read and verify only"],
                "note":"Recorded by the suite"}}),
        ),
        ("finish", json!({"op":"finish","req":"r","revision":1,"run":"r1"})),
        // A blocking dependency on a criterion: its evidence decides readiness.
        (
            "note waits for a criterion",
            json!({"op":"save_object","kind":"note","id":"waiting","revision":0,"parent":null,
                "data":{"title":"Waiting input","owner":"owner","body":"Needs the criterion"},
                "links":[{"target":"ac:key","relation":"depends_on","revision":1,"blocking":true}],
                "reason":"Depend on a criterion"}),
        ),
        (
            "task",
            json!({"op":"task","req":"r","revision":1,"id":"t1","title":"Carry the work",
                "acs":["ios"],"projects":["app"],"purpose":"implementation",
                "contract":"Change the access path"}),
        ),
        (
            "claim",
            json!({"op":"claim_task","req":"r","revision":1,"id":"t1"}),
        ),
        (
            "process the input",
            json!({"op":"decide_note","target":"note:top","revision":1,"outcome":"changed_spec",
                "reason":"Applied to the tree","links":["requirement:r"]}),
        ),
        // A new revision resets readiness and marks carried work for review.
        ("revise", revision(1)),
    ]);
    steps
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn incremental_projection_matches_a_full_rebuild() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let name: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    drop(conn);
    store::migrate(&pool).await?;
    for seed in [0usize, 1] {
        let ws = format!(
            "incremental-{seed}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let mut counter = 0;
        for command in setup() {
            step(&pool, &ws, &mut counter, "setup", command).await?;
        }
        for (label, command) in sequence(seed) {
            step(&pool, &ws, &mut counter, label, command).await?;
        }
    }
    Ok(())
}
