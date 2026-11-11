//! Part of the card read: the facts of all cards. Each fact takes at most one query for all
//! addresses, and a fact that `CardNeeds` does not name takes none.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

type Tx<'a> = tokio_postgres::Transaction<'a>;

// The active comments, the attached files and the description of each annotation. $3 also
// gives the whole annotation with the comment and attachment maps.
const NOTES: &str = "SELECT key,value->>'description',
    (SELECT count(*) FROM jsonb_each(value->'comments') c WHERE (c.value->>'withdrawn')::boolean IS NOT TRUE),
    (SELECT count(*) FROM jsonb_each(value->'attachments') f WHERE (f.value->>'removed')::boolean IS NOT TRUE),
    CASE WHEN $3 THEN value::text END
    FROM joppa.state_entries WHERE workspace=$1 AND section='notes' AND key=ANY($2)";

// The incoming edges of each object by relation and kind, also through its aliases.
const GROUPS: &str = "WITH t AS (SELECT u.k AS key,u.k AS target FROM unnest($2::text[]) u(k)
      UNION SELECT a.target,a.address FROM joppa.object_aliases a WHERE a.workspace=$1 AND a.target=ANY($2))
    SELECT t.key,l.relation,o.kind,count(*) FROM t
    JOIN joppa.object_links l ON l.workspace=$1 AND l.target=t.target
    JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source AND o.present
    GROUP BY 1,2,3 ORDER BY 1,2,3";

