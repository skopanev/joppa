//! ac:3kejvme2 over a real server: the filter of POST /api/data matches each of the 5 field
//! types, a computed field, owner and assignee. A node without a value matches each filter of
//! the field. A wrong filter gives 400.
#[path = "common/data_tree.rs"]
mod data_tree;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use data_tree::{node, put_types, save};
use read_key::{addresses, data, read, with_key};
use record_world::World;
use serde_json::{Value, json};

/// box:root holds one type_a node for each value below. type_a:fast has the child
/// type_b:lane, whose lane the server computes from the parent.
async fn tree(w: &World) -> Result<()> {
    let lane = |from: &str| json!({"type":"enum","values":["fast","slow"],"from":from});
    let fields = json!({
        "area":{"type":"text"}, "retries":{"type":"number"}, "strict":{"type":"boolean"},
        "env":data_tree::env(), "tags":{"type":"list"}, "lane":lane("own"),
    });
    put_types(
        w,
        json!({
            "box": node("Boxes", "children", &[], json!({})),
            "type_a": node("Type A", "own", &["box"], fields),
            "type_b": node("Type B", "own", &["type_a"], json!({"lane":lane("parent")})),
        }),
    )
    .await?;
    save(w, "box:root", None, json!({})).await?;
    for (id, value) in [
        ("text", json!({"area":"x"})),
        ("text_star", json!({"area":"*"})),
        ("num", json!({"retries":3})),
        ("flag", json!({"strict":true})),
        ("en", json!({"env":"prod"})),
        ("list", json!({"tags":["api","db"]})),
        ("list_star", json!({"tags":["*"]})),
        ("fast", json!({"lane":"fast"})),
        ("own", json!({"owner":"alex"})),
        ("assigned", json!({"assignee":"alex"})),
        ("blank", json!({"area":"","tags":[],"retries":null})),
    ] {
        save(w, &format!("type_a:{id}"), Some("box:root"), value).await?;
    }
    save(w, "type_b:lane", Some("type_a:fast"), json!({})).await
}

/// Check, for each case, that the walk with the filter keeps the node exactly when `kept`.
async fn cases(w: &World, key: &str, cases: &[(&str, Value, bool)]) -> Result<()> {
    let mut wrong = vec![];
    for (address, filter, kept) in cases {
        let body = json!({"root":"box:root","kind":["type_a","type_b"],"filter":filter});
        let found = addresses(&read(w, key, body).await?);
        if found.iter().any(|a| a == address) != *kept {
            wrong.push(format!("{address} with {filter}: kept should be {kept}"));
        }
    }
    ensure!(wrong.is_empty(), "{wrong:#?}");
    Ok(())
}

// ac:3kejvme2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn text_number_boolean_and_enum_filters_match_a_value_a_list_and_a_star() -> Result<()> {
    with_key(tree, async |w, key| {
        let (a, n, b, e) = ("type_a:text", "type_a:num", "type_a:flag", "type_a:en");
        let list = [
            (a, json!({"area":"x"}), true),
            (a, json!({"area":["y","x"]}), true),
            (a, json!({"area":"*"}), true),
            (a, json!({"area":"y"}), false),
            ("type_a:text_star", json!({"area":"y"}), true),
            (n, json!({"retries":3}), true),
            (n, json!({"retries":[1,3]}), true),
            (n, json!({"retries":4}), false),
            (b, json!({"strict":true}), true),
            (b, json!({"strict":[false,true]}), true),
            (b, json!({"strict":false}), false),
            (e, json!({"env":"prod"}), true),
            (e, json!({"env":["dev","prod"]}), true),
            (e, json!({"env":"*"}), true),
            (e, json!({"env":"dev"}), false),
        ];
        cases(w, key, &list).await
    })
    .await
}

// ac:3kejvme2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_filter_matches_a_shared_value_and_a_star_list_matches_any() -> Result<()> {
    with_key(tree, async |w, key| {
        let l = "type_a:list";
        let list = [
            (l, json!({"tags":"db"}), true),
            (l, json!({"tags":["ui","api"]}), true),
            (l, json!({"tags":["ui"]}), false),
            ("type_a:list_star", json!({"tags":["ui"]}), true),
        ];
        cases(w, key, &list).await
    })
    .await
}

// ac:3kejvme2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_computed_lane_and_owner_and_assignee_match_as_text() -> Result<()> {
    with_key(tree, async |w, key| {
        let (own, assigned) = ("type_a:own", "type_a:assigned");
        let list = [
            ("type_b:lane", json!({"lane":"fast"}), true),
            ("type_b:lane", json!({"lane":"slow"}), false),
            (own, json!({"owner":"alex"}), true),
            (own, json!({"owner":"another-member"}), false),
            (assigned, json!({"assignee":"alex"}), true),
            (assigned, json!({"assignee":"another-member"}), false),
        ];
        cases(w, key, &list).await
    })
    .await
}

// ac:3kejvme2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_node_with_an_absent_null_empty_or_empty_list_value_matches_each_filter() -> Result<()> {
    with_key(tree, async |w, key| {
        // type_a:blank has area "", tags [] and retries null, and no strict, env, lane, owner
        // or assignee.
        let filters = [
            json!({"area":"y"}),
            json!({"tags":["ui"]}),
            json!({"retries":4}),
            json!({"strict":false}),
            json!({"env":"dev"}),
            json!({"lane":"slow"}),
            json!({"owner":"another-member"}),
            json!({"assignee":"another-member"}),
        ];
        let list: Vec<_> = filters
            .into_iter()
            .map(|f| ("type_a:blank", f, true))
            .collect();
        cases(w, key, &list).await
    })
    .await
}

// ac:3kejvme2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_undeclared_key_a_wrong_json_type_or_an_empty_list_gives_400() -> Result<()> {
    with_key(tree, async |w, key| {
        for filter in [
            json!({"nope":1}),
            json!({"retries":"3"}),
            json!({"retries":[1,"2"]}),
            json!({"strict":"true"}),
            json!({"env":1}),
            json!({"tags":[]}),
        ] {
            let body = json!({"root":"box:root","filter":filter});
            let (code, reply) = data(w, Some(key), &body).await?;
            ensure!(code == 400, "{filter}: {code} {reply}");
        }
        Ok(())
    })
    .await
}
