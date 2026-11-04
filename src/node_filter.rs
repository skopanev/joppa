//! The node filters that every read takes in one form: joppa_objects, joppa_search and their
//! HTTP routes. The filters are `parent` with `depth`, `kind`, `status`, `owner`,
//! `assignee`, `ready` and `data`. All filters must match.
//!
//! - `parent` names a node: the read keeps each descendant of it at any depth. `depth: N`
//!   keeps the descendants at most N levels below it; `depth: 1` keeps the direct children.
//!   Without a parent address, `depth` counts from the top: `depth: 1` keeps the roots.
//!   `parent: "*"` keeps every level.
//! - `kind` and `status` take one value or a list.
//! - `status` is the stored tone of the node: the rollup tone, else the signal tone. The tone
//!   `purple` marks archived nodes: a list read with it also keeps the current archived nodes.
//! - `owner` is `data.owner` and `assignee` is `data.assignee`. Any node can have both.
//! - `ready` is false when the node waits: for a dependency, or for the answer that its
//!   readiness mark asks for (`assess` with `ready: false`). A node of a type with the status
//!   rule `children` or `links` is ready only when every counted child or linking object is
//!   ready. Every other node is ready.
//! - `data` is a JSON object that `document.data` contains, as the JSONB operator `@>`.
//!
//! The same SQL clause serves every read, with the alias `o` for `joppa.objects`.
use crate::status_colors::STATUS_TONES;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value};

#[path = "node_filter_sql.rs"]
mod sql;
pub use sql::{ASSIGNEE, Bound, READY, TONE, bind};

/// The node filters of one read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeFilter {
    /// A node address or public item ID, or `*` for every level.
    pub parent: Option<String>,
    pub depth: Option<i32>,
    pub kinds: Option<Vec<String>>,
    pub statuses: Option<Vec<String>>,
    pub owner: Option<String>,
    pub assignee: Option<String>,
    pub ready: Option<bool>,
    /// The JSON text of the containment filter.
    pub data: Option<String>,
}

/// The names of the filters, in the order of the reply rules.
pub const NAMES: [&str; 8] = [
    "parent", "depth", "kind", "status", "owner", "assignee", "ready", "data",
];

impl NodeFilter {
    /// The filters of MCP arguments. A missing or null filter is absent.
    pub fn parse(a: &Value) -> Result<Self> {
        let text = |name: &str| match a.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
            Some(_) => bail!("{name} must be a non-empty string"),
        };
        let parent = text("parent")?;
        // A client with an older tool schema sends a number, a boolean or an object as text.
        let depth = match a.get("depth") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    .filter(|n| (1..=100).contains(n))
                    .context("depth must be an integer from 1 to 100")? as i32,
            ),
        };
        let statuses = list(a, "status")?;
        for tone in statuses.iter().flatten() {
            ensure!(
                STATUS_TONES.contains(&tone.as_str()),
                "unknown status {tone}; use {}",
                STATUS_TONES.join(", ")
            );
        }
        let ready = match a.get("ready") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_bool()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    .context("ready must be a boolean")?,
            ),
        };
        let data = match a.get("data") {
            None | Some(Value::Null) => None,
            Some(v @ Value::Object(_)) => Some(v.to_string()),
            Some(Value::String(s)) if s.trim_start().starts_with('{') => {
                let v: Value = serde_json::from_str(s).context("data filter must be an object")?;
                Some(v.to_string())
            }
            Some(_) => bail!("data filter must be an object"),
        };
        Ok(Self {
            parent,
            depth,
            kinds: list(a, "kind")?,
            statuses,
            owner: text("owner")?,
            assignee: text("assignee")?,
            ready,
            data,
        })
    }

    /// The filters of HTTP query text: `kind` and `status` are comma-separated, `depth` is a
    /// number, `ready` is true or false and `data` is JSON text.
    pub fn parse_query(pairs: &[(&str, Option<&str>)]) -> Result<Self> {
        let mut map = Map::new();
        for (name, value) in pairs {
            let Some(value) = value.filter(|v| !v.is_empty()) else {
                continue;
            };
            let value = match *name {
                "kind" | "status" if value.contains(',') => {
                    Value::from(value.split(',').map(str::to_owned).collect::<Vec<_>>())
                }
                "depth" => Value::from(value.parse::<i64>().unwrap_or(0)),
                "ready" => match value {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => bail!("ready must be a boolean"),
                },
                "data" => serde_json::from_str(value).context("data filter must be an object")?,
                _ => Value::from(value),
            };
            map.insert((*name).to_owned(), value);
        }
        Self::parse(&Value::Object(map))
    }

    /// True when a filter other than `parent: "*"` is present.
    pub fn narrows(&self) -> bool {
        self.parent.as_deref().is_some_and(|p| p != "*")
            || self.depth.is_some()
            || self.kinds.is_some()
            || self.statuses.is_some()
            || self.owner.is_some()
            || self.assignee.is_some()
            || self.ready.is_some()
            || self.data.is_some()
    }

    /// True when `status` names the tone of archived nodes: a list read then also keeps the
    /// current archived nodes, and the walk below `parent` also passes archived nodes.
    pub fn archived(&self) -> bool {
        let tone = crate::status_colors::ARCHIVED_TONE;
        self.statuses.iter().flatten().any(|s| s == tone)
    }

    /// The type of the read when it names exactly one.
    pub fn one_kind(&self) -> Option<&str> {
        match self.kinds.as_deref() {
            Some([kind]) => Some(kind),
            _ => None,
        }
    }

    /// The filters of a parent and a type, as an internal read gives them.
    pub fn of(parent: Option<&str>, kind: Option<&str>) -> Self {
        Self {
            parent: parent.map(str::to_owned),
            kinds: kind.map(|k| vec![k.to_owned()]),
            ..Self::default()
        }
    }

    /// Keep only nodes of this type.
    pub fn kind(mut self, kind: &str) -> Self {
        self.kinds = Some(vec![kind.to_owned()]);
        self
    }

    /// Keep only the descendants of this node, or every level with `*`.
    pub fn parent(mut self, parent: &str) -> Self {
        self.parent = Some(parent.to_owned());
        self
    }
}

fn list(a: &Value, name: &str) -> Result<Option<Vec<String>>> {
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
