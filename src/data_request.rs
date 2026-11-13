//! The request of `POST /api/data` and its checks against the workspace schema. Every type
//! name and field name comes from `types` of the schema. A refusal carries its HTTP status.
use crate::objects::TreeType;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The start of each read key. The rest is 64 lowercase hex characters.
pub const KEY_PREFIX: &str = "jpr_";

/// True when `token` has the form of a read key.
pub fn is_read_key(token: &str) -> bool {
    token.strip_prefix(KEY_PREFIX).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// A refused request: the HTTP status and the error text of the reply.
#[derive(Debug)]
pub struct Refusal {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

/// A refusal with status 400.
pub fn bad(message: impl Into<String>) -> anyhow::Error {
    refuse(400, message)
}

/// A refusal with `status`.
pub fn refuse(status: u16, message: impl Into<String>) -> anyhow::Error {
    let message = message.into();
    Refusal { status, message }.into()
}

/// The body of `POST /api/data`. Each field is optional, and an unknown field is refused.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub workspace: Option<String>,
    pub root: Option<String>,
    pub kind: Option<Value>,
    pub filter: Option<Map<String, Value>>,
    pub query: Option<String>,
    pub fields: Option<Vec<String>>,
    pub limit: Option<i64>,
    pub max_chars: Option<i64>,
}

/// The checked request.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub root: Option<String>,
    /// The types that the reply gives. None gives every type.
    pub kinds: Option<BTreeSet<String>>,
    /// Each filter field with its wanted values.
    pub filter: Vec<(String, Vec<Value>)>,
    pub query: Option<String>,
    pub fields: Vec<String>,
    pub limit: Option<usize>,
    pub max_chars: usize,
}

/// The item keys that each type has: the stored tone, the parent and the document fields.
pub const FIXED_FIELDS: [&str; 6] = ["title", "body", "owner", "assignee", "parent", "status"];
/// The default of `max_chars`.
pub const MAX_CHARS: usize = 32768;
/// The most items of a walk without `query`.
pub const WALK_LIMIT: i64 = 1000;
/// The most items of a search, and the default.
pub const QUERY_LIMIT: i64 = 25;
pub const QUERY_DEFAULT: i64 = 10;

impl Request {
    /// Check the request against the types of the workspace schema.
    pub fn plan(self, types: &BTreeMap<String, TreeType>) -> Result<Plan> {
        let declared = declared(types);
        let kinds = match &self.kind {
            None | Some(Value::Null) => None,
            Some(kind) => Some(kinds(kind, types)?),
        };
        let mut filter = vec![];
        for (field, wanted) in self.filter.unwrap_or_default() {
            let rules = match declared.get(&field) {
                Some(rules) => rules.clone(),
                None if ["owner", "assignee"].contains(&field.as_str()) => ["text".into()].into(),
                None => return Err(bad(format!("filter field {field} is not in the schema"))),
            };
            filter.push((field.clone(), values(&field, wanted, &rules)?));
        }
        let fields = match self.fields {
            None => vec!["title".into(), "body".into()],
            Some(fields) if fields.is_empty() => return Err(bad("fields must not be empty")),
            Some(fields) => fields,
        };
        for field in &fields {
            if !FIXED_FIELDS.contains(&field.as_str()) && !declared.contains_key(field) {
                return Err(bad(format!("field {field} is not in the schema")));
            }
        }
        let query = match self.query {
            Some(q) if q.trim().is_empty() || q.chars().count() > 2000 => {
                return Err(bad("query must contain 1 to 2000 characters"));
            }
            query => query,
        };
        let limit = match (&query, self.limit) {
            (Some(_), limit) => Some(within(
                limit.unwrap_or(QUERY_DEFAULT),
                QUERY_LIMIT,
                "limit",
            )?),
            (None, Some(limit)) => Some(within(limit, WALK_LIMIT, "limit")?),
            (None, None) => None,
        };
        let ceiling = crate::field_limit::FIELD_CEILING as i64;
        let max_chars = within(
            self.max_chars.unwrap_or(MAX_CHARS as i64),
            ceiling,
            "max_chars",
        )?;
        Ok(Plan {
            root: self.root,
            kinds,
            filter,
            query,
            fields,
            limit,
            max_chars,
        })
    }
}

/// The field types of each declared field, from every type of the schema.
fn declared(types: &BTreeMap<String, TreeType>) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in types.values() {
        for (field, rule) in &t.fields {
            out.entry(field.clone())
                .or_default()
                .insert(rule.kind.clone());
        }
    }
    out
}

/// A type name or a non-empty list of them. Each name must be a type of the schema.
fn kinds(kind: &Value, types: &BTreeMap<String, TreeType>) -> Result<BTreeSet<String>> {
    let names: Vec<&Value> = match kind {
        Value::Array(items) if !items.is_empty() => items.iter().collect(),
        Value::String(_) => vec![kind],
        _ => {
            return Err(bad(
                "kind must be a type name or a non-empty list of type names",
            ));
        }
    };
    let mut out = BTreeSet::new();
    for name in names {
        let name = name
            .as_str()
            .ok_or_else(|| bad("kind must hold type names"))?;
        if !types.contains_key(name) {
            return Err(bad(format!("kind {name} is not a type of the schema")));
        }
        out.insert(name.to_owned());
    }
    Ok(out)
}

/// The wanted values of one filter field: one value or a non-empty list. Each value must
/// have the JSON type of one field type of `rules`.
fn values(field: &str, wanted: Value, rules: &BTreeSet<String>) -> Result<Vec<Value>> {
    let items = match wanted {
        Value::Array(items) if items.is_empty() => {
            return Err(bad(format!("filter {field} must not be an empty list")));
        }
        Value::Array(items) => items,
        value => vec![value],
    };
    let fits = |rule: &String| {
        items.iter().all(|v| match rule.as_str() {
            "number" => v.is_number(),
            "boolean" => v.is_boolean(),
            _ => v.is_string(),
        })
    };
    if !rules.iter().any(fits) {
        let types = rules.iter().cloned().collect::<Vec<_>>().join(" or ");
        return Err(bad(format!(
            "filter {field} needs a {types} value or a list of them"
        )));
    }
    Ok(items)
}

/// `value` when it is 1 to `max`, else a refusal that names `name`.
fn within(value: i64, max: i64, name: &str) -> Result<usize> {
    match (1..=max).contains(&value) {
        true => Ok(value as usize),
        false => Err(bad(format!("{name} must be 1 to {max}"))),
    }
}

#[cfg(test)]
#[path = "data_request_tests.rs"]
mod tests;
