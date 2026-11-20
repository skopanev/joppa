//! Part of the status projection: the values of the computed data fields (computed_fields.rs)
//! in `objects.computed`. The full rebuild stores the values of each node. A write stores the
//! values of the nodes that it reaches: the changed nodes, the nodes that link to them or that
//! they link to (also when a changed node is now absent), each node above these, and with a parent rule each node below. A reached node
//! reads the stored values of its other children, as the tones of the same delta do.
use super::projection::{store_column, store_rows};
use super::*;
use crate::computed_fields::{Graph, Memo, rules};
use crate::objects::resolve_address;
use std::collections::BTreeSet;

/// Store the computed values of each node of the workspace.
pub(in crate::store) async fn rebuild(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
) -> Result<()> {
    if rules(&crate::objects::schema(state).types).is_empty() {
        let sql =
            "UPDATE joppa.objects SET computed=NULL WHERE workspace=$1 AND computed IS NOT NULL";
        client.execute(sql, &[&workspace]).await?;
        return Ok(());
    }
    let graph = Graph::new(state);
    let mut memo = Memo::new();
    let rows: Vec<Value> = state
        .objects
        .keys()
        .filter_map(|a| Some(json!({"address":a,"value":graph.row(a, &mut memo)?})))
        .collect();
    store_column(client, workspace, "computed", &rows).await
}

/// Store the computed values of the nodes that the write of `dirty` reaches.
pub(in crate::store) async fn update(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    previous: &Workspace,
    dirty: &BTreeSet<String>,
) -> Result<()> {
    let rules = rules(&crate::objects::schema(state).types);
    if rules.is_empty() {
        return Ok(());
    }
    // A computed value reads only objects: their data, parents, links and presence. A write
    // that changes no object, such as a note or a record of results, changes no value.
    let same = |a: &String| match (state.objects.get(a), previous.objects.get(a)) {
        (Some(new), Some(old)) => std::sync::Arc::ptr_eq(new, old),
        (None, None) => true,
        _ => false,
    };
    if dirty.iter().all(same) {
        return Ok(());
    }
    let graph = Graph::new(state);
    let downward = rules
        .values()
        .flat_map(|kinds| kinds.values())
        .any(|rule| rule.source() == "parent");
    let reached = reach(&graph, state, previous, dirty, downward);
    let mut memo = stored(client, workspace, &graph, &reached).await?;
    let rows: Vec<Value> = reached
        .iter()
        .filter(|a| state.objects.contains_key(*a))
        .map(|a| json!({"address":a,"value":graph.row(a, &mut memo)}))
        .collect();
    store_rows(client, workspace, "computed", &rows).await
}

/// The stored values of the children of the reached nodes outside the reached set. The write
/// does not change them, so a reached node reads them and does not recompute the tree below.
async fn stored(
    client: &impl GenericClient,
    workspace: &str,
    graph: &Graph<'_>,
    reached: &BTreeSet<String>,
) -> Result<Memo> {
    let outside: BTreeSet<&str> = reached
        .iter()
        .flat_map(|a| graph.children(a))
        .copied()
        .filter(|c| !reached.contains(*c) && graph.fields(c).is_some())
        .collect();
    let mut memo = Memo::new();
    if outside.is_empty() {
        return Ok(memo);
    }
    let addresses: Vec<&str> = outside.into_iter().collect();
    let sql = "SELECT address,computed::text FROM joppa.objects WHERE workspace=$1 AND address=ANY($2) AND computed IS NOT NULL";
    crate::store::stream::each(client, sql, &[&workspace, &addresses], |row| {
        let address: String = row.get(0);
        let values: Value = serde_json::from_str(row.get(1))?;
        for field in graph.fields(&address).into_iter().flat_map(|f| f.keys()) {
            let value = values.get(field).filter(|v| !v.is_null()).cloned();
            memo.insert((field.clone(), address.clone()), value);
        }
        Ok(())
    })
    .await?;
    Ok(memo)
}

/// The changed nodes, their links before and after the write (also the present nodes that
/// link to a changed node that is now absent), each node above these, and
/// with `downward` each present node below them.
fn reach(
    graph: &Graph,
    state: &Workspace,
    previous: &Workspace,
    dirty: &BTreeSet<String>,
    downward: bool,
) -> BTreeSet<String> {
    let mut set = dirty.clone();
    for address in dirty {
        let linked = graph.linked.get(address.as_str()).into_iter().flatten();
        set.extend(linked.map(|a| a.to_string()));
        // An archived or removed node: the nodes that still link to it read it no longer.
        set.extend(
            graph
                .linked_absent
                .get(address.as_str())
                .into_iter()
                .flatten()
                .map(|a| a.to_string()),
        );
        if let Some(o) = previous.objects.get(address) {
            let targets = o.current().counted();
            set.extend(targets.map(|e| resolve_address(previous, &e.to).to_owned()));
        }
    }
    let mut above = BTreeSet::new();
    for address in &set {
        for w in [state, previous] {
            above.extend(ancestors(w, address));
        }
    }
    set.extend(above);
    if downward {
        let mut queue: Vec<String> = set.iter().cloned().collect();
        while let Some(address) = queue.pop() {
            for child in graph.children(&address) {
                if set.insert(child.to_string()) {
                    queue.push(child.to_string());
                }
            }
        }
    }
    set
}

/// The ancestors of a node in `w`, the parent first.
fn ancestors(w: &Workspace, address: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut next = w
        .objects
        .get(address)
        .and_then(|o| o.current().parent.clone());
    while let Some(parent) = next {
        let parent = resolve_address(w, &parent).to_owned();
        if parent == address || out.contains(&parent) {
            break;
        }
        next = w
            .objects
            .get(&parent)
            .and_then(|o| o.current().parent.clone());
        out.push(parent);
    }
    out
}
