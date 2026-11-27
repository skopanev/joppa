//! Part of `store_data`: one item of a reply with its requested fields, moved from
//! store_data.rs.
use super::*;

/// One item: the address, the type, the revision and each requested field that is not null.
pub(super) fn item(
    r: &tokio_postgres::Row,
    fields: &[String],
    facts: &crate::dependency_sql::Facts,
) -> Result<Value> {
    let address: String = r.get(0);
    let mut item = json!({
        "address": r.get::<_, String>(0),
        "kind": r.get::<_, String>(1),
        "revision": r.get::<_, i32>(2),
    });
    let data: Option<&str> = r.get(8);
    let data: Value = data
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    for field in fields {
        let value = match field.as_str() {
            "title" => json!(r.get::<_, String>(4)),
            "body" => json!(r.get::<_, Option<String>>(5)),
            "owner" => json!(r.get::<_, Option<String>>(6)),
            "parent" => json!(r.get::<_, Option<String>>(3)),
            "status" => json!(r.get::<_, Option<String>>(7)),
            "relations" => relations(r.get(9))?,
            "contract_hash" => json!(r.get::<_, Option<String>>(10)),
            "parent_hash" => json!(r.get::<_, Option<String>>(11)),
            "parent_hash_current" => json!(r.get::<_, Option<String>>(12)),
            "parent_changed" => {
                let stamp: Option<String> = r.get(11);
                json!(stamp.is_some() && stamp != r.get::<_, Option<String>>(12))
            }
            "blocked" => json!(facts.is_blocked(&address)),
            "dependents" => json!(facts.count(&address)),
            other => data.get(other).cloned().unwrap_or_default(),
        };
        if !value.is_null() {
            item[field] = value;
        }
    }
    Ok(item)
}

/// The stored relations of a document, also of an older one with links, or {} without any.
fn relations(text: Option<&str>) -> Result<Value> {
    let parts: Value = text
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    Ok(match crate::relations::relations_value(&parts) {
        Value::Null => json!({}),
        value => value,
    })
}
