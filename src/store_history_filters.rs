//! Part of `store_history`: the query parameters, object extraction and subtree resolution.
use super::*;

/// A cursor holds the last position of the page that it continues.
fn cursor_position(cursor: &str) -> Result<i64> {
    cursor
        .strip_prefix('h')
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| commands::ClientField("invalid history cursor").into())
}

pub(super) fn page_limit(args: &Value) -> Result<i64> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(HISTORY_LIMIT),
        // A larger limit is clamped to HISTORY_MAX_LIMIT, never refused.
        Some(value) => value
            .as_i64()
            .filter(|n| *n >= 1)
            .map(|n| n.min(HISTORY_MAX_LIMIT))
            .ok_or_else(|| commands::ClientField("limit must be a whole number from 1").into()),
    }
}

/// One History read: open only or every entry, oldest or newest first, after a position.
pub(super) struct Scope {
    pub limit: i64,
    pub open: bool,
    pub newest: bool,
    pub after: i64,
    pub cursor: Option<i64>,
    pub alias: Option<String>,
    pub op: Option<String>,
    pub kind: Option<String>,
    pub target: Option<String>,
    pub under: Option<String>,
}

/// The raw target address of an event before alias resolution, matching `history_entry::address`.
const RAW: &str = "CASE \
    WHEN e.command->>'op'='save_object' THEN (e.command->>'kind')||':'||(e.command->>'id') \
    WHEN e.command->>'op'='record_report' THEN NULL \
    WHEN e.command->>'op' IN ('assess','claim_planning','finish_planning','release_planning') THEN \
        CASE WHEN COALESCE(e.command->>'ac','')<>'' THEN \
            CASE WHEN strpos(e.command->>'ac',':')>0 THEN e.command->>'ac' ELSE 'ac:'||(e.command->>'ac') END \
            ELSE NULLIF(e.command->>'target','') END \
    WHEN e.command->>'op'='comment' THEN \
        CASE WHEN COALESCE(e.command->>'ac','')<>'' THEN \
            CASE WHEN strpos(e.command->>'ac',':')>0 THEN e.command->>'ac' ELSE 'ac:'||(e.command->>'ac') END \
            WHEN COALESCE(e.command->>'req','')<>'' THEN \
            CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END \
        END \
    WHEN e.command->>'op'='revise' AND COALESCE(e.command->>'req','')<>'' THEN \
        CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END \
    WHEN e.command->>'op' IN ('domain','capability','consumer','edit_domain','edit_capability','edit_consumer','set_domain_archived') AND COALESCE(e.command->>'id','')<>'' THEN \
        regexp_replace(regexp_replace(e.command->>'op','^(edit_|set_)',''),'_archived$','')||':'||(e.command->>'id') \
    WHEN e.command->>'op' IN ('task','claim_task','claim_next_task','amend_task','release_task','block_task','unblock_task','complete_task','review_task','retire_task','heartbeat_task') AND COALESCE(e.command->>'id','')<>'' THEN \
        CASE WHEN COALESCE(e.command->>'req','')<>'' THEN 'task:'||(e.command->>'req')||'/'||(e.command->>'id') \
            WHEN strpos(e.command->>'id',':')>0 THEN e.command->>'id' \
            ELSE 'task:'||(e.command->>'id') END \
    WHEN COALESCE(e.command->>'target','')<>'' THEN e.command->>'target' \
    WHEN COALESCE(e.command->>'req','')<>'' THEN \
        CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END \
END";

/// The canonical object address of an event, resolving historical aliases.
const TARGET: &str = const_format_target();
const fn const_format_target() -> &'static str {
    "COALESCE((SELECT a.target FROM joppa.object_aliases a WHERE a.workspace=e.workspace AND a.address=(CASE WHEN e.command->>'op'='save_object' THEN (e.command->>'kind')||':'||(e.command->>'id') WHEN e.command->>'op'='record_report' THEN NULL WHEN e.command->>'op' IN ('assess','claim_planning','finish_planning','release_planning') THEN CASE WHEN COALESCE(e.command->>'ac','')<>'' THEN CASE WHEN strpos(e.command->>'ac',':')>0 THEN e.command->>'ac' ELSE 'ac:'||(e.command->>'ac') END ELSE NULLIF(e.command->>'target','') END WHEN e.command->>'op'='comment' THEN CASE WHEN COALESCE(e.command->>'ac','')<>'' THEN CASE WHEN strpos(e.command->>'ac',':')>0 THEN e.command->>'ac' ELSE 'ac:'||(e.command->>'ac') END WHEN COALESCE(e.command->>'req','')<>'' THEN CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END END WHEN e.command->>'op'='revise' AND COALESCE(e.command->>'req','')<>'' THEN CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END WHEN e.command->>'op' IN ('domain','capability','consumer','edit_domain','edit_capability','edit_consumer','set_domain_archived') AND COALESCE(e.command->>'id','')<>'' THEN regexp_replace(regexp_replace(e.command->>'op','^(edit_|set_)',''),'_archived$','')||':'||(e.command->>'id') WHEN e.command->>'op' IN ('task','claim_task','claim_next_task','amend_task','release_task','block_task','unblock_task','complete_task','review_task','retire_task','heartbeat_task') AND COALESCE(e.command->>'id','')<>'' THEN CASE WHEN COALESCE(e.command->>'req','')<>'' THEN 'task:'||(e.command->>'req')||'/'||(e.command->>'id') WHEN strpos(e.command->>'id',':')>0 THEN e.command->>'id' ELSE 'task:'||(e.command->>'id') END WHEN COALESCE(e.command->>'target','')<>'' THEN e.command->>'target' WHEN COALESCE(e.command->>'req','')<>'' THEN CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END END)),(CASE WHEN e.command->>'op'='save_object' THEN (e.command->>'kind')||':'||(e.command->>'id') WHEN e.command->>'op'='record_report' THEN NULL WHEN e.command->>'op' IN ('assess','claim_planning','finish_planning','release_planning') THEN CASE WHEN COALESCE(e.command->>'ac','')<>'' THEN CASE WHEN strpos(e.command->>'ac',':')>0 THEN e.command->>'ac' ELSE 'ac:'||(e.command->>'ac') END ELSE NULLIF(e.command->>'target','') END WHEN e.command->>'op'='comment' THEN CASE WHEN COALESCE(e.command->>'ac','')<>'' THEN CASE WHEN strpos(e.command->>'ac',':')>0 THEN e.command->>'ac' ELSE 'ac:'||(e.command->>'ac') END WHEN COALESCE(e.command->>'req','')<>'' THEN CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END END WHEN e.command->>'op'='revise' AND COALESCE(e.command->>'req','')<>'' THEN CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END WHEN e.command->>'op' IN ('domain','capability','consumer','edit_domain','edit_capability','edit_consumer','set_domain_archived') AND COALESCE(e.command->>'id','')<>'' THEN regexp_replace(regexp_replace(e.command->>'op','^(edit_|set_)',''),'_archived$','')||':'||(e.command->>'id') WHEN e.command->>'op' IN ('task','claim_task','claim_next_task','amend_task','release_task','block_task','unblock_task','complete_task','review_task','retire_task','heartbeat_task') AND COALESCE(e.command->>'id','')<>'' THEN CASE WHEN COALESCE(e.command->>'req','')<>'' THEN 'task:'||(e.command->>'req')||'/'||(e.command->>'id') WHEN strpos(e.command->>'id',':')>0 THEN e.command->>'id' ELSE 'task:'||(e.command->>'id') END WHEN COALESCE(e.command->>'target','')<>'' THEN e.command->>'target' WHEN COALESCE(e.command->>'req','')<>'' THEN CASE WHEN strpos(e.command->>'req',':')>0 THEN e.command->>'req' ELSE 'requirement:'||(e.command->>'req') END END))"
}

