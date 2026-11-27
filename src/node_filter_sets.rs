//! The filters that take a set of values. `kind` and `status` take one value or a list.
//! `data_any`, `data_all` and `data_none` map a data field to one string or a list of strings.
//! A dotted key such as `delivery.state` names a nested field. A value matches a string field
//! that is equal to it, or a list field that holds it. The data has the computed fields too.
//! - `data_any` keeps the nodes whose field matches at least one value, for each key. An
//!   empty list keeps no node.
//! - `data_all` keeps the nodes whose list field holds every value, for each key. An empty
//!   list keeps every node.
//! - `data_none` drops the nodes whose field matches a value, for any key.
//!
//! The GIN indexes of the data and of the computed values find the candidates, and the data
//! with the computed values then decides exactly.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use tokio_postgres::GenericClient;
use tokio_postgres::types::ToSql;

/// The most values of one key.
pub const MOST: usize = 10_000;

/// One key of a set filter: the path of the field and the values.
pub type Entry = (Vec<String>, Vec<String>);

/// The data set filters of one read. An empty object is set and keeps every node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataSets {
    pub any: Option<Vec<Entry>>,
    pub all: Option<Vec<Entry>>,
    pub none: Option<Vec<Entry>>,
}

impl DataSets {
    /// The names of the filters.
    pub const NAMES: [&str; 3] = ["data_any", "data_all", "data_none"];

    /// The filters of MCP arguments or of JSON text. A missing or null filter is absent.
    pub fn parse(a: &Value) -> Result<Self> {
        Ok(Self {
            any: entries(a, "data_any")?,
            all: entries(a, "data_all")?,
            none: entries(a, "data_none")?,
        })
    }

    /// True when a filter is set.
    pub fn is_set(&self) -> bool {
        self.any.is_some() || self.all.is_some() || self.none.is_some()
    }

    /// The probes of each key that a kept node matches, and of each key that drops a node.
    pub(crate) fn groups(&self) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
        // An empty data_all list keeps every node, and an empty data_none list drops none.
        let each = |entries: &Option<Vec<Entry>>, all: bool| -> Vec<Vec<String>> {
            let entries = entries.iter().flatten();
            let entries = entries.filter(|(_, values)| !all || !values.is_empty());
            entries
                .map(|(path, values)| probes(path, values, all))
                .collect()
        };
        let mut keep = each(&self.any, false);
        keep.extend(each(&self.all, true));
        let skip = each(&self.none, false);
        (keep, skip.into_iter().filter(|p| !p.is_empty()).collect())
    }
}

/// The JSON objects that a matching node contains. Each value matches as a string or as an
/// item of a list. `all` asks for a list with every value.
fn probes(path: &[String], values: &[String], all: bool) -> Vec<String> {
    let nest = |leaf: Value| {
        let nested = path.iter().rev().fold(leaf, |inner, key| {
            Value::Object(Map::from_iter([(key.clone(), inner)]))
        });
        nested.to_string()
    };
    let one = |v: &String| {
        [
            nest(Value::from(v.as_str())),
            nest(Value::from(vec![v.as_str()])),
        ]
    };
    match all {
        false => values.iter().flat_map(one).collect(),
        true => {
            let distinct: BTreeSet<&String> = values.iter().collect();
            let mut out = vec![nest(Value::from(values.to_vec()))];
            out.extend((distinct.len() == 1).then(|| nest(Value::from(values[0].as_str()))));
            out
        }
    }
}

/// The present nodes of the workspace (also the archived nodes with `archived`) within
/// `within`, that match each group of probes (`join` " AND ") or one of them (" OR ").
pub(crate) async fn addresses(
    client: &impl GenericClient,
    workspace: &str,
    archived: bool,
    within: Option<&BTreeSet<String>>,
    groups: &[Vec<String>],
    join: &str,
) -> Result<BTreeSet<String>> {
    let within: Option<Vec<&String>> = within.map(|set| set.iter().collect());
    let mut params: Vec<&(dyn ToSql + Sync)> = vec![&workspace, &archived];
    let mut sql = "SELECT o.address FROM joppa.objects o
        WHERE o.workspace=$1 AND (o.present OR $2)"
        .to_owned();
    if let Some(within) = &within {
        params.push(within);
        sql.push_str(" AND o.address=ANY($3::text[])");
    }
    let mut each = vec![];
    for probes in groups {
        params.push(probes);
        each.push(matches(params.len()));
    }
    sql.push_str(&format!(" AND ({})", each.join(join)));
    let rows = client.query(&sql, &params).await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// The condition that a node contains one of the probes `$n`: a GIN index finds the
/// candidates, and the data with the computed values decides.
fn matches(n: usize) -> String {
    let probes = format!("ANY(${n}::text[]::jsonb[])");
    format!(
        "((o.computed @> {probes} OR o.document->'data' @> {probes}) AND {} @> {probes})",
        crate::computed_fields::DATA_SQL
    )
}

/// The keys of one set filter, each with its path and values.
fn entries(a: &Value, name: &str) -> Result<Option<Vec<Entry>>> {
    let parsed: Value;
    let map = match a.get(name) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(map)) => map,
        Some(Value::String(s)) if s.trim_start().starts_with('{') => {
            parsed = serde_json::from_str(s).unwrap_or_default();
            parsed
                .as_object()
                .context(format!("{name} filter must be an object"))?
        }
        Some(_) => bail!("{name} filter must be an object"),
    };
    let entry = |(key, value): (&String, &Value)| {
        let path: Vec<String> = key.split('.').map(str::to_owned).collect();
        ensure!(
            path.iter().all(|s| field_name(s)),
            "{name} key {key} must be a field name or a dotted path of field names"
        );
        let refusal = || format!("{name}.{key} must be a string or a list of up to {MOST} strings");
        let values = match value {
            Value::String(s) => vec![s.clone()],
            Value::Array(items) if items.len() <= MOST => items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .with_context(refusal)?,
            _ => bail!(refusal()),
        };
        Ok((path, values))
    };
    map.iter().map(entry).collect::<Result<_>>().map(Some)
}

/// A field name: 1 to 100 ASCII letters, digits, `_` and `-`.
fn field_name(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One value or a list of 1 or more values of up to 100 characters, as `kind` and `status`.
pub(super) fn list(a: &Value, name: &str) -> Result<Option<Vec<String>>> {
    let values = match a.get(name) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .with_context(|| format!("{name} must be a string or a list of strings"))?,
        Some(_) => bail!("{name} must be a string or a list of strings"),
    };
    ensure!(
        !values.is_empty() && values.iter().all(|v| !v.is_empty() && v.len() <= 100),
        "{name} must be a string or a list of strings"
    );
    Ok(Some(values))
}

#[cfg(test)]
#[path = "node_filter_sets_tests.rs"]
mod tests;
