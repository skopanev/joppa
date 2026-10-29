//! The rule tone of one node: tests, unset or children.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// The tone of a node by the status rule of its type, or None for the links and none rules.
pub(super) fn tone(
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
                // A board value that does not apply to the criterion: no tone counts.
                Some("n/a") => "none",
                _ => "grey",
            }
            .to_owned(),
        ),
        // A type without a status rule shows that its rule is not set.
        "unset" => Some("orange".into()),
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
