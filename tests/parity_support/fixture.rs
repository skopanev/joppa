//! The parity fixture: a domain, two capabilities, two requirements, three criteria with
//! test nodes and two Tasks.
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

/// A test node that binds the test `name` of FILE to a criterion.
pub fn bind(target: &str, name: &str) -> Value {
    json!({"op":"save_object","kind":"test","revision":0,"parent":target,"data":{"title":format!("{FILE} > {name}"),"owner":"owner"},"reason":"Bind the tests"})
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
                json!({"title":format!("Criterion {id}"),"owner":"owner","consumer":"user","body":gwt}),
            ),
        )
        .await?;
    }
    for (id, parent) in [("t1", "ac:a1"), ("t2", "ac:a3")] {
        run(
            pool,
            &mut n,
            save(
                "task",
                id,
                Some(parent),
                json!({"title":format!("Task {id}"),"owner":"owner","body":"Do it","projects":[]}),
            ),
        )
        .await?;
    }
    for (target, name) in [
        ("ac:a1", "x1"),
        ("ac:a1", "x2"),
        ("ac:a1", "x3"),
        ("ac:a2", "y1"),
        ("ac:a3", "z1"),
    ] {
        run(pool, &mut n, bind(target, name)).await?;
    }
    Ok(n)
}
