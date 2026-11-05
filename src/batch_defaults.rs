//! The defaults of joppa_batch (requirement:mcp-tools): the fields of save_object that each
//! save_object command of the batch takes when it omits them. A field of the command wins.
//! `data` merges by key, so a command keeps each data field that it gives.
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};

/// Complete each save_object command of `operations` with `defaults`. No defaults change
/// nothing. The completed commands go to the receipt, so an identical retry matches.
pub fn apply(defaults: &Value, operations: &mut Value) -> Result<()> {
    if defaults.is_null() {
        return Ok(());
    }
    let defaults = defaults
        .as_object()
        .context("defaults is an object of save_object fields")?;
    crate::commands::reject_actor_field(&Value::Object(defaults.clone()))?;
    ensure!(
        !defaults.contains_key("op"),
        "defaults has no op: it completes each save_object of the batch"
    );
    let data = defaults
        .get("data")
        .map(|d| d.as_object().context("defaults.data is an object"))
        .transpose()?;
    let commands = operations.as_array_mut().into_iter().flatten();
    for command in commands.filter_map(|o| o.get_mut("command")?.as_object_mut()) {
        if command.get("op").and_then(Value::as_str) != Some("save_object") {
            continue;
        }
        for (key, value) in defaults.iter().filter(|(key, _)| *key != "data") {
            command.entry(key).or_insert_with(|| value.clone());
        }
        if let Some(data) = data {
            let target = command
                .entry("data")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(target) = target.as_object_mut() {
                for (key, value) in data {
                    target.entry(key).or_insert_with(|| value.clone());
                }
            }
        }
    }
    Ok(())
}
