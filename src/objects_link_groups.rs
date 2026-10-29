//! The links of an object in groups. An object read gives one count for each direction,
//! relation and kind of the other object. A group read gives the links of one group in
//! pages, in address order, so a card with many links loads only what the reader opens.
use super::*;

const GROUPS: &str = "SELECT 'outgoing',l.relation,COALESCE(o.kind,split_part(l.target,':',1)),count(*) FROM joppa.object_links l
    LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target
    LEFT JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=COALESCE(a.target,l.target)
    WHERE l.workspace=$1 AND l.source=$2 AND l.relation NOT LIKE 'reference:%' GROUP BY 1,2,3
  UNION ALL SELECT 'incoming',l.relation,o.kind,count(*) FROM joppa.object_links l
    JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source
    WHERE l.workspace=$1 AND l.target IN (SELECT $2 UNION SELECT address FROM joppa.object_aliases WHERE workspace=$1 AND target=$2) AND o.present GROUP BY 1,2,3
  ORDER BY 1,2,3";

// One page of links in both directions, filtered by direction ($3), relation ($4) and kind
// ($5) when they are given, in the order direction, relation, address after the cursor
// ($6, $7, $8). Without a relation, the outgoing links omit the reference fields, as the
// groups do. `total` counts every link of the filter, also the links before the cursor.
const LINKS: &str = "SELECT * FROM (SELECT x.*,count(*) OVER() FROM (
    SELECT 'outgoing' AS direction,l.relation,l.target AS other,COALESCE(o.title,l.target),COALESCE(o.kind,split_part(l.target,':',1)) AS kind,l.revision,l.blocking,COALESCE(o.present,false) FROM joppa.object_links l
    LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target
    LEFT JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=COALESCE(a.target,l.target)
    WHERE l.workspace=$1 AND l.source=$2 AND ($4::text IS NOT NULL OR l.relation NOT LIKE 'reference:%')
  UNION ALL SELECT 'incoming',l.relation,l.source,o.title,o.kind,l.revision,l.blocking,o.present FROM joppa.object_links l
    JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source
    WHERE l.workspace=$1 AND l.target IN (SELECT $2 UNION SELECT address FROM joppa.object_aliases WHERE workspace=$1 AND target=$2) AND o.present) x
  WHERE ($3::text IS NULL OR x.direction=$3) AND ($4::text IS NULL OR x.relation=$4) AND ($5::text IS NULL OR x.kind=$5)) y
  WHERE $6::text IS NULL OR (y.direction,y.relation,y.other)>($6,$7,$8)
  ORDER BY y.direction,y.relation,y.other LIMIT $9";

/// The link groups of the current revision of `key`.
pub(super) async fn groups(
    client: &impl GenericClient,
    workspace: &str,
    key: &str,
) -> Result<Value> {
    Ok(json!(client
        .query(GROUPS, &[&workspace, &key])
        .await?
        .iter()
        .map(|r| json!({"direction":r.get::<_,String>(0),"relation":r.get::<_,String>(1),"kind":r.get::<_,String>(2),"count":r.get::<_,i64>(3)}))
        .collect::<Vec<_>>()))
}

/// The links of `address`, in pages: `direction` is outgoing or incoming, `relation` the
/// link relation and `kind` the type of the other object. Each filter is optional. At most
/// `limit` links (1–100) after `cursor`, with next_cursor, the total of the filter, the
/// workspace and the journal position of the snapshot.
#[allow(clippy::too_many_arguments)]
pub async fn link_page(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    address: &str,
    direction: Option<&str>,
    relation: Option<&str>,
    kind: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Value> {
    ensure!((1..=100).contains(&limit), "limit must be 1–100");
    if let Some(direction) = direction {
        ensure!(
            ["outgoing", "incoming"].contains(&direction),
            "direction must be outgoing or incoming"
        );
    }
    // The cursor is "direction/relation/address" of the last link of the previous page.
    let after: Option<Vec<&str>> = cursor.map(|c| c.splitn(3, '/').collect());
    if let Some(after) = &after {
        ensure!(
            after.len() == 3,
            "invalid link cursor; start again without cursor"
        );
    }
    let at = |i: usize| after.as_ref().map(|a| a[i]);
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let (public, position) = crate::store::head(&*conn, &workspace).await?;
    let key: String = conn
        .query_opt("SELECT COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2) WHERE EXISTS(SELECT 1 FROM joppa.objects WHERE workspace=$1 AND address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2))", &[&workspace, &address])
        .await?
        .context("unknown object")?
        .get(0);
    let rows = conn
        .query(
            LINKS,
            &[
                &workspace,
                &key,
                &direction,
                &relation,
                &kind,
                &at(0),
                &at(1),
                &at(2),
                &((limit + 1) as i64),
            ],
        )
        .await?;
    conn.commit().await?;
    let total: i64 = rows.first().map_or(0, |r| r.get(8));
    let more = rows.len() > limit;
    let rows = &rows[..rows.len().min(limit)];
    let items = rows
        .iter()
        .map(|r| json!({"address":r.get::<_,String>(2),"title":r.get::<_,String>(3),"kind":r.get::<_,String>(4),"relation":r.get::<_,String>(1),"direction":r.get::<_,String>(0),"revision":r.get::<_,Option<i32>>(5),"blocking":r.get::<_,bool>(6),"present":r.get::<_,bool>(7)}))
        .collect::<Vec<_>>();
    let next = more
        .then(|| {
            items.last().map(|i| {
                format!(
                    "{}/{}/{}",
                    i["direction"].as_str().unwrap_or(""),
                    i["relation"].as_str().unwrap_or(""),
                    i["address"].as_str().unwrap_or("")
                )
            })
        })
        .flatten();
    Ok(
        json!({"workspace":public,"position":position,"address":key,"direction":direction,"relation":relation,"kind":kind,"total":total,"items":items,"next_cursor":next}),
    )
}