/// Add the facts of `needs` to `items`. `tests` holds the stored test status of each item.
pub(super) async fn add(
    conn: &Tx<'_>,
    workspace: &str,
    actor: &str,
    items: &mut [Value],
    tests: &[Value],
    needs: CardNeeds,
) -> Result<()> {
    let keys: Vec<String> = items
        .iter()
        .filter_map(|i| i["address"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if needs.edges {
        edges(conn, workspace, items).await?;
    }
    if needs.notes || needs.counts {
        notes(conn, workspace, items, needs).await?;
    }
    if needs.groups {
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for r in conn.query(GROUPS, &[&workspace, &keys]).await? {
            let group = json!({"relation": r.get::<_, String>(1), "kind": r.get::<_, String>(2),
                "count": r.get::<_, i64>(3)});
            groups.entry(r.get(0)).or_default().push(group);
        }
        set_each(items, "relation_groups", |a| json!(groups.get(a)));
    }
    if needs.children {
        let sql = "SELECT parent,count(*) FROM joppa.objects WHERE workspace=$1 AND parent=ANY($2) AND present GROUP BY parent";
        let rows = conn.query(sql, &[&workspace, &keys]).await?;
        let counts: BTreeMap<String, i64> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
        set_each(items, "children", |a| json!(counts.get(a).unwrap_or(&0)));
    }
    let types = match needs.result || needs.references || needs.inbox {
        true => {
            let sql = "SELECT (value->'types')::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1";
            let row = conn.query_opt(sql, &[&workspace]).await?;
            let text: Option<String> = row.and_then(|r| r.get(0));
            serde_json::from_str(text.as_deref().unwrap_or("null"))?
        }
        false => Value::Null,
    };
    for (item, test) in items.iter_mut().zip(tests) {
        let kind = &types[item["kind"].as_str().unwrap_or_default()];
        // A test node gives the deciding result of its test: status, commit and message.
        if needs.result
            && kind["role"] == crate::test_nodes::ROLE
            && let Some(result) = test["tests"].get(0)
        {
            item["result"] = result.clone();
        }
        if needs.inbox && kind["inbox"] == true {
            item["inbox"] = json!(true);
        }
    }
    if needs.references {
        super::references::add(conn, workspace, items, &types).await?;
    }
    if needs.remote {
        remote(conn, workspace, actor, items).await?;
    }
    Ok(())
}

/// Set `key` of each item to `value` of its address.
fn set_each(items: &mut [Value], key: &str, value: impl Fn(&str) -> Value) {
    for item in items.iter_mut() {
        let found = value(item["address"].as_str().unwrap_or_default());
        item[key] = found;
    }
}

/// Each edge with the title, the current revision and the presence of its target.
async fn edges(conn: &Tx<'_>, workspace: &str, items: &mut [Value]) -> Result<()> {
    let edges = |item: &Value| -> Vec<String> {
        let relations = item["relations"].as_object().into_iter().flatten();
        let edges = relations.flat_map(|(_, r)| r["edges"].as_array().into_iter().flatten());
        edges
            .filter_map(|e| e["to"].as_str().map(str::to_owned))
            .collect()
    };
    let targets: BTreeSet<String> = items.iter().flat_map(edges).collect();
    if targets.is_empty() {
        return Ok(());
    }
    let targets: Vec<String> = targets.into_iter().collect();
    let found: BTreeMap<String, Value> = conn.query("SELECT t.address,o.title,o.revision,o.present FROM unnest($2::text[]) t(address) LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=t.address JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target,t.address)", &[&workspace, &targets]).await?.iter().map(|r| (r.get(0), json!({"title": r.get::<_, String>(1), "current_revision": r.get::<_, i32>(2), "present": r.get::<_, bool>(3)}))).collect();
    for item in items.iter_mut() {
        let relations = item["relations"].as_object_mut().into_iter().flatten();
        let edges = relations.flat_map(|(_, r)| r["edges"].as_array_mut().into_iter().flatten());
        for edge in edges {
            let target = found.get(edge["to"].as_str().unwrap_or(""));
            for (key, value) in target.and_then(Value::as_object).into_iter().flatten() {
                edge[key] = value.clone();
            }
            if target.is_none() {
                edge["present"] = json!(false);
            }
        }
    }
    Ok(())
}

/// The counts, the description and, with `needs.notes`, the comments and the short files.
async fn notes(
    conn: &Tx<'_>,
    workspace: &str,
    items: &mut [Value],
    needs: CardNeeds,
) -> Result<()> {
    let annotation = |item: &Value| -> String {
        let address = item["address"].as_str().unwrap_or_default();
        let annotation = item["process"]["annotation_address"].as_str();
        annotation.unwrap_or(address).to_owned()
    };
    let keys: Vec<String> = items.iter().map(annotation).collect();
    let rows = conn
        .query(NOTES, &[&workspace, &keys, &needs.notes])
        .await?;
    let rows: BTreeMap<String, &tokio_postgres::Row> = rows.iter().map(|r| (r.get(0), r)).collect();
    for (item, key) in items.iter_mut().zip(&keys) {
        let row = rows.get(key);
        if needs.counts {
            let count = |i: usize| row.map_or(0, |r| r.get::<_, i64>(i));
            item["comments_count"] = json!(count(2));
            item["files_count"] = json!(count(3));
        }
        let text = row.and_then(|r| r.get::<_, Option<String>>(4));
        let mut annotations: Value = serde_json::from_str(text.as_deref().unwrap_or("{}"))?;
        if let Some(description) = row.and_then(|r| r.get::<_, Option<String>>(1)) {
            annotations["description"] = json!(description);
        }
        if needs.notes {
            let files = crate::file_list::present(&annotations["attachments"]);
            let files: Vec<Value> = files.iter().map(crate::file_list::short).collect();
            item["files"] = json!(files);
        }
        item["annotations"] = annotations;
    }
    Ok(())
}

/// The remote links and the child workspace of each item, with the access of the reader.
async fn remote(conn: &Tx<'_>, workspace: &str, actor: &str, items: &mut [Value]) -> Result<()> {
    let keys: Vec<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    let sql = "SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='remote_links' AND key=ANY($2)";
    let mut stored: BTreeMap<String, crate::remote_links::RemoteLinks> = BTreeMap::new();
    for row in conn.query(sql, &[&workspace, &keys]).await? {
        stored.insert(row.get(0), serde_json::from_str(row.get(1))?);
    }
    for item in items.iter_mut() {
        let address = item["address"].as_str().unwrap_or_default();
        let links = stored.get(address).cloned().unwrap_or_default();
        crate::store::remote_status::decorate_with(conn, workspace, actor, item, links).await?;
    }
    Ok(())
}
