//! The server runtime has at least two workers, also on a host with one CPU. So a long
//! synchronous write on one worker leaves another worker free for `GET /health`.
use joppa::runtime;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

/// The CPU time of a long write.
const WRITE: Duration = Duration::from_secs(2);

/// The wait of a timer future, as the timeout of `GET /health`, while a task blocks one
/// worker of a runtime with `workers` workers for `WRITE`.
fn probe_wait(workers: usize) -> Duration {
    let rt = runtime::build_with(workers).unwrap();
    let (started, blocked) = tokio::sync::oneshot::channel();
    let write = rt.spawn(async move {
        started.send(()).unwrap();
        std::thread::sleep(WRITE);
    });
    let wait = rt.block_on(async {
        blocked.await.unwrap();
        let start = Instant::now();
        let probe = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            start.elapsed()
        });
        probe.await.unwrap()
    });
    rt.block_on(write).unwrap();
    wait
}

// ac:brsycumc
#[test]
fn the_server_runtime_has_two_workers_also_on_one_cpu() {
    assert_eq!(runtime::worker_threads(NonZeroUsize::new(1)), 2);
    assert_eq!(runtime::worker_threads(None), 2);
    assert_eq!(runtime::worker_threads(NonZeroUsize::new(8)), 8);
    let rt = runtime::build().unwrap();
    assert!(rt.metrics().num_workers() >= runtime::MIN_WORKERS);
}

// ac:brsycumc
#[test]
fn health_answers_while_a_synchronous_write_blocks_a_worker_on_one_cpu() {
    // One CPU gives two workers: the probe runs on the free worker at once.
    let wait = probe_wait(runtime::worker_threads(NonZeroUsize::new(1)));
    assert!(wait < WRITE / 2, "the probe waited {wait:?}");
    // A runtime with one worker, as before, holds the probe until the write ends.
    let wait = probe_wait(1);
    assert!(wait >= WRITE / 2, "the probe waited only {wait:?}");
}
