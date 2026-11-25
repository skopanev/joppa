//! Part of `objects`: the parent hash (rule parent_hash). The contract hash of a node is the
//! sha256 of its contract (contract_view.rs) in a canonical form: the data without owner,
//! assignee and the typed fields that do not reset, with each source as its title, body and
//! live files. A save, move or restore of a node stores the contract hash of its parent in the
//! new revision. A save that changes only quiet fields keeps the stamp, and a save that sends
//! `data.parent_hash` equal to the current hash confirms it. A read compares the stamp with
//! the current hash of the parent (`parent_changed`).
use super::*;
use sha2::{Digest, Sha256};

/// The fields that a read gives and only the server writes. A save may send `parent_hash`
/// to confirm the current parent, and refuses the other two.
pub const PARENT_HASH_FIELDS: [&str; 3] = ["parent_hash", "parent_hash_current", "parent_changed"];

/// The contract hash of the object at `address`, present or archived. A compact Inbox type
/// (store_state_cache.rs) has none, so the hash never depends on how the state loads.
pub fn contract_hash(w: &Workspace, address: &str) -> Option<String> {
    let o = w.objects.get(resolve_address(w, address))?;
    let types = schema(w).types;
    let t = types.get(&o.kind)?;
    if t.inbox == Some(true) && ["group", "document"].contains(&t.role.as_str()) {
        return None;
    }
    let contract = contract(w, &types, &o.kind, &o.current().data);
    let mut text = String::new();
    canonical(&contract, &mut text);
    let digest = Sha256::digest(text.as_bytes());
    Some(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// The current contract hash of the parent of the object at `address`.
pub fn parent_hash_current(w: &Workspace, address: &str) -> Option<String> {
    let o = w.objects.get(resolve_address(w, address))?;
    contract_hash(w, o.current().parent.as_deref()?)
}

/// JSON with the keys of each object in order, so equal values give equal text.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The save without `data.parent_hash`, and the sent value. A save refuses the other read
/// fields. Only a save of the epoch of parent_hash reads the fields.
pub(crate) fn without_parent_hash<'a>(
    w: &Workspace,
    command: &'a Command,
    position: u64,
) -> Result<(std::borrow::Cow<'a, Command>, Option<Value>)> {
    use std::borrow::Cow;
    let Command::SaveObject(op) = command else {
        return Ok((Cow::Borrowed(command), None));
    };
    if !commands::rule_applies(w, commands::PARENT_HASH, position) {
        return Ok((Cow::Borrowed(command), None));
    }
    for field in &PARENT_HASH_FIELDS[1..] {
        ensure!(
            op.data.get(field).is_none(),
            "{field} is computed by the server"
        );
    }
    let Some(sent) = op.data.get("parent_hash") else {
        return Ok((Cow::Borrowed(command), None));
    };
    let mut op = op.clone();
    op.data.as_object_mut().map(|m| m.remove("parent_hash"));
    Ok((Cow::Owned(Command::SaveObject(op)), Some(sent.clone())))
}

/// Refuse a confirmation that names no live parent or another hash than its current one. A
/// replay takes the stored value as the live write checked it.
pub(crate) fn check_confirmation(w: &Workspace, sent: &Value, parent: Option<&str>) -> Result<()> {
    if w.replaying {
        return Ok(());
    }
    let live = parent.filter(|p| w.objects.get(*p).is_some_and(|o| o.current().present));
    let current = live.and_then(|p| contract_hash(w, p));
    ensure!(
        current.is_some() && sent.as_str() == current.as_deref(),
        "parent_hash does not match the current parent contract"
    );
    Ok(())
}

/// The stamp of a new revision: the current hash of a new or moved parent, of a new contract
/// or of a confirmation, else the stamp of the old revision.
pub(crate) fn stamp(
    w: &Workspace,
    kind: &str,
    old: Option<&Object>,
    (parent, data): (Option<&str>, &Value),
    confirmed: bool,
) -> Option<String> {
    let current = || parent.and_then(|p| contract_hash(w, p));
    let Some(old) = old.map(|o| o.current()) else {
        return current();
    };
    let types = schema(w).types;
    let moved = old.parent.as_deref() != parent;
    let rewritten = contract(w, &types, kind, data) != contract(w, &types, kind, &old.data);
    if moved || rewritten || confirmed {
        current()
    } else {
        old.parent_hash.clone()
    }
}

/// Add to `changed` each object that names a changed source in a reference field: a new title,
/// body or file of the source gives the object a new contract hash.
pub fn source_users(state: &Workspace, changed: &mut BTreeSet<String>) {
    let types = schema(state).types;
    let is_source = |kind: &str| types.get(kind).is_some_and(|t| t.source);
    let sources: BTreeSet<&str> = changed
        .iter()
        .filter(|k| state.objects.get(*k).is_some_and(|o| is_source(&o.kind)))
        .map(String::as_str)
        .collect();
    if sources.is_empty() {
        return;
    }
    let mut users = vec![];
    for (key, o) in &state.objects {
        let Some(t) = types.get(&o.kind) else {
            continue;
        };
        let data = &o.current().data;
        let names = t.references.iter().filter(|(_, r)| is_source(&r.target));
        let named = names.flat_map(|(field, r)| {
            let values = match &data[field] {
                Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                Value::String(text) => vec![text.as_str()],
                _ => vec![],
            };
            values
                .into_iter()
                .map(move |text| match text.contains(':') {
                    true => text.to_owned(),
                    false => format!("{}:{text}", r.target),
                })
        });
        if named
            .map(|a| resolve_address(state, &a).to_owned())
            .any(|a| sources.contains(a.as_str()))
        {
            users.push(key.clone());
        }
    }
    changed.extend(users);
}

#[cfg(test)]
#[path = "objects_parent_hash_tests.rs"]
mod tests;
