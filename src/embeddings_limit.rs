//! The provider limits of the indexer and of the query: at most 4 requests at one time and
//! at most 200 requests in each 60 seconds. A rejected input is its own error class, so the
//! indexer counts an attempt only when the text itself is the cause. A breaker stops the query
//! vectors for 30 seconds after 2 transient provider failures in sequence.
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

pub const CONCURRENCY: usize = 4;
pub const PER_WINDOW: usize = 200;
pub const WINDOW: Duration = Duration::from_secs(60);
pub const BREAKER_FAILURES: usize = 2;
pub const BREAKER_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct Limits {
    slots: Arc<Semaphore>,
    sent: Arc<Mutex<VecDeque<Instant>>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(CONCURRENCY)),
            sent: Arc::new(Mutex::new(VecDeque::with_capacity(PER_WINDOW))),
        }
    }
}

impl Limits {
    /// Wait for a free slot and for room in the window, then record the request start. The
    /// caller keeps the permit until the provider answers.
    pub async fn admit(&self) -> anyhow::Result<OwnedSemaphorePermit> {
        let permit = self.slots.clone().acquire_owned().await?;
        loop {
            let mut sent = self.sent.lock().await;
            let now = Instant::now();
            while sent
                .front()
                .is_some_and(|t| now.duration_since(*t) >= WINDOW)
            {
                sent.pop_front();
            }
            if sent.len() < PER_WINDOW {
                sent.push_back(now);
                return Ok(permit);
            }
            let open = sent[0] + WINDOW;
            drop(sent);
            tokio::time::sleep_until(open).await;
        }
    }
}

/// The transient provider failures in sequence and the end of the open time. Each embedder
/// has its own breaker. A success closes it, and an input fault does not count.
#[derive(Clone, Default)]
pub struct Breaker(Arc<std::sync::Mutex<(usize, Option<Instant>)>>);

impl Breaker {
    /// True while the breaker is open: a query then gets no vector at once.
    pub fn is_open(&self) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.1.is_some_and(|until| Instant::now() < until)
    }

    /// Count one transient failure. The failure BREAKER_FAILURES in sequence opens the breaker.
    pub fn fail(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.0 += 1;
        if state.0 >= BREAKER_FAILURES {
            state.1 = Some(Instant::now() + BREAKER_COOLDOWN);
        }
    }

    /// A success closes the breaker and clears the count.
    pub fn succeed(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = (0, None);
    }

    /// Count the error of a request when it is transient: a transport error, a timeout, HTTP
    /// 429 or HTTP 500 and above.
    pub fn record(&self, error: &anyhow::Error) {
        let status = error.downcast_ref::<ProviderStatus>().map(|s| s.0);
        let transport = error.chain().any(|c| c.is::<reqwest::Error>());
        if transport || status.is_some_and(|s| s == 429 || s >= 500) {
            self.fail();
        }
    }
}

/// The provider refused the input itself with HTTP 400, 413 or 422. The same text cannot
/// succeed on a new request, so the indexer counts an attempt for it.
#[derive(Debug)]
pub struct InputFault(pub u16);

impl std::fmt::Display for InputFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embedding provider refused the input: HTTP {}", self.0)
    }
}

impl std::error::Error for InputFault {}

/// The provider answered with another error status. The text may succeed later, so the
/// indexer adds no attempt, and it still tries the other texts of the batch.
#[derive(Debug)]
pub struct ProviderStatus(pub u16);

impl std::fmt::Display for ProviderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embedding provider HTTP {}", self.0)
    }
}

impl std::error::Error for ProviderStatus {}

pub fn is_input_fault(status: u16) -> bool {
    matches!(status, 400 | 413 | 422)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn no_window_of_60_seconds_admits_more_than_200_requests() {
        let limits = Limits::default();
        let start = Instant::now();
        let mut starts = vec![];
        for _ in 0..450 {
            drop(limits.admit().await.unwrap());
            starts.push(start.elapsed());
        }
        for (i, at) in starts.iter().enumerate() {
            let in_window = starts[i..]
                .iter()
                .take_while(|t| **t < *at + WINDOW)
                .count();
            assert!(in_window <= PER_WINDOW, "{in_window} requests after {at:?}");
        }
        assert_eq!(starts.iter().filter(|t| **t < WINDOW).count(), PER_WINDOW);
    }

    #[tokio::test(start_paused = true)]
    async fn at_most_four_requests_hold_a_slot_at_one_time() {
        let limits = Limits::default();
        let held = futures_held(&limits).await;
        assert_eq!(held, CONCURRENCY);
    }

    async fn futures_held(limits: &Limits) -> usize {
        let mut permits = vec![];
        for _ in 0..CONCURRENCY {
            permits.push(limits.admit().await.unwrap());
        }
        let blocked = tokio::time::timeout(Duration::from_secs(1), limits.admit()).await;
        assert!(blocked.is_err(), "a fifth request must wait for a slot");
        permits.len()
    }

    // ac:ewp7bgsu
    #[tokio::test(start_paused = true)]
    async fn two_transient_failures_open_the_breaker_for_30_seconds() {
        let breaker = Breaker::default();
        breaker.record(&InputFault(422).into());
        breaker.record(&ProviderStatus(401).into());
        breaker.record(&ProviderStatus(503).into());
        assert!(!breaker.is_open(), "input faults and 401 do not count");
        breaker.record(&ProviderStatus(429).into());
        assert!(breaker.is_open());
        tokio::time::advance(BREAKER_COOLDOWN - Duration::from_millis(1)).await;
        assert!(breaker.is_open());
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(!breaker.is_open());
        breaker.fail();
        assert!(breaker.is_open(), "the next failure opens it again");
        breaker.succeed();
        assert!(!breaker.is_open());
        breaker.fail();
        assert!(!breaker.is_open(), "a success clears the count");
    }

    #[test]
    fn only_400_413_and_422_are_input_faults() {
        assert!([400, 413, 422].into_iter().all(is_input_fault));
        assert!(![401, 403, 429, 500, 503].into_iter().any(is_input_fault));
    }
}
