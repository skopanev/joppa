//! Test probes: counts of the status, rollup and wait computations and of the projection
//! rebuilds. A test reads them before and after a request to prove that a read computes
//! nothing. Each count costs one relaxed atomic add.
use std::sync::atomic::{AtomicU64, Ordering};

/// Criterion test status computations (`statuses_for_criteria`).
pub static STATUS: AtomicU64 = AtomicU64::new(0);
/// Row decoration walks: tones, verification summaries and waits.
pub static ROLLUP: AtomicU64 = AtomicU64::new(0);
/// Wait graph computations.
pub static WAITS: AtomicU64 = AtomicU64::new(0);
/// Reads of the stored waits of a whole workspace.
pub static WAIT_READS: AtomicU64 = AtomicU64::new(0);
/// Rebuilds of the status projection.
pub static REBUILD: AtomicU64 = AtomicU64::new(0);
/// Status projections of one write that recompute only the reached nodes.
pub static DELTA: AtomicU64 = AtomicU64::new(0);
/// Replays of a whole workspace journal.
pub static REPLAY: AtomicU64 = AtomicU64::new(0);
/// Copies of the whole workspace that a save_object makes.
pub static SAVE_COPY: AtomicU64 = AtomicU64::new(0);
/// Syncs of the document ledger, each of which builds every document row.
pub static SYNC: AtomicU64 = AtomicU64::new(0);
/// Objects and requirements that a write serializes to compare or store their current rows.
pub static ROWS: AtomicU64 = AtomicU64::new(0);

pub fn count(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn read(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// Each counter with its name, for a test that prints the counts of one request.
pub static COUNTERS: [(&str, &AtomicU64); 10] = [
    ("status", &STATUS),
    ("rollup", &ROLLUP),
    ("waits", &WAITS),
    ("wait_reads", &WAIT_READS),
    ("rebuild", &REBUILD),
    ("delta", &DELTA),
    ("replay", &REPLAY),
    ("save_copy", &SAVE_COPY),
    ("sync", &SYNC),
    ("rows", &ROWS),
];
