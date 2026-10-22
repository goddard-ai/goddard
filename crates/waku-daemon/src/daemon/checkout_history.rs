//! Reuse ref-dependent history counts across sidebar clients. Dirty status
//! stays fresh, and archive previews deliberately bypass this cache.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail};
use parking_lot::{Condvar, Mutex};
use waku_git::checkout_history::CheckoutHistoryState;
use waku_git::git_commit::{CheckoutStatus, checkout_status_with_history};

const MAX_ENTRIES: usize = 128;
const MAX_KEY_BYTES: usize = 8 * 1024 * 1024;
const MAX_CALCULATIONS: usize = 4;

#[derive(Default)]
pub(super) struct CheckoutHistoryCache {
    state: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    entries: HashMap<Arc<CheckoutHistoryState>, Entry>,
    key_bytes: usize,
    clock: u64,
    joined: u64,
}

enum Entry {
    Running(Arc<Flight>),
    Ready { count: u64, used: u64 },
}

#[derive(Default)]
struct Flight {
    result: Mutex<Option<Result<u64, String>>>,
    completed: Condvar,
}

impl Flight {
    fn wait(&self, deadline: Instant) -> anyhow::Result<u64> {
        let mut result = self.result.lock();
        loop {
            if let Some(result) = result.as_ref() {
                return result.clone().map_err(|message| anyhow!(message));
            }
            if self.completed.wait_until(&mut result, deadline).timed_out() {
                bail!("checkout history deadline expired while waiting for another request");
            }
        }
    }

    fn finish(&self, result: Result<u64, String>) {
        *self.result.lock() = Some(result);
        self.completed.notify_all();
    }
}

impl CheckoutHistoryCache {
    pub(super) fn inspect(
        &self,
        cwd: &Path,
        base: Option<&str>,
    ) -> anyhow::Result<Option<CheckoutStatus>> {
        checkout_status_with_history(cwd, |deadline| {
            let Some(state) = CheckoutHistoryState::read(cwd, base)? else {
                return Ok(0);
            };
            let key = Arc::new(state);
            self.get_or_compute(key.clone(), deadline, || key.count_if_unchanged(cwd, base))
        })
    }

    fn get_or_compute(
        &self,
        key: Arc<CheckoutHistoryState>,
        deadline: Instant,
        compute: impl FnOnce() -> anyhow::Result<u64>,
    ) -> anyhow::Result<u64> {
        let flight = {
            let mut state = self.state.lock();
            state.clock = state.clock.wrapping_add(1);
            let now = state.clock;
            match state.entries.get_mut(&key) {
                Some(Entry::Ready { count, used }) => {
                    *used = now;
                    return Ok(*count);
                }
                Some(Entry::Running(flight)) => {
                    let flight = Arc::clone(flight);
                    state.joined = state.joined.saturating_add(1);
                    drop(state);
                    return flight.wait(deadline);
                }
                None => {}
            }
            if state
                .entries
                .values()
                .filter(|entry| matches!(entry, Entry::Running(_)))
                .count()
                >= MAX_CALCULATIONS
            {
                bail!("checkout history calculations are busy; retry shortly");
            }
            state.make_room(key.retained_bytes(), MAX_ENTRIES, MAX_KEY_BYTES)?;
            let flight = Arc::new(Flight::default());
            state.key_bytes += key.retained_bytes();
            state
                .entries
                .insert(Arc::clone(&key), Entry::Running(Arc::clone(&flight)));
            flight
        };

        // This guard also releases waiters if an unexpected unwind interrupts
        // Git. No cache lock is held through subprocess execution or waiting.
        let mut calculation = Calculation {
            cache: self,
            key,
            flight,
            finished: false,
        };
        let result = compute();
        calculation.finish(
            result
                .as_ref()
                .map(|count| *count)
                .map_err(|error| format!("{error:#}")),
        );
        result
    }
}

