//! Row decoration: the branch verification summaries and the Task verification.
use super::*;

/// Branch summaries and Task verification share one read of the criterion statuses.
pub(super) async fn add_verification_summaries(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
    types: &std::collections::BTreeMap<String, objects::TreeType>,
) -> Result<()> {
    crate::probe::count(&crate::probe::ROLLUP);
    use std::collections::{BTreeMap, BTreeSet};
    let task_parent = |item: &Value| {
        types
            .get(item["kind"].as_str()?)
            .filter(|t| t.role == "task" && item["process"].is_object())?;
        item["parent"]
            .as_str()
            .or(item["document"]["parent"].as_str())
            .map(String::from)
    };
    let task_parents: BTreeSet<String> = items.iter().filter_map(task_parent).collect();
    let criteria: Vec<String> = types
        .iter()
        .filter(|(_, t)| t.role == "ac")
        .map(|(k, _)| k.clone())
        .collect();
    let mut branches: BTreeSet<String> = criteria.iter().cloned().collect();
    loop {
        let before = branches.clone();
        for (kind, t) in types {
            if before.contains(kind) {
                for parent in &t.parents {
                    if parent == "*" {
                        branches.extend(types.keys().cloned());
                    } else {
                        branches.insert(parent.clone());
                    }
                }
            }
        }
        if branches == before {
            break;
        }
    }
    let addresses: Vec<String> = items
        .iter()
        .filter(|i| {
            i["present"] != false
                && i["kind"]
                    .as_str()
                    .is_some_and(|k| branches.contains(k) && !criteria.iter().any(|c| c == k))
        })
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect();
    if addresses.is_empty() && task_parents.is_empty() {
        return Ok(());
    }
    let branches: Vec<String> = branches.into_iter().collect();
    let rows = if addresses.is_empty() {
        vec![]
    } else {
        client
            .query(
                r#"
        WITH RECURSIVE descendants(root,child,address) AS (
            SELECT p.address,c.address,c.address FROM joppa.objects p
            JOIN joppa.objects c ON c.workspace=p.workspace AND c.parent=p.address AND c.present
            WHERE p.workspace=$1 AND p.address=ANY($2) AND p.present AND c.kind=ANY($3)
            UNION
            SELECT d.root,d.child,c.address FROM descendants d
            JOIN joppa.objects p ON p.workspace=$1 AND p.address=d.address
            JOIN joppa.objects c ON c.workspace=$1 AND c.parent=d.address AND c.present
            WHERE NOT p.kind=ANY($4) AND c.kind=ANY($3)
        )
        SELECT d.root,d.child,CASE WHEN c.kind=ANY($4) THEN c.address END
        FROM descendants d JOIN joppa.objects c ON c.workspace=$1 AND c.address=d.address
    "#,
                &[&workspace, &addresses, &branches, &criteria],
            )
            .await?
    };
    let mut children = BTreeMap::<String, BTreeMap<String, BTreeSet<String>>>::new();
    let mut targets = BTreeSet::new();
    for row in rows {
        let child = children
            .entry(row.get(0))
            .or_default()
            .entry(row.get(1))
            .or_default();
        if let Some(criterion) = row.get::<_, Option<String>>(2) {
            targets.insert(criterion.clone());
            child.insert(criterion);
        }
    }
    // A Task parent that no summary needs keeps the bound-run verdict when unreadable.
    let optional: BTreeSet<String> = task_parents.difference(&targets).cloned().collect();
    targets.extend(task_parents);
    let statuses = match super::reports::criterion_test_statuses(
        client,
        workspace,
        &targets.into_iter().collect::<Vec<_>>(),
        &optional,
    )
    .await
    {
        Ok(statuses) => statuses,
        // Without branch summaries, a Task keeps its bound-run verdict when no status is read.
        Err(_) if addresses.is_empty() => return Ok(()),
        Err(error) => return Err(error),
    };
    for item in items {
        // A saved test list made the criterion status the Task verification before
        // migrate_test_rules. After it a Task has no verification, so this changes nothing.
        if let Some(status) = task_parent(item)
            .filter(|_| item["process"].get("verification").is_some())
            .and_then(|parent| statuses.get(&parent))
            .filter(|s| !s["tests_revision"].is_null())
            && let Some(verification) = match status["status"].as_str() {
                Some("green") => Some("pass"),
                Some("red") => Some("fail"),
                Some("grey") => Some("unverified"),
                _ => None,
            }
        {
            // Earlier signal rules, such as retirement or review, keep their signal.
            let task_rule = objects::task_signal(&item["process"]);
            let decided =
                item["signal"]["tone"] == task_rule.0 && item["signal"]["label"] == task_rule.1;
            item["process"]["verification"] = json!(verification);
            if decided {
                let (tone, label) = objects::task_signal(&item["process"]);
                item["signal"]["tone"] = json!(tone);
                item["signal"]["label"] = json!(label);
            }
        }
        let Some(address) = item["address"]
            .as_str()
            .filter(|a| addresses.iter().any(|v| v == a))
        else {
            continue;
        };
        let mut total = 0;
        let mut passed = 0;
        let mut failed = 0;
        if let Some(children) = children.get(address) {
            for criteria in children.values() {
                total += 1;
                passed += usize::from(
                    !criteria.is_empty()
                        && criteria
                            .iter()
                            .all(|a| statuses.get(a).is_some_and(|s| s["status"] == "green")),
                );
                failed += usize::from(
                    criteria
                        .iter()
                        .any(|a| statuses.get(a).is_some_and(|s| s["status"] == "red")),
                );
            }
        }
        item["verification_summary"] = json!({"total":total,"passed":passed,"failed":failed});
    }
    Ok(())
}
