//! Row decoration: the status tone of each node, from one rule for every type.
//!
//! The status rule of the type decides, never the kind name:
//! - `own` (a test node) and `tests`: the test status of the node, green, red or grey.
//! - `children`: the tones of the present direct children, `links`: the tones of the present
//!   objects that link to the node. The highest tone wins in the order red, orange, yellow,
//!   grey, green. A node without a counted child or link is orange.
//! - `none`: the process tone stays, and a parent does not count the node.
//! - A type without a rule is orange.
//!
//! A node that waits is at least yellow: it waits for a dependency (`work_ready` false), for a
//! review (`review_required`) or for the answer to its assessment question (state `waiting`).
//! An archived node is purple and does not count. A rule tone that differs from the process
//! tone sets the label.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// The tones of a rollup, the winning tone first.
pub const PRIORITY: [&str; 5] = ["red", "orange", "yellow", "grey", "green"];

fn highest<'a>(tones: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let found: BTreeSet<&str> = tones.into_iter().collect();
    PRIORITY.into_iter().find(|t| found.contains(t))
}

/// True when the node waits for a dependency, a review or the answer to its question.
pub(super) fn waits(item: &Value) -> bool {
    item["work_ready"] == false
        || item["review_required"] == true
        || item["process"]["assessment"]["state"] == "waiting"
}

pub(super) async fn add_status_tones(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
    types: &BTreeMap<String, objects::TreeType>,
) -> Result<()> {
    crate::probe::count(&crate::probe::ROLLUP);
    let rule = |kind: &str| rule_of(types, kind);
    let present: Vec<&Value> = items.iter().filter(|i| i["present"] != false).collect();
    let kinds: BTreeMap<String, String> = present
        .iter()
        .filter_map(|i| Some((i["address"].as_str()?.into(), i["kind"].as_str()?.into())))
        .collect();
    // An old address of a renamed node still names it as a parent or a link target.
    let aliases: BTreeMap<String, String> = client
        .query(
            "SELECT address,target FROM joppa.object_aliases WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let resolve = |a: &str| aliases.get(a).cloned().unwrap_or_else(|| a.to_owned());
    let mut children = BTreeMap::<String, Vec<String>>::new();
    for i in &present {
        if let (Some(a), Some(p)) = (i["address"].as_str(), i["parent"].as_str()) {
            children.entry(resolve(p)).or_default().push(a.to_owned());
        }
    }
    let mut linked = BTreeMap::<String, BTreeSet<String>>::new();
    for r in client
        .query(
            "SELECT source,target FROM joppa.object_links WHERE workspace=$1",
            &[&workspace],
        )
        .await?
    {
        let (source, target): (String, String) = (r.get(0), r.get(1));
        if kinds.contains_key(&source) {
            linked.entry(resolve(&target)).or_default().insert(source);
        }
    }
    let tested: Vec<String> = kinds
        .iter()
        .filter(|(_, k)| matches!(rule(k), "own" | "tests"))
        .map(|(a, _)| a.clone())
        .collect();
    let statuses =
        super::reports::criterion_test_statuses(client, workspace, &tested, &BTreeSet::new())
            .await?;
    let by_address: BTreeMap<String, &Value> = present
        .iter()
        .filter_map(|i| Some((i["address"].as_str()?.to_owned(), *i)))
        .collect();
    let mut tones = BTreeMap::<String, Option<&'static str>>::new();
    for address in kinds.keys() {
        tone_of(
            address,
            &kinds,
            types,
            &children,
            &linked,
            &statuses,
            &by_address,
            &mut tones,
            &mut BTreeSet::new(),
        );
    }
    for item in items.iter_mut() {
        let Some(address) = item["address"].as_str() else {
            continue;
        };
        let (Some(Some(t)), Some(kind)) = (tones.get(address).copied(), kinds.get(address)) else {
            continue;
        };
        if item["signal"]["tone"].as_str() != Some(t) {
            let counted = children.get(address).is_some_and(|c| !c.is_empty())
                || linked.get(address).is_some_and(|l| !l.is_empty());
            item["signal"]["label"] = json!(label(rule(kind), t, waits(item), counted));
        }
        item["signal"]["tone"] = json!(t);
    }
    for item in items.iter_mut().filter(|i| i["present"] == false) {
        item["signal"]["tone"] = json!(crate::status_colors::ARCHIVED_TONE);
        item["signal"]["label"] = json!("Archived");
    }
    Ok(())
}

/// The tone of one node, or None when a parent does not count it (rule none).
#[allow(clippy::too_many_arguments)]
fn tone_of(
    address: &str,
    kinds: &BTreeMap<String, String>,
    types: &BTreeMap<String, objects::TreeType>,
    children: &BTreeMap<String, Vec<String>>,
    linked: &BTreeMap<String, BTreeSet<String>>,
    statuses: &BTreeMap<String, Value>,
    items: &BTreeMap<String, &Value>,
    memo: &mut BTreeMap<String, Option<&'static str>>,
    seen: &mut BTreeSet<String>,
) -> Option<&'static str> {
    if let Some(t) = memo.get(address) {
        return *t;
    }
    let kind = kinds.get(address)?;
    let item = items.get(address)?;
    let rule = |kind: &str| rule_of(types, kind);
    // A cycle through links counts as grey.
    if !seen.insert(address.to_owned()) {
        return Some("grey");
    }
    let own = match rule(kind) {
        "own" | "tests" => Some(
            match statuses.get(address).and_then(|s| s["status"].as_str()) {
                Some("green") => "green",
                Some("red") => "red",
                _ => "grey",
            },
        ),
        "children" | "links" => {
            let counted: Vec<String> = if rule(kind) == "children" {
                children.get(address).cloned().unwrap_or_default()
            } else {
                linked.get(address).into_iter().flatten().cloned().collect()
            };
            let found: Vec<&str> = counted
                .iter()
                .filter_map(|c| {
                    tone_of(
                        c, kinds, types, children, linked, statuses, items, memo, seen,
                    )
                })
                .collect();
            Some(highest(found).unwrap_or("orange"))
        }
        "none" => None,
        _ => Some("orange"),
    };
    let own = own.map(|t| {
        if waits(item) {
            highest([t, "yellow"]).unwrap_or(t)
        } else {
            t
        }
    });
    memo.insert(address.to_owned(), own);
    own
}

/// The status rule of a type; a type outside the schema counts as none.
fn rule_of<'a>(types: &'a BTreeMap<String, objects::TreeType>, kind: &str) -> &'a str {
    types.get(kind).map_or("none", objects::status_rule)
}

/// The label of a rule tone.
fn label(rule: &str, tone: &str, waits: bool, counted: bool) -> &'static str {
    match (rule, tone) {
        (_, "yellow") if waits => "Waits for a dependency, a review or an answer",
        ("own" | "tests", "green") => "Checks passed",
        ("own" | "tests", "red") => "Checks failed",
        ("own" | "tests", _) => "Awaits checks",
        ("children", "green") => "Children checks passed",
        ("children", "red") => "Child checks failed",
        ("children", "orange") if !counted => "No children",
        ("children", "orange") => "A child has no tests",
        ("children", "yellow") => "A child waits",
        ("children", _) => "Children await checks",
        ("links", "green") => "Linked work passed",
        ("links", "red") => "Linked work failed",
        ("links", "orange") if !counted => "No linked work",
        ("links", "orange") => "Linked work has no tests",
        ("links", "yellow") => "Linked work waits",
        ("links", _) => "Linked work awaits checks",
        _ => "Status rule not set",
    }
}
