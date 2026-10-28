//! The links of an object in groups. An object read gives one count for each direction,
//! relation and kind of the other object. A group read gives the links of one group in
//! pages, in address order, so a card with many links loads only what the reader opens.
use super::*;
use anyhow::bail;

const GROUPS: &str = "SELECT 'outgoing',l.relation,COALESCE(o.kind,split_part(l.target,':',1)),count(*) FROM joppa.object_links l
    LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target
    LEFT JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=COALESCE(a.target,l.target)
    WHERE l.workspace=$1 AND l.source=$2 AND l.relation NOT LIKE 'reference:%' GROUP BY 1,2,3
  UNION ALL SELECT 'incoming',l.relation,o.kind,count(*) FROM joppa.object_links l
    JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source
    WHERE l.workspace=$1 AND l.target IN (SELECT $2 UNION SELECT address FROM joppa.object_aliases WHERE workspace=$1 AND target=$2) AND o.present GROUP BY 1,2,3
  ORDER BY 1,2,3";

const OUTGOING: &str = "SELECT l.target,COALESCE(o.title,l.target),COALESCE(o.kind,split_part(l.target,':',1)),l.revision,l.blocking,COALESCE(o.present,false) FROM joppa.object_links l
    LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target
    LEFT JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=COALESCE(a.target,l.target)
    WHERE l.workspace=$1 AND l.source=$2 AND l.relation=$3 AND COALESCE(o.kind,split_part(l.target,':',1))=$4 AND ($5::text IS NULL OR l.target>$5)
    ORDER BY l.target LIMIT $6";

const INCOMING: &str = "SELECT l.source,o.title,o.kind,l.revision,l.blocking,o.present FROM joppa.object_links l
    JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source
    WHERE l.workspace=$1 AND l.target IN (SELECT $2 UNION SELECT address FROM joppa.object_aliases WHERE workspace=$1 AND target=$2)
      AND o.present AND l.relation=$3 AND o.kind=$4 AND ($5::text IS NULL OR l.source>$5)
    ORDER BY l.source LIMIT $6";

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

/// One group of links of `address`: `direction` is outgoing or incoming, `kind` is the type
/// of the other object. At most `limit` links (1–100) after `cursor`, with next_cursor.
#[allow(clippy::too_many_arguments)]
pub async fn link_page(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    address: &str,
    direction: &str,
    relation: &str,
    kind: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Value> {
    ensure!((1..=100).contains(&limit), "limit must be 1–100");
    let sql = match direction {
        "outgoing" => OUTGOING,
        "incoming" => INCOMING,
        _ => bail!("direction must be outgoing or incoming"),
    };
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    let key: String = conn
        .query_opt("SELECT COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2) WHERE EXISTS(SELECT 1 FROM joppa.objects WHERE workspace=$1 AND address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2))", &[&workspace, &address])
        .await?
        .context("unknown object")?
        .get(0);
    let rows = conn
        .query(
            sql,
            &[
                &workspace,
                &key,
                &relation,
                &kind,
                &cursor,
                &((limit + 1) as i64),
            ],
        )
        .await?;
    let more = rows.len() > limit;
    let items = rows
        .iter()
        .take(limit)
        .map(|r| json!({"address":r.get::<_,String>(0),"title":r.get::<_,String>(1),"kind":r.get::<_,String>(2),"relation":relation,"direction":direction,"revision":r.get::<_,Option<i32>>(3),"blocking":r.get::<_,bool>(4),"present":r.get::<_,bool>(5)}))
        .collect::<Vec<_>>();
    let next = more
        .then(|| items.last().map(|i| i["address"].clone()))
        .flatten();
    Ok(
        json!({"address":key,"direction":direction,"relation":relation,"kind":kind,"items":items,"next_cursor":next}),
    )
}