impl CacheState {
    fn make_room(&mut self, bytes: usize, entries: usize, limit: usize) -> anyhow::Result<()> {
        if bytes > limit {
            bail!("checkout history metadata exceeds the cache budget");
        }
        while self.entries.len() >= entries || self.key_bytes + bytes > limit {
            let oldest = self
                .entries
                .iter()
                .filter_map(|(key, entry)| match entry {
                    Entry::Ready { used, .. } => Some((Arc::clone(key), *used)),
                    Entry::Running(_) => None,
                })
                .min_by_key(|(_, used)| *used);
            let Some((key, _)) = oldest else {
                bail!("checkout history cache is busy; retry shortly");
            };
            self.entries.remove(&key);
            self.key_bytes -= key.retained_bytes();
        }
        Ok(())
    }
}

struct Calculation<'a> {
    cache: &'a CheckoutHistoryCache,
    key: Arc<CheckoutHistoryState>,
    flight: Arc<Flight>,
    finished: bool,
}

impl Calculation<'_> {
    fn finish(&mut self, result: Result<u64, String>) {
        let mut state = self.cache.state.lock();
        match &result {
            Ok(count) => {
                state.clock = state.clock.wrapping_add(1);
                let used = state.clock;
                state.entries.insert(
                    Arc::clone(&self.key),
                    Entry::Ready {
                        count: *count,
                        used,
                    },
                );
            }
            Err(_) => {
                state.entries.remove(&self.key);
                state.key_bytes -= self.key.retained_bytes();
            }
        }
        drop(state);
        self.flight.finish(result);
        self.finished = true;
    }
}