/// The conditions of the entry filters, on the parameters `$first` to `$first+4`.
pub(super) fn filters(first: usize) -> String {
    let _ = RAW;
    let [a, o, k, t, u] =
        [first, first + 1, first + 2, first + 3, first + 4].map(|n| format!("${n}"));
    format!(
        "({a}::text IS NULL OR e.alias={a}) AND ({o}::text IS NULL OR e.command->>'op'={o}) AND ({k}::text IS NULL OR split_part({TARGET},':',1)={k}) AND ({t}::text IS NULL OR {TARGET}=COALESCE((SELECT x.target FROM joppa.object_aliases x WHERE x.workspace=e.workspace AND x.address={t}),{t})) AND ({u}::text[] IS NULL OR {TARGET}=ANY({u}))"
    )
}

pub(super) fn scope(args: &Value) -> Result<Scope> {
    let open = match args.get("open") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or(commands::ClientField("open must be true or false"))?,
    };
    let newest = match args.get("order").and_then(Value::as_str) {
        None | Some("oldest") => false,
        Some("newest") => true,
        Some(_) => return Err(commands::ClientField("order must be oldest or newest").into()),
    };
    let after = match args.get("after") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or(commands::ClientField("after must be a whole number"))?,
    };
    let cursor = args
        .get("cursor")
        .and_then(Value::as_str)
        .map(cursor_position)
        .transpose()?;
    let text = |name: &str| match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        Some(_) => Err(commands::ClientField(
            "a History filter is a non-empty text",
        )),
    };
    Ok(Scope {
        limit: page_limit(args)?,
        open,
        newest,
        after,
        cursor,
        alias: text("by_alias")?,
        op: text("op")?,
        kind: text("kind")?,
        target: text("target")?,
        under: text("under")?,
    })
}

const UNDER_TREE: &str = "WITH RECURSIVE root(key) AS (
        SELECT CASE
            WHEN strpos($2,':')>0 THEN $2
            ELSE COALESCE((SELECT kind||':'||local_id FROM joppa.item_ids WHERE workspace=$1 AND id=joppa.short_item_id($2) LIMIT 1),$2)
        END
    ), sub(address) AS (
        SELECT COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=r.key),r.key)
        FROM root r
        UNION
        SELECT c.address FROM sub s
            LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.target=s.address
            CROSS JOIN LATERAL (VALUES (s.address),(a.address)) k(key)
            JOIN joppa.objects c ON c.workspace=$1 AND c.parent=k.key
    ) SELECT DISTINCT address FROM sub";

/// Resolve `target` (when it is a public item ID) and `under` (into the canonical addresses of
/// the node and all of its present and archived descendants). Runs zero queries when unset.
pub(super) async fn bind_targets(
    conn: &impl GenericClient,
    workspace: &str,
    s: &Scope,
) -> Result<(Option<String>, Option<Vec<String>>)> {
    let target = match s.target.as_deref() {
        Some(t) if !t.contains(':') => Some(
            conn.query_opt(
                "SELECT kind||':'||local_id FROM joppa.item_ids WHERE workspace=$1 AND id=joppa.short_item_id($2) LIMIT 1",
                &[&workspace, &t],
            )
            .await?
            .map_or_else(|| t.to_owned(), |r| r.get(0)),
        ),
        Some(t) => Some(t.to_owned()),
        None => None,
    };
    let under = match s.under.as_deref() {
        Some(u) => Some(
            conn.query(UNDER_TREE, &[&workspace, &u])
                .await?
                .iter()
                .map(|r| r.get(0))
                .collect(),
        ),
        None => None,
    };
    Ok((target, under))
}
