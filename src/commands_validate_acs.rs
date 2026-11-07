//! Part of `commands`: `validate_acs` and the items after it, moved from commands.rs unchanged.
use super::*;

pub(crate) fn validate_acs(w: &Workspace, acs: &[Ac]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for ac in acs {
        verification_identifier(w, &ac.id, "ac")?;
        nonempty(&ac.text)?;
        ensure!(ids.insert(&ac.id), "duplicate AC id");
        if let Some(d) = &ac.owner {
            human(w, d)?;
        }
    }
    // A dependency names a criterion of this revision or of any other requirement; a cycle
    // through other requirements is refused as well. Keys compare as criterion addresses.
    fn visit(
        w: &Workspace,
        id: &str,
        acs: &[Ac],
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = crate::objects::criterion_address(id);
        if done.contains(&key) {
            return Ok(());
        }
        ensure!(stack.insert(key.clone()), "AC dependency cycle");
        let local = acs
            .iter()
            .find(|a| crate::objects::criterion_address(&a.id) == key);
        let a = match local {
            Some(a) => a,
            None => {
                crate::objects::criterion_context(w, &key)
                    .ok_or_else(|| anyhow::anyhow!("unknown AC dependency"))?
                    .2
            }
        };
        for dep in &a.depends_on {
            visit(w, dep, acs, stack, done)?;
        }
        stack.remove(&key);
        done.insert(key);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for a in acs {
        visit(w, &a.id, acs, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

pub fn unknown() -> Result<()> {
    bail!("unsupported operation")
}
