//! The status projection of one write. Only the changed nodes and the nodes whose status
//! reads them get new values: each parent up to the top, and each node of the `links` rule
//! that a changed node links to. A node reads its children and its linking objects from
//! their stored values. The values equal a full rebuild (objects_projection.rs), which the
//! daily catch-up also runs; a write that changes the schema or that meets a cycle of
//! recomputed nodes runs the full rebuild.
use super::projection::{
    STATUS_PROJECTION_VERSION, base_row, child_counts, patch, stamp, store_rows,
};
use super::tone_items::{PRIORITY, highest, label, rule_of};
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[path = "objects_projection_walk.rs"]
mod walk;
use walk::{link_targets, parent};

type Types = BTreeMap<String, objects::TreeType>;

/// A cycle among the recomputed nodes: only the full rebuild gives its order.
struct Cycle;

/// Recompute the nodes that `dirty` reaches. Returns false when the full rebuild must run.
pub(in crate::store) async fn update(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    previous: &Workspace,
    (before, position): (u64, u64),
    reports: &[i64],
    mut dirty: BTreeSet<String>,
) -> Result<bool> {
    // Only a projection of this code version at the position before the write is a base.
    let stamped = client.query_one("SELECT (first_view->'status_projection'->>'version')::int,(first_view->'status_projection'->>'position')::bigint FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?;
    if stamped.get::<_, Option<i32>>(0) != Some(STATUS_PROJECTION_VERSION)
        || stamped.get::<_, Option<i64>>(1) != Some(before as i64)
    {
        return Ok(false);
    }
    crate::probe::count(&crate::probe::DELTA);
    let types = crate::objects::schema(state).types;
    // A record of results changes the test nodes whose addresses it stores.
    for report in reports {
        for row in client.query("SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND o.present AND o.address IN (SELECT address FROM joppa.test_results WHERE report_id=$2)", &[&workspace, report]).await? {
            dirty.insert(row.get(0));
        }
    }
    // The test status of a node reads its own tests and the test nodes below it.
    let mut tested = dirty.clone();
    for address in &dirty {
        for w in [state, previous] {
            tested.extend(parent(w, address));
        }
    }
    tested.retain(|a| state.objects.contains_key(a));
    let targets: Vec<String> = tested.iter().cloned().collect();
    let statuses =
        super::reports::state_criterion_statuses(client, workspace, state, &targets).await?;
    let rows: Vec<Value> = targets
        .iter()
        .map(|a| json!({"address":a,"value":statuses.get(a)}))
        .collect();
    store_rows(client, workspace, "test_status", &rows).await?;
    let mut nodes = tested;
    // Each parent and each links target above a recomputed node.
    let mut queue: Vec<String> = nodes.iter().cloned().collect();
    while let Some(address) = queue.pop() {
        for w in [state, previous] {
            let mut above: Vec<String> = parent(w, &address).into_iter().collect();
            // A links target reads the tone of each linking node. Any target counts the
            // presence of a link in its label, which only a changed node changes.
            above.extend(link_targets(w, &address).into_iter().filter(|t| {
                dirty.contains(&address)
                    || w.objects
                        .get(t)
                        .is_some_and(|o| rule_of(&types, &o.kind) == "links")
            }));
            for next in above {
                if state.objects.contains_key(&next) && nodes.insert(next.clone()) {
                    queue.push(next);
                }
            }
        }
    }
    let rows = Rows::read(client, workspace, &nodes).await?;
    let Ok(patches) = rows.patches(&types, &nodes) else {
        return Ok(false);
    };
    store_rows(client, workspace, "rollup", &patches).await?;
    stamp(client, workspace, position).await?;
    super::computed::update(client, workspace, state, previous, &dirty).await?;
    super::dimensions::project(client, workspace, state).await?;
    Ok(true)
}

/// The rows of the recomputed nodes, of their children and of their linking objects.
struct Rows {
    base: BTreeMap<String, Value>,
    stored: BTreeMap<String, (Value, Value)>,
    children: BTreeMap<String, Vec<String>>,
    linked: BTreeMap<String, BTreeSet<String>>,
}

impl Rows {
    async fn read(
        client: &impl GenericClient,
        workspace: &str,
        nodes: &BTreeSet<String>,
    ) -> Result<Self> {
        let nodes: Vec<&String> = nodes.iter().collect();
        let aliases: BTreeMap<String, String> = client
            .query("SELECT address,target FROM joppa.object_aliases WHERE workspace=$1 AND target=ANY($2)", &[&workspace, &nodes])
            .await?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();
        let keys: Vec<&String> = nodes.iter().copied().chain(aliases.keys()).collect();
        let resolve = |a: String| aliases.get(&a).cloned().unwrap_or(a);
        let links = client
            .query("SELECT source,target FROM joppa.object_links WHERE workspace=$1 AND target=ANY($2) AND (relation IN ('depends_on','related') OR relation LIKE 'reference:%')", &[&workspace, &keys])
            .await?;
        let sources: Vec<String> = links.iter().map(|r| r.get(0)).collect();
        let mut out = Rows {
            base: BTreeMap::new(),
            stored: BTreeMap::new(),
            children: BTreeMap::new(),
            linked: BTreeMap::new(),
        };
        let mut present = BTreeSet::new();
        for r in client.query("SELECT address,kind,signal::text,process::text,parent,present,rollup::text,test_status::text FROM joppa.objects WHERE workspace=$1 AND (address=ANY($2) OR parent=ANY($3) OR address=ANY($4))", &[&workspace, &nodes, &keys, &sources]).await? {
            let item = base_row(&r);
            let address: String = r.get(0);
            let parse = |i: usize| r.get::<_, Option<String>>(i).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            if item["present"] == true {
                present.insert(address.clone());
                if let Some(p) = item["parent"].as_str() {
                    out.children.entry(resolve(p.to_owned())).or_default().push(address.clone());
                }
            }
            out.stored.insert(address.clone(), (parse(6), parse(7)));
            out.base.insert(address, item);
        }
        for r in links {
            let (source, target): (String, String) = (r.get(0), r.get(1));
            if present.contains(&source) {
                out.linked
                    .entry(resolve(target))
                    .or_default()
                    .insert(source);
            }
        }
        Ok(out)
    }

    /// The new rollup of each recomputed node, or a cycle.
    fn patches(
        &self,
        types: &Types,
        nodes: &BTreeSet<String>,
    ) -> std::result::Result<Vec<Value>, Cycle> {
        let mut tones = BTreeMap::new();
        for address in nodes {
            self.tone(address, types, nodes, &mut tones, &mut BTreeSet::new())?;
        }
        let mut out = vec![];
        for address in nodes {
            let Some(base) = self.base.get(address) else {
                continue;
            };
            let mut item = base.clone();
            if base["present"] == false {
                item["signal"]["tone"] = json!(crate::status_colors::ARCHIVED_TONE);
                item["signal"]["label"] = json!("Archived");
            } else if let Some(Some(tone)) = tones.get(address) {
                item["signal"]["label"] = json!(self.label(address, types, tone));
                item["signal"]["tone"] = json!(tone);
            }
            let children = self.children.get(address).into_iter().flatten();
            let toned: Vec<(&str, &str)> = children
                .filter_map(|c| Some((c.as_str(), self.tone_of(c, nodes, &tones, types)?)))
                .collect();
            if let Some(counts) = child_counts(toned, types) {
                item["child_counts"] = counts;
            }
            out.push(patch(base, &item));
        }
        Ok(out)
    }
}
