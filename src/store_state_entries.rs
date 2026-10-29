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

pub(crate) fn affected(
    state: &Workspace,
    previous: Option<&Workspace>,
) -> Result<Option<std::collections::BTreeSet<String>>> {
    use std::collections::{BTreeSet, VecDeque};
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
    for (key, o) in &state.objects {
        if previous.objects.get(key).is_none_or(|old| {
            serde_json::to_value(old.current()).ok() != serde_json::to_value(o.current()).ok()
        }) {
            changed.insert(key.clone());
        }
    }
    for (req, q) in &state.requirements {
        if serde_json::to_value(q)?
            != previous
                .requirements
                .get(req)
                .map(serde_json::to_value)
                .transpose()?
                .unwrap_or(Value::Null)
        {
            changed.insert(crate::objects::requirement_address(req));
            for w in [previous, state] {
                for key in w.objects.keys() {
                    if crate::objects::verification_context(w, key)
                        .is_some_and(|(context, _)| context == req)
                    {
                        changed.insert(key.clone());
                    }
                }
            }
        }
    }
    for key in state
        .signal_assessments
        .keys()
        .chain(previous.signal_assessments.keys())
    {
        if state.signal_assessments.get(key) != previous.signal_assessments.get(key) {
            changed.insert(key.clone());
        }
    }
    for (key, acceptances) in &state.acceptances {
        if previous.acceptances.get(key) != Some(acceptances) {
            changed.insert(key.clone());
        }
    }
    for (key, t) in &state.task_process {
        if Some(serde_json::to_value(t)?)
            != previous
                .task_process
                .get(key)
                .map(serde_json::to_value)
                .transpose()?
        {
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
    for (key, notes) in &state.notes {
        if Some(serde_json::to_value(notes)?)
            != previous
                .notes
                .get(key)
                .map(serde_json::to_value)
                .transpose()?
        {
            changed.insert(crate::objects::resolve_address(state, key).to_owned());
        }
    }
    let mut incoming: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for w in [previous, state] {
        for (key, o) in &w.objects {
            let r = o.current();
            let mut targets: Vec<String> = r
                .parent
                .iter()
                .cloned()
                .chain(
                    r.links
                        .iter()
                        .filter(|l| l.relation == "depends_on")
                        .map(|l| l.target.clone()),
                )
                .collect();
            if let Some(deps) = r.data["depends_on"].as_array() {
                targets.extend(
                    deps.iter()
                        .filter_map(|v| v.as_str())
                        .map(crate::objects::criterion_address),
                );
            }
            for target in targets {
                incoming
                    .entry(crate::objects::resolve_address(w, &target).to_owned())
                    .or_default()
                    .insert(key.clone());
            }
        }
    }
    let mut queue: VecDeque<String> = changed.iter().cloned().collect();
    while let Some(key) = queue.pop_front() {
        for source in incoming.get(&key).into_iter().flatten() {
            if changed.insert(source.clone()) {
                queue.push_back(source.clone());
            }
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
                    value["revisions"] = json!([value["revisions"]
                        .as_array()
                        .and_then(|r| r.last())
                        .context("state entry has no revision")?]);
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

pub(crate) fn metadata(state: &Workspace) -> Result<String> {
    Ok(entries(state)?
        .remove(&("meta".into(), String::new()))
        .unwrap()
        .to_string())
}
