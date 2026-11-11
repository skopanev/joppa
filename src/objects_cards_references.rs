//! Part of the card read: the references of each card, only on request (tac:jogrtcra). One
//! query reads the own references (the reference edges) and the computed ones (the addresses
//! in a data field whose rule reads `from` other nodes) of all cards, each with the title of
//! its target object.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

// The own references of $2, then the computed (source, field, target) triples $3, $4, $5.
const REFERENCES: &str = "SELECT l.source,l.target,substr(l.relation,11),o.title,o.revision,o.present FROM joppa.object_links l
      JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.target
      WHERE l.workspace=$1 AND l.source=ANY($2) AND l.relation LIKE 'reference:%'
    UNION ALL SELECT c.source,c.target,c.field,o.title,o.revision,o.present
      FROM unnest($3::text[],$4::text[],$5::text[]) c(source,field,target)
      JOIN joppa.objects o ON o.workspace=$1 AND o.address=c.target";

/// Set `references` on each item, sorted by field and target. `types` holds the type of each
/// item from the board projection.
pub(super) async fn add(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    items: &mut [Value],
    types: &[Value],
) -> Result<()> {
    let keys: BTreeSet<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    // Each address once: a duplicate address in the request reads the same references.
    let mut seen = BTreeSet::new();
    let (mut sources, mut fields, mut targets) = (vec![], vec![], vec![]);
    for (item, kind) in items.iter().zip(types) {
        let source = item["address"].as_str().unwrap_or_default();
        if !seen.insert(source) {
            continue;
        }
        for (field, target) in computed(item, kind) {
            sources.push(source.to_owned());
            fields.push(field);
            targets.push(target);
        }
    }
    let keys: Vec<&str> = keys.into_iter().collect();
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
        [&workspace, &keys, &sources, &fields, &targets];
    let mut found: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in conn.query(REFERENCES, &params).await? {
        let (title, revision) = (r.get::<_, String>(3), r.get::<_, i32>(4));
        let reference = json!({"target": r.get::<_, String>(1), "field": r.get::<_, String>(2),
            "title": title, "current_revision": revision, "present": r.get::<_, bool>(5)});
        found.entry(r.get(0)).or_default().push(reference);
    }
    for item in items.iter_mut() {
        let address = item["address"].as_str().unwrap_or_default();
        let mut references = found.get(address).cloned().unwrap_or_default();
        references.sort_by(|a, b| {
            (a["field"].as_str(), a["target"].as_str())
                .cmp(&(b["field"].as_str(), b["target"].as_str()))
        });
        item["references"] = json!(references);
    }
    Ok(())
}

/// The (field, target) pairs of the computed references of `item` of type `kind`.
fn computed(item: &Value, kind: &Value) -> Vec<(String, String)> {
    let rules = kind["references"].as_object().into_iter().flatten();
    let data = &item["document"]["data"];
    let mut wanted = vec![];
    for (field, _) in rules.filter(|(_, r)| r["from"].is_string()) {
        for target in data[field].as_array().into_iter().flatten() {
            wanted.extend(target.as_str().map(|t| (field.clone(), t.to_owned())));
        }
    }
    wanted
}
