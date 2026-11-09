//! Manual checks. A person proves a criterion with a file that has a check result, not with a
//! fake test. The file carries check_result (pass or fail) and, with it, an optional
//! valid_until: the UTC date on which a pass expires. A file without check_result changes no
//! status, and a check result belongs only to a node of a type with the role ac.
//!
//! A criterion with a live test node takes its status only from its tests. For another
//! criterion, the latest live file with a check result decides, from the files that the server
//! recorded after the boundary of the criterion (`Workspace::criterion_boundaries`). A file at
//! the same time as the boundary does not count. Without reset_on_change, every file counts.
//! fail gives red. pass gives green until the UTC date valid_until starts, then blue "Manual
//! check expired". Only older files give blue "Awaits checks". Without files the criterion
//! stays blue "no tests". The status projection uses the UTC date of today, and
//! store::expire_manual_checks the day of its run.
use crate::ac_tests::{Boundary, CriterionStatus};
use crate::model::Workspace;
use crate::objects::TreeType;
use anyhow::{Result, ensure};
use chrono::NaiveDate;
use std::collections::BTreeMap;

pub const RESULT_REFUSAL: &str = "check_result is pass or fail";
pub const DATE_REFUSAL: &str = "valid_until is a date YYYY-MM-DD";
pub const DATE_NEEDS_RESULT: &str = "valid_until needs check_result";
pub const TARGET_REFUSAL: &str = "a manual check belongs to a criterion";
pub const EXPIRED: &str = "Manual check expired";
pub const AWAITS: &str = "Awaits checks";

tokio::task_local! {
    /// The day of a run of store::expire_manual_checks, inside that run.
    static DAY: NaiveDate;
}

/// The date of the statuses: the day of a check run, else the UTC date of today.
pub fn today() -> NaiveDate {
    DAY.try_with(|day| *day)
        .unwrap_or_else(|_| chrono::Utc::now().date_naive())
}

/// Run `work` with the statuses of `day`.
pub async fn on_day<F: Future>(day: NaiveDate, work: F) -> F::Output {
    DAY.scope(day, work).await
}

/// The date of a text with exactly the form YYYY-MM-DD.
fn date(text: &str) -> Option<NaiveDate> {
    let form = text.len() == 10
        && text.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        });
    form.then(|| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        .flatten()
}

/// Refuse a bad check result, a bad date, or a date without a check result.
pub fn check_fields(check_result: Option<&str>, valid_until: Option<&str>) -> Result<()> {
    ensure!(
        check_result.is_none_or(|r| matches!(r, "pass" | "fail")),
        RESULT_REFUSAL
    );
    ensure!(valid_until.is_none_or(|d| date(d).is_some()), DATE_REFUSAL);
    ensure!(
        valid_until.is_none() || check_result.is_some(),
        DATE_NEEDS_RESULT
    );
    Ok(())
}

/// Refuse bad check fields of an attach, or a check result on a node that is not a criterion.
pub fn check_attach(w: &Workspace, op: &crate::commands::AttachOp) -> Result<()> {
    let result = op.check_result.as_deref();
    check_fields(result, op.valid_until.as_deref())?;
    if result.is_some() {
        let types = crate::objects::schema(w).types;
        let target = crate::objects::resolve_address(w, &op.target);
        ensure!(criterion(w, &types, target), TARGET_REFUSAL);
    }
    Ok(())
}

/// The node at `address` is of a type with the role ac.
fn criterion(w: &Workspace, types: &BTreeMap<String, TreeType>, address: &str) -> bool {
    let kind = w.objects.get(address).map(|o| &o.kind);
    kind.and_then(|k| types.get(k))
        .is_some_and(|t| t.role == "ac")
}

/// The tone of a node without counted children from its stored status, when manual checks
/// decided that status.
pub fn tone(status: Option<&serde_json::Value>) -> Option<&'static str> {
    let status = status.filter(|s| s["manual"] == true)?;
    Some(match status["status"].as_str() {
        Some("green") => "green",
        Some("red") => "red",
        _ => "blue",
    })
}

