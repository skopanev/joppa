//! Part of the card read: the references of each card, only on request. The own references
//! take one query for all cards, and the computed ones (a reference rule with `from`) one
//! more query for all their targets. Each item has the title of its target object.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const OWN: &str = "SELECT l.source,l.target,l.relation,o.title,o.revision,o.present FROM joppa.object_links l JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.target WHERE l.workspace=$1 AND l.source=ANY($2) AND l.relation LIKE 'reference:%' ORDER BY l.relation,l.target";

/// Set `references` on each item, sorted by field and target. `types` are the types of the
/// current tree schema.
pub(super) async fn add(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    items: &mut [Value],
    types: &Value,
) -> Result<()> {
    let keys: Vec<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    let mut found: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in conn.query(OWN, &[&workspace, &keys]).await? {
        let relation: String = r.get(2);
        let field = relation.strip_prefix("reference:").unwrap_or(&relation);
        let (title, revision) = (r.get::<_, String>(3), r.get::<_, i32>(4));
        let reference = json!({"target": r.get::<_, String>(1), "field": field, "title": title,
            "current_revision": revision, "present": r.get::<_, bool>(5)});
        found.entry(r.get(0)).or_default().push(reference);
    }
    // The computed values: each address in a data field whose rule reads `from` other nodes.
    let wanted: Vec<Vec<(String, String)>> = items.iter().map(|i| computed(i, types)).collect();
    let targets: BTreeSet<&String> = wanted.iter().flatten().map(|(_, t)| t).collect();
    let titles: BTreeMap<String, Value> = match targets.is_empty() {
        true => BTreeMap::new(),
        false => {
            let targets: Vec<&String> = targets.into_iter().collect();
            let sql = "SELECT address,title,revision,present FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)";
            let rows = conn.query(sql, &[&workspace, &targets]).await?;
            let title = |r: &tokio_postgres::Row| {
                json!({"title": r.get::<_, String>(1), "current_revision": r.get::<_, i32>(2),
                    "present": r.get::<_, bool>(3)})
            };
            rows.iter().map(|r| (r.get(0), title(r))).collect()
        }
    };
    for (item, wanted) in items.iter_mut().zip(wanted) {
        let address = item["address"].as_str().unwrap_or_default();
        let mut references = found.get(address).cloned().unwrap_or_default();
        references.extend(wanted.into_iter().filter_map(|(field, target)| {
            let mut reference = titles.get(&target)?.clone();
            reference["target"] = json!(target);
            reference["field"] = json!(field);
            Some(reference)
        }));
        references.sort_by(|a, b| {
            (a["field"].as_str(), a["target"].as_str())
                .cmp(&(b["field"].as_str(), b["target"].as_str()))
        });
        item["references"] = json!(references);
    }
    Ok(())
}

/// The (field, target) pairs of the computed references of `item`.
fn computed(item: &Value, types: &Value) -> Vec<(String, String)> {
    let rules = &types[item["kind"].as_str().unwrap_or_default()]["references"];
    let rules = rules.as_object().into_iter().flatten();
    let data = &item["document"]["data"];
    let mut wanted = vec![];
    for (field, _) in rules.filter(|(_, r)| r["from"].is_string()) {
        for target in data[field].as_array().into_iter().flatten() {
            wanted.extend(target.as_str().map(|t| (field.clone(), t.to_owned())));
        }
    }
    wanted
}
