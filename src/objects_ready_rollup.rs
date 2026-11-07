//! The readiness rollup. The status rule of the type decides, as for the tone:
//! - `children` and `links`: the node is ready only when every counted child or linking
//!   object is ready. The counted ones are the same as for the tone. A node without a counted
//!   one keeps its own readiness: a criterion without tests can be ready for work.
//! - Every other rule: the node keeps its own readiness.
//!
//! A node is ready when it waits for no dependency (`work_ready`). A rollup node stores
//! `work_ready` false when the rollup is not ready, so the `ready` filter
//! (node_filter_sql::READY) agrees with the rollup.
use super::tone_items::{Counted, rule_of, waits};
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn roll_ready(
    items: &mut [Value],
    counted: &Counted,
    types: &BTreeMap<String, objects::TreeType>,
) {
    let own: BTreeMap<String, (String, bool)> = items
        .iter()
        .filter(|i| i["present"] != false)
        .filter_map(|i| {
            let address = i["address"].as_str()?.to_owned();
            Some((address, (i["kind"].as_str()?.to_owned(), !waits(i))))
        })
        .collect();
    let mut memo = BTreeMap::new();
    for address in own.keys() {
        ready_of(
            address,
            &own,
            counted,
            types,
            &mut memo,
            &mut BTreeSet::new(),
        );
    }
    for item in items.iter_mut() {
        let Some(address) = item["address"].as_str() else {
            continue;
        };
        let Some((kind, _)) = own.get(address) else {
            continue;
        };
        if matches!(rule_of(types, kind), "children" | "links") && memo.get(address) == Some(&false)
        {
            item["work_ready"] = json!(false);
        }
    }
}

/// The readiness of one node with its rollup. A cycle through links is not ready.
fn ready_of(
    address: &str,
    own: &BTreeMap<String, (String, bool)>,
    counted: &Counted,
    types: &BTreeMap<String, objects::TreeType>,
    memo: &mut BTreeMap<String, bool>,
    seen: &mut BTreeSet<String>,
) -> bool {
    if let Some(ready) = memo.get(address) {
        return *ready;
    }
    let Some((kind, ready)) = own.get(address) else {
        return false;
    };
    if !seen.insert(address.to_owned()) {
        return false;
    }
    let rule = rule_of(types, kind);
    let below: Vec<String> = match rule {
        "children" => counted.children.get(address).cloned().unwrap_or_default(),
        "links" => counted
            .linked
            .get(address)
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let below: Vec<String> = below
        .into_iter()
        .filter(|c| counted.tones.get(c).is_some_and(Option::is_some))
        .collect();
    let rolled = below
        .iter()
        .all(|c| ready_of(c, own, counted, types, memo, seen));
    let result = *ready && rolled;
    memo.insert(address.to_owned(), result);
    result
}
