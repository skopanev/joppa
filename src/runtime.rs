//! The Tokio runtime of the server and the CLI. A write runs synchronous CPU work on its
//! worker. With only one worker, that work stops every other request, `GET /health` too. So
//! the runtime always has at least two workers, also on a host with one CPU.
use std::num::NonZeroUsize;

/// The minimum number of runtime workers.
pub const MIN_WORKERS: usize = 2;

/// The number of workers for a host with `parallelism` CPUs.
pub fn worker_threads(parallelism: Option<NonZeroUsize>) -> usize {
    parallelism.map_or(1, NonZeroUsize::get).max(MIN_WORKERS)
}

/// The multi-thread runtime with `workers` workers.
pub fn build_with(workers: usize) -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
}

/// The runtime of this host: `worker_threads` of its available parallelism.
pub fn build() -> std::io::Result<tokio::runtime::Runtime> {
    build_with(worker_threads(std::thread::available_parallelism().ok()))
}
