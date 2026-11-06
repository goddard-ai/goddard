//! Process-wide FIFO coordination for operations that integrate onto a base.
//!
//! Worktrees share a queue through their canonical Git common directory. Keep
//! the permit for the entire operation, including validation and verification,
//! so the next arrival reads the base left by the preceding landing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
struct State {
    next: u64,
    serving: u64,
}

#[derive(Default)]
struct Queue {
    state: Mutex<State>,
    wake: Condvar,
}

/// Releases the repository's next waiting landing on every exit path.
#[must_use]
pub struct LandingPermit {
    queue: Arc<Queue>,
}

impl Drop for LandingPermit {
    fn drop(&mut self) {
        let mut state = self.queue.state.lock();
        state.serving = state.serving.wrapping_add(1);
        self.queue.wake.notify_all();
    }
}

/// Wait in arrival order for this repository. Call only from a blocking worker;
/// no Git state read before acquiring this permit may drive the integration.
pub fn acquire(cwd: &Path) -> anyhow::Result<LandingPermit> {
    let common = crate::git_commit::git_stdout(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = dunce::canonicalize(Path::new(&common))?;
    Ok(acquire_queue(repository_queue(common)))
}

fn repository_queue(common: PathBuf) -> Arc<Queue> {
    static QUEUES: OnceLock<Mutex<HashMap<PathBuf, Weak<Queue>>>> = OnceLock::new();
    let mut queues = QUEUES.get_or_init(Mutex::default).lock();
    // Idle repositories must not accumulate for the daemon's lifetime.
    queues.retain(|_, queue| queue.strong_count() > 0);
    if let Some(queue) = queues.get(&common).and_then(Weak::upgrade) {
        return queue;
    }
    let queue = Arc::new(Queue::default());
    queues.insert(common, Arc::downgrade(&queue));
    queue
}

fn acquire_queue(queue: Arc<Queue>) -> LandingPermit {
    let ticket = reserve(&queue);
    wait(queue, ticket)
}

fn reserve(queue: &Queue) -> u64 {
    let mut state = queue.state.lock();
    let ticket = state.next;
    state.next = state.next.wrapping_add(1);
    ticket
}

fn wait(queue: Arc<Queue>, ticket: u64) -> LandingPermit {
    let mut state = queue.state.lock();
    while state.serving != ticket {
        queue.wake.wait(&mut state);
    }
    drop(state);
    LandingPermit { queue }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn arrivals_run_fifo_and_other_repositories_are_independent() {
        let common = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let queue = repository_queue(common.clone());
        let first = acquire_queue(queue.clone());
        let second = reserve(&queue);
        let third = reserve(&queue);
        let (ran, received) = mpsc::channel();
        let jobs = [third, second].map(|ticket| {
            let queue = queue.clone();
            let ran = ran.clone();
            std::thread::spawn(move || {
                let _permit = wait(queue, ticket);
                ran.send(ticket).unwrap();
            })
        });

        assert!(Arc::ptr_eq(&queue, &repository_queue(common)));
        let other = repository_queue(PathBuf::from(uuid::Uuid::new_v4().to_string()));
        let (independent, independent_rx) = mpsc::channel();
        let other_job = std::thread::spawn(move || {
            let _permit = acquire_queue(other);
            independent.send(()).unwrap();
        });
        independent_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        other_job.join().unwrap();
        assert!(received.try_recv().is_err());
        drop(first);
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            second
        );
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            third
        );
        for job in jobs {
            job.join().unwrap();
        }
    }
}
