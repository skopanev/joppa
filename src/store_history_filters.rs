//! Part of `store_history`: the query parameters, object extraction and subtree resolution.
use super::*;
use tokio_postgres::types::ToSql;

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

/// The raw target address of an event before alias resolution, as `history_entry::address`
/// gives it (migrations/083_speed_fixes.sql). Indexes on it serve `kind`, `target` and `under`.
const RAW: &str = "joppa.event_raw_address(e.command)";

/// The History filters with the addresses that their aliases add. Each filter that is set adds
/// one indexable condition, and an unset filter adds only a test of its null parameter.
pub(super) struct Bound {
    alias: Option<String>,
    op: Option<String>,
    kind: Option<String>,
    /// The alias addresses of another type that name a node of `kind`.
    added: Option<Vec<String>>,
    /// The alias addresses of `kind` that name a node of another type.
    excluded: Option<Vec<String>>,
    /// The raw addresses of the target: its address and its aliases.
    target: Option<Vec<String>>,
    /// The raw addresses of the subtree: each address and its aliases.
    under: Option<Vec<String>>,
}

impl Bound {
    /// The conditions of the entry filters, on the parameters `$first` to `$first+6`.
    pub(super) fn clause(&self, first: usize) -> String {
        let [a, o, k, ka, kx, t, u] = std::array::from_fn(|i| format!("${}", first + i));
        // The aliases that change the type are rare: without them the kind filter reads its
        // index alone.
        let aliased = self.added.is_some() || self.excluded.is_some();
        let kind = match aliased {
            false => format!("split_part({RAW},':',1)={k}"),
            true => format!(
                "((split_part({RAW},':',1)={k} AND NOT {RAW}=ANY(COALESCE({kx}::text[],'{{}}'))) OR {RAW}=ANY(COALESCE({ka}::text[],'{{}}')))"
            ),
        };
        let pick = |on: bool, when: String, unset: String| if on { when } else { unset };
        let parts = [
            pick(
                self.alias.is_some(),
                format!("e.alias={a}"),
                format!("{a}::text IS NULL"),
            ),
            pick(
                self.op.is_some(),
                format!("e.command->>'op'={o}"),
                format!("{o}::text IS NULL"),
            ),
            pick(self.kind.is_some(), kind, format!("{k}::text IS NULL")),
            pick(
                aliased,
                "TRUE".into(),
                format!("{ka}::text[] IS NULL AND {kx}::text[] IS NULL"),
            ),
            pick(
                self.target.is_some(),
                format!("{RAW}=ANY({t}::text[])"),
                format!("{t}::text[] IS NULL"),
            ),
            pick(
                self.under.is_some(),
                format!("{RAW}=ANY({u}::text[])"),
                format!("{u}::text[] IS NULL"),
            ),
        ];
        parts.map(|p| format!("({p})")).join(" AND ")
    }

    /// The 7 parameters of the clause, in its order.
    pub(super) fn params(&self) -> [&(dyn ToSql + Sync); 7] {
        [
            &self.alias,
            &self.op,
            &self.kind,
            &self.added,
            &self.excluded,
            &self.target,
            &self.under,
        ]
    }
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

/// The canonical addresses of the node of $2 and of its present and archived descendants, with
/// the alias addresses of each.
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
    ) SELECT address FROM sub
    UNION SELECT a.address FROM joppa.object_aliases a JOIN sub ON a.workspace=$1 AND a.target=sub.address";

/// The address of $2 after alias resolution, and the alias addresses that name it.
const TARGET_ALIASES: &str = "WITH c AS (SELECT COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2) AS a)
    SELECT a FROM c UNION SELECT x.address FROM joppa.object_aliases x, c WHERE x.workspace=$1 AND x.target=c.a";

/// The aliases that change the type: true when an alias of another type names a node of $2,
/// false when an alias of $2 names a node of another type.
const KIND_ALIASES: &str = "SELECT address, split_part(target,':',1)=$2 FROM joppa.object_aliases
    WHERE workspace=$1 AND (split_part(target,':',1)=$2) <> (split_part(address,':',1)=$2)";

/// Resolve the filters: `target` (also a public item ID) and `under` into their raw
/// addresses, and `kind` into the aliases that change the type. Runs zero queries when unset.
pub(super) async fn bind_targets(
    conn: &impl GenericClient,
    workspace: &str,
    s: &Scope,
) -> Result<Bound> {
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
    let addresses = async |sql: &str, key: &str| -> Result<Vec<String>> {
        let rows = conn.query(sql, &[&workspace, &key]).await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    };
    let target = match target {
        Some(t) => Some(addresses(TARGET_ALIASES, &t).await?),
        None => None,
    };
    let under = match s.under.as_deref() {
        Some(u) => Some(addresses(UNDER_TREE, u).await?),
        None => None,
    };
    let (mut added, mut excluded) = (None, None);
    if let Some(kind) = &s.kind {
        for row in conn.query(KIND_ALIASES, &[&workspace, kind]).await? {
            let list = if row.get(1) {
                &mut added
            } else {
                &mut excluded
            };
            list.get_or_insert_with(Vec::new).push(row.get(0));
        }
    }
    Ok(Bound {
        alias: s.alias.clone(),
        op: s.op.clone(),
        kind: s.kind.clone(),
        added,
        excluded,
        target,
        under,
    })
}

#[cfg(test)]
#[path = "store_history_filters_tests.rs"]
mod tests;
