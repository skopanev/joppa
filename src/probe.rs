//! Test probes: counts of the status and rollup computations and of the projection
//! rebuilds. A test reads them before and after a request to prove that a read computes
//! nothing. Each count costs one relaxed atomic add.
use std::sync::atomic::{AtomicU64, Ordering};

/// Criterion test status computations (`statuses_for_criteria`).
pub static STATUS: AtomicU64 = AtomicU64::new(0);
/// Row decoration walks: tones and verification summaries.
pub static ROLLUP: AtomicU64 = AtomicU64::new(0);
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
/// Reads of the current rows of a workspace (`state_rows::current`).
pub static STATE_LOADS: AtomicU64 = AtomicU64::new(0);
/// Writes that start from the state that the last write of this process kept in memory.
pub static STATE_REUSES: AtomicU64 = AtomicU64::new(0);

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
    ("rebuild", &REBUILD),
    ("delta", &DELTA),
    ("replay", &REPLAY),
    ("save_copy", &SAVE_COPY),
    ("sync", &SYNC),
    ("rows", &ROWS),
    ("state_loads", &STATE_LOADS),
    ("state_reuses", &STATE_REUSES),
];
