mod common;
use common::*;
use deadpool_postgres::Pool;
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};

async fn apply(pool: &Pool, ws: &str, actor: &str, command: Value) -> anyhow::Result<Value> {
    store::execute_input(pool, ws, actor, &uuid::Uuid::new_v4().to_string(), &command).await
}
async fn fixture() -> anyhow::Result<(Pool, String)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, format!("planning-{}", uuid::Uuid::new_v4().simple())))
}
async fn candidates(pool: &Pool, ws: &str) -> anyhow::Result<Value> {
    store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            limit: 100,
            ..Default::default()
        },
    )
    .await
}
fn criteria(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            i["signal"]["assessment"]["criterion"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}
async fn task_detail(pool: &Pool, ws: &str, address: &str) -> anyhow::Result<Value> {
    store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}
async fn assess_current(pool: &Pool, ws: &str, address: &str, ready: bool) -> anyhow::Result<()> {
    let obj = task_detail(pool, ws, address).await?;
    apply(pool, ws, "agent", json!({"op":"assess","req":obj["process"]["req"],"revision":obj["process"]["revision"],"ac":obj["process"]["criterion"],"assessment":{"ready":ready,"reason":if ready {"Contract is clear"} else {"Which behavior is required?"},"recipient":if ready {Value::Null} else {json!("owner")}}})).await?;
    Ok(())
}

// [ac:unstarted-work-visible-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn native_planning_preserves_tasks_and_excludes_current_passing_criteria()
-> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    for command in setup() {
        apply(&pool, &ws, "owner", command).await?;
    }
    let roots = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            kind: Some("domain"),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        roots["items"][0]["verification_summary"],
        json!({"total":1,"passed":0,"failed":0})
    );
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    apply(&pool, &ws, "agent", assess("ios", true)).await?;
    apply(&pool, &ws, "agent", assess("key", false)).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["ios"]);
    assert_eq!(page["items"][0]["signal"]["assessment"]["revision"], 1);
    assert!(
        store::read_objects(
            &pool,
            &ws,
            "outsider",
            &ObjectScope {
                work_filter: Some("unstarted"),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    let task = |id: &str| json!({"op":"task","req":"r","revision":1,"id":id,"title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Implement the agreed login behavior"});
    apply(&pool, &ws, "agent", task("one")).await?;
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    let queue = store::task_queue(&pool, &ws, "agent", "available", 100, None).await?;
    let address = queue["tasks"][0]["address"].as_str().unwrap().to_owned();
    assert_eq!(queue["tasks"][0]["assessment"]["criterion"], "ios");
    apply(
        &pool,
        &ws,
        "agent",
        json!({"op":"claim_task","id":address,"revision":queue["tasks"][0]["revision"]}),
    )
    .await?;
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    let current = task_detail(&pool, &ws, &address).await?;
    apply(&pool, &ws, "agent", json!({"op":"complete_task","id":address,"revision":current["current_revision"],"evidence":"The fixture implementation is complete"})).await?;
    let done = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&done), vec!["ios"]);
    assert_eq!(
        done["items"][0]["work_summary"],
        json!({"active":0,"done":1,"retired":0,"done_verification":{"pass":0,"fail":0,"unverified":1}})
    );
    assert_eq!(
        task_detail(&pool, &ws, &address).await?["process"]["verification"],
        "unverified"
    );
    apply(&pool, &ws, "agent", task("two")).await?;
    let all = store::task_queue(&pool, &ws, "agent", "all", 100, None).await?;
    let retired = all["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["status"] == "open")
        .unwrap();
    apply(&pool, &ws, "owner", json!({"op":"retire_task","id":retired["address"],"revision":retired["revision"],"reason":"The fixture work is no longer required"})).await?;
    let retired_page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&retired_page), vec!["ios"]);
    assert_eq!(
        retired_page["items"][0]["work_summary"],
        json!({"active":0,"done":1,"retired":1,"done_verification":{"pass":0,"fail":0,"unverified":1}})
    );
    assert_eq!(
        store::task_queue(&pool, &ws, "agent", "all", 100, None).await?["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let old = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some(&address),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        old["document"]["data"]["contract"],
        "Implement the agreed login behavior"
    );
    apply(&pool, &ws, "agent", assess("key", true)).await?;
    let first = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    let next = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            limit: 1,
            cursor: first["next_cursor"].as_str(),
            ..Default::default()
        },
    )
    .await?;
    assert_ne!(first["items"][0]["address"], next["items"][0]["address"]);
    assert!(next["next_cursor"].is_null());
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"confirm","req":"r","revision":1}),
    )
    .await?;
    assert!(
        store::read_objects(
            &pool,
            &ws,
            "agent",
            &ObjectScope {
                work_filter: Some("unstarted"),
                cursor: first["next_cursor"].as_str(),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    apply(&pool, &ws, "owner", json!({"op":"define_verification_scope","req":"r","revision":1,"subject_keys":["app","backend","infra","environment"],"scope":"iOS access and provider settings","reason":"Use the complete fixture subject"})).await?;
    apply(&pool, &ws, "runner", plan("mixed")).await?;
    apply(
        &pool,
        &ws,
        "runner",
        result(
            "mixed",
            "[ios] paid access",
            "pass",
            vec!["access retained"],
        ),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "owner",
        result("mixed", "key-permissions", "fail", vec!["read", "verify"]),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"mixed"}),
    )
    .await?;
    let roots = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            kind: Some("domain"),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        roots["items"][0]["verification_summary"],
        json!({"total":1,"passed":0,"failed":0})
    );
    assert_eq!(roots["items"][0]["signal"]["tone"], "grey");
    apply(&pool, &ws, "runner", plan("pass")).await?;
    apply(
        &pool,
        &ws,
        "runner",
        result("pass", "[ios] paid access", "pass", vec!["access retained"]),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "owner",
        result("pass", "key-permissions", "pass", vec!["read", "verify"]),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"pass"}),
    )
    .await?;
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    let roots = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            kind: Some("domain"),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        roots["items"][0]["verification_summary"],
        json!({"total":1,"passed":0,"failed":0})
    );
    assert_eq!(roots["items"][0]["signal"]["tone"], "grey");
    apply(&pool, &ws, "agent", revision(1)).await?;
    let roots = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            kind: Some("domain"),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        roots["items"][0]["verification_summary"],
        json!({"total":1,"passed":0,"failed":0})
    );
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    for ac in ["ios", "key"] {
        let mut a = assess(ac, true);
        a["revision"] = json!(2);
        apply(&pool, &ws, "agent", a).await?;
    }
    assert_eq!(criteria(&candidates(&pool, &ws).await?).len(), 2);
    Ok(())
}

