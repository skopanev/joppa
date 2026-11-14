//! Part of objects_dimensions.rs: the dimension tones of one write. Only the nodes that the
//! status projection of the write recomputes get new tones, and a node outside that set gives
//! its stored tones. The values equal the full projection (`project`), which runs instead
//! when the dimensions change, when a node that the write did not change gets other
//! dimension values, or when the recomputed nodes form a cycle.
use super::super::projection::store_rows;
use super::super::tone_items::{PRIORITY, highest, rule_of};
use super::*;

/// A cycle among the recomputed nodes: only the full projection gives its order.
struct Cycle;

/// The rows that the tones of the recomputed nodes read.
struct Rows {
    /// The kind of each present recomputed node.
    kinds: BTreeMap<String, String>,
    children: BTreeMap<String, Vec<String>>,
    linked: BTreeMap<String, BTreeSet<String>>,
    /// The stored tones of each present child and linking object.
    stored: BTreeMap<String, Value>,
}

/// Store the dimension values and tones of the nodes `nodes` after the write that changed
/// `dirty`.
pub(in crate::store) async fn update(
    client: &impl GenericClient,
    workspace: &str,
    (state, previous): (&Workspace, &Workspace),
    dirty: &BTreeSet<String>,
    nodes: &BTreeSet<String>,
) -> Result<()> {
    let dimensions = dims::dimensions(state);
    if dimensions != dims::dimensions(previous) {
        return project(client, workspace, state).await;
    }
    if dimensions.is_empty() {
        return Ok(());
    }
    let effective = dims::effective(state, &dimensions);
    let before = dims::effective(previous, &dimensions);
    let keys: BTreeSet<&String> = effective.keys().chain(before.keys()).collect();
    let changed: Vec<&String> = keys
        .into_iter()
        .filter(|a| effective.get(*a) != before.get(*a))
        .collect();
    if changed.iter().any(|a| !dirty.contains(*a)) {
        return project(client, workspace, state).await;
    }
    let values: Vec<Value> = dirty
        .iter()
        .map(|a| json!({"address":a,"value":effective.get(a)}))
        .collect();
    store_rows(client, workspace, "dims", &values).await?;
    let rows = Rows::read(client, workspace, nodes).await?;
    let types = crate::objects::schema(state).types;
    let tested: Vec<String> = rows
        .kinds
        .iter()
        .filter(|(_, k)| crate::manual_checks::reads_status(types.get(*k)))
        .map(|(a, _)| a.clone())
        .collect();
    let criteria = super::super::reports::criteria_of(state, &tested);
    let rules = crate::test_rules::rules(state)?;
    let (reports, results) =
        super::super::reports::results_of(client, workspace, &criteria).await?;
    let checks = crate::manual_checks::Checks::of(state, &tested);
    let mut tones: BTreeMap<&String, serde_json::Map<String, Value>> = BTreeMap::new();
    for combination in &dims::combinations(&dimensions) {
        let counted: Vec<_> = results
            .iter()
            .filter(|(_, r)| dims::counts(r.fields.as_ref(), combination))
            .cloned()
            .collect();
        let statuses =
            super::super::reports::compute(criteria.clone(), &reports, &counted, &rules, &checks)?;
        let applies = |a: &str| dims::applies(effective.get(a), combination);
        let key = dims::key(combination);
        let mut memo = BTreeMap::new();
        for address in nodes.iter().filter(|a| applies(a)) {
            let mut walk = Walk {
                rows: &rows,
                types: &types,
                nodes,
                statuses: &statuses,
                applies: &applies,
                key: &key,
            };
            let Ok(tone) = walk.tone(address, &mut memo, &mut BTreeSet::new()) else {
                return project(client, workspace, state).await;
            };
            if let Some(tone) = tone {
                tones
                    .entry(address)
                    .or_default()
                    .insert(key.clone(), json!(tone));
            }
        }
    }
    let values: Vec<Value> = nodes
        .iter()
        .map(|a| json!({"address":a,"value":tones.remove(a)}))
        .collect();
    store_rows(client, workspace, "dim_tones", &values).await
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
        let mut out = Rows {
            kinds: BTreeMap::new(),
            children: BTreeMap::new(),
            linked: BTreeMap::new(),
            stored: BTreeMap::new(),
        };
        let parse = |r: &tokio_postgres::Row, i: usize| {
            r.get::<_, Option<String>>(i)
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null)
        };
        for r in client.query("SELECT address,kind FROM joppa.objects WHERE workspace=$1 AND present AND address=ANY($2)", &[&workspace, &nodes]).await? {
            out.kinds.insert(r.get(0), r.get(1));
        }
        for r in client.query("SELECT address,parent,dim_tones::text FROM joppa.objects WHERE workspace=$1 AND present AND parent=ANY($2)", &[&workspace, &keys]).await? {
            let address: String = r.get(0);
            out.stored.insert(address.clone(), parse(&r, 2));
            out.children.entry(resolve(r.get(1))).or_default().push(address);
        }
        for r in client.query("SELECT l.source,l.target,o.dim_tones::text FROM joppa.object_links l JOIN joppa.objects o ON o.workspace=l.workspace AND o.address=l.source AND o.present WHERE l.workspace=$1 AND l.target=ANY($2) AND (l.relation IN ('depends_on','related') OR l.relation LIKE 'reference:%')", &[&workspace, &keys]).await? {
            let source: String = r.get(0);
            out.stored.insert(source.clone(), parse(&r, 2));
            out.linked.entry(resolve(r.get(1))).or_default().insert(source);
        }
        Ok(out)
    }
}

