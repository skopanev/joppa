//! The label of a tone that a status rule sets. A rule tone that differs from the process
//! tone gets a label that names the rule result, so a green node never says that it waits.
//! A grey rule tone keeps a process label that is not green or red: that label names the
//! next step, for example "Readiness not assessed: send assess".

/// The label for the rule tone `tone` of a node with the status rule `rule` and the process
/// tone `process`, or None when the process label stays.
pub(super) fn label(rule: &str, tone: &str, process: Option<&str>) -> Option<&'static str> {
    if process == Some(tone) || tone == "grey" && !matches!(process, Some("green" | "red")) {
        return None;
    }
    Some(match (rule, tone) {
        ("tests", "green") => "Checks passed",
        ("tests", "red") => "Checks failed",
        ("tests", "grey") => "Awaits checks",
        ("tests", "none") => "Checks do not apply",
        ("children", "green") => "Children checks passed",
        ("children", "red") => "Child checks failed",
        ("children", "grey") => "Children await checks",
        ("links", "green") => "Linked work passed",
        ("links", "red") => "Linked work failed",
        ("links", "grey") => "Linked work awaits checks",
        (_, "orange") => "Status rule not set",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::label;

    #[test]
    fn a_rule_tone_gets_a_label_that_agrees() {
        assert_eq!(
            label("children", "green", Some("grey")),
            Some("Children checks passed")
        );
        assert_eq!(label("tests", "red", Some("grey")), Some("Checks failed"));
        assert_eq!(label("tests", "grey", Some("green")), Some("Awaits checks"));
        // The same tone keeps the process label, such as "REQ accepted".
        assert_eq!(label("children", "green", Some("green")), None);
        // A grey rule tone keeps the next step of a yellow process label.
        assert_eq!(label("tests", "grey", Some("yellow")), None);
        assert_eq!(label("unset", "orange", None), Some("Status rule not set"));
    }
}
