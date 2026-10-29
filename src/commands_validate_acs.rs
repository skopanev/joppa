//! Part of `commands`: `validate_acs` and the items after it, moved from commands.rs unchanged.
use super::*;

pub(crate) fn validate_acs(w: &Workspace, acs: &[Ac]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for ac in acs {
        verification_identifier(w, &ac.id, "ac")?;
        nonempty(&ac.text)?;
        ensure!(ids.insert(&ac.id), "duplicate AC id");
        ensure!(
            ac.assessment.is_none(),
            "assess each AC after saving its revision"
        );
        if let Some(d) = &ac.owner {
            human(w, d)?;
        }
    }
    fn visit<'a>(
        id: &'a str,
        acs: &'a [Ac],
        stack: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        ensure!(stack.insert(id), "AC dependency cycle");
        let a = acs
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown AC dependency"))?;
        for dep in &a.depends_on {
            visit(dep, acs, stack, done)?;
        }
        stack.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for a in acs {
        visit(&a.id, acs, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

/// The revision kept the criterion record, apart from its assessment.
pub(crate) fn same_criterion(old: &Ac, new: &Ac) -> bool {
    old.id == new.id
        && old.text == new.text
        && old.description == new.description
        && old.depends_on == new.depends_on
        && old.owner == new.owner
        && old.before_launch == new.before_launch
}

/// Each criterion that `previous` holds unchanged keeps its assessment.
pub(crate) fn carry_assessments(previous: &[Ac], acs: &mut [Ac]) {
    for ac in acs {
        if let Some(old) = previous.iter().find(|old| same_criterion(old, ac)) {
            ac.assessment = old.assessment.clone();
        }
    }
}

pub fn ready(r: &Revision, id: &str) -> bool {
    let Some(ac) = r.acs.iter().find(|a| a.id == id) else {
        return false;
    };
    ac.assessment.as_ref().is_some_and(|a| a.ready) && ac.depends_on.iter().all(|d| ready(r, d))
}

pub fn unknown() -> Result<()> {
    bail!("unsupported operation")
}
