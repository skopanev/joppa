//! The tree schema sets the colour of each status dot.
mod common;

use common::World;
use serde_json::{Value, json};

fn schema_types(w: &World) -> Value {
    let mut types = joppa::objects::schema(&w.state).types;
    types.remove("intake");
    types.remove("point");
    serde_json::to_value(types).unwrap()
}

#[test]
fn ordered_statuses_save_with_both_themes() {
    let mut w = World::new();
    let types = schema_types(&w);
    let statuses = json!([
        {"tone":"green","light":"#22a559","dark":"#2fbf6a"},
        {"tone":"red","light":"#bc5d52","dark":"#e07a70"},
        {"tone":"grey","light":"#c8c3b8","dark":"#8a8f96"}
    ]);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"statuses":statuses,"reason":"Order"}),
    );
    let schema = joppa::objects::schema(&w.state);
    assert_eq!(serde_json::to_value(&schema.statuses).unwrap(), statuses);
    let e = w
        .apply(
            "owner",
            json!({"op":"set_tree_schema","revision":1,"types":types,
                   "statuses":[{"tone":"red","light":"#000000","dark":"red"}],"reason":"Bad"}),
        )
        .expect_err("a bad dark colour must be refused");
    assert!(e.to_string().contains("#RRGGBB"), "{e}");
    assert_eq!(joppa::objects::schema(&w.state).revision, 1);
}