// [ac:unstarted-work-visible-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn custom_planning_uses_roles_nested_task_bindings_and_current_dependencies()
-> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"workspace","title":"Custom planning"}),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"member","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let types = json!({"spec":{"title":"Specification","role":"requirement","root":true,"parents":["*"]},"condition":{"title":"Condition","role":"ac","root":false,"parents":["*"]},"section":{"title":"Section","role":"group","root":true,"parents":["*"]},"job":{"title":"Job","role":"task","root":false,"parents":["*"],"parent_gate":"ready"},"input":{"title":"Input","role":"document","inbox":true,"root":true,"parents":["*"]}});
    apply(&pool,&ws,"agent",json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Use arbitrary type names"})).await?;
    for (kind, id, parent, data) in [
        (
            "spec",
            "s",
            None,
            json!({"title":"Spec","owner":"owner","body":"The exact contract"}),
        ),
        (
            "condition",
            "a",
            Some("spec:s"),
            json!({"title":"First condition","owner":"owner"}),
        ),
        (
            "condition",
            "b",
            Some("spec:s"),
            json!({"title":"Second condition","owner":"owner","depends_on":["condition:a"]}),
        ),
        (
            "section",
            "nested",
            Some("condition:a"),
            json!({"title":"Nested work","owner":"owner"}),
        ),
        (
            "section",
            "blocker",
            None,
            json!({"title":"Unverified dependency","owner":"owner"}),
        ),
        (
            "input",
            "raw",
            None,
            json!({"title":"Unrelated source","owner":"owner","body":"raw input ".repeat(100_000)}),
        ),
    ] {
        apply(&pool,&ws,"agent",json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"links":[],"reason":"Set up the planning fixture"})).await?;
    }
    assess_current(&pool, &ws, "condition:a", false).await?;
    assess_current(&pool, &ws, "condition:b", true).await?;
    assert!(
        criteria(&candidates(&pool, &ws).await?).is_empty(),
        "waiting prerequisite must exclude the dependent criterion"
    );
    assess_current(&pool, &ws, "condition:a", true).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["condition:a", "condition:b"]);
    assert!(!page.to_string().contains("raw input"));
    for item in page["items"].as_array().unwrap() {
        let source = task_detail(&pool, &ws, item["address"].as_str().unwrap()).await?;
        assert_eq!(item["revision"], source["current_revision"]);
        assert_eq!(
            item["signal"]["assessment"],
            source["process"]["assessment"]
        );
    }
    let ordinary_before = store::read_objects(&pool, &ws, "agent", &ObjectScope::default()).await?;
    let spec = ordinary_before["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["address"] == "spec:s")
        .unwrap();
    assert_eq!(
        spec["verification_summary"],
        json!({"total":2,"passed":0,"failed":0})
    );
    let unrelated = ordinary_before["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["address"] == "input:raw")
        .unwrap();
    // The wildcard schema permits criteria below this type, but it has no children.
    assert_eq!(
        unrelated["verification_summary"],
        json!({"total":0,"passed":0,"failed":0})
    );
    assert!(
        ordinary_before["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["parent"].is_null() && item.get("work_summary").is_none())
    );
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"job","id":"spec-work","revision":0,"parent":"spec:s","data":{"title":"Work on the specification","owner":"owner","contract":"Work on the specification as a whole","purpose":"implementation"},"reason":"Check the specification binding boundary"})).await?;
    assert_eq!(
        criteria(&candidates(&pool, &ws).await?),
        vec!["condition:a", "condition:b"]
    );
    assert!(
        candidates(&pool, &ws).await?["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["work_summary"] == json!({"active":0,"done":0,"retired":0,"done_verification":{"pass":0,"fail":0,"unverified":0}}))
    );
    let ordinary_after = store::read_objects(&pool, &ws, "agent", &ObjectScope::default()).await?;
    let mut expected = ordinary_before["items"].clone();
    // The wildcard schema also permits a criterion below the new direct work child.
    let spec = expected
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["address"] == "spec:s")
        .unwrap();
    spec["verification_summary"] = json!({"total":3,"passed":0,"failed":0});
    assert_eq!(expected, ordinary_after["items"]);
    let direct = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            parent: Some("spec:s"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(criteria(&direct), vec!["condition:a", "condition:b"]);
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":"section:nested","data":{"title":"Implement the first condition","owner":"owner","contract":"Implement this exact condition","purpose":"implementation"},"reason":"Bind work through an intermediate group"})).await?;
    assert_eq!(
        criteria(&candidates(&pool, &ws).await?),
        vec!["condition:b"]
    );
    let queue = store::task_queue(&pool, &ws, "agent", "available", 100, None).await?;
    assert!(
        queue["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["address"] == "job:one")
    );
    let b = task_detail(&pool, &ws, "condition:b").await?;
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"condition","id":"b","revision":b["current_revision"],"parent":b["document"]["parent"],"data":b["document"]["data"],"links":[{"target":"section:blocker","relation":"depends_on","revision":1,"blocking":true}],"reason":"Require an unmet dependency"})).await?;
    assess_current(&pool, &ws, "condition:a", true).await?;
    assess_current(&pool, &ws, "condition:b", true).await?;
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    let b = task_detail(&pool, &ws, "condition:b").await?;
    assert_eq!(b["work_ready"], false);
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"condition","id":"b","revision":b["current_revision"],"parent":b["document"]["parent"],"data":b["document"]["data"],"links":[],"reason":"Remove the fixture dependency"})).await?;
    assess_current(&pool, &ws, "condition:a", true).await?;
    assess_current(&pool, &ws, "condition:b", true).await?;
    let task = task_detail(&pool, &ws, "job:one").await?;
    apply(&pool,&ws,"owner",json!({"op":"retire_task","id":"job:one","revision":task["current_revision"],"reason":"Keep the fixture history"})).await?;
    let retired_page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&retired_page).len(), 2);
    assert_eq!(
        retired_page["items"][0]["work_summary"],
        json!({"active":0,"done":0,"retired":1,"done_verification":{"pass":0,"fail":0,"unverified":0}})
    );
    assert_eq!(
        task_detail(&pool, &ws, "job:one").await?["process"]["status"],
        "retired"
    );
    Ok(())
}

