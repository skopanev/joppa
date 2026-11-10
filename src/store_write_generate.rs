//! Part of `store_write`: `generate` and the items after it, moved from store_write.rs unchanged.
use super::*;

/// A new local ID: 8 characters of [a-z0-9] from a random UUID, about 2.8e12 values.
pub(crate) fn short_id() -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut n = Uuid::new_v4().as_u128();
    (0..8)
        .map(|_| {
            let digit = DIGITS[(n % 36) as usize] as char;
            n /= 36;
            digit
        })
        .collect()
}

/// A random id for a `save_object` without one. The write path takes a free id instead.
pub(crate) fn generate(command: &mut Value) -> Result<()> {
    let op = command["op"].as_str().unwrap_or("").to_owned();
    let c = command
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("command must be an object"))?;
    if op == "save_object" {
        c.entry("id").or_insert_with(|| json!(short_id()));
    }
    Ok(())
}

// Explicit references avoid interpreting titles or user-provided text as templates.
pub(crate) fn resolve(value: &Value, refs: &BTreeMap<String, Value>) -> Result<Value> {
    match value {
        Value::Object(map) if map.contains_key("$ref") => {
            ensure!(map.len() == 1, "$ref must be the only field");
            let path = map["$ref"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("$ref must be a string"))?;
            let (name, path) = path
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("use $ref: label/field"))?;
            refs.get(name)
                .and_then(|v| v.pointer(&format!("/{path}")))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown or forward batch reference: {name}/{path}"))
        }
        Value::Object(map) => Ok(Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, refs)?)))
                .collect::<Result<_>>()?,
        )),
        Value::Array(values) => Ok(Value::Array(
            values
                .iter()
                .map(|v| resolve(v, refs))
                .collect::<Result<_>>()?,
        )),
        _ => Ok(value.clone()),
    }
}
