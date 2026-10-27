//! The tone of each node for each value of a board field of the result shape. The board
//! shows these tones when a viewer filters by one value. The status rules of the types
//! make them from the criterion statuses of that value, as they make the main tone.
use super::*;
use std::collections::BTreeMap;

/// Store `value_tones` on each row: the key is `field:value`, the value is the tone.
pub(super) async fn add(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    base: &[Value],
    items: &mut [Value],
    types: &BTreeMap<String, objects::TreeType>,
) -> Result<()> {
    let board = crate::result_shape::board_fields(objects::schema(state).test_results.as_ref());
    let mut tones = BTreeMap::<String, serde_json::Map<String, Value>>::new();
    for (field, values) in &board {
        for value in values {
            let mut copy = base.to_vec();
            super::add_status_tones(client, workspace, &mut copy, types, Some((field, value)))
                .await?;
            for item in &copy {
                let (Some(address), Some(tone)) =
                    (item["address"].as_str(), item["signal"]["tone"].as_str())
                else {
                    continue;
                };
                tones
                    .entry(address.into())
                    .or_default()
                    .insert(format!("{field}:{value}"), json!(tone));
            }
        }
    }
    for item in items.iter_mut() {
        if let Some(map) = item["address"].as_str().and_then(|a| tones.remove(a)) {
            item["value_tones"] = Value::Object(map);
        }
    }
    Ok(())
}
