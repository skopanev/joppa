//! The dependency check of the published planning Tasks.
use super::*;

pub(super) fn validate_dependencies(w: &Workspace, roots: &[String]) -> Result<()> {
    fn visit(
        w: &Workspace,
        key: &str,
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = objects::resolve_address(w, key);
        if done.contains(key) {
            return Ok(());
        }
        let object = w
            .objects
            .get(key)
            .filter(|o| o.current().present)
            .context("planning dependency target is missing")?;
        ensure!(
            stack.insert(key.into()),
            "planning dependencies contain a cycle"
        );
        for link in object
            .current()
            .links
            .iter()
            .filter(|l| l.relation == "depends_on")
        {
            visit(w, &link.target, stack, done)?;
        }
        stack.remove(key);
        done.insert(key.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for key in roots {
        visit(w, key, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}
