//! Part of `objects`: the contract revision that the replay and the remote pins compare, the
//! work readiness, the tree validation and the title length.
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
        && before.relations == after.relations
        && before.link_form() == after.link_form()
        && before.present == after.present
        && without(&before.data) == without(&after.data)
}

/// The revision `after` changes only flow keys of `before`. A flow is no part of the contract,
/// so a pin of the node stays current. Before relations, no revision has a flow.
pub fn flow_only_change(before: &ObjectRevision, after: &ObjectRevision) -> bool {
    let keys = before.relations.keys().chain(after.relations.keys());
    let mut changed = keys.filter(|k| before.relations.get(*k) != after.relations.get(*k));
    before.data == after.data
        && before.parent == after.parent
        && before.present == after.present
        && before.relations != after.relations
        && changed.all(|k| crate::relations::flow(k))
}

/// True when the node and each ancestor are present. The depends_on edges of a node are
/// decided at read time by the test status of their criteria (dependencies::Graph).
pub fn work_ready(w: &Workspace, key: &str) -> bool {
    let mut current = Some(key);
    let mut seen = BTreeSet::new();
    while let Some(key) = current {
        let key = resolve_address(w, key);
        let Some(o) = w.objects.get(key).filter(|o| o.current().present) else {
            return false;
        };
        if !seen.insert(key) {
            return false;
        }
        current = o.current().parent.as_deref();
    }
    true
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
        parent_valid(&types, w, &o.kind, o.current().parent.as_deref(), Some(key))?;
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
