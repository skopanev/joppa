//! Part of `objects`: `contract_revision` and the items after it, moved from objects.rs unchanged.
use super::*;

/// The revision of the contract of an object: the current revision before the last
/// revisions that change only the test list.
pub fn contract_revision(o: &Object) -> u32 {
    contract_of(o.current())
}

pub(crate) fn contract_of(r: &ObjectRevision) -> u32 {
    match r.contract {
        Some([revision, contract]) if revision == r.revision => contract,
        _ => r.revision,
    }
}

/// The revision `after` changes only the field tests of `before`.
pub fn tests_only_change(before: &ObjectRevision, after: &ObjectRevision) -> bool {
    let without = |d: &Value| {
        let mut d = d.clone();
        if let Some(map) = d.as_object_mut() {
            map.remove("tests");
        }
        d
    };
    before.data.get("tests") != after.data.get("tests")
        && before.parent == after.parent
        && before.links == after.links
        && before.present == after.present
        && without(&before.data) == without(&after.data)
}

/// A Task type: its dependencies pin the target contract.
pub fn task_kind(w: &Workspace, kind: &str) -> bool {
    schema(w).types.get(kind).is_some_and(|t| t.role == "task")
}

/// The Task dependencies whose pinned revision is no longer current. The dependencies of
/// another node order work and pin nothing, so a stored revision there needs no review.
pub fn review_links(w: &Workspace, key: &str) -> Vec<Value> {
    w.objects.get(resolve_address(w, key)).filter(|o| task_kind(w, &o.kind)).map(|o|o.current().links.iter().filter_map(|link| {
        let current=w.objects.get(resolve_address(w, &link.target)).filter(|o|o.current().present).map(|o|o.current().revision);
        (link.relation=="depends_on" && !pin_current(w, &link.target, link.revision)).then(||json!({"target":link.target,"revision":link.revision,"current_revision":current}))
    }).collect()).unwrap_or_default()
}

/// Task dependency pins of the object and its ancestors. The ordering links of other nodes
/// wait at read time and in live writes, where the test status of their targets is known.
/// A criterion with a saved test list: its test status from reports decides a link to it.
pub fn report_decided(w: &Workspace, key: &str) -> bool {
    let key = resolve_address(w, key);
    w.objects.get(key).is_some_and(|o| {
        schema(w).types.get(&o.kind).is_some_and(|t| {
            if w.test_rules_migrated {
                status_rule(t) == "tests"
            } else {
                t.role == "ac"
            }
        })
    }) && crate::ac_tests::listed(w, key).is_some_and(|r| !r.tests.is_empty())
}

pub fn work_ready(w: &Workspace, key: &str) -> bool {
    fn visit(
        w: &Workspace,
        key: &str,
        types: &BTreeMap<String, TreeType>,
        stack: &mut BTreeSet<String>,
    ) -> bool {
        let key = resolve_address(w, key);
        let Some(o) = w.objects.get(resolve_address(w, key)) else {
            return false;
        };
        let task = types.get(&o.kind).is_some_and(|t| t.role == "task");
        let mut links = o.current().links.iter().filter(|l| task && l.blocking);
        if !o.current().present
            || links
                .clone()
                .any(|l| !pin_current(w, &l.target, l.revision))
            || !stack.insert(key.into())
        {
            return false;
        }
        let links_ready = links.all(|l| {
            let Some(target) = w.objects.get(resolve_address(w, &l.target)) else {
                return false;
            };
            // A Task target is ready when it is done. Any other target is decided at read
            // time by the test status of its criteria (dependencies::Graph).
            let target_key = address(&target.kind, &target.id);
            let evidence = !task_kind(w, &target.kind) || verified(w, &target_key);
            evidence && visit(w, &l.target, types, stack)
        });
        let parent_ready = o
            .current()
            .parent
            .as_deref()
            .is_none_or(|p| visit(w, p, types, stack));
        stack.remove(key);
        links_ready && parent_ready
    }
    visit(w, key, &schema(w).types, &mut BTreeSet::new())
}

/// The first direct blocking dependency that keeps the object out of work.
pub fn unready_dependency(w: &Workspace, key: &str) -> Option<String> {
    let o = w.objects.get(resolve_address(w, key))?;
    o.current()
        .links
        .iter()
        .filter(|l| l.blocking)
        .find(|l| {
            !pin_current(w, &l.target, l.revision)
                || w.objects
                    .get(resolve_address(w, &l.target))
                    .is_none_or(|t| task_kind(w, &t.kind) && !verified(w, &address(&t.kind, &t.id)))
                || !work_ready(w, &l.target)
        })
        .map(|l| resolve_address(w, &l.target).to_owned())
}

pub(crate) fn legacy_task_binding(w: &Workspace, obj: &Object) -> bool {
    obj.kind == "task"
        && obj.id.split_once('/').is_some_and(|(req, id)| {
            obj.current().parent.as_deref() == Some(&requirement_address(req))
                && w.requirements
                    .get(req)
                    .and_then(|q| q.revisions.last())
                    .and_then(|r| r.tasks.get(id))
                    .is_some_and(|t| {
                        t.acs.len() != 1 && obj.current().data["legacy_acs"] == json!(t.acs)
                    })
        })
}

/// New writes validate the active schema. Replay preserves earlier schema rules.
pub fn validate_tree(w: &Workspace) -> Result<()> {
    let types = schema(w).types;
    for (key, o) in w.objects.iter().filter(|(_, o)| o.current().present) {
        ensure!(
            types.contains_key(&o.kind),
            "object type is absent from the tree schema"
        );
        validate_required_fields(&types[&o.kind], &o.current().data)?;
        if !legacy_task_binding(w, o) {
            parent_valid(&types, w, &o.kind, o.current().parent.as_deref(), Some(key))?;
        }
        parent_cycle(w, key, o.current().parent.as_deref())?;
    }
    Ok(())
}

pub fn within(w: &Workspace, key: &str, target: &str) -> bool {
    let target = resolve_address(w, target);
    let mut current = Some(key);
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        let id = resolve_address(w, id);
        if id == target {
            return true;
        }
        if !seen.insert(id) {
            return false;
        }
        current = w
            .objects
            .get(resolve_address(w, id))
            .and_then(|o| o.current().parent.as_deref());
    }
    false
}

/// Default title length when a type does not set `title_preview_length`.
pub const TITLE_LENGTH_DEFAULT: u32 = 128;

/// A new title fits its row: it is at most the type's `title_preview_length`.
/// Only new writes are checked, so old journals with long titles still replay.
pub(crate) fn validate_title_length(w: &Workspace, cmd: &Command) -> Result<()> {
    let Command::SaveObject(crate::commands::SaveObjectOp { kind, data, .. }) = cmd else {
        return Ok(());
    };
    let Some(title) = data.get("title").and_then(Value::as_str) else {
        return Ok(());
    };
    let limit = schema(w)
        .types
        .get(kind)
        .and_then(|t| t.title_preview_length)
        .unwrap_or(TITLE_LENGTH_DEFAULT) as usize;
    let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    ensure!(
        title.chars().count() <= limit,
        "{article} {kind} title is at most {limit} characters; put the details in the text"
    );
    Ok(())
}