/// The tones of one combination, as `tone_of` in objects_tones.rs.
struct Walk<'a, F: Fn(&str) -> bool> {
    rows: &'a Rows,
    types: &'a BTreeMap<String, crate::objects::TreeType>,
    nodes: &'a BTreeSet<String>,
    statuses: &'a BTreeMap<String, Value>,
    applies: &'a F,
    key: &'a str,
}

impl<F: Fn(&str) -> bool> Walk<'_, F> {
    fn tone(
        &mut self,
        address: &str,
        memo: &mut BTreeMap<String, Option<&'static str>>,
        seen: &mut BTreeSet<String>,
    ) -> std::result::Result<Option<&'static str>, Cycle> {
        if let Some(t) = memo.get(address) {
            return Ok(*t);
        }
        let Some(kind) = self.rows.kinds.get(address) else {
            return Ok(None);
        };
        if !seen.insert(address.to_owned()) {
            return Err(Cycle);
        }
        let rule = rule_of(self.types, kind);
        let own = match rule {
            "own" | "tests" => Some(
                match self
                    .statuses
                    .get(address)
                    .and_then(|s| s["status"].as_str())
                {
                    Some("green") => "green",
                    Some("red") => "red",
                    _ => "blue",
                },
            ),
            "children" | "links" => {
                let counted: Vec<String> = match rule {
                    "children" => self.rows.children.get(address).cloned().unwrap_or_default(),
                    _ => self
                        .rows
                        .linked
                        .get(address)
                        .into_iter()
                        .flatten()
                        .cloned()
                        .collect(),
                };
                let mut found = vec![];
                for c in counted.iter().filter(|c| (self.applies)(c)) {
                    let tone = match self.nodes.contains(c) {
                        true => self.tone(c, memo, seen)?,
                        false => self
                            .rows
                            .stored
                            .get(c)
                            .and_then(|t| t[self.key].as_str())
                            .map(stored_tone),
                    };
                    found.extend(tone);
                }
                let manual = || crate::manual_checks::tone(self.statuses.get(address));
                Some(highest(found).or_else(manual).unwrap_or("orange"))
            }
            "none" => None,
            _ => Some("orange"),
        };
        memo.insert(address.to_owned(), own);
        Ok(own)
    }
}

/// A stored tone as one of the rollup tones; another stored value counts as blue.
fn stored_tone(tone: &str) -> &'static str {
    PRIORITY.into_iter().find(|t| *t == tone).unwrap_or("blue")
}
