//! The `order` of an object list read (requirement:w17jbz70): 1 to 4 clauses, each
//! a key with `asc` (the default) or `desc`. A key is one of three kinds.
//!
//! - A column: `address`, `title`, `revision`, `age` or `created` (the journal position of the
//!   first revision), and the dependency facts `dependents` and `blocked` (dependency_sql.rs).
//! - A ranked value: `status` in the order red, orange, yellow, blue, green, `priority` in the
//!   order of `schema.priorities`, and a data field that the schema declares as an enum.
//! - Any other data field `<field>` or `data.<field>`: the authored value, else the computed
//!   value. Numbers sort as numbers, other values as text.
//!
//! An absent value sorts last in both directions, and `address` breaks each tie. The field
//! name and the ranks are SQL parameters, so no text of the request becomes SQL.
use crate::objects::TreeType;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio_postgres::types::ToSql;

/// The refusal of an unknown order key.
pub const KEYS: &str = "order key must be dependents, blocked, priority, age, created, address, revision, title, status or a data field";
/// The refusal of a cursor that is not from a list with order.
const CURSOR: &str = "invalid object cursor, start again without cursor";
/// The status tones in work order. The archived tone has no rank.
const TONES: [&str; 5] = ["red", "orange", "yellow", "blue", "green"];

/// One key of an order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Dependents,
    Blocked,
    Priority,
    Age,
    Address,
    Revision,
    Title,
    Status,
    Field(String),
}

/// The clauses of an order: each key with true for `desc`.
#[derive(Clone, Debug, PartialEq)]
pub struct Order(pub Vec<(Key, bool)>);

/// A parameter of the SQL of an order.
pub type Param = Box<dyn ToSql + Sync + Send>;

/// What the SQL of an order reads besides the row: the facts as JSON text (Facts::json), the
/// priorities of the schema and the types of the workspace.
pub struct Inputs<'a> {
    pub facts: Option<String>,
    pub priorities: &'a [String],
    pub types: &'a BTreeMap<String, TreeType>,
}

impl Order {
    /// The order of `value`: a text with clauses separated by commas, or a list of clauses.
    /// The address order alone is the default order of a list, so it gives None.
    pub fn parse(value: &Value) -> Result<Option<Self>> {
        let clauses: Vec<&str> = match value {
            Value::Null => return Ok(None),
            Value::String(text) => text.split(',').collect(),
            Value::Array(items) => (items.iter().map(Value::as_str))
                .collect::<Option<_>>()
                .context("order must be a string or a list of strings")?,
            _ => bail!("order must be a string or a list of strings"),
        };
        ensure!(
            (1..=4).contains(&clauses.len()),
            "order takes 1 to 4 clauses"
        );
        let mut out: Vec<(Key, bool)> = vec![];
        for clause in clauses {
            let mut words = clause.split_whitespace();
            let key = key(words.next().unwrap_or_default())?;
            let desc = match (words.next(), words.next()) {
                (None | Some("asc"), None) => false,
                (Some("desc"), None) => true,
                _ => bail!("order direction must be asc or desc"),
            };
            ensure!(
                out.iter().all(|(k, _)| *k != key),
                "order names each key once"
            );
            out.push((key, desc));
        }
        Ok((out != [(Key::Address, false)]).then_some(Self(out)))
    }

    /// True when the order reads the dependency facts.
    pub fn needs_facts(&self) -> bool {
        (self.0.iter()).any(|(k, _)| matches!(k, Key::Dependents | Key::Blocked))
    }

    /// True when the order reads the priorities of the schema.
    pub fn needs_priorities(&self) -> bool {
        self.0.iter().any(|(k, _)| *k == Key::Priority)
    }

