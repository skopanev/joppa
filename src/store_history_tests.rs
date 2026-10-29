use super::{page_limit, parse_mark};
use serde_json::json;

#[test]
fn a_history_page_holds_1_to_25_entries() {
    assert_eq!(page_limit(&json!({})).unwrap(), 25);
    assert_eq!(page_limit(&json!({"limit": 1})).unwrap(), 1);
    assert_eq!(page_limit(&json!({"limit": 25})).unwrap(), 25);
    assert_eq!(page_limit(&json!({"limit": 26})).unwrap(), 25);
    for bad in [json!(0), json!("5")] {
        assert!(
            page_limit(&json!({"limit": bad}))
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
    }
}

#[test]
fn a_mark_refuses_actor_at_16_nodes_and_repeats() {
    let nodes: Vec<String> = (0..16).map(|i| format!("task:t{i}")).collect();
    for (input, error) in [
        (
            json!({"position": 2, "reason": "Done", "actor": "x"}),
            "actor comes from the server",
        ),
        (
            json!({"position": 2, "reason": "Done", "at": "2020-01-01T00:00:00Z"}),
            "at comes from the server",
        ),
        (
            json!({"position": 2, "reason": "Done", "nodes": nodes}),
            "at most 15 nodes",
        ),
        (
            json!({"position": 2, "reason": "Done", "nodes": ["task:a", "task:a"]}),
            "each node once",
        ),
        (json!({"position": 2, "reason": " "}), "needs a reason"),
        (json!({"position": 0, "reason": "Done"}), "journal position"),
    ] {
        let message = parse_mark(&input).unwrap_err().to_string();
        assert!(message.contains(error), "{input}: {message}");
    }
    assert!(
        parse_mark(&json!({"workspace": "w", "position": 2, "reason": "Cut", "nodes": ["task:a"]}))
            .is_ok()
    );
}
