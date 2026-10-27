//! Row decoration: the status tone of each node.
use super::*;

/// The tone of each node from the status rule of its type: tests takes the test status of
/// the criterion, children takes the first status in the type's order that a live direct
/// child has, links takes the first status in that order that a live object linking to the
/// node has, and none keeps the process tone. A node with no counted child or link is grey.
/// A retired Task is not counted, and a process tone outside red, grey and green counts as
/// grey. The rule sets only the tone; the process label stays.
pub(super) async fn add_status_tones(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
    types: &std::collections::BTreeMap<String, objects::TreeType>,
    value: Option<(&str, &str)>,
) -> Result<()> {
    crate::probe::count(&crate::probe::ROLLUP);
    use std::collections::{BTreeMap, BTreeSet};
    let rule = |kind: &str| types.get(kind).map_or("none", objects::status_rule);
    let ruled: Vec<(String, String)> = items
        .iter()
        .filter(|i| i["present"] != false)
        .filter_map(|i| {
            Some((
                i["address"].as_str()?.to_owned(),
                i["kind"].as_str()?.to_owned(),
            ))
        })
        .filter(|(_, kind)| rule(kind) != "none")
        .collect();
    if ruled.is_empty() {
        return Ok(());
    }
    let children_kinds: Vec<String> = types
        .iter()
        .filter(|(_, t)| objects::status_rule(t) == "children")
        .map(|(kind, _)| kind.clone())
        .collect();
    // The live objects that link to each links node. Retired Tasks are history and do not count.
    struct Source {
        address: String,
        kind: String,
        done: bool,
        parent: Option<(String, String)>,
        tone: String,
    }
    let link_targets: Vec<String> = ruled
        .iter()
        .filter(|(_, kind)| rule(kind) == "links")
        .map(|(address, _)| address.clone())
        .collect();
    let mut linked = BTreeMap::<String, Vec<Source>>::new();
    if !link_targets.is_empty() {
        for row in client
            .query(
                r#"
        SELECT DISTINCT COALESCE(a.target,l.target),o.address,o.kind,
            COALESCE(o.process->>'status','')='done',o.parent,p.kind,
            COALESCE(o.signal->>'tone','grey')
        FROM joppa.object_links l
        JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source AND o.present
        LEFT JOIN joppa.objects p ON p.workspace=o.workspace AND p.address=o.parent
        LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target
        WHERE l.workspace=$1 AND COALESCE(a.target,l.target)=ANY($2)
            AND COALESCE(o.process->>'status','')<>'retired'
    "#,
                &[&workspace, &link_targets],
            )
            .await?
        {
            let parent = row
                .get::<_, Option<String>>(4)
                .zip(row.get::<_, Option<String>>(5));
            linked.entry(row.get(0)).or_default().push(Source {
                address: row.get(1),
                kind: row.get(2),
                done: row.get(3),
                parent,
                tone: row.get(6),
            });
        }
    }
    let sources = || linked.values().flatten();
    let roots: Vec<String> = ruled
        .iter()
        .filter(|(_, kind)| rule(kind) == "children")
        .map(|(address, _)| address.clone())
        .chain(
            sources()
                .filter(|s| rule(&s.kind) == "children")
                .map(|s| s.address.clone()),
        )
        .collect();
    // The live subtree below each children node, down through children nodes only.
    let mut below = BTreeMap::<String, Vec<(String, String)>>::new();
    if !roots.is_empty() {
        for row in client
            .query(
                r#"
        WITH RECURSIVE sub(address,kind,parent) AS (
            SELECT c.address,c.kind,c.parent FROM joppa.objects c
            WHERE c.workspace=$1 AND c.parent=ANY($2) AND c.present
            UNION
            SELECT c.address,c.kind,c.parent FROM sub
            JOIN joppa.objects c ON c.workspace=$1 AND c.parent=sub.address AND c.present
            WHERE sub.kind=ANY($3)
        )
        SELECT address,kind,parent FROM sub
    "#,
                &[&workspace, &roots, &children_kinds],
            )
            .await?
        {
            below
                .entry(row.get(2))
                .or_default()
                .push((row.get(0), row.get(1)));
        }
    }
    // A done Task that links to a links node takes the test status of its parent criterion.
    let task_role = |kind: &str| types.get(kind).is_some_and(|t| t.role == "task");
    let criteria: BTreeSet<String> = ruled
        .iter()
        .cloned()
        .chain(below.values().flatten().cloned())
        .chain(sources().map(|s| (s.address.clone(), s.kind.clone())))
        .chain(
            sources()
                .filter(|s| s.done && task_role(&s.kind))
                .filter_map(|s| s.parent.clone()),
        )
        .filter(|(_, kind)| rule(kind) == "tests")
        .map(|(address, _)| address)
        .collect();
    let statuses = if criteria.is_empty() {
        BTreeMap::new()
    } else {
        super::reports::criterion_test_statuses(
            client,
            workspace,
            &criteria.into_iter().collect::<Vec<_>>(),
            &BTreeSet::new(),
        )
        .await?
    };
    // A board value replaces each criterion status with its status for that value.
    let statuses: BTreeMap<String, Value> = match value {
        None => statuses,
        Some((field, v)) => statuses
            .into_iter()
            .map(|(a, mut s)| match s["values"][field][v].take() {
                Value::Null => (a, s),
                mut one => {
                    one["tests_revision"] = s["tests_revision"].take();
                    (a, one)
                }
            })
            .collect(),
    };
    fn tone(
        address: &str,
        kind: &str,
        types: &std::collections::BTreeMap<String, objects::TreeType>,
        below: &BTreeMap<String, Vec<(String, String)>>,
        statuses: &BTreeMap<String, Value>,
        seen: &mut BTreeSet<String>,
    ) -> Option<String> {
        let t = types.get(kind)?;
        match objects::status_rule(t) {
            "tests" => Some(
                match statuses.get(address).and_then(|s| s["status"].as_str()) {
                    Some("green") => "green",
                    Some("red") => "red",
                    _ => "grey",
                }
                .to_owned(),
            ),
            "children" => {
                if !seen.insert(address.to_owned()) {
                    return Some("grey".into());
                }
                let found: BTreeSet<String> = below
                    .get(address)
                    .into_iter()
                    .flatten()
                    .filter_map(|(child, kind)| tone(child, kind, types, below, statuses, seen))
                    .collect();
                Some(
                    objects::status_order(t)
                        .into_iter()
                        .find(|s| found.contains(s))
                        .unwrap_or_else(|| "grey".into()),
                )
            }
            _ => None,
        }
    }
    for item in items.iter_mut() {
        let (Some(address), Some(kind)) = (item["address"].as_str(), item["kind"].as_str()) else {
            continue;
        };
        if item["present"] == false {
            continue;
        }
        let ruled_tone = if rule(kind) == "links" {
            let found: BTreeSet<String> = linked
                .get(address)
                .into_iter()
                .flatten()
                .map(|s| {
                    let parent = s
                        .parent
                        .as_ref()
                        .filter(|(_, k)| s.done && task_role(&s.kind) && rule(k) == "tests")
                        .and_then(|(p, _)| statuses.get(p))
                        .filter(|status| !status["tests_revision"].is_null());
                    if let Some(status) = parent {
                        match status["status"].as_str() {
                            Some("green") => "green",
                            Some("red") => "red",
                            _ => "grey",
                        }
                        .to_owned()
                    } else {
                        tone(
                            &s.address,
                            &s.kind,
                            types,
                            &below,
                            &statuses,
                            &mut BTreeSet::new(),
                        )
                        // A process tone outside red, grey and green, such as yellow, counts as grey.
                        .unwrap_or_else(|| {
                            if objects::STATUS_TONES.contains(&s.tone.as_str()) {
                                s.tone.clone()
                            } else {
                                "grey".into()
                            }
                        })
                    }
                })
                .collect();
            types.get(kind).map(|t| {
                objects::status_order(t)
                    .into_iter()
                    .find(|s| found.contains(s))
                    .unwrap_or_else(|| "grey".into())
            })
        } else {
            tone(
                address,
                kind,
                types,
                &below,
                &statuses,
                &mut BTreeSet::new(),
            )
        };
        if let Some(t) = ruled_tone {
            item["signal"]["tone"] = json!(t);
        }
    }
    Ok(())
}
