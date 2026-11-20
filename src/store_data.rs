//! `POST /api/data` of a read key: the walk of the live tree, the optional search within the
//! walked nodes, the `fields` of each item and the `max_chars` guard. The read writes no
//! journal event and no object row.
use super::*;
use crate::data_request::{Request, refuse};
use crate::data_walk::{Node, Tree};
use crate::node_filter::{SUBTREE, TONE};

/// The root of a read: an address, an old alias or a public item ID of a live node.
const ROOT: &str = "WITH k AS (SELECT CASE WHEN strpos($2,':')>0 THEN $2 ELSE
        (SELECT kind||':'||local_id FROM joppa.item_ids WHERE workspace=$1 AND id=joppa.short_item_id($2) LIMIT 1) END AS key)
    SELECT o.address FROM k JOIN joppa.objects o ON o.workspace=$1 AND o.present
        AND o.address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=k.key),k.key)";

/// The item fields that are columns, not data fields.
const COLUMNS: [&str; 5] = ["title", "body", "owner", "parent", "status"];

/// The reply of a read key of `workspace`, as JSON text. A large workspace has tens of
/// thousands of live nodes, so the walk streams light rows (address, type, ID, parent and the
/// filter fields), and only the selected nodes give their fields, one item at a time. The reply
/// text grows item by item and stops growing past `max_chars`, while the count goes on for the
/// refusal.
pub async fn read_data(
    pool: &Pool,
    workspace: &str,
    request: Request,
    embedder: Option<&crate::embeddings::Embedder>,
) -> Result<String> {
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
    let (public, _) = head(&*tx, workspace).await?;
    // The position is the last journal position of the workspace.
    let sql = "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1";
    let position: i64 = tx.query_one(sql, &[&workspace]).await?.get(0);
    let root = match &plan.root {
        None => None,
        Some(key) => match tx.query_opt(ROOT, &[&workspace, key]).await? {
            Some(row) => Some(row.get::<_, String>(0)),
            None => return Err(refuse(404, format!("root {key} is not a live node"))),
        },
    };
    let filter: Vec<&String> = plan.filter.iter().map(|(field, _)| field).collect();
    let nodes = walk_nodes(&tx, workspace, root.as_deref(), &filter).await?;
    let tree = Tree::new(&w, &schema.types, nodes);
    let walked = tree.walk(root.as_deref(), &plan.filter, plan.kinds.as_ref());
    drop(tree);
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
    // The keys of the reply object in serde order: items, position, workspace.
    let tail = format!(
        "],\"position\":{position},\"workspace\":{}}}",
        serde_json::to_string(&public)?
    );
    let mut text = String::from("{\"items\":[");
    let mut length = text.chars().count() + tail.chars().count();
    let mut first = true;
    let data: Vec<&String> = plan
        .fields
        .iter()
        .filter(|f| !COLUMNS.contains(&f.as_str()))
        .collect();
    let sql = format!(
        "SELECT o.address,o.kind,o.revision,
            COALESCE((SELECT target FROM joppa.object_aliases a WHERE a.workspace=o.workspace AND a.address=o.parent),o.parent),
            o.title,o.description,o.owner,{TONE},
            (SELECT jsonb_object_agg(key,value) FROM jsonb_each({DATA_SQL}) WHERE key=ANY($3))::text
        FROM joppa.objects o WHERE o.workspace=$1 AND o.present AND o.address=ANY($2)
        ORDER BY array_position($2::text[],o.address)",
        DATA_SQL = crate::computed_fields::DATA_SQL
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 3] = [&workspace, &selected, &data];
    let rows = tx.query_raw(&sql, params).await?;
    futures_util::pin_mut!(rows);
    while let Some(r) = futures_util::TryStreamExt::try_next(&mut rows).await? {
        let part = serde_json::to_string(&item(&r, &plan.fields)?)?;
        length += part.chars().count() + usize::from(!first);
        if length <= plan.max_chars {
            if !first {
                text.push(',');
            }
            text.push_str(&part);
        }
        first = false;
    }
    tx.commit().await?;
    if length > plan.max_chars {
        let max = plan.max_chars;
        return Err(refuse(
            422,
            format!("response is {length} characters, max_chars is {max}"),
        ));
    }
    text.push_str(&tail);
    Ok(text)
}

/// The light nodes of the walk: the live nodes below `root` with the root, or every live node
/// from the roots, each with the data fields of `filter` only. The walk of SUBTREE passes no
/// archived node, so an archived branch never loads.
async fn walk_nodes(
    tx: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    root: Option<&str>,
    filter: &[&String],
) -> Result<Vec<Node>> {
    let sql = format!(
        "SELECT o.address,o.kind,o.local_id,
            COALESCE((SELECT target FROM joppa.object_aliases a WHERE a.workspace=o.workspace AND a.address=o.parent),o.parent),
            CASE WHEN cardinality($5::text[])>0 THEN (SELECT jsonb_object_agg(key,value) FROM jsonb_each({data}) WHERE key=ANY($5))::text END
        FROM joppa.objects o
        WHERE o.workspace=$1 AND o.present AND (o.address=$2 OR o.address IN ({SUBTREE}))",
        data = crate::computed_fields::DATA_SQL
    );
    let depth: Option<i32> = None;
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
        [&workspace, &root, &depth, &false, &filter];
    let rows = tx.query_raw(&sql, params).await?;
    futures_util::pin_mut!(rows);
    let mut out = vec![];
    while let Some(r) = futures_util::TryStreamExt::try_next(&mut rows).await? {
        let data: Option<&str> = r.get(4);
        out.push(Node {
            address: r.get(0),
            kind: r.get(1),
            id: r.get(2),
            parent: r.get(3),
            data: data
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// One item: the address, the type, the revision and each requested field that is not null.
fn item(r: &tokio_postgres::Row, fields: &[String]) -> Result<Value> {
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
            other => data.get(other).cloned().unwrap_or_default(),
        };
        if !value.is_null() {
            item[field] = value;
        }
    }
    Ok(item)
}
