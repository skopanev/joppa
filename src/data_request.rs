//! The request of `POST /api/data` and its checks against the workspace schema. Every type
//! name and field name comes from `types` of the schema. A refusal carries its HTTP status.
use crate::objects::TreeType;
use anyhow::Result;
use checks::{declared, kinds, values, within};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
#[path = "data_request_checks.rs"]
mod checks;

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
    /// True keeps the blocked nodes, false the other nodes (dependency_sql.rs).
    pub blocked: Option<bool>,
    /// data_any, data_all and data_none, as the node filters take them (node_filter_sets.rs).
    pub sets: crate::node_filter::DataSets,
    pub query: Option<String>,
    pub fields: Vec<String>,
    pub limit: Option<usize>,
    pub max_chars: usize,
}

/// The item keys that each type has: the stored tone, the parent and the document fields.
pub const FIXED_FIELDS: [&str; 13] = [
    "title",
    "body",
    "owner",
    "assignee",
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
        let (mut filter, mut blocked, mut sets) = (vec![], None, Map::new());
        for (field, wanted) in self.filter.unwrap_or_default() {
            if crate::node_filter::DataSets::NAMES.contains(&field.as_str()) {
                sets.insert(field, wanted);
                continue;
            }
            if field == "blocked" {
                blocked = Some(
                    wanted
                        .as_bool()
                        .ok_or_else(|| bad("filter blocked is true or false"))?,
                );
                continue;
            }
            let rules = match declared.get(&field) {
                Some(rules) => rules.clone(),
                None if ["owner", "assignee"].contains(&field.as_str()) => ["text".into()].into(),
                None => return Err(bad(format!("filter field {field} is not in the schema"))),
            };
            filter.push((field.clone(), values(&field, wanted, &rules)?));
        }
        let sets = crate::node_filter::DataSets::parse(&Value::Object(sets));
        let sets = sets.map_err(|e| bad(e.to_string()))?;
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
            blocked,
            sets,
            query,
            fields,
            limit,
            max_chars,
        })
    }
}

#[cfg(test)]
#[path = "data_request_tests.rs"]
mod tests;
