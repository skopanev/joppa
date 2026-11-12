//! Part of the status projection: the tones of each combination of filter dimension values
//! (filter_dimensions.rs). Each write that stores the statuses also stores, for each node, the
//! dimension values that it applies to (`dims`) and its tone for each combination
//! (`dim_tones`). A combination counts only the results that count for it, and a node counts
//! only the children and the linking objects that apply to it. A read with a dimension value
//! in its data filter reads these tones (objects_dimension_reads.rs).
use super::projection::{base_row, store_column};
use super::tone_items::{Graph, graph, tone_of};
use super::*;
use crate::filter_dimensions as dims;
use std::collections::{BTreeMap, BTreeSet};

/// Store the dimension values and the combination tones of each node of the workspace.
pub(in crate::store) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
) -> Result<()> {
    let dimensions = dims::dimensions(state);
    if dimensions.is_empty() {
        let sql = "UPDATE joppa.objects SET dims=NULL,dim_tones=NULL WHERE workspace=$1 AND (dims IS NOT NULL OR dim_tones IS NOT NULL)";
        client.execute(sql, &[&workspace]).await?;
        return Ok(());
    }
    let effective = dims::effective(state, &dimensions);
    let combinations = dims::combinations(&dimensions);
    let types = crate::objects::schema(state).types;
    let rows = client
        .query("SELECT address,kind,signal::text,process::text,parent,present FROM joppa.objects WHERE workspace=$1 AND present", &[&workspace])
        .await?;
    let items: Vec<Value> = rows.iter().map(base_row).collect();
    let present: Vec<&Value> = items.iter().collect();
    let all = graph(client, workspace, &present).await?;
    let tested: Vec<String> = all
        .kinds
        .iter()
        .filter(|(_, k)| crate::manual_checks::reads_status(types.get(*k)))
        .map(|(a, _)| a.clone())
        .collect();
    let criteria = super::reports::criteria_of(state, &tested);
    let rules = crate::test_rules::rules(state)?;
    let (reports, results) = super::reports::results_of(client, workspace, &criteria).await?;
    let checks = crate::manual_checks::Checks::of(state, &tested);
    let mut tones: BTreeMap<String, BTreeMap<String, &'static str>> = BTreeMap::new();
    for combination in &combinations {
        let counted: Vec<_> = results
            .iter()
            .filter(|(_, r)| dims::counts(r.fields.as_ref(), combination))
            .cloned()
            .collect();
        let statuses =
            super::reports::compute(criteria.clone(), &reports, &counted, &rules, &checks)?;
        let applies = |a: &str| dims::applies(effective.get(a), combination);
        let shown = Graph {
            kinds: all.kinds.clone(),
            children: keep(&all.children, &applies),
            linked: all
                .linked
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.iter().filter(|a| applies(a)).cloned().collect(),
                    )
                })
                .collect(),
        };
        let mut memo = BTreeMap::new();
        for address in all.kinds.keys().filter(|a| applies(a)) {
            let (k, c, l) = (&shown.kinds, &shown.children, &shown.linked);
            let mut seen = BTreeSet::new();
            let tone = tone_of(address, k, &types, c, l, &statuses, &mut memo, &mut seen);
            if let Some(tone) = tone {
                tones
                    .entry(address.clone())
                    .or_default()
                    .insert(dims::key(combination), tone);
            }
        }
    }
    let values: Vec<Value> = effective
        .iter()
        .map(|(address, values)| json!({"address":address,"value":values}))
        .collect();
    store_column(client, workspace, "dims", &values).await?;
    let tones: Vec<Value> = tones
        .iter()
        .map(|(address, tones)| json!({"address":address,"value":tones}))
        .collect();
    store_column(client, workspace, "dim_tones", &tones).await
}

/// The children that apply to the combination.
fn keep(
    children: &BTreeMap<String, Vec<String>>,
    applies: &impl Fn(&str) -> bool,
) -> BTreeMap<String, Vec<String>> {
    children
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.iter().filter(|a| applies(a)).cloned().collect(),
            )
        })
        .collect()
}

/// Give each listed item its tone for the combination of `key`. An item without a stored
/// tone for it keeps its tone. `set` writes the tone into the shape of the item.
pub(in crate::store) async fn read_tones(
    client: &impl GenericClient,
    workspace: &str,
    key: &str,
    items: &mut [Value],
    set: fn(&mut Value, &str),
) -> Result<()> {
    let addresses: Vec<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    let sql = "SELECT address,dim_tones->>$3 FROM joppa.objects WHERE workspace=$1 AND address=ANY($2) AND dim_tones ? $3";
    let tones: BTreeMap<String, String> = client
        .query(sql, &[&workspace, &addresses, &key])
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    for item in items.iter_mut() {
        let tone = item["address"].as_str().and_then(|a| tones.get(a)).cloned();
        if let Some(tone) = tone {
            set(item, &tone);
        }
    }
    Ok(())
}
