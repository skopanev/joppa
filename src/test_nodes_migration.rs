//! The event migrate_test_nodes: each test list becomes test nodes, see the module above.
use super::{REASON, ROLE, RULE, node_id};
use crate::model::Workspace;
use crate::objects::{Object, ObjectRevision, TreeType, contract_revision, schema, status_rule};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::collections::BTreeMap;

/// Give a type map the test type under each criterion type, unless a type with role test
/// exists, and give each criterion type the status children. Return the test type.
pub fn convert_types(types: &mut BTreeMap<String, TreeType>) -> Result<String> {
    let listing: Vec<String> = types
        .iter()
        .filter(|(_, t)| t.role == "ac" || status_rule(t) == "tests")
        .map(|(kind, _)| kind.clone())
        .collect();
    let kind = match types.iter().find(|(_, t)| t.role == ROLE) {
        Some((kind, _)) => kind.clone(),
        None => {
            let kind = ["test", "test_case", "test_node"]
                .into_iter()
                .find(|k| !types.contains_key(*k))
                .context("no free name for the test type")?;
            let t = json!({"title":"Tests","role":ROLE,"status":RULE,"parents":listing,
                "badge":"TEST","title_preview_length":400});
            types.insert(kind.into(), serde_json::from_value(t)?);
            kind.to_owned()
        }
    };
    for k in &listing {
        types.get_mut(k).expect("listed type").status = Some("children".into());
    }
    Ok(kind)
}

/// Apply migrate_test_nodes once. The system actor (commands::system_actor) is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.test_nodes_migrated {
        return Ok(());
    }
    ensure!(
        actor == crate::commands::system_actor(w),
        "only the server runs this migration"
    );
    let mut next = schema(w);
    let kind = convert_types(&mut next.types)?;
    next.revision += 1;
    next.actor = actor.into();
    next.at = at.into();
    next.position = position;
    next.reason = REASON.into();
    let schema_revision = next.revision;
    w.tree_schema.push(next);
    let lists: Vec<(String, Vec<String>)> = w
        .objects
        .iter()
        .filter(|(_, o)| o.current().present)
        .filter_map(|(key, o)| {
            let tests = o.current().data.get("tests")?.as_array()?;
            Some((
                key.clone(),
                tests
                    .iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect(),
            ))
        })
        .collect();
    for (key, tests) in lists {
        let o = w.objects.get_mut(&key).expect("listed object");
        let contract = contract_revision(o);
        let mut revision = o.current().clone();
        revision.data.as_object_mut().map(|d| d.remove("tests"));
        revision.revision += 1;
        revision.contract = Some([revision.revision, contract]);
        revision.schema_revision = schema_revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        let owner = revision.data["owner"].clone();
        o.revisions.push(revision);
        let boundary = w.criterion_boundaries.get(&key).cloned();
        for name in tests {
            let id = node_id(&key, &name);
            let address = crate::objects::address(&kind, &id);
            let node = ObjectRevision {
                revision: 1,
                schema_revision,
                parent: Some(key.clone()),
                data: json!({"title": name, "owner": owner}),
                relations: Default::default(),
                older_links: Default::default(),
                actor: actor.into(),
                alias: w.alias.clone(),
                at: at.into(),
                position,
                reason: REASON.into(),
                present: true,
                contract: None,
            };
            w.objects.insert(
                address.clone(),
                Object {
                    kind: kind.clone(),
                    id,
                    revisions: vec![node],
                    parent_override: false,
                },
            );
            if let Some(at) = &boundary {
                w.criterion_boundaries.insert(address, at.clone());
            }
        }
    }
    w.test_nodes_migrated = true;
    Ok(())
}