    /// The ORDER BY text on the alias `o`, with its parameters numbered from `first`.
    pub fn sql(&self, first: usize, inputs: Inputs<'_>) -> (String, Vec<Param>) {
        let mut params: Vec<Param> = vec![];
        let mut add = |param: Param| {
            params.push(param);
            format!("${}", first + params.len() - 1)
        };
        let facts = (self.needs_facts()).then(|| add(Box::new(inputs.facts.unwrap_or_default())));
        let facts = facts.map(|n| format!("COALESCE({n}::text,'{{}}')::jsonb->o.address"));
        let mut parts = vec![];
        for (key, desc) in &self.0 {
            let dir = if *desc { "DESC" } else { "ASC" };
            let facts = facts.as_deref().unwrap_or_default();
            let column = |expr: &str| format!("{expr} {dir} NULLS LAST");
            let rank = |expr: &str, ranks: String| {
                format!("array_position({ranks}::text[],{expr}) {dir} NULLS LAST")
            };
            parts.push(match key {
                Key::Dependents => column(&format!("COALESCE(({facts}->>'dependents')::bigint,0)")),
                Key::Blocked => column(&format!("COALESCE(({facts}->>'blocked')::boolean,false)")),
                Key::Age => column("COALESCE(o.created_position,0)"),
                Key::Address => column("o.address"),
                Key::Revision => column("o.revision"),
                Key::Title => column("o.title"),
                Key::Status => rank(crate::node_filter::TONE, add(Box::new(tones()))),
                Key::Priority => {
                    let ranks = add(Box::new(inputs.priorities.to_vec()));
                    rank(&text(&value(&add(Box::new("priority".to_owned())))), ranks)
                }
                Key::Field(field) => {
                    let value = value(&add(Box::new(field.clone())));
                    match enum_values(inputs.types, field) {
                        Some(values) => rank(&text(&value), add(Box::new(values))),
                        None => format!(
                            "CASE WHEN jsonb_typeof({value})='number' THEN ({value})::text::numeric END {dir} NULLS LAST,{} {dir} NULLS LAST",
                            text(&value)
                        ),
                    }
                }
            });
        }
        if !self.0.iter().any(|(k, _)| *k == Key::Address) {
            parts.push("o.address ASC".to_owned());
        }
        (parts.join(","), params)
    }
}

/// The key of one order name.
fn key(name: &str) -> Result<Key> {
    Ok(match name {
        "dependents" => Key::Dependents,
        "blocked" => Key::Blocked,
        "priority" | "data.priority" => Key::Priority,
        "age" | "created" => Key::Age,
        "address" => Key::Address,
        "revision" => Key::Revision,
        "title" => Key::Title,
        "status" => Key::Status,
        other => {
            let field = other.strip_prefix("data.").unwrap_or(other);
            crate::commands::identifier(field).context(KEYS)?;
            Key::Field(field.to_owned())
        }
    })
}

/// The JSON value of the data field named by the parameter `name`: authored, else computed.
fn value(name: &str) -> String {
    format!("COALESCE(o.document->'data'->{name}::text,o.computed->{name}::text)")
}

/// The text of a JSON value, or NULL for an absent value or "".
fn text(value: &str) -> String {
    format!("NULLIF(({value})#>>'{{}}','')")
}

fn tones() -> Vec<String> {
    TONES.map(str::to_owned).to_vec()
}

/// The values of `field` when each type that declares it as an enum gives the same values.
fn enum_values(types: &BTreeMap<String, TreeType>, field: &str) -> Option<Vec<String>> {
    let rules = types.values().filter_map(|t| t.fields.get(field));
    let mut lists = rules.filter(|r| r.kind == "enum" && !r.values.is_empty());
    let first = lists.next()?.values.clone();
    lists.all(|r| r.values == first).then_some(first)
}

/// The order of joppa_objects arguments. It applies only to an object list, so a read by
/// address, an edge read, a member read and a count refuse it.
pub fn from_args(a: &Value) -> Result<Option<Order>> {
    let order = Order::parse(&a["order"])?;
    let other = ["address", "relations_of", "in_relation", "group_by"];
    let other = other.iter().any(|k| !a[*k].is_null()) || a["count"] == true;
    ensure!(
        order.is_none() || !other,
        "order applies only to object list reads"
    );
    Ok(order)
}

/// The offset of a page of a list with order: 0 without a cursor, else the offset of a cursor
/// `<position>:@<offset>` of the same snapshot.
pub fn offset(cursor: Option<&str>, position: i64) -> Result<usize> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let (p, offset) = cursor.split_once(":@").context(CURSOR)?;
    ensure!(
        p.parse::<i64>().ok() == Some(position),
        "object list changed, start again without cursor"
    );
    offset.parse().ok().filter(|n| *n > 0).context(CURSOR)
}

/// The cursor of the next page of a list with order.
pub fn next_cursor(position: i64, offset: usize, limit: usize) -> String {
    format!("{position}:@{}", offset + limit)
}

/// The priorities of the current schema revision, or p0, p1 and p2 without a saved list.
pub async fn priorities(
    client: &impl tokio_postgres::GenericClient,
    workspace: &str,
) -> Result<Vec<String>> {
    let sql = "SELECT (value->'priorities')::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' AND revision=(SELECT (state->'tree_schema'->-1->>'revision')::bigint FROM joppa.board_cache WHERE workspace=$1)";
    let row = client.query_opt(sql, &[&workspace]).await?;
    let text = row.and_then(|r| r.get::<_, Option<String>>(0));
    let saved: Option<Vec<String>> = text.map(|t| serde_json::from_str(&t)).transpose()?;
    Ok(match saved.filter(|list| !list.is_empty()) {
        Some(list) => list,
        None => ["p0", "p1", "p2"].map(str::to_owned).to_vec(),
    })
}

#[cfg(test)]
#[path = "dependency_order_tests.rs"]
mod tests;
