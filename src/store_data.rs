//! `POST /api/data` of a read key: the walk of the live tree, the optional search within the
//! walked nodes, the `fields` of each item and the `max_chars` guard. The read writes no
//! journal event and no object row.
use super::*;
use crate::data_request::{Request, refuse};
use crate::data_walk::{Node, Tree};
use crate::node_filter::{SUBTREE, TONE};
use std::collections::BTreeMap;

/// The root of a read: an address, an old alias or a public item ID of a live node.
const ROOT: &str = "WITH k AS (SELECT CASE WHEN strpos($2,':')>0 THEN $2 ELSE
        (SELECT kind||':'||local_id FROM joppa.item_ids WHERE workspace=$1 AND id=joppa.short_item_id($2) LIMIT 1) END AS key)
    SELECT o.address FROM k JOIN joppa.objects o ON o.workspace=$1 AND o.present
        AND o.address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=k.key),k.key)";

/// One row of the walk for each fact of `row`.
struct Row {
    node: Node,
    revision: i32,
    title: String,
    body: Option<String>,
    owner: Option<String>,
    status: Option<String>,
}

/// The reply of a read key of `workspace`.
pub async fn read_data(
    pool: &Pool,
    workspace: &str,
    request: Request,
    embedder: Option<&crate::embeddings::Embedder>,
) -> Result<Value> {
    let mut conn = pool.get().await?;
    let tx = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    if let Some(name) = &request.workspace
        && resolve_workspace(&*tx, name).await? != workspace
    {
        return Err(refuse(403, "the read key belongs to another workspace"));
    }
    let w = state_rows::outline(&*tx, workspace).await?;
    let schema = crate::objects::schema(&w);
    let plan = request.plan(&schema.types)?;
    let (public, position) = head(&*tx, workspace).await?;
    let root = match &plan.root {
        None => None,
        Some(key) => match tx.query_opt(ROOT, &[&workspace, key]).await? {
            Some(row) => Some(row.get::<_, String>(0)),
            None => return Err(refuse(404, format!("root {key} is not a live node"))),
        },
    };
    let rows = load(&tx, workspace, root.as_deref()).await?;
    tx.commit().await?;
    drop(conn);
    let tree = Tree::new(&w, &schema.types, rows.iter().map(|r| r.node.clone()));
    let facts: BTreeMap<&str, &Row> = rows.iter().map(|r| (r.node.address.as_str(), r)).collect();
    let walked = tree.walk(root.as_deref(), &plan.filter, plan.kinds.as_ref());
    let selected = match &plan.query {
        None => walked
            .into_iter()
            .take(plan.limit.unwrap_or(usize::MAX))
            .collect(),
        Some(_) if walked.is_empty() => vec![],
        Some(query) => {
            let limit = plan.limit.unwrap_or_default() as i64;
            search_query::find_within(pool, workspace, query, &walked, limit, embedder).await?
        }
    };
    let items: Vec<Value> = selected
        .iter()
        .filter_map(|address| facts.get(address.as_str()))
        .map(|row| item(row, &plan.fields))
        .collect();
    let reply = json!({"workspace": public, "position": position, "items": items});
    let length = serde_json::to_string(&reply)?.chars().count();
    if length > plan.max_chars {
        let max = plan.max_chars;
        return Err(refuse(
            422,
            format!("response is {length} characters, max_chars is {max}"),
        ));
    }
    Ok(reply)
}

/// The live nodes below `root` with the root, or every live node from the roots. The walk of
/// SUBTREE passes no archived node, so an archived branch never loads.
async fn load(
    tx: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    root: Option<&str>,
) -> Result<Vec<Row>> {
    let sql = format!(
        "SELECT o.address,o.kind,o.local_id,o.revision,
            COALESCE((SELECT target FROM joppa.object_aliases a WHERE a.workspace=o.workspace AND a.address=o.parent),o.parent),
            o.title,o.description,o.owner,{TONE},{data}::text
        FROM joppa.objects o
        WHERE o.workspace=$1 AND o.present AND (o.address=$2 OR o.address IN ({SUBTREE}))",
        data = crate::computed_fields::DATA_SQL
    );
    let depth: Option<i32> = None;
    let rows = tx.query(&sql, &[&workspace, &root, &depth, &false]).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(Row {
            node: Node {
                address: r.get(0),
                kind: r.get(1),
                id: r.get(2),
                parent: r.get(4),
                data: serde_json::from_str(r.get(9))?,
            },
            revision: r.get(3),
            title: r.get(5),
            body: r.get(6),
            owner: r.get(7),
            status: r.get(8),
        });
    }
    Ok(out)
}

/// One item: the address, the type, the revision and each requested field that is not null.
fn item(row: &Row, fields: &[String]) -> Value {
    let mut item = json!({
        "address": row.node.address,
        "kind": row.node.kind,
        "revision": row.revision,
    });
    for field in fields {
        let value = match field.as_str() {
            "title" => json!(row.title),
            "body" => json!(row.body),
            "owner" => json!(row.owner),
            "parent" => json!(row.node.parent),
            "status" => json!(row.status),
            other => row.node.data.get(other).cloned().unwrap_or_default(),
        };
        if !value.is_null() {
            item[field] = value;
        }
    }
    item
}
