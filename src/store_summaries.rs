//! The requirement summaries that the board reads.
use super::*;

pub(super) fn summaries(state: &Workspace) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .map(|(id, q)| {
            (
                id.clone(),
                crate::objects::requirement_summary(state, id, q.revisions.last().unwrap()),
            )
        })
        .collect();
    Value::Object(summaries)
}
