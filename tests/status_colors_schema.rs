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

fn colors(w: &World) -> Value {
    serde_json::to_value(joppa::objects::schema(&w.state).status_colors).unwrap()
}

#[test]
fn a_schema_change_saves_status_colours_and_a_later_change_keeps_them() {
    let mut w = World::new();
    let types = schema_types(&w);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,
               "status_colors":{"green":"#22a559","grey":"#c8c3b8"},"reason":"Contrast"}),
    );
    assert_eq!(colors(&w), json!({"green":"#22a559","grey":"#c8c3b8"}));
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":1,"types":types,"reason":"Other change"}),
    );
    assert_eq!(
        colors(&w),
        json!({"green":"#22a559","grey":"#c8c3b8"}),
        "a change without status_colors keeps them"
    );
}

#[test]
fn an_unknown_tone_or_a_bad_colour_is_refused_without_a_new_revision() {
    let mut w = World::new();
    let types = schema_types(&w);
    for (colors_value, error) in [
        (json!({"blue":"#000000"}), "status tones are"),
        (json!({"grey":"grey"}), "#RRGGBB"),
    ] {
        let e = w
            .apply(
                "owner",
                json!({"op":"set_tree_schema","revision":0,"types":types,
                       "status_colors":colors_value,"reason":"Bad"}),
            )
            .expect_err("a bad colour must be refused");
        assert!(e.to_string().contains(error), "{e}");
    }
    assert_eq!(joppa::objects::schema(&w.state).revision, 0);
    assert_eq!(colors(&w), Value::Null);
}

#[test]
fn ordered_statuses_save_with_both_themes_and_set_the_children_order() {
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
    let ordered = joppa::objects::ordered_types(&schema);
    assert_eq!(
        ordered["requirement"].status_order.as_deref(),
        Some(&["green".to_owned(), "red".to_owned(), "grey".to_owned()][..]),
        "a higher entry wins in a children rule"
    );
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
