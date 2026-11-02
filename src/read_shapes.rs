//! Smaller shapes of large read replies, for clients with a reply size limit.
use serde_json::Value;

/// Keep only the named keys of one list item. The address always stays.
pub fn keep_fields(item: &mut Value, fields: &[String]) {
    if let Some(map) = item.as_object_mut() {
        map.retain(|key, _| key == "address" || fields.iter().any(|f| f == key));
    }
}

/// Keep only the named keys in each entry of `items` of a list reply.
pub fn keep_item_fields(mut reply: Value, fields: Option<&[String]>) -> Value {
    if let Some(fields) = fields {
        for item in reply["items"].as_array_mut().into_iter().flatten() {
            keep_fields(item, fields);
        }
    }
    reply
}

/// Read the optional `fields` argument: a list of key names.
pub fn fields_argument(value: &Value) -> anyhow::Result<Option<Vec<String>>> {
    match value {
        Value::Null => Ok(None),
        Value::Array(names) => names
            .iter()
            .map(|n| {
                n.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow::anyhow!("fields is a list of key names"))
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map(Some),
        _ => anyhow::bail!("fields is a list of key names"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keep_fields_keeps_the_address_and_the_named_keys() {
        let mut item = json!({"address":"ac:a","title":"T","process":{},"signal":{}});
        keep_fields(&mut item, &["title".into()]);
        assert_eq!(item, json!({"address":"ac:a","title":"T"}));
    }
}
