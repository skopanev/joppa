//! Private enrollment rules. Explicit addresses override verified organization domains.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default)]
    pub domains: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub emails: BTreeMap<String, BTreeSet<String>>,
}
impl Rules {
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let rules: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        for (name, workspaces) in rules.domains.iter().chain(&rules.emails) {
            ensure!(
                name == &name.to_ascii_lowercase() && name == name.trim(),
                "access rule keys must be lowercase"
            );
            for workspace in workspaces {
                crate::commands::identifier(workspace)?;
            }
        }
        Ok(rules)
    }
    pub fn resolve(&self, email: &str, hosted_domain: Option<&str>) -> BTreeSet<String> {
        let email = email.to_ascii_lowercase();
        if let Some(exact) = self.emails.get(&email) {
            return exact.clone();
        }
        let Some((_, domain)) = email.rsplit_once('@') else {
            return BTreeSet::new();
        };
        // A personal Google account with an external address does not prove domain ownership.
        if !hosted_domain.is_some_and(|hd| hd.eq_ignore_ascii_case(domain)) {
            return BTreeSet::new();
        }
        self.domains.get(domain).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_email_overrides_domain_and_empty_rule_revokes_access() {
        let rules: Rules = serde_json::from_value(serde_json::json!({
            "domains":{"team.example":["alpha"],"other.example":["beta"]},
            "emails":{"contractor@personal.example":["alpha"],"blocked@team.example":[],"special@team.example":["beta"]}
        })).unwrap();
        assert_eq!(
            rules.resolve("USER@TEAM.EXAMPLE", Some("team.example")),
            BTreeSet::from(["alpha".into()])
        );
        assert_eq!(
            rules.resolve("same@other.example", Some("other.example")),
            BTreeSet::from(["beta".into()])
        );
        assert_eq!(
            rules.resolve("special@team.example", Some("team.example")),
            BTreeSet::from(["beta".into()])
        );
        assert_eq!(
            rules.resolve("contractor@personal.example", None),
            BTreeSet::from(["alpha".into()])
        );
        assert!(
            rules
                .resolve("blocked@team.example", Some("team.example"))
                .is_empty()
        );
        for (email, hd) in [
            ("user@team.example", None),
            ("user@team.example", Some("other.example")),
            ("user@sub.team.example", Some("sub.team.example")),
            ("outsider@personal.example", None),
        ] {
            assert!(rules.resolve(email, hd).is_empty());
        }
    }
}
