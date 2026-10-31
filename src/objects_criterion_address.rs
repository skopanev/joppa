//! Part of `objects`: `criterion_address` and the items after it, moved from objects.rs unchanged.
use super::*;

pub fn criterion_address(key: &str) -> String {
    if key.contains(':') {
        key.into()
    } else {
        address("ac", key)
    }
}

pub(crate) fn process_key(w: &Workspace, kind: &str, id: &str, legacy_kind: &str) -> String {
    let key = address(kind, id);
    if w.documents_migrated {
        if legacy_kind == "requirement" {
            if let Some(existing) = requirement_key(w, &key) {
                return existing.into();
            }
        } else if let Some((_, _, criterion)) = criterion_record_context(w, &key) {
            return criterion.id.clone();
        }
        return key;
    }
    if kind == legacy_kind { id.into() } else { key }
}

pub(crate) fn requirement_key<'a>(w: &'a Workspace, key: &str) -> Option<&'a str> {
    let key = resolve_address(w, key);
    let obj = w.objects.get(key)?;
    w.requirements
        .get_key_value(key)
        .or_else(|| {
            (obj.kind == "requirement")
                .then(|| w.requirements.get_key_value(&obj.id))
                .flatten()
        })
        .map(|(key, _)| key.as_str())
}

pub(crate) fn document_adapter_fields(
    w: &Workspace,
    kind: &str,
) -> Option<&'static [&'static str]> {
    let configured = schema(w);
    let role = configured.types.get(kind).map(|t| t.role.as_str());
    if w.documents_migrated && role == Some("requirement") {
        return Some(&["title", "body", "owner"]);
    }
    let fields = native_document_fields(match role {
        Some(role @ ("requirement" | "ac")) => role,
        _ => kind,
    });
    if w.body_migrated {
        return fields.map(crate::body_migration::body_fields);
    }
    fields
}

pub(crate) fn document_rows(w: &Workspace) -> BTreeMap<String, (String, Option<String>, Value)> {
    let mut rows = BTreeMap::new();
    // After migrate_body the typed description and contract are data.body.
    let text = crate::body_migration::text_field(w, "description");
    let contract = crate::body_migration::text_field(w, "contract");
    let mut put = |kind: &str, id: &str, parent: Option<String>, data: Value| {
        rows.insert(address(kind, id), (kind.into(), parent, data));
    };
    for (id, d) in &w.domains {
        put(
            "domain",
            id,
            None,
            json!({"title":d.title,"owner":d.owner,(text):d.description,"archived":d.archived}),
        );
    }
    for (id, c) in &w.capabilities {
        put(
            "capability",
            id,
            c.domain.as_deref().map(|id| address("domain", id)),
            json!({"title":c.title,"owner":c.owner,(text):c.description}),
        );
    }
    for (id, c) in &w.consumers {
        put(
            "consumer",
            id,
            None,
            json!({"title":c.title,"owner":c.owner,(text):c.description}),
        );
    }
    for (id, p) in &w.projects {
        put(
            "project",
            id,
            None,
            json!({"title":id,"repository":p,"owner":w.admin}),
        );
    }
    for (id, q) in &w.requirements {
        if let Some(r) = q.revisions.last() {
            let req_address = requirement_address(id);
            let (req_kind, req_id) = req_address.split_once(':').unwrap();
            let mut data =
                json!({"title":r.title,"body":r.body,"owner":r.owner,"consumers":r.consumers});
            if w.documents_migrated && r.consumers.is_empty() {
                data.as_object_mut().unwrap().remove("consumers");
            }
            put(
                req_kind,
                req_id,
                q.capability.as_deref().map(|id| address("capability", id)),
                data,
            );
            for ac in &r.acs {
                let ac_address = criterion_address(&ac.id);
                let (ac_kind, ac_id) = ac_address.split_once(':').unwrap();
                put(
                    ac_kind,
                    ac_id,
                    Some(req_address.clone()),
                    json!({"title":ac.text,(text):ac.description,"owner":ac.owner.as_ref().unwrap_or(&r.owner),"decision_required":ac.owner.is_some(),"depends_on":ac.depends_on,"before_launch":ac.before_launch}),
                );
            }
            for (tid, t) in &r.tasks {
                let mut data = json!({"title":t.title,"owner":r.owner,(contract):t.contract,"projects":t.projects,"purpose":t.purpose});
                let parent = if t.acs.len() == 1 {
                    criterion_address(&t.acs[0])
                } else {
                    // Preserve old bindings without permitting new Tasks with multiple ACs.
                    data["legacy_acs"] = json!(t.acs);
                    requirement_address(id)
                };
                put("task", &format!("{id}/{tid}"), Some(parent), data);
            }
        }
    }
    for (id, i) in &w.intake {
        put(
            "intake",
            id,
            None,
            json!({"title":id,"body":i.said,"owner":i.owner.as_ref().unwrap_or(&w.admin),"author":i.author}),
        );
        for (pid, p) in &i.points {
            put(
                "point",
                &format!("{id}/{pid}"),
                Some(address("intake", id)),
                json!({"title":p.said,"owner":i.owner.as_ref().unwrap_or(&w.admin),"kind":p.kind,"answers":p.answers,"outcome":p.outcome,"reason":p.reason,"requirement":p.requirement}),
            );
        }
    }
    rows
}

/// Replay old commands into the same document ledger. Process evidence stays in its original revision.
pub fn sync(w: &mut Workspace, actor: &str, command: &Command, position: u64, at: &str) {
    sync_with(w, actor, command, position, at, true, None);
}
