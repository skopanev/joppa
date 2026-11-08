//! The in-process queue of the writers of each workspace. A write and a record of test results
//! wait here before they take a database connection, so the writers of one workspace hold at
//! most 1 connection and the other connections of the pool serve readers. The transaction
//! still takes the advisory lock of the workspace: it orders the writers of other processes.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::{Mutex as Queue, OwnedMutexGuard};

/// The queue of each workspace that has a writer now. The last writer removes it.
static QUEUES: LazyLock<Mutex<HashMap<String, Arc<Queue<()>>>>> = LazyLock::new(Default::default);
/// The workspace ID of each name that an accepted write resolved.
static NAMES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// The turn of one writer. The drop ends the turn.
pub(super) struct Turn {
    key: String,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        drop(self.guard.take());
        let mut queues = QUEUES.lock().unwrap();
        // Only the map holds the queue: no writer waits in it.
        if queues
            .get(&self.key)
            .is_some_and(|q| Arc::strong_count(q) == 1)
        {
            queues.remove(&self.key);
        }
    }
}

/// Wait for the turn of a writer of `name`. A name that a write resolved before waits in the
/// queue of its workspace ID.
pub(super) async fn turn(name: &str) -> Turn {
    let key = NAMES.lock().unwrap().get(name).cloned();
    let key = key.unwrap_or_else(|| name.to_owned());
    let queue = QUEUES
        .lock()
        .unwrap()
        .entry(key.clone())
        .or_default()
        .clone();
    let guard = Some(queue.lock_owned().await);
    Turn { key, guard }
}

/// Keep the workspace ID of `name` after the write passed the access check.
pub(super) fn resolved(name: &str, workspace: &str) {
    if name != workspace {
        NAMES
            .lock()
            .unwrap()
            .insert(name.to_owned(), workspace.to_owned());
    }
}
