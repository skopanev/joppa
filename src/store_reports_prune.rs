//! Part of `reports`: the results that can still decide a status. The rows of a workspace only
//! grow, so the stream keeps a row only while its commit can be the deciding commit of its test.
//! A test decides by its newest eligible counted record, and fail_wins reads every record of
//! that commit. So a row whose commit has no record as new as the best counted row of the same
//! test, field dimensions and boundary can decide nothing. A skip or a todo outside a record
//! never counts.
use crate::ac_tests::{Boundary, Listed, Report};
use std::collections::HashMap;

/// The order of a record: its commit order, then its first receipt. Higher is newer.
type Key = (i64, i64);

pub(super) struct Prune<'a> {
    reports: HashMap<i64, &'a Report>,
    /// The newest record of each commit.
    commits: HashMap<&'a str, Key>,
    /// The boundaries of each stored test name: one for each criterion or test node that reads it.
    boundaries: HashMap<String, Vec<Boundary>>,
    /// The best counted record of each test name, field dimensions and boundary.
    best: HashMap<(String, Option<String>, usize), Key>,
}

impl<'a> Prune<'a> {
    pub(super) fn new(
        reports: &'a [Report],
        criteria: &[(String, Option<Listed>, Boundary)],
        reset: bool,
    ) -> Self {
        let mut commits: HashMap<&str, Key> = HashMap::new();
        for r in reports {
            let top = commits.entry(&r.commit).or_insert((r.order, r.received_us));
            *top = (*top).max((r.order, r.received_us));
        }
        let effective = |b: Boundary| if reset { b } else { Boundary::None };
        let mut boundaries: HashMap<String, Vec<Boundary>> = HashMap::new();
        for (_, listed, boundary) in criteria {
            let Some(listed) = listed else { continue };
            let each: Vec<Boundary> = match &listed.boundaries {
                Some(own) => own.clone(),
                None => {
                    // A list without test nodes reads the results under the test names.
                    for test in &listed.tests {
                        let all = boundaries.entry(test.clone()).or_default();
                        all.push(effective(*boundary));
                    }
                    vec![*boundary; listed.keys.len()]
                }
            };
            for (keys, b) in listed.keys.iter().zip(each) {
                for key in keys {
                    boundaries
                        .entry(key.clone())
                        .or_default()
                        .push(effective(b));
                }
            }
        }
        Prune {
            reports: reports.iter().map(|r| (r.id, r)).collect(),
            commits,
            boundaries,
            best: HashMap::new(),
        }
    }

    /// True when the row of `report` for `address` can still decide a status.
    pub(super) fn keep(
        &mut self,
        report: i64,
        address: &str,
        dims: Option<&str>,
        status: &str,
    ) -> bool {
        let Some(r) = self.reports.get(&report) else {
            return false;
        };
        if !r.recorded && matches!(status, "skipped" | "todo") {
            return false;
        }
        let key = (r.order, r.received_us);
        let top = self.commits[r.commit.as_str()];
        // A name that no list names here keeps its rows: the status decides without this pass.
        let Some(boundaries) = self.boundaries.get(address) else {
            return true;
        };
        let mut keep = false;
        for (index, boundary) in boundaries.iter().enumerate() {
            let eligible = match boundary {
                Boundary::None => true,
                Boundary::After(at) => r.receipt_us > *at,
                Boundary::Unreadable => false,
            };
            if !eligible {
                continue;
            }
            let slot = (address.to_owned(), dims.map(str::to_owned), index);
            let best = self.best.entry(slot).or_insert(key);
            keep |= top >= *best;
            *best = (*best).max(key);
        }
        keep
    }
}
