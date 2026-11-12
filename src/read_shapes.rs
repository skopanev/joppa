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

/// Read the optional `fields` argument: a list of key names. A removed key is refused.
pub fn fields_argument(value: &Value) -> anyhow::Result<Option<Vec<String>>> {
    let fields = match value {
        Value::Null => return Ok(None),
        Value::Array(names) => names
            .iter()
            .map(|n| {
                n.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow::anyhow!("fields is a list of key names"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
        _ => anyhow::bail!("fields is a list of key names"),
    };
    check_removed(&fields)?;
    Ok(Some(fields))
}

/// Refuse a field that names a removed reply key, such as work_ready.
pub fn check_removed(fields: &[String]) -> anyhow::Result<()> {
    match fields
        .iter()
        .find_map(|f| crate::removed_ops::removed_read_key(f))
    {
        Some(refusal) => Err(anyhow::anyhow!(refusal)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_field_with_a_removed_key_is_refused() {
        // ac:q2qmqne7
        for name in ["work_ready", "waits_for"] {
            let error = fields_argument(&json!(["title", name])).unwrap_err();
            assert_eq!(error.to_string(), format!("{name} was removed"));
        }
        assert!(fields_argument(&json!(["title"])).is_ok());
    }

    #[test]
    fn keep_fields_keeps_the_address_and_the_named_keys() {
        let mut item = json!({"address":"ac:a","title":"T","process":{},"signal":{}});
        keep_fields(&mut item, &["title".into()]);
        assert_eq!(item, json!({"address":"ac:a","title":"T"}));
    }
}
