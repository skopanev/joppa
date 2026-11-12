//! The aggregate counts of a list read: joppa_objects and HTTP /objects.
//!
//! - `count: true` gives `{"total": N}`: the number of nodes that the list keeps.
//! - `group_by` takes 1 to 3 keys and gives `{"total": N, "groups": [...]}`. Each group has
//!   the value of each key, under the key as sent, and its `count`.
//! - A key is `kind`, `status` (the status tone), `owner`, `assignee`, or a data field as
//!   `<field>` or `data.<field>`. A data field reads the computed fields too.
//! - A missing value groups as null. An assignee "" groups as null: it means no assignee.
//! - The groups come in the order of the key values, ascending, with null last.
//!
//! The counts take the node filters and count the same nodes as the list: without a filter,
//! the roots. A parameter of one page or of one object is refused, and `count` with
//! `group_by` is refused too: `group_by` already gives the total.
use anyhow::{Result, bail, ensure};
use serde_json::Value;

/// The parameters that a read with count or group_by refuses.
pub const REFUSED: [&str; 13] = [
    "address",
    "cursor",
    "limit",
    "fields",
    "relations_of",
    "in_relation",
    "related_to",
    "note_status",
    "revision",
    "history",
    "comments_cursor",
    "direction",
    "relation",
];

/// One key of group_by.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Kind,
    Status,
    Owner,
    Assignee,
    /// A data field, with the key as sent.
    Data {
        sent: String,
        field: String,
    },
}

impl Key {
    fn parse(sent: &str) -> Result<Self> {
        let field = sent.strip_prefix("data.").unwrap_or(sent);
        let named = !field.is_empty()
            && field.len() <= 100
            && field
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        ensure!(
            named,
            "group_by key {sent} is unknown; use kind, status, owner, assignee, a data field name or data.<field>"
        );
        Ok(match sent {
            "kind" => Self::Kind,
            "status" => Self::Status,
            "owner" => Self::Owner,
            "assignee" => Self::Assignee,
            _ => Self::Data {
                sent: sent.to_owned(),
                field: field.to_owned(),
            },
        })
    }

    /// The key as sent: the name of its value in each group.
    pub fn sent(&self) -> &str {
        match self {
            Self::Kind => "kind",
            Self::Status => "status",
            Self::Owner => "owner",
            Self::Assignee => "assignee",
            Self::Data { sent, .. } => sent,
        }
    }
}

/// The aggregate of one read: only the total, or the groups of 1 to 3 keys.
#[derive(Clone, Debug, PartialEq)]
pub enum Aggregate {
    Count,
    GroupBy(Vec<Key>),
}

impl Aggregate {
    /// The aggregate of MCP arguments, or None for a plain read. `sent` names each other
    /// parameter that the request sends.
    pub fn parse<'a>(a: &Value, sent: impl IntoIterator<Item = &'a str>) -> Result<Option<Self>> {
        let count = match a.get("count") {
            None | Some(Value::Null) => false,
            Some(v) => v
                .as_bool()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| anyhow::anyhow!("count must be a boolean"))?,
        };
        let keys = match a.get("group_by") {
            None | Some(Value::Null) => None,
            Some(Value::Array(items)) => Some(
                items
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| anyhow::anyhow!("group_by must be a list of 1 to 3 keys"))?,
            ),
            // HTTP sends the keys separated by commas.
            Some(Value::String(text)) if text.is_empty() => Some(vec![]),
            Some(Value::String(text)) => Some(text.split(',').map(str::to_owned).collect()),
            Some(_) => bail!("group_by must be a list of 1 to 3 keys"),
        };
        let aggregate = match (count, keys) {
            (false, None) => return Ok(None),
            (true, Some(_)) => {
                bail!("count cannot be combined with group_by; group_by also gives total")
            }
            (true, None) => Self::Count,
            (false, Some(keys)) => Self::GroupBy(parse_keys(&keys)?),
        };
        let name = aggregate.name();
        for other in sent {
            ensure!(
                !REFUSED.contains(&other),
                "{name} cannot be combined with {other}"
            );
        }
        Ok(Some(aggregate))
    }

    /// The parameter name of the aggregate, for a refusal.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::GroupBy(_) => "group_by",
        }
    }
}

fn parse_keys(keys: &[String]) -> Result<Vec<Key>> {
    ensure!(
        (1..=3).contains(&keys.len()),
        "group_by takes 1 to 3 keys, not {}",
        keys.len()
    );
    let keys: Vec<Key> = keys.iter().map(|k| Key::parse(k)).collect::<Result<_>>()?;
    for (i, key) in keys.iter().enumerate() {
        ensure!(
            !keys[..i].contains(key),
            "group_by names {} twice",
            key.sent()
        );
    }
    Ok(keys)
}

#[cfg(test)]
#[path = "node_counts_tests.rs"]
mod tests;
