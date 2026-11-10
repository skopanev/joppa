//! The test nodes of each result of a record. A result names its test by the address of a
//! present test node, such as `test:<id>`, or by the public item ID of that node: it binds that
//! one node. Else a title binds every present test node whose current or earlier title it is,
//! and the record stores one row for each of them under its address. Else, when the schema sets
//! test_results.match_field, the one present test node whose data has the name as the value of
//! that field binds it. Two such nodes leave the name unbound, and no name trims its spaces.
//! A name with no present test node is unbound:
//! the record stores its row under the name, so a test node that gets that title later counts
//! it. Each lookup is one index probe for each distinct name, so a record costs its own names
//! and not the whole tree.
use super::*;
use crate::ac_tests::TestResult;
use std::collections::{BTreeMap, BTreeSet};

/// Replace the name of each result with the address of each of its test nodes. Returns the rows
/// to store in record order and the unbound names in name order.
pub(super) async fn bind(
    client: &impl GenericClient,
    workspace: &str,
    results: Vec<TestResult>,
) -> Result<(Vec<TestResult>, Vec<String>)> {
    // The types with the role test in the latest schema revision.
    let kinds: Vec<String> = client
        .query(
            "SELECT t.key FROM (SELECT value FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1) s, jsonb_each(s.value->'types') t WHERE t.value->>'role'=$2",
            &[&workspace, &crate::test_nodes::ROLE],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let names: Vec<String> = results
        .iter()
        .map(|r| r.address.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // A public item ID gives the address of its object. Another name is its own address.
    let mut wanted: BTreeMap<String, String> =
        names.iter().map(|n| (n.clone(), n.clone())).collect();
    let ids: Vec<&String> = names
        .iter()
        .filter(|n| super::public_ids::public_item_id(n))
        .collect();
    if !ids.is_empty() {
        for row in client
            .query(
                "SELECT k.id,i.kind||':'||i.local_id FROM unnest($2::text[]) k(id) JOIN joppa.item_ids i ON i.workspace=$1 AND i.id=joppa.short_item_id(k.id)",
                &[&workspace, &ids],
            )
            .await?
        {
            wanted.insert(row.get(0), row.get(1));
        }
    }
    let addresses: Vec<&String> = wanted.values().collect();
    let present: BTreeSet<String> = client
        .query(
            "SELECT o.address FROM unnest($2::text[]) a JOIN joppa.objects o ON o.workspace=$1 AND o.address=a AND o.present AND o.kind=ANY($3)",
            &[&workspace, &addresses, &kinds],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    // The present test nodes with each name as the title.
    let mut titled: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in client
        .query(
            "SELECT o.title,o.address FROM unnest($2::text[]) n JOIN joppa.objects o ON o.workspace=$1 AND o.title=n AND o.present AND o.kind=ANY($3) ORDER BY o.address",
            &[&workspace, &names, &kinds],
        )
        .await?
    {
        titled.entry(row.get(0)).or_default().push(row.get(1));
    }
    // The present test nodes with each name as an earlier title, from the GIN index of the
    // section former_test_titles.
    for row in client
        .query(
            "SELECT n,o.address FROM unnest($2::text[]) n JOIN joppa.state_entries s ON s.section='former_test_titles' AND s.value @> jsonb_build_array(n) AND s.workspace=$1 JOIN joppa.objects o ON o.workspace=$1 AND o.address=s.key AND o.present AND o.kind=ANY($3)",
            &[&workspace, &names, &kinds],
        )
        .await?
    {
        let nodes = titled.entry(row.get(0)).or_default();
        let address: String = row.get(1);
        if !nodes.contains(&address) {
            nodes.push(address);
            nodes.sort();
        }
    }
    let left: Vec<&String> = names
        .iter()
        .filter(|n| !present.contains(&wanted[*n]) && !titled.contains_key(*n))
        .collect();
    let matched = matched(client, workspace, &kinds, &left).await?;
    // An address or a public item ID binds one node. A title binds every node with it. Then
    // the value of match_field binds the one node that has it.
    let mut bound: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut unbound = vec![];
    for name in &names {
        if present.contains(&wanted[name]) {
            bound.insert(name.as_str(), vec![wanted[name].clone()]);
        } else if let Some(nodes) = titled.remove(name) {
            bound.insert(name.as_str(), nodes);
        } else if let Some([node]) = matched.get(name).map(Vec::as_slice) {
            bound.insert(name.as_str(), vec![node.clone()]);
        } else {
            unbound.push(name.clone());
        }
    }
    // A bound result gives one row for each of its nodes. An unbound result keeps its name, so a
    // test node that gets that title later counts it.
    let results = results
        .into_iter()
        .flat_map(|result| match bound.get(result.address.as_str()) {
            Some(nodes) => nodes
                .iter()
                .map(|address| TestResult {
                    address: address.clone(),
                    ..result.clone()
                })
                .collect(),
            None => vec![result],
        })
        .collect();
    Ok((results, unbound))
}

/// The present test nodes for each name of `names` whose data has the name as the value of
/// test_results.match_field in the latest schema revision. Empty without match_field. The GIN
/// index of document->'data' serves the probe.
async fn matched(
    client: &impl GenericClient,
    workspace: &str,
    kinds: &[String],
    names: &[&String],
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if names.is_empty() {
        return Ok(out);
    }
    let field: Option<String> = client
        .query_opt(
            "SELECT value->'test_results'->>'match_field' FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1",
            &[&workspace],
        )
        .await?
        .and_then(|row| row.get(0));
    let Some(field) = field else {
        return Ok(out);
    };
    for row in client
        .query(
            "SELECT n,o.address FROM unnest($2::text[]) n JOIN joppa.objects o ON o.workspace=$1 AND o.document->'data' @> jsonb_build_object($4::text,n) AND o.present AND o.kind=ANY($3) ORDER BY o.address",
            &[&workspace, &names, &kinds, &field],
        )
        .await?
    {
        out.entry(row.get(0)).or_default().push(row.get(1));
    }
    Ok(out)
}
