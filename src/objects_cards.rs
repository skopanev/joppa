//! The MCP card read of 1 to 25 objects by address (tac:m7qjm2lj, tac:jogrtcra). One
//! read-only REPEATABLE READ transaction reads all addresses: one query reads the object rows
//! through their aliases in the order of the request, and each other fact takes at most one
//! query for all of them. The facts follow `CardNeeds`, so a read with narrow fields reads
//! only the object rows. The HTTP object read (read_objects) keeps its full shape.
use super::*;

#[path = "objects_cards_facts.rs"]
mod facts;
#[path = "objects_cards_references.rs"]
mod references;

/// What a card read loads besides the object row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CardNeeds {
    /// The stored document with its data and relations. Else the row columns give the
    /// revision, parent, time, presence, title, owner, body and Inbox state.
    pub document: bool,
    /// The title, current revision and presence of each edge target.
    pub edges: bool,
    /// The comment and attachment maps, and the short files.
    pub notes: bool,
    /// `comments_count` (without the withdrawn comments) and `files_count`.
    pub counts: bool,
    /// The incoming edges by relation and kind.
    pub groups: bool,
    /// The own and the computed references with the titles of their targets.
    pub references: bool,
    /// The remote links and the child workspace.
    pub remote: bool,
    /// The number of present children.
    pub children: bool,
    /// The deciding test result of a test node.
    pub result: bool,
    /// `inbox: true` on an object of an Inbox type.
    pub inbox: bool,
}

impl CardNeeds {
    /// The MCP card of a read by address without fields.
    pub const CARD: CardNeeds = CardNeeds {
        document: true,
        edges: true,
        notes: false,
        counts: true,
        groups: true,
        references: false,
        remote: true,
        children: false,
        result: true,
        inbox: false,
    };
}

/// The document of a card without the document: what the list row gives.
const ROW_DOCUMENT: &str = "jsonb_build_object('revision',o.revision,'parent',o.parent,'at',o.document_at,'present',o.present,'data',jsonb_build_object('title',o.title,'owner',o.owner,'body',o.description,'status',o.inbox_status))";

/// The current objects at `addresses`, in the order of the request, with the facts of
/// `needs`. A duplicate address gives the object again, an alias gives its target, and an
/// unknown address refuses the whole read with `unknown object`.
pub async fn read_cards(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    addresses: &[&str],
    needs: CardNeeds,
) -> Result<Vec<Value>> {
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let document = match needs.document {
        true => format!(
            "jsonb_set(o.document,'{{data}}',{})",
            crate::computed_fields::DATA_SQL
        ),
        false => ROW_DOCUMENT.to_owned(),
    };
    let tests = if needs.result {
        "o.test_status::text"
    } else {
        "NULL::text"
    };
    let sql = format!(
        "SELECT o.address,o.kind,o.local_id,o.revision,{document}::text,o.work_ready,o.signal::text,o.process::text,o.rollup::text,{tests}
        FROM unnest($2::text[]) WITH ORDINALITY k(asked,n)
        LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=k.asked
        LEFT JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target,k.asked)
        ORDER BY k.n"
    );
    let rows = conn.query(&sql, &[&workspace, &addresses]).await?;
    let mut items = Vec::with_capacity(rows.len());
    let mut tests = Vec::with_capacity(rows.len());
    for row in &rows {
        let address: String = row.get::<_, Option<String>>(0).context("unknown object")?;
        let parse = |i: usize, empty: Value| -> Result<Value> {
            match row.get::<_, Option<&str>>(i) {
                Some(text) => Ok(serde_json::from_str(text)?),
                None => Ok(empty),
            }
        };
        let mut document = parse(4, json!({}))?;
        if needs.document {
            // A document stored before migration 062 reads with relations by the replay rule.
            document = crate::relations::adapt_document(document)?;
        }
        let mut item = json!({"address": address, "kind": row.get::<_, String>(1),
            "id": row.get::<_, String>(2), "current_revision": row.get::<_, i32>(3),
            "relations": document["relations"], "document": document,
            "work_ready": row.get::<_, bool>(5), "signal": parse(6, json!({}))?,
            "process": parse(7, Value::Null)?});
        // The decoration comes from the status projection of the last write.
        super::projection::apply_rollup(&mut item, &super::projection::stored(row.get(8))?);
        tests.push(super::projection::stored(row.get(9))?);
        items.push(item);
    }
    facts::add(&conn, &workspace, actor, &mut items, &tests, needs).await?;
    Ok(items)
}
