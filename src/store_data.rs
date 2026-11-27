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
const COLUMNS: [&str; 12] = [
    "title",
    "body",
    "owner",
    "parent",
    "status",
    "relations",
    "contract_hash",
    "parent_hash",
    "parent_hash_current",
    "parent_changed",
    "blocked",
    "dependents",
];

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
    let mut walked = tree.walk(root.as_deref(), &plan.filter, plan.kinds.as_ref());
    drop(tree);
    // The blocked filter and the fields blocked and dependents read the dependency facts.
    let named = |f: &String| f == "blocked" || f == "dependents";
    let facts = match plan.blocked.is_some() || plan.fields.iter().any(named) {
        true => crate::dependency_sql::facts(&*tx, workspace, false).await?,
        false => Default::default(),
    };
    if let Some(wanted) = plan.blocked {
        walked.retain(|address| facts.is_blocked(address) == wanted);
    }
    if plan.sets.is_set() {
        let kept = crate::node_filter::kept_by_sets(&*tx, workspace, &plan.sets).await?;
        walked.retain(|address| kept.contains(address));
    }
    // The keys of the reply object in serde order: items, position, workspace.
    let tail = format!(
        "],\"position\":{position},\"workspace\":{}}}",
        serde_json::to_string(&public)?
    );
    let start = "{\"items\":[";
    let mut reply = Reply {
        text: start.to_owned(),
        length: start.chars().count() + tail.chars().count(),
        first: true,
        max: plan.max_chars,
        facts,
    };
    match &plan.query {
        None => {
            let take = plan.limit.unwrap_or(usize::MAX);
            let selected: Vec<String> = walked.into_iter().take(take).collect();
            reply
                .items(&*tx, workspace, &selected, &plan.fields)
                .await?;
            tx.commit().await?;
        }
        Some(query) => {
            // The search calls the embedding provider and takes its own connection, so the
            // read gives its connection back first.
            tx.commit().await?;
            drop(conn);
            if !walked.is_empty() {
                let limit = plan.limit.unwrap_or_default() as i64;
                let found =
                    search_query::find_within(pool, workspace, query, &walked, limit, embedder)
                        .await?;
                let conn = pool.get().await?;
                reply
                    .items(&**conn, workspace, &found, &plan.fields)
                    .await?;
            }
        }
    }
    if reply.length > plan.max_chars {
        let (length, max) = (reply.length, plan.max_chars);
        return Err(refuse(
            422,
            format!("response is {length} characters, max_chars is {max}"),
        ));
    }
    reply.text.push_str(&tail);
    Ok(reply.text)
}

/// The reply text of a read. It grows one item at a time and stops growing past `max`, while
/// `length` counts every character for the refusal.
struct Reply {
    text: String,
    length: usize,
    first: bool,
    max: usize,
    facts: crate::dependency_sql::Facts,
}

impl Reply {
    /// Add the items of `selected`, in this order, with the requested `fields`.
    async fn items(
        &mut self,
        client: &impl GenericClient,
        workspace: &str,
        selected: &[String],
        fields: &[String],
    ) -> Result<()> {
        let data: Vec<&String> = fields
            .iter()
            .filter(|f| !COLUMNS.contains(&f.as_str()))
            .collect();
        let body = fields.iter().any(|f| f == "body");
        let relations = fields.iter().any(|f| f == "relations");
        let sql = format!(
            "SELECT o.address,o.kind,o.revision,
                COALESCE((SELECT target FROM joppa.object_aliases a WHERE a.workspace=o.workspace AND a.address=o.parent),o.parent),
                o.title,CASE WHEN $4::bool THEN o.description END,o.owner,{TONE},
                CASE WHEN cardinality($3::text[])>0 THEN (SELECT jsonb_object_agg(key,value) FROM jsonb_each({DATA_SQL}) WHERE key=ANY($3))::text END,
                CASE WHEN $5::bool THEN jsonb_build_object('relations',o.document->'relations','links',o.document->'links')::text END,
                o.contract_hash,o.document->>'parent_hash',o.parent_hash_current
            FROM unnest($2::text[]) WITH ORDINALITY s(address,ord)
            JOIN joppa.objects o ON o.workspace=$1 AND o.address=s.address AND o.present
            ORDER BY s.ord",
            DATA_SQL = crate::computed_fields::DATA_SQL
        );
        let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
            [&workspace, &selected, &data, &body, &relations];
        let rows = client.query_raw(&sql, params).await?;
        futures_util::pin_mut!(rows);
        while let Some(r) = futures_util::TryStreamExt::try_next(&mut rows).await? {
            let part = serde_json::to_string(&item(&r, fields, &self.facts)?)?;
            self.length += part.chars().count() + usize::from(!self.first);
            if self.length <= self.max {
                if !self.first {
                    self.text.push(',');
                }
                self.text.push_str(&part);
            }
            self.first = false;
        }
        Ok(())
    }
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

#[path = "store_data_item.rs"]
mod data_item;
use data_item::item;