impl Drop for Calculation<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(Err(
                "checkout history calculation was interrupted".to_owned()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use uuid::Uuid;

    struct Repository(PathBuf);

    impl Repository {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("waku-history-cache-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-b", "main"]);
            git(&root, &["config", "core.excludesFile", ""]);
            git(&root, &["config", "user.name", "History Cache Test"]);
            git(&root, &["config", "user.email", "history@example.com"]);
            git(&root, &["commit", "--allow-empty", "-m", "base"]);
            git(
                &root,
                &["remote", "add", "origin", "https://example.com/repo.git"],
            );
            git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
            git(&root, &["checkout", "-b", "session"]);
            git(&root, &["commit", "--allow-empty", "-m", "session work"]);
            Self(root)
        }

        fn key(&self) -> Arc<CheckoutHistoryState> {
            Arc::new(CheckoutHistoryState::read(&self.0, None).unwrap().unwrap())
        }
    }

    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    #[test]
    fn history_cache_reuses_counts_but_refreshes_dirt_and_moved_refs() {
        let repo = Repository::new();
        let cache = CheckoutHistoryCache::default();
        let first = cache.inspect(&repo.0, None).unwrap().unwrap();
        assert_eq!(first.unpushed_commits, 1);
        assert!(!first.uncommitted_changes);
        let count =
            cache.get_or_compute(repo.key(), deadline(), || panic!("history was not cached"));
        assert_eq!(count.unwrap(), 1);

        fs::write(repo.0.join("untracked.txt"), "dirty\n").unwrap();
        let dirty = cache.inspect(&repo.0, None).unwrap().unwrap();
        assert!(dirty.uncommitted_changes);
        assert_eq!(dirty.unpushed_commits, 1);
        assert_eq!(cache.state.lock().entries.len(), 1);

        git(&repo.0, &["tag", "saved"]);
        assert_eq!(
            cache
                .inspect(&repo.0, None)
                .unwrap()
                .unwrap()
                .unpushed_commits,
            0
        );
        assert!(
            crate::git_commit::archive_preview(&repo.0, None)
                .unwrap()
                .unwrap()
                .unpushed_commits
                .is_empty()
        );
        git(&repo.0, &["tag", "-d", "saved"]);
        assert_eq!(
            cache
                .inspect(&repo.0, None)
                .unwrap()
                .unwrap()
                .unpushed_commits,
            1
        );
        assert_eq!(
            crate::git_commit::archive_preview(&repo.0, None)
                .unwrap()
                .unwrap()
                .unpushed_commits,
            ["session work"]
        );
    }

    #[test]
    fn history_cache_shares_one_running_calculation_and_does_not_cache_errors() {
        let repo = Repository::new();
        let cache = Arc::new(CheckoutHistoryCache::default());
        let key = repo.key();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = {
            let cache = Arc::clone(&cache);
            let key = Arc::clone(&key);
            thread::spawn(move || {
                cache.get_or_compute(key, deadline(), || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    bail!("history inspection failed")
                })
            })
        };
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let second = {
            let cache = Arc::clone(&cache);
            let key = Arc::clone(&key);
            thread::spawn(move || {
                cache.get_or_compute(key, deadline(), || panic!("duplicate calculation"))
            })
        };
        let joined_deadline = deadline();
        while cache.state.lock().joined == 0 && Instant::now() < joined_deadline {
            thread::park_timeout(Duration::from_millis(1));
        }
        assert_eq!(cache.state.lock().joined, 1);
        release_tx.send(()).unwrap();
        assert!(first.join().unwrap().is_err());
        assert!(second.join().unwrap().is_err());
        assert!(cache.state.lock().entries.is_empty());
        assert_eq!(cache.state.lock().key_bytes, 0);
        assert_eq!(cache.get_or_compute(key, deadline(), || Ok(7)).unwrap(), 7);
    }

    #[test]
    fn history_cache_preserves_patch_equivalence_after_the_base_is_rewritten() {
        let repo = Repository::new();
        git(&repo.0, &["checkout", "-b", "dev", "main"]);
        fs::write(repo.0.join("base.txt"), "base patch\n").unwrap();
        git(&repo.0, &["add", "base.txt"]);
        git(&repo.0, &["commit", "-m", "base patch"]);
        git(&repo.0, &["checkout", "-b", "task"]);
        fs::write(repo.0.join("task.txt"), "task patch\n").unwrap();
        git(&repo.0, &["add", "task.txt"]);
        git(&repo.0, &["commit", "-m", "task patch"]);
        let cache = CheckoutHistoryCache::default();
        assert_eq!(
            cache
                .inspect(&repo.0, Some("dev"))
                .unwrap()
                .unwrap()
                .unpushed_commits,
            1
        );

        git(&repo.0, &["checkout", "dev"]);
        git(
            &repo.0,
            &["commit", "--amend", "-m", "rewritten base patch"],
        );
        git(&repo.0, &["checkout", "task"]);
        assert_eq!(
            cache
                .inspect(&repo.0, Some("main"))
                .unwrap()
                .unwrap()
                .unpushed_commits,
            2
        );
        assert_eq!(
            cache
                .inspect(&repo.0, Some("dev"))
                .unwrap()
                .unwrap()
                .unpushed_commits,
            1
        );
    }

    #[test]
    fn history_cache_bounds_retained_keys_without_evicting_running_work() {
        let repo = Repository::new();
        let first = repo.key();
        git(&repo.0, &["tag", "saved"]);
        let second = repo.key();
        let mut state = CacheState::default();
        state
            .entries
            .insert(Arc::clone(&first), Entry::Ready { count: 1, used: 1 });
        state.entries.insert(
            Arc::clone(&second),
            Entry::Running(Arc::new(Flight::default())),
        );
        state.key_bytes = first.retained_bytes() + second.retained_bytes();

        state
            .make_room(first.retained_bytes(), 2, MAX_KEY_BYTES)
            .unwrap();
        assert!(!state.entries.contains_key(&first));
        assert!(state.entries.contains_key(&second));
        assert_eq!(state.key_bytes, second.retained_bytes());
        assert!(state.make_room(1, 1, MAX_KEY_BYTES).is_err());
        assert!(state.make_room(1, 2, second.retained_bytes()).is_err());
        assert!(
            state
                .make_room(MAX_KEY_BYTES + 1, MAX_ENTRIES, MAX_KEY_BYTES)
                .is_err()
        );
    }

    #[test]
    fn history_cache_waiters_observe_their_own_deadline() {
        let flight = Flight::default();
        assert!(flight.wait(Instant::now()).is_err());
        flight.finish(Ok(3));
        assert_eq!(flight.wait(deadline()).unwrap(), 3);
    }
}
