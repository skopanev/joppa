//! The parity fixture: a domain, two capabilities, two requirements, three criteria with
//! test lists and two Tasks.
use super::*;

pub const ADDRESSES: [&str; 10] = [
    "domain:d",
    "capability:c",
    "capability:c2",
    "requirement:r1",
    "requirement:r2",
    "ac:a1",
    "ac:a2",
    "ac:a3",
    "task:t1",
    "task:t2",
];
pub const CRITERIA: [&str; 3] = ["ac:a1", "ac:a2", "ac:a3"];

pub fn list(tests: &[&str]) -> Vec<String> {
    tests.iter().map(|t| format!("{FILE} > {t}")).collect()
}

/// Build the fixture and return the command count.
pub async fn fixture(pool: &deadpool_postgres::Pool) -> Result<usize> {
    let mut n = 0;
    let owner = |title: &str| json!({"title":title,"owner":"owner"});
    run(pool, &mut n, json!({"op":"workspace","title":"Parity"})).await?;
    for command in [
        save("consumer", "user", None, owner("User")),
        save("domain", "d", None, owner("Domain")),
        save("capability", "c", Some("domain:d"), owner("Capability")),
        save(
            "capability",
            "c2",
            Some("domain:d"),
            owner("Second capability"),
        ),
        save(
            "requirement",
            "r2",
            Some("capability:c"),
            json!({"title":"Second","owner":"owner","body":"Rule two.","consumers":["user"]}),
        ),
        save(
            "requirement",
            "r1",
            Some("capability:c"),
            json!({"title":"First","owner":"owner","body":"Rule one.","consumers":["user"]}),
        ),
    ] {
        run(pool, &mut n, command).await?;
    }
    let gwt = "Given: a state.\nWhen: an action.\nThen: a result.\nEvidence: a check.";
    for (id, parent) in [
        ("a1", "requirement:r1"),
        ("a2", "requirement:r1"),
        ("a3", "requirement:r2"),
    ] {
        run(
            pool,
            &mut n,
            save(
                "ac",
                id,
                Some(parent),
                json!({"title":format!("Criterion {id}"),"owner":"owner","consumer":"user","description":gwt}),
            ),
        )
        .await?;
    }
    for (id, parent) in [("t1", "ac:a1"), ("t2", "ac:a3")] {
        run(pool, &mut n, save("task", id, Some(parent), json!({"title":format!("Task {id}"),"owner":"owner","contract":"Do it","purpose":"implementation","projects":[]}))).await?;
    }
    for (target, tests) in [
        ("ac:a1", list(&["x1", "x2", "x3"])),
        ("ac:a2", list(&["y1"])),
        ("ac:a3", list(&["z1"])),
    ] {
        run(
            pool,
            &mut n,
            json!({"op":"set_ac_tests","target":target,"tests":tests,"reason":"Bind the tests"}),
        )
        .await?;
    }
    Ok(n)
}
