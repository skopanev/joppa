//! An older journal keeps readiness marks and a readiness gate. Its replay accepts them, and
//! no read, filter or later write gives them an effect. A live write refuses them.
mod common;
use common::*;
use joppa::objects;
use joppa::removed_ops::{PARENT_GATE, refusal};
use serde_json::json;

// ac:sc6zw0sm
#[test]
fn an_old_journal_with_marks_and_a_gate_replays_and_a_live_write_refuses_them() {
    let mut w = World::new();
    // Each older form of a mark replays: of a criterion, of a node and of a signal.
    w.legacy(
        "agent",
        json!({"op":"assess","req":"r","revision":1,"ac":"ios","assessment":{"ready":false,
            "reason":"Which exact permissions?","recipient":"legal","decision":null}}),
    );
    w.legacy(
        "agent",
        json!({"op":"assess","target":"ac:key","ready":false,"reason":"Waits for legal"}),
    );
    w.legacy(
        "agent",
        json!({"op":"assess_signal","target":"ac:ios","ready":true,"reason":"Known scope"}),
    );
    // An older schema event names the gate of the Task type.
    let schema = objects::schema(&w.state);
    let mut types = serde_json::to_value(&schema.types).unwrap();
    types["task"]["parent_gate"] = json!("ready");
    let put = json!({"op":"set_tree_schema","revision":schema.revision,"types":types,
        "reason":"Gate the Tasks"});
    w.legacy("owner", put.clone());
    // No state, read or schema gives a mark or the gate.
    let state = serde_json::to_value(&w.state).unwrap();
    assert!(state.get("signal_assessments").is_none());
    for ac in state["requirements"]["r"]["revisions"].as_array().unwrap() {
        for criterion in ac["acs"].as_array().unwrap() {
            assert!(criterion.get("assessment").is_none(), "{criterion}");
        }
    }
    for key in ["ac:ios", "ac:key"] {
        assert!(objects::process(&w.state, key).get("assessment").is_none());
        let signal = objects::signal(&w.state, key);
        assert!(signal.get("assessment").is_none(), "{signal}");
        assert!(objects::present_in_tree(&w.state, key), "{key}");
    }
    let task = serde_json::to_value(&objects::schema(&w.state).types["task"]).unwrap();
    assert!(task.get("parent_gate").is_none(), "{task}");
    // A live write refuses a mark and the gate, and names the replacement.
    let live = json!({"op":"assess","target":"ac:ios","ready":true,"reason":"Known scope"});
    let error = w.apply("agent", live).unwrap_err().to_string();
    assert_eq!(error, refusal("assess"));
    assert!(error.contains("data.stage"), "{error}");
    let mut put = put;
    put["revision"] = json!(objects::schema(&w.state).revision);
    assert_eq!(w.apply("owner", put).unwrap_err().to_string(), PARENT_GATE);
}
