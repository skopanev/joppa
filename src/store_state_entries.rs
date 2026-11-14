//! Part of `store_state`: `entries` and the items after it, moved from store_state.rs unchanged.
use super::*;

pub(crate) type Entries = BTreeMap<(String, String), Value>;

pub(crate) fn changed_summaries(
    state: &Workspace,
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .filter(|(req, q)| {
            selected.is_none_or(|s| {
                s.contains(&crate::objects::requirement_address(req))
                    || q.revisions.last().is_some_and(|r| {
                        r.acs
                            .iter()
                            .any(|ac| s.contains(&crate::objects::criterion_address(&ac.id)))
                    })
            })
        })
        .map(|(req, q)| {
            (
                req.clone(),
                crate::objects::requirement_summary(state, req, q.revisions.last().unwrap()),
            )
        })
        .collect();
    Value::Object(summaries)
}

/// The JSON of `new` differs from the JSON of `old`. Equal values have equal JSON, so only
/// unequal values are serialized.
pub(crate) fn differs<T: serde::Serialize + PartialEq>(new: &T, old: Option<&T>) -> Result<bool> {
    if old == Some(new) {
        return Ok(false);
    }
    crate::probe::count(&crate::probe::ROWS);
    Ok(Some(serde_json::to_value(new)?) != old.map(serde_json::to_value).transpose()?)
}

pub(crate) fn affected(
    state: &Workspace,
    previous: Option<&Workspace>,
) -> Result<Option<std::collections::BTreeSet<String>>> {
    use std::collections::BTreeSet;
    let Some(previous) = previous else {
        return Ok(None);
    };
    if serde_json::to_value(crate::objects::schema(state))?
        != serde_json::to_value(crate::objects::schema(previous))?
        || state.members != previous.members
        || state.admin != previous.admin
    {
        return Ok(None);
    }
    let mut changed = BTreeSet::new();
    // A write copies the workspace by pointer, so an object that it did not touch is the
    // same allocation and needs no comparison. The two maps go side by side in key order.
    // An object of `previous` alone is the only old object that the walk below reads: an
    // object with the same pointer has the same edges in both.
    let mut gone = vec![];
    let mut old = previous.objects.iter().peekable();
    for (key, o) in &state.objects {
        while let Some((k, b)) = old.next_if(|(k, _)| k.as_str() < key.as_str()) {
            gone.push((k.as_str(), b));
        }
        let before = old.next_if(|(k, _)| *k == key).map(|(_, b)| b);
        if before.is_some_and(|b| std::sync::Arc::ptr_eq(o, b)) {
            continue;
        }
        if differs(o.current(), before.map(|b| b.current()))? {
            changed.insert(key.clone());
        }
    }
    gone.extend(old.map(|(k, b)| (k.as_str(), b)));
    // A changed alias resolves the old edges otherwise: then every old object counts.
    if state.object_aliases != previous.object_aliases {
        gone = previous
            .objects
            .iter()
            .map(|(k, b)| (k.as_str(), b))
            .collect();
    }
    // Each object under a revised requirement changes: the walk below the changed keys
    // reaches each object whose verification context the requirement is.
    for (req, q) in &state.requirements {
        let old = previous.requirements.get(req);
        if !old.is_some_and(|old| std::sync::Arc::ptr_eq(q, old)) && differs(q, old)? {
            let address = crate::objects::requirement_address(req);
            for w in [previous, state] {
                changed.insert(crate::objects::resolve_address(w, &address).to_owned());
            }
            changed.insert(address);
        }
    }
    for (key, t) in &state.task_process {
        if differs(t, previous.task_process.get(key))? {
            changed.insert(key.clone());
        }
    }
    for key in state
        .planning_claims
        .keys()
        .chain(previous.planning_claims.keys())
    {
        if state.planning_claims.get(key) != previous.planning_claims.get(key) {
            changed.insert(key.clone());
        }
    }
    // A source reset gives a new boundary to criteria beside the changed node.
    let (new, old) = (&state.criterion_boundaries, &previous.criterion_boundaries);
    if !std::sync::Arc::ptr_eq(new, old) {
        for pair in crate::map_pairs::pairs(old, new) {
            if pair.old != pair.new {
                changed.insert(pair.key.to_owned());
            }
        }
    }
    for (key, notes) in &state.notes {
        let old = previous.notes.get(key);
        if !old.is_some_and(|old| std::sync::Arc::ptr_eq(notes, old)) && differs(notes, old)? {
            changed.insert(crate::objects::resolve_address(state, key).to_owned());
        }
    }
    // Each object that points to a changed key by its parent or a depends_on edge changes
    // too. One pass finds the objects that point to the changed keys, so a write of a leaf
    // needs no index. After a pass that adds objects, an index of the targets reaches the
    // rest of the tree.
    let scan = || {
        let now = state.objects.iter().map(|(k, o)| (state, k.as_str(), o));
        now.chain(gone.iter().map(|(k, o)| (previous, *k, *o)))
    };
    let mut added = vec![];
    for (w, key, o) in scan() {
        if !changed.contains(key) && targets(w, o).iter().any(|t| changed.contains(t.as_ref())) {
            added.push(key);
        }
    }
    if added.is_empty() {
        return Ok(Some(changed));
    }
    let mut incoming: BTreeMap<std::borrow::Cow<str>, Vec<&str>> = BTreeMap::new();
    for (w, key, o) in scan() {
        for target in targets(w, o) {
            incoming.entry(target).or_default().push(key);
        }
    }
    while let Some(key) = added.pop() {
        if changed.insert(key.to_owned()) {
            added.extend(incoming.get(key).into_iter().flatten());
        }
    }
    Ok(Some(changed))
}

pub(crate) fn entries(state: &Workspace) -> Result<Entries> {
    let value = serde_json::to_value(state)?;
    let mut entries = Entries::new();
    let mut meta = json!({});
    for (section, value) in value.as_object().unwrap() {
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                let mut value = value.clone();
                if ["objects", "requirements"].contains(&section.as_str()) {
                    crate::probe::count(&crate::probe::ROWS);
                    trim(&mut value)?;
                }
                entries.insert((section.clone(), key.clone()), value);
            }
        } else if section == "tree_schema" {
            meta[section] = json!(
                value
                    .as_array()
                    .unwrap()
                    .last()
                    .into_iter()
                    .collect::<Vec<_>>()
            );
        } else {
            meta[section] = value.clone();
        }
    }
    entries.insert(("meta".into(), String::new()), meta);
    Ok(entries)
}

/// Keep only the last revision of an object or a requirement row.
pub(crate) fn trim(value: &mut Value) -> Result<()> {
    value["revisions"] = json!([value["revisions"]
        .as_array()
        .and_then(|r| r.last())
        .context("state entry has no revision")?]);
    Ok(())
}

/// The keys that an object points to by its parent and its depends_on edges, also the legacy
/// list in data, by their current addresses.
fn targets<'a>(w: &'a Workspace, o: &'a crate::objects::Object) -> Vec<std::borrow::Cow<'a, str>> {
    let r = o.current();
    let edges = r.parent.iter().chain(r.depends_on().map(|e| &e.to));
    let mut out: Vec<std::borrow::Cow<str>> = edges
        .map(|t| crate::objects::resolve_address(w, t).into())
        .collect();
    let legacy = r.data["depends_on"].as_array().into_iter().flatten();
    for d in legacy.filter_map(|v| v.as_str()) {
        let target = crate::objects::criterion_address(d);
        out.push(
            crate::objects::resolve_address(w, &target)
                .to_owned()
                .into(),
        );
    }
    out
}