/// Fixture 1: criterion A is ready, has valid dependencies, no green JUnit status and no
/// planning claim. L (`agent`) holds T at revision 4 and blocks it. Fixture 2 adds Task V
/// under A in progress for `worker`.
async fn blocked_fixture(with_v: bool) -> anyhow::Result<(Pool, String)> {
    let (pool, ws) = fixture().await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"workspace","title":"Blocked work"}),
    )
    .await?;
    for actor in ["agent", "worker"] {
        apply(
            &pool,
            &ws,
            "owner",
            json!({"op":"member","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&pool, &ws, "owner", None).await?;
    apply(&pool, &ws, "owner", json!({"op":"set_tree_schema","revision":schema["current_revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
        "work":{"title":"Work","role":"task","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    let work = |id: &str| json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":"criterion:a","data":{"title":id,"owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Set the work"});
    for command in [
        json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":"owner"},"reason":"Set the rule"}),
        json!({"op":"save_object","kind":"criterion","id":"a","revision":0,"parent":"spec:one","data":{"title":"A","owner":"owner"},"reason":"Set the criterion"}),
    ] {
        apply(&pool, &ws, "owner", command).await?;
    }
    let spec = task_detail(&pool, &ws, "spec:one").await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":spec["process"]["revision"]}),
    )
    .await?;
    assess_current(&pool, &ws, "criterion:a", true).await?;
    apply(&pool, &ws, "owner", work("t")).await?;
    for (op, extra) in [
        ("claim_task", json!({})),
        ("release_task", json!({"reason":"Return the Task"})),
        ("claim_task", json!({})),
    ] {
        let revision = task_detail(&pool, &ws, "work:t").await?["current_revision"].clone();
        let mut command = json!({"op":op,"id":"work:t","revision":revision});
        command
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        apply(&pool, &ws, "agent", command).await?;
    }
    assert_eq!(
        task_detail(&pool, &ws, "work:t").await?["current_revision"],
        4
    );
    if with_v {
        apply(&pool, &ws, "owner", work("v")).await?;
        apply(
            &pool,
            &ws,
            "worker",
            json!({"op":"claim_task","id":"work:v","revision":1}),
        )
        .await?;
    }
    apply(&pool, &ws, "agent", json!({"op":"block_task","id":"work:t","revision":4,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
    let a = task_detail(&pool, &ws, "criterion:a").await?;
    assert_eq!(a["process"]["assessment"]["state"], "ready");
    assert!(a["work_ready"] == true && a["process"]["planning_claim"].is_null());
    assert_ne!(a["process"]["verification"], "pass");
    Ok((pool, ws))
}

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_blocked_task_does_not_keep_its_criterion_out_of_unstarted_work() -> anyhow::Result<()> {
    let (pool, ws) = blocked_fixture(false).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["criterion:a"]);
    assert_eq!(page["items"][0]["work_summary"]["active"], 0);

    // Fixture 2: ordinary work in progress keeps A out; planning still claims A.
    let (pool, ws) = blocked_fixture(true).await?;
    let page = candidates(&pool, &ws).await?;
    assert!(criteria(&page).is_empty(), "{page}");
    let a = task_detail(&pool, &ws, "criterion:a").await?;
    apply(
        &pool,
        &ws,
        "agent",
        json!({"op":"claim_planning","ac":"criterion:a","revision":a["current_revision"]}),
    )
    .await?;
    assert_eq!(
        task_detail(&pool, &ws, "criterion:a").await?["process"]["planning_claim"]["actor"],
        "agent"
    );

    // Fixture 2 without a planning claim: A returns after V ends.
    let (pool, ws) = blocked_fixture(true).await?;
    apply(&pool, &ws, "worker", json!({"op":"complete_task","id":"work:v","revision":2,"evidence":"The ordinary work is complete"})).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["criterion:a"]);
    store::check_journal(&pool).await?;
    Ok(())
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_workspace_owner_retires_a_blocked_task_without_a_planning_claim() -> anyhow::Result<()>
{
    // Fixture 5: T is blocked at revision 5 and A has no planning claim.
    let (pool, ws) = blocked_fixture(false).await?;
    apply(&pool, &ws, "owner", json!({"op":"retire_task","id":"work:t","revision":5,"reason":"The blocked work is no longer required"})).await?;
    assert_eq!(
        task_detail(&pool, &ws, "work:t").await?["process"]["status"],
        "retired"
    );
    let block = store::read_objects(
        &pool,
        &ws,
        "owner",
        &ObjectScope {
            address: Some("work:t"),
            revision: Some(5),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(block["document"]["reason"], "Verification failed");
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["criterion:a"]);
    assert!(task_detail(&pool, &ws, "criterion:a").await?["process"]["planning_claim"].is_null());
    store::check_journal(&pool).await?;
    Ok(())
}
