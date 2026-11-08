//! The stored rows of two workspaces in a form that compares: each row as JSON, with the
//! values that differ between two equal workspaces made equal.
//!
//! The normalization replaces:
//! - each public item ID of the workspace with `ITEM(<kind>:<local id>)`: the server
//!   generates a random code for each item ID;
//! - the workspace ID with `W`: the two workspaces have different IDs;
//! - the reference prefix with `PREFIX`: the server makes it from the title and gives the
//!   second workspace with the same title a prefix with a number;
//! - each timestamp string with `TIME`: two writes never run at the same instant;
//! - the number under each key `record` or `report`: the ID of a test report comes from one
//!   sequence for all workspaces.
//!
//! Positions, revisions, local IDs, request IDs and every other value stay as they are.
#![allow(dead_code)]
use anyhow::Result;
use serde_json::Value;

pub type Pool = deadpool_postgres::Pool;

/// The tables that the comparison reads, with the order of their rows.
pub const TABLES: [(&str, &str); 4] = [
    ("state_entries", "section,key"),
    ("objects", "address"),
    ("object_revisions", "address,revision"),
    ("board_cache", "workspace"),
];

fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 19
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[7] == b'-'
        && (b[10] == b'T' || b[10] == b' ')
        && b[13] == b':'
        && b[16] == b':'
}

fn walk(value: &mut Value) {
    match value {
        Value::String(s) if is_time(s) => *s = "TIME".into(),
        Value::Array(items) => items.iter_mut().for_each(walk),
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if (key == "record" || key == "report") && item.is_number() {
                    *item = Value::from(0);
                } else {
                    walk(item);
                }
            }
        }
        _ => {}
    }
}

/// The text replacements of one workspace: its item IDs first, then its ID and its prefix.
pub async fn names(pool: &Pool, ws: &str) -> Result<Vec<(String, String)>> {
    let conn = pool.get().await?;
    let sql =
        "SELECT id,kind,local_id FROM joppa.item_ids WHERE workspace=$1 ORDER BY length(id) DESC";
    let mut out: Vec<(String, String)> = conn
        .query(sql, &[&ws])
        .await?
        .iter()
        .map(|r| {
            let (kind, local): (String, String) = (r.get(1), r.get(2));
            (r.get(0), format!("ITEM({kind}:{local})"))
        })
        .collect();
    out.push((ws.to_owned(), "W".to_owned()));
    let sql = "SELECT value->>'reference_prefix' FROM joppa.state_entries WHERE workspace=$1 AND section='meta' AND key=''";
    if let Some(row) = conn.query_opt(sql, &[&ws]).await?
        && let Some(prefix) = row.get::<_, Option<String>>(0)
    {
        out.push((prefix, "PREFIX".to_owned()));
    }
    Ok(out)
}

/// A value of the workspace with the replacements of `names`.
pub fn normalize(value: &Value, names: &[(String, String)]) -> Result<Value> {
    let mut text = value.to_string();
    for (from, to) in names {
        text = text.replace(from.as_str(), to);
    }
    let mut value: Value = serde_json::from_str(&text)?;
    walk(&mut value);
    Ok(value)
}

/// The normalized rows of each table of `TABLES` for one workspace.
pub async fn rows(pool: &Pool, ws: &str) -> Result<Vec<(String, Vec<Value>)>> {
    let names = names(pool, ws).await?;
    let conn = pool.get().await?;
    let mut out = vec![];
    for (table, order) in TABLES {
        let sql = format!(
            "SELECT row_to_json(t)::text FROM joppa.{table} t WHERE workspace=$1 ORDER BY {order}"
        );
        let mut rows = vec![];
        for row in conn.query(&sql, &[&ws]).await? {
            let value: Value = serde_json::from_str(row.get(0))?;
            rows.push(normalize(&value, &names)?);
        }
        out.push((table.to_owned(), rows));
    }
    Ok(out)
}

/// The first difference of two row lists, or None when they are equal.
pub fn difference(a: &[Value], b: &[Value]) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!("{} rows and {} rows", a.len(), b.len()));
    }
    a.iter()
        .zip(b)
        .find(|(x, y)| x != y)
        .map(|(x, y)| format!("A: {x}\nB: {y}"))
}