/// The label of a tone that manual checks gave, else the label of the status rule.
pub fn label(status: Option<&serde_json::Value>, tone: &str, rule: &'static str) -> &'static str {
    if self::tone(status) != Some(tone) {
        return rule;
    }
    match tone {
        "green" => "Manual check passed",
        "red" => "Manual check failed",
        _ if status.is_some_and(|s| s["reason"] == EXPIRED) => EXPIRED,
        _ => AWAITS,
    }
}

/// The tone of a node of this type reads its stored status: a node of the own or tests rule,
/// and a criterion of the children rule, whose manual checks decide its tone when its children
/// give none.
pub fn reads_status(t: Option<&TreeType>) -> bool {
    t.is_some_and(|t| match crate::objects::status_rule(t) {
        "own" | "tests" => true,
        "children" => t.role == "ac",
        _ => false,
    })
}

/// One live file with a check result.
#[derive(Clone, Debug)]
pub struct Check {
    id: String,
    /// The time of the attach event, in microseconds.
    at_us: i64,
    pass: bool,
    valid_until: Option<NaiveDate>,
}

/// The live checks of each criterion, and the date that decides an expiry.
pub struct Checks {
    pub by_criterion: BTreeMap<String, Vec<Check>>,
    pub day: NaiveDate,
}

impl Checks {
    /// The checks of each criterion among `targets`, on the date of `today`.
    pub fn of(w: &Workspace, targets: &[String]) -> Self {
        let mut by_criterion = BTreeMap::new();
        if !w.notes.is_empty() {
            let types = crate::objects::schema(w).types;
            for address in targets.iter().filter(|a| criterion(w, &types, a)) {
                let checks = live(w, address);
                if !checks.is_empty() {
                    by_criterion.insert(address.clone(), checks);
                }
            }
        }
        Checks {
            by_criterion,
            day: today(),
        }
    }

    /// The status of a criterion without test nodes, and the expiry date of a deciding pass.
    /// A criterion without checks gives none and keeps blue "no tests".
    pub fn decide(
        &self,
        address: &str,
        boundary: Boundary,
        reset_on_change: bool,
    ) -> Option<(CriterionStatus, Option<NaiveDate>)> {
        let checks = self.by_criterion.get(address)?;
        let counts = |c: &&Check| match (reset_on_change, boundary) {
            (false, _) | (true, Boundary::None) => true,
            (true, Boundary::After(at)) => c.at_us > at,
            (true, Boundary::Unreadable) => false,
        };
        let latest = checks
            .iter()
            .filter(counts)
            .max_by(|a, b| a.at_us.cmp(&b.at_us).then_with(|| a.id.cmp(&b.id)));
        let expires = latest.filter(|c| c.pass).and_then(|c| c.valid_until);
        let (status, reason) = match latest {
            None => ("blue", Some(AWAITS)),
            Some(c) if !c.pass => ("red", None),
            Some(_) if expires.is_some_and(|d| d <= self.day) => ("blue", Some(EXPIRED)),
            Some(_) => ("green", None),
        };
        let status = CriterionStatus {
            status: status.into(),
            reason: reason.map(String::from),
            tests_draft: false,
            commit: None,
            oldest_order: None,
            tests: vec![],
        };
        Some((status, expires))
    }
}

/// The live files with a check result of the node at `address`.
fn live(w: &Workspace, address: &str) -> Vec<Check> {
    let annotation = crate::objects::annotation_address(w, address);
    let files = w.notes.get(&annotation).map(|n| &n.attachments);
    let files = files.into_iter().flatten().filter(|(_, a)| !a.removed);
    files
        .filter_map(|(id, a)| {
            let pass = match a.check_result.as_deref()? {
                "pass" => true,
                "fail" => false,
                _ => return None,
            };
            let Boundary::After(at_us) = Boundary::read(Some(&a.at)) else {
                return None;
            };
            let valid_until = a.valid_until.as_deref().and_then(date);
            Some(Check {
                id: id.clone(),
                at_us,
                pass,
                valid_until,
            })
        })
        .collect()
}
