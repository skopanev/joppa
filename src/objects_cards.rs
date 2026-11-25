//! The MCP card read of 1 to 25 objects by address (ac:q90d25sm, ac:1z012wub). One
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

/// The current objects at `addresses`, in the order of the request, each with the address that
/// asked for it, and the facts of `needs`. A duplicate address gives the object again, an alias
/// gives its target, and each unknown address goes into the second list.
pub async fn read_cards(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    addresses: &[&str],
    needs: CardNeeds,
) -> Result<(Vec<(String, Value)>, Vec<String>)> {
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    // A computed reference reads the addresses in the data, so it needs the document.
    let document = match needs.document || needs.references {
        true => format!(
            "jsonb_set(o.document,'{{data}}',{})",
            crate::computed_fields::DATA_SQL
        ),
        false => ROW_DOCUMENT.to_owned(),
    };
    let tests = match needs.result {
        true => "o.test_status::text",
        false => "NULL::text",
    };
    // The type of each row from the board projection: role, Inbox flag and reference rules.
    let kind = match needs.result || needs.references || needs.inbox {
        true => {
            "(SELECT (b.first_view->'object_types'->o.kind)::text FROM joppa.board_cache b WHERE b.workspace=$1)"
        }
        false => "NULL::text",
    };
    let sql = format!(
        "SELECT o.address,o.kind,o.local_id,o.revision,{document}::text,o.signal::text,o.process::text,o.rollup::text,{tests},{kind},
            CASE WHEN $3 THEN (o.document->'data')::text END,COALESCE(o.computed,'{{}}'::jsonb)::text,
            o.document->>'parent_hash',o.parent_hash_current
        FROM unnest($2::text[]) WITH ORDINALITY k(asked,n)
        LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=k.asked
        LEFT JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target,k.asked)
        ORDER BY k.n"
    );
    let authored = needs.document || needs.references;
    let rows = conn
        .query(&sql, &[&workspace, &addresses, &authored])
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    let mut tests = Vec::with_capacity(rows.len());
    let mut types = Vec::with_capacity(rows.len());
    let (mut asked, mut missing, mut authors) = (vec![], vec![], vec![]);
    for (row, wanted) in rows.iter().zip(addresses) {
        let Some(address) = row.get::<_, Option<String>>(0) else {
            missing.push((*wanted).to_owned());
            continue;
        };
        asked.push((*wanted).to_owned());
        let parse = |i: usize, empty: Value| -> Result<Value> {
            match row.get::<_, Option<&str>>(i) {
                Some(text) => Ok(serde_json::from_str(text)?),
                None => Ok(empty),
            }
        };
        let mut document = parse(4, json!({}))?;
        if needs.document || needs.references {
            // A document stored before migration 062 reads with relations by the replay rule.
            document = crate::relations::adapt_document(document)?;
        }
        let mut item = json!({"address": address, "kind": row.get::<_, String>(1),
            "id": row.get::<_, String>(2), "current_revision": row.get::<_, i32>(3),
            "relations": document["relations"], "document": document,
            "signal": parse(5, json!({}))?, "process": parse(6, Value::Null)?});
        // The decoration comes from the status projection of the last write.
        super::projection::apply_rollup(&mut item, &super::projection::stored(row.get(7))?);
        tests.push(super::projection::stored(row.get(8))?);
        types.push(parse(9, Value::Null)?);
        // The data stays as its authors wrote it, and the computed values come apart.
        authors.push(parse(10, Value::Null)?);
        item["computed"] = strip_nulls(parse(11, json!({}))?);
        crate::read_compact::parent_hash_facts(&mut item, row.get(12), row.get(13));
        items.push(item);
    }
    let known = facts::Known {
        tests: &tests,
        types: &types,
    };
    facts::add(&conn, &workspace, actor, &mut items, known, needs).await?;
    // A computed reference reads the computed values, so the data parts only after the facts.
    for (item, data) in items.iter_mut().zip(authors) {
        if !data.is_null() {
            item["document"]["data"] = data;
        }
    }
    // The commit ends the snapshot inside the read, so no ROLLBACK follows it later.
    conn.commit().await?;
    Ok((asked.into_iter().zip(items).collect(), missing))
}

/// Add the list detail facts (`notes`, `counts`, `references`) to `items` in the same list snapshot.
pub(super) async fn add_list_facts(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    selected: &std::collections::BTreeMap<String, objects::TreeType>,
    items: &mut [Value],
    needs: CardNeeds,
) -> Result<()> {
    let narrow = CardNeeds {
        notes: needs.notes,
        counts: needs.counts,
        references: needs.references,
        ..CardNeeds::default()
    };
    let tests = vec![Value::Null; items.len()];
    let types: Vec<Value> = items
        .iter()
        .map(|i| {
            i["kind"]
                .as_str()
                .and_then(|k| selected.get(k))
                .and_then(|t| serde_json::to_value(t).ok())
                .unwrap_or(Value::Null)
        })
        .collect();
    let known = facts::Known {
        tests: &tests,
        types: &types,
    };
    facts::add(conn, workspace, "", items, known, narrow).await
}

/// The computed values without the fields that have no value.
pub(in crate::store) fn strip_nulls(mut computed: Value) -> Value {
    if let Some(map) = computed.as_object_mut() {
        map.retain(|_, v| !v.is_null());
    }
    computed
}
