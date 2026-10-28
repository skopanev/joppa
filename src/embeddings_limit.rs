//! The provider limits of the indexer and of the query: at most 4 requests at one time and
//! at most 200 requests in each 60 seconds. A rejected input is its own error class, so the
//! indexer counts an attempt only when the text itself is the cause.
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

pub const CONCURRENCY: usize = 4;
pub const PER_WINDOW: usize = 200;
pub const WINDOW: Duration = Duration::from_secs(60);

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

    #[test]
    fn only_400_413_and_422_are_input_faults() {
        assert!([400, 413, 422].into_iter().all(is_input_fault));
        assert!(![401, 403, 429, 500, 503].into_iter().any(is_input_fault));
    }
}
