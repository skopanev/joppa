//! Part of objects_projection_delta.rs: the tone and the readiness of one recomputed node,
//! by the same rules as the full rebuild (objects_tones.rs and objects_ready_rollup.rs). A
//! node outside the recomputed set gives its stored value.
use super::*;

impl Rows {
    /// Mark each base row that waits for a dependency, so its tone is at least yellow. The
    /// mark is not a stored field: the patch of the row leaves it out.
    pub(super) fn mark_waiting(&mut self, waiting: &BTreeMap<String, String>) {
        for (address, base) in &mut self.base {
            if waiting.contains_key(address) {
                base["waiting"] = json!(true);
            }
        }
    }

    /// The counted nodes below a node of the `children` or `links` rule.
    fn below(&self, address: &str, rule: &str) -> Vec<String> {
        match rule {
            "children" => self.children.get(address).cloned().unwrap_or_default(),
            "links" => self
                .linked
                .get(address)
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
            _ => vec![],
        }
    }

    /// The stored tone of a node outside the recomputed set; None when a parent does not
    /// count it.
    fn stored_tone(&self, address: &str, types: &Types) -> Option<&str> {
        let base = self.base.get(address).filter(|b| b["present"] == true)?;
        if rule_of(types, base["kind"].as_str()?) == "none" {
            return None;
        }
        let (rollup, _) = self.stored.get(address)?;
        rollup["signal"]["tone"]
            .as_str()
            .or_else(|| base["signal"]["tone"].as_str())
    }

    /// The tone of a node: the new one when it is recomputed, else the stored one.
    pub(super) fn tone_of<'a>(
        &'a self,
        address: &str,
        nodes: &BTreeSet<String>,
        tones: &BTreeMap<String, Option<&'static str>>,
        types: &Types,
    ) -> Option<&'a str> {
        if nodes.contains(address) {
            return tones.get(address).copied().flatten();
        }
        self.stored_tone(address, types)
    }

    /// The label of the new tone of a recomputed node: the label of a manual check that gave the
    /// tone, else the label of the status rule.
    pub(super) fn label(&self, address: &str, types: &Types, tone: &str) -> &'static str {
        let base = &self.base[address];
        let counted = self.children.get(address).is_some_and(|c| !c.is_empty())
            || self.linked.get(address).is_some_and(|l| !l.is_empty());
        let kind = base["kind"].as_str().unwrap_or_default();
        let rule = label(rule_of(types, kind), tone, waits(base), counted);
        crate::manual_checks::label(self.stored.get(address).map(|s| &s.1), tone, rule)
    }

    /// The new tone of a recomputed node, as `tone_of` in objects_tones.rs.
    pub(super) fn tone(
        &self,
        address: &str,
        types: &Types,
        nodes: &BTreeSet<String>,
        memo: &mut BTreeMap<String, Option<&'static str>>,
        seen: &mut BTreeSet<String>,
    ) -> std::result::Result<Option<&'static str>, Cycle> {
        if let Some(t) = memo.get(address) {
            return Ok(*t);
        }
        let Some(base) = self.base.get(address).filter(|b| b["present"] == true) else {
            memo.insert(address.to_owned(), None);
            return Ok(None);
        };
        if !seen.insert(address.to_owned()) {
            return Err(Cycle);
        }
        let rule = rule_of(types, base["kind"].as_str().unwrap_or_default());
        let own = match rule {
            "own" | "tests" => {
                let status = self.stored.get(address).map(|(_, s)| &s["status"]);
                Some(match status.and_then(Value::as_str) {
                    Some("green") => "green",
                    Some("red") => "red",
                    _ => "grey",
                })
            }
            "children" | "links" => {
                let mut found = vec![];
                for c in self.below(address, rule) {
                    let tone = if nodes.contains(&c) {
                        self.tone(&c, types, nodes, memo, seen)?
                    } else {
                        self.stored_tone(&c, types).and_then(static_tone)
                    };
                    found.extend(tone);
                }
                let manual = || crate::manual_checks::tone(self.stored.get(address).map(|s| &s.1));
                Some(highest(found).or_else(manual).unwrap_or("orange"))
            }
            "none" => None,
            _ => Some("orange"),
        };
        let own = own.map(|t| {
            if waits(base) {
                highest([t, "yellow"]).unwrap_or(t)
            } else {
                t
            }
        });
        memo.insert(address.to_owned(), own);
        Ok(own)
    }

    /// The new readiness of a recomputed node, as `ready_of` in objects_ready_rollup.rs.
    /// `walk` holds the types, the recomputed nodes, their new tones and the waits.
    pub(super) fn ready(
        &self,
        address: &str,
        walk: Walk,
        memo: &mut BTreeMap<String, bool>,
        seen: &mut BTreeSet<String>,
    ) -> std::result::Result<bool, Cycle> {
        let (types, nodes, tones, waiting) = walk;
        if let Some(ready) = memo.get(address) {
            return Ok(*ready);
        }
        let Some(base) = self.base.get(address).filter(|b| b["present"] == true) else {
            return Ok(false);
        };
        if !seen.insert(address.to_owned()) {
            return Err(Cycle);
        }
        let mut item = base.clone();
        if waiting.contains_key(address) {
            item["work_ready"] = json!(false);
        }
        let rule = rule_of(types, base["kind"].as_str().unwrap_or_default());
        let mut rolled = true;
        for c in self.below(address, rule) {
            if self.tone_of(&c, nodes, tones, types).is_none() {
                continue;
            }
            let ready = if nodes.contains(&c) {
                self.ready(&c, walk, memo, seen)?
            } else {
                self.stored_ready(&c)
            };
            if !ready {
                rolled = false;
                break;
            }
        }
        let result = !waits(&item) && rolled;
        memo.insert(address.to_owned(), result);
        Ok(result)
    }

    /// The stored readiness of a node outside the recomputed set.
    fn stored_ready(&self, address: &str) -> bool {
        let (Some(base), Some((rollup, _))) = (self.base.get(address), self.stored.get(address))
        else {
            return false;
        };
        let mut item = base.clone();
        if let Some(ready) = rollup.get("work_ready") {
            item["work_ready"] = ready.clone();
        }
        base["present"] == true && !waits(&item)
    }
}

/// The types, the recomputed nodes, their new tones and the waits of one readiness walk.
pub(super) type Walk<'a> = (
    &'a Types,
    &'a BTreeSet<String>,
    &'a BTreeMap<String, Option<&'static str>>,
    &'a BTreeMap<String, String>,
);

/// A stored tone as one of the rollup tones; another stored value counts as grey.
fn static_tone(tone: &str) -> Option<&'static str> {
    PRIORITY.into_iter().find(|t| *t == tone).or(Some("grey"))
}

/// The parent of a node, by its current address.
pub(super) fn parent(w: &Workspace, address: &str) -> Option<String> {
    let parent = w.objects.get(address)?.current().parent.as_deref()?;
    Some(crate::objects::resolve_address(w, parent).to_owned())
}

/// The targets of the counted links of a node: its depends_on and related edges and its
/// reference fields. A flow counts for no status rule.
pub(super) fn link_targets(w: &Workspace, address: &str) -> Vec<String> {
    let Some(o) = w.objects.get(address) else {
        return vec![];
    };
    let r = o.current();
    let mut out: Vec<String> = r.counted().map(|e| e.to.clone()).collect();
    if let Ok(targets) = crate::objects::stored_reference_targets(w, &o.kind, &r.data) {
        out.extend(targets.into_iter().map(|(_, target, _)| target));
    }
    out.iter()
        .map(|t| crate::objects::resolve_address(w, t).to_owned())
        .collect()
}
