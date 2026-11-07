//! The data parameter of the node filters: the data filter, or with a dimension value or a
//! computed field the envelope {data, computed, each, combination, key}.
use anyhow::Result;
use tokio_postgres::GenericClient;

/// The data parameter and what the clause reads from it.
#[derive(Default)]
pub(super) struct Data {
    pub text: Option<String>,
    /// The stored key of the combination that the data filter names.
    pub key: Option<String>,
    /// True when `text` is the envelope.
    pub envelope: bool,
    /// The number of computed fields that the filter names.
    pub computed: usize,
}

/// The data parameter of a data filter. The dimension fields of the workspace leave the
/// containment filter and name the combination. The computed fields leave it too: they
/// match the data with the computed values.
pub(super) async fn data(
    client: &impl GenericClient,
    workspace: &str,
    data: Option<&str>,
) -> Result<Data> {
    let Some(text) = data else {
        return Ok(Data::default());
    };
    let sql = "SELECT COALESCE(first_view->'dimensions','[]'::jsonb)::text,COALESCE(first_view->'computed_fields','[]'::jsonb)::text FROM joppa.board_cache WHERE workspace=$1";
    let row = client.query_opt(sql, &[&workspace]).await?;
    let (stored, computed): (serde_json::Value, Vec<String>) = match row {
        Some(row) => (
            serde_json::from_str(row.get(0))?,
            serde_json::from_str(row.get(1))?,
        ),
        None => (serde_json::Value::Null, vec![]),
    };
    let fields: Vec<String> = stored
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d["field"].as_str().map(String::from))
        .collect();
    let filter: serde_json::Value = serde_json::from_str(text)?;
    let (combination, mut rest) = crate::filter_dimensions::split(&filter, &fields);
    let mut part = serde_json::Map::new();
    for field in &computed {
        if let Some(value) = rest.as_object_mut().and_then(|m| m.remove(field)) {
            part.insert(field.clone(), value);
        }
    }
    if combination.is_empty() && part.is_empty() {
        let text = Some(text.to_owned());
        return Ok(Data {
            text,
            ..Data::default()
        });
    }
    let key = (!combination.is_empty()).then(|| crate::filter_dimensions::key(&combination));
    let mut envelope = serde_json::json!({"combination":combination,"key":key});
    // An empty part leaves its key out, so the clause reads it as SQL NULL.
    if rest.as_object().is_some_and(|m| !m.is_empty()) {
        envelope["data"] = rest;
    }
    let computed = part.len();
    if !part.is_empty() {
        // Each field alone, so each one can use the index of the data or of the values.
        let each: Vec<serde_json::Value> = part
            .iter()
            .map(|(k, v)| serde_json::json!({ k: v }))
            .collect();
        envelope["each"] = serde_json::Value::Array(each);
        envelope["computed"] = serde_json::Value::Object(part);
    }
    let text = Some(envelope.to_string());
    Ok(Data {
        text,
        key,
        envelope: true,
        computed,
    })
}
