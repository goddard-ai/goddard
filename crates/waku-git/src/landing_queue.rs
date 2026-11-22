//! Repository FIFO jobs. Admission never parks a Git worker; a fixed executor
//! runs at most one job per canonical Git common directory.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail, ensure};
use parking_lot::{Condvar, Mutex};
use uuid::Uuid;
use waku_protocol::git::PullStrategy;
use waku_protocol::landing::{
    CancelLandingOutcome, LandingJob, LandingKind, LandingQueueSnapshot, LandingResult,
    LandingState,
};

const WORKERS: usize = 4;
const MAX_PENDING: usize = 256;
const MAX_FINISHED: usize = 128;
const RETENTION: Duration = Duration::from_secs(30 * 60);
type Operation = Box<dyn FnOnce(&LandingPermit) -> anyhow::Result<LandingResult> + Send>;
/// Return false when the observing connection is gone. Never called under a queue lock.
pub type Observer = Arc<dyn Fn(Uuid, u64) -> bool + Send + Sync>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Request {
    cwd: PathBuf,
    repository: PathBuf,
    task: Option<Uuid>,
    kind: LandingKind,
    base: Option<String>,
    strategy: PullStrategy,
}
struct Entry {
    request: Request,
    job: LandingJob,
    head: String,
    branch: String,
    operation: Option<Operation>,
    finished: Option<Instant>,
}
#[derive(Default)]
struct State {
    revision: u64,
    repositories: HashMap<PathBuf, Uuid>,
    entries: HashMap<Uuid, Entry>,
    aliases: HashMap<Uuid, (Uuid, Request)>,
    pending: VecDeque<Uuid>,
    active: HashSet<Uuid>,
    observers: HashMap<(Uuid, u64), (Observer, Instant)>,
}
struct Executor {
    epoch: Uuid,
    state: Mutex<State>,
    wake: Condvar,
}

/// Proof that this job owns its repository until execution returns. Low-level
/// integration functions require this token to prevent accidental nested admission.
pub struct LandingPermit {
    executor: Arc<Executor>,
    id: Uuid,
    repository: PathBuf,
    result: Option<LandingResult>,
}
impl LandingPermit {
    pub fn resolved_base(&self, base: &str) {
        let mut state = self.executor.state.lock();
        if let Some(entry) = state.entries.get_mut(&self.id) {
            entry.job.base = Some(base.to_owned());
            let queue = entry.job.queue_id;
            state.revision += 1;
            drop(state);
            self.executor.notify(queue);
        }
    }
    pub fn check_repository(&self, cwd: &Path) -> anyhow::Result<()> {
        ensure!(
            repository(cwd)? == self.repository,
            "landing permit belongs to another repository"
        );
        Ok(())
    }
    fn complete(mut self, result: LandingResult) {
        self.result = Some(result);
    }
}
impl Drop for LandingPermit {
    fn drop(&mut self) {
        let result = self.result.take().unwrap_or_else(|| LandingResult::Failed {
            message: "landing worker stopped before returning a result".into(),
        });
        let mut state = self.executor.state.lock();
        if let Some(entry) = state.entries.get_mut(&self.id) {
            entry.job.state = LandingState::Finished;
            entry.job.result = Some(result);
            entry.finished = Some(Instant::now());
            let queue = entry.job.queue_id;
            state.active.remove(&queue);
            state.revision += 1;
            drop(state);
            self.executor.wake.notify_all();
            self.executor.notify(queue);
        }
    }
}

fn repository(cwd: &Path) -> anyhow::Result<PathBuf> {
    let common = crate::git_commit::git_stdout(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    dunce::canonicalize(common).context("could not resolve landing repository")
}
fn source_identity(cwd: &Path) -> anyhow::Result<(String, String)> {
    Ok((
        crate::git_commit::git_stdout(cwd, &["rev-parse", "--verify", "HEAD"])?,
        crate::git_commit::git_optional_stdout(cwd, &["symbolic-ref", "-q", "HEAD"])?
            .unwrap_or_default(),
    ))
}
fn executor() -> anyhow::Result<&'static Arc<Executor>> {
    static EXECUTOR: OnceLock<Result<Arc<Executor>, String>> = OnceLock::new();
    EXECUTOR
        .get_or_init(|| {
            let executor = Arc::new(Executor {
                epoch: Uuid::new_v4(),
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
            });
            for worker in 0..WORKERS {
                let instance = executor.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name(format!("landing-{worker}"))
                    .spawn(move || instance.run())
                {
                    if worker == 0 {
                        return Err(format!("could not start landing executor: {error}"));
                    }
                    break; // A smaller bounded executor is still safe.
                }
            }
            Ok(executor)
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

/// Register one immutable request. The same ID may be retried after a lost
/// response, but cannot be reused for different inputs. All Git reads run on
/// the caller's admission worker, never a UI or control thread.
pub fn enqueue(
    cwd: &Path,
    id: Uuid,
    task: Option<Uuid>,
    kind: LandingKind,
    base: Option<&str>,
    strategy: PullStrategy,
    operation: impl FnOnce(&LandingPermit) -> anyhow::Result<LandingResult> + Send + 'static,
) -> anyhow::Result<LandingJob> {
    let repository = repository(cwd)?;
    let root = crate::git_commit::git_stdout(cwd, &["rev-parse", "--show-toplevel"])?;
    let cwd = dunce::canonicalize(root)?;
    let request = Request {
        cwd: cwd.clone(),
        repository: repository.clone(),
        task,
        kind,
        base: base.map(str::to_owned),
        strategy,
    };
    let executor = executor()?;
    let (head, branch) = source_identity(&cwd)?;
    let mut state = executor.state.lock();
    state.prune();
    if let Some(existing) = state.entries.get(&id) {
        ensure!(
            existing.request == request,
            "landing ID was already used for different inputs"
        );
        return Ok(state.job(id).expect("existing landing"));
    }
    if let Some((original, inputs)) = state.aliases.get(&id) {
        ensure!(
            *inputs == request,
            "landing ID was already used for different inputs"
        );
        return Ok(state.job(*original).expect("aliased landing"));
    }
    // Two clients landing the same source/target attach to one operation.
    let duplicate = state
        .entries
        .values()
        .find(|entry| {
            request.kind == LandingKind::Land
                && matches!(
                    entry.job.state,
                    LandingState::Queued | LandingState::Running
                )
                && entry.request.cwd == request.cwd
                && entry.request.repository == request.repository
                && entry.request.kind == request.kind
                && entry.request.base == request.base
                && entry.request.strategy == request.strategy
                && (entry.job.state == LandingState::Running
                    || (entry.head == head && entry.branch == branch))
        })
        .map(|entry| entry.job.id);
    if let Some(original) = duplicate {
        ensure!(
            state.aliases.len() < 2048,
            "too many landing aliases; wait for existing jobs to expire"
        );
        state.aliases.insert(id, (original, request));
        return Ok(state.job(original).expect("duplicate landing"));
    }
    ensure!(
        state.pending.len() + state.active.len() < MAX_PENDING,
        "landing queue is full; retry after a landing finishes"
    );
    let queue = *state
        .repositories
        .entry(repository)
        .or_insert_with(Uuid::new_v4);
    let job = LandingJob {
        id,
        sequence: state.revision + 1,
        queue_id: queue,
        task_id: task,
        cwd: cwd.to_string_lossy().into_owned(),
        kind,
        base: request.base.clone(),
        strategy,
        state: LandingState::Queued,
        position: None,
        result: None,
    };
    state.entries.insert(
        id,
        Entry {
            request,
            job,
            head,
            branch,
            operation: Some(Box::new(operation)),
            finished: None,
        },
    );
    state.pending.push_back(id);
    state.revision += 1;
    let job = state.job(id).expect("registered landing");
    drop(state);
    executor.wake.notify_all();
    executor.notify(queue);
    Ok(job)
}

pub fn job(id: Uuid) -> anyhow::Result<Option<LandingJob>> {
    let mut state = executor()?.state.lock();
    state.prune();
    Ok(state.job(id))
}
/// Initial discovery may resolve Git paths. Subsequent snapshots use the opaque queue ID.
pub fn inspect(cwd: &Path) -> anyhow::Result<LandingQueueSnapshot> {
    let repository = repository(cwd)?;
    let executor = executor()?;
    let mut state = executor.state.lock();
    let queue = *state
        .repositories
        .entry(repository)
        .or_insert_with(Uuid::new_v4);
    Ok(state.snapshot(executor.epoch, queue))
}
pub fn snapshot(queue: Uuid) -> anyhow::Result<LandingQueueSnapshot> {
    let executor = executor()?;
    let mut state = executor.state.lock();
    state.prune();
    ensure!(
        state.repositories.values().any(|id| *id == queue),
        "landing queue is unavailable; refresh its repository"
    );
    Ok(state.snapshot(executor.epoch, queue))
}
pub fn observe(queue: Uuid, subscriber: u64, observer: Observer) -> anyhow::Result<()> {
    let mut state = executor()?.state.lock();
    if state.observers.len() >= 1024 && !state.observers.contains_key(&(queue, subscriber)) {
        if let Some(key) = state
            .observers
            .iter()
            .min_by_key(|(_, (_, at))| *at)
            .map(|(key, _)| *key)
        {
            state.observers.remove(&key);
        }
    }
    state
        .observers
        .insert((queue, subscriber), (observer, Instant::now()));
    Ok(())
}
pub fn cancel(id: Uuid) -> anyhow::Result<CancelLandingOutcome> {
    let executor = executor()?;
    let mut state = executor.state.lock();
    let id = state.aliases.get(&id).map(|(id, _)| *id).unwrap_or(id);
    let Some(entry) = state.entries.get_mut(&id) else {
        return Ok(CancelLandingOutcome::NotFound);
    };
    match entry.job.state {
        LandingState::Running => return Ok(CancelLandingOutcome::AlreadyRunning),
        LandingState::Finished => return Ok(CancelLandingOutcome::AlreadyFinished),
        LandingState::Canceled => return Ok(CancelLandingOutcome::Canceled),
        LandingState::Queued => {}
    }
    entry.job.state = LandingState::Canceled;
    entry.finished = Some(Instant::now());
    let operation = entry.operation.take();
    let queue = entry.job.queue_id;
    state.pending.retain(|job| *job != id);
    state.revision += 1;
    drop(state);
    drop(operation);
    executor.wake.notify_all();
    executor.notify(queue);
    Ok(CancelLandingOutcome::Canceled)
}
/// Legacy synchronous callers retain their result contract, while server
/// dispatch keeps these bounded parked waits out of its heavy worker pool.
pub fn wait(id: Uuid) -> anyhow::Result<LandingResult> {
    let executor = executor()?;
    let mut state = executor.state.lock();
    loop {
        let id = state.aliases.get(&id).map(|(id, _)| *id).unwrap_or(id);
        let entry = state
            .entries
            .get(&id)
            .context("landing result expired or daemon restarted")?;
        match entry.job.state {
            LandingState::Canceled => bail!("landing canceled before it started"),
            LandingState::Finished => {
                return Ok(entry.job.result.clone().context("landing has no result")?);
            }
            _ => executor.wake.wait(&mut state),
        }
    }
}
impl Entry {
    // Do not clone terminal payloads during position refreshes. A client reads
    // its result once with LandingJob, independently of the shared snapshot.
    fn metadata(&self) -> LandingJob {
        LandingJob {
            id: self.job.id,
            sequence: self.job.sequence,
            queue_id: self.job.queue_id,
            task_id: self.job.task_id,
            cwd: self.job.cwd.clone(),
            kind: self.job.kind,
            base: self.job.base.clone(),
            strategy: self.job.strategy,
            state: self.job.state,
            position: None,
            result: None,
        }
    }
}
impl State {
    fn job(&self, id: Uuid) -> Option<LandingJob> {
        let id = self.aliases.get(&id).map(|(id, _)| *id).unwrap_or(id);
        let entry = self.entries.get(&id)?;
        let mut job = entry.metadata();
        job.result = entry.job.result.clone();
        if job.state == LandingState::Queued {
            job.position = self
                .pending
                .iter()
                .filter(|id| {
                    self.entries
                        .get(id)
                        .is_some_and(|e| e.job.queue_id == job.queue_id)
                })
                .position(|candidate| *candidate == id)
                .map(|index| (index + 1) as u32);
        }
        Some(job)
    }
    fn snapshot(&self, epoch: Uuid, queue: Uuid) -> LandingQueueSnapshot {
        let positions: HashMap<_, _> = self
            .pending
            .iter()
            .filter(|id| {
                self.entries
                    .get(id)
                    .is_some_and(|entry| entry.job.queue_id == queue)
            })
            .enumerate()
            .map(|(index, id)| (*id, (index + 1) as u32))
            .collect();
        let mut jobs: Vec<_> = self
            .entries
            .values()
            .filter(|entry| entry.job.queue_id == queue)
            .map(|entry| {
                let mut job = entry.metadata();
                job.position = positions.get(&job.id).copied();
                job
            })
            .collect();
        jobs.sort_by_key(|job| {
            (
                match job.state {
                    LandingState::Running => 0,
                    LandingState::Queued => 1,
                    _ => 2,
                },
                job.position.unwrap_or(0),
                job.id,
            )
        });
        LandingQueueSnapshot {
            epoch,
            queue_id: queue,
            revision: self.revision,
            jobs,
        }
    }
    fn prune(&mut self) {
        let now = Instant::now();
        self.entries.retain(|_, entry| {
            entry
                .finished
                .is_none_or(|at| now.duration_since(at) < RETENTION)
        });
        let mut finished: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| entry.finished.map(|at| (*id, at)))
            .collect();
        finished.sort_by_key(|(_, at)| *at);
        for (id, _) in finished.into_iter().rev().skip(MAX_FINISHED) {
            self.entries.remove(&id);
        }
        self.aliases
            .retain(|_, (id, _)| self.entries.contains_key(id));
        self.observers
            .retain(|_, (_, at)| now.duration_since(*at) < RETENTION);
        self.repositories.retain(|_, queue| {
            self.entries
                .values()
                .any(|entry| entry.job.queue_id == *queue)
                || self.observers.keys().any(|(id, _)| id == queue)
        });
    }
}
impl Executor {
    fn notify(&self, queue: Uuid) {
        let (revision, observers) = {
            let state = self.state.lock();
            (
                state.revision,
                state
                    .observers
                    .iter()
                    .filter(|((id, _), _)| *id == queue)
                    .map(|(key, value)| (*key, value.0.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        for (key, observer) in observers {
            let connected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                observer(queue, revision)
            }))
            .unwrap_or(false);
            if !connected {
                self.state.lock().observers.remove(&key);
            }
        }
    }
    fn run(self: Arc<Self>) {
        loop {
            let (id, queue, request, head, branch, operation) = {
                let mut state = self.state.lock();
                loop {
                    let next = state
                        .pending
                        .iter()
                        .position(|id| !state.active.contains(&state.entries[id].job.queue_id));
                    if let Some(index) = next {
                        let id = state.pending.remove(index).expect("pending landing");
                        let entry = state.entries.get_mut(&id).expect("pending entry");
                        entry.job.state = LandingState::Running;
                        let queue = entry.job.queue_id;
                        let work = (
                            id,
                            queue,
                            entry.request.clone(),
                            entry.head.clone(),
                            entry.branch.clone(),
                            entry.operation.take().expect("queued operation"),
                        );
                        state.active.insert(queue);
                        state.revision += 1;
                        break work;
                    }
                    self.wake.wait(&mut state);
                }
            };
            self.notify(queue);
            let permit = LandingPermit {
                executor: self.clone(),
                id,
                repository: request.repository,
                result: None,
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ensure!(
                    source_identity(&request.cwd)? == (head, branch),
                    "worktree changed while queued; run /land again"
                );
                operation(&permit)
            }));
            let result = match result {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => LandingResult::Failed {
                    message: format!("{error:#}"),
                },
                Err(_) => LandingResult::Failed {
                    message: "landing worker failed; inspect the worktree before retrying".into(),
                },
            };
            permit.complete(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_commit::{git_stdout, git_success};
    use std::sync::mpsc;

    struct Repo(PathBuf);
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn repo() -> Repo {
        let path = std::env::temp_dir().join(format!("landing-jobs-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.com"],
            vec!["commit", "--allow-empty", "-qm", "initial"],
        ] {
            git_success(&path, &args).unwrap();
        }
        Repo(path)
    }
    fn add(
        cwd: &Path,
        id: Uuid,
        operation: impl FnOnce(&LandingPermit) -> anyhow::Result<LandingResult> + Send + 'static,
    ) -> LandingJob {
        enqueue(
            cwd,
            id,
            None,
            LandingKind::Land,
            Some(&id.to_string()),
            PullStrategy::Rebase,
            operation,
        )
        .unwrap()
    }
    fn done() -> anyhow::Result<LandingResult> {
        Ok(LandingResult::Submitted { sha: "test".into() })
    }

    #[test]
    fn cancellation_preserves_git_and_order_while_other_repositories_progress() {
        let repo = repo();
        let initial = git_stdout(&repo.0, &["rev-parse", "HEAD"]).unwrap();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let first = add(&repo.0, Uuid::new_v4(), move |_| {
            started.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            done()
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let marker = repo.0.join("must-not-exist");
        let second = add(&repo.0, Uuid::new_v4(), move |_| {
            std::fs::write(marker, "unexpected mutation")?;
            done()
        });
        let third = add(&repo.0, Uuid::new_v4(), |_| done());
        assert_eq!(job(second.id).unwrap().unwrap().position, Some(1));
        assert_eq!(job(third.id).unwrap().unwrap().position, Some(2));
        assert_eq!(
            cancel(first.id).unwrap(),
            CancelLandingOutcome::AlreadyRunning
        );
        assert_eq!(cancel(second.id).unwrap(), CancelLandingOutcome::Canceled);
        assert_eq!(cancel(second.id).unwrap(), CancelLandingOutcome::Canceled);
        assert_eq!(job(third.id).unwrap().unwrap().position, Some(1));
        let retry = add(&repo.0, third.id, |_| {
            panic!("duplicate operation executed")
        });
        assert_eq!(retry.id, third.id);
        let alias_id = Uuid::new_v4();
        let alias = enqueue(
            &repo.0,
            alias_id,
            Some(Uuid::new_v4()),
            LandingKind::Land,
            Some(&third.id.to_string()),
            PullStrategy::Rebase,
            |_| panic!("second client executed a duplicate"),
        )
        .unwrap();
        assert_eq!(alias.id, third.id);
        assert_eq!(job(alias_id).unwrap().unwrap().id, third.id);
        assert!(
            enqueue(
                &repo.0,
                third.id,
                None,
                LandingKind::Submit,
                Some("main"),
                PullStrategy::Rebase,
                |_| done()
            )
            .is_err()
        );
        let other = self::repo();
        let independent = add(&other.0, Uuid::new_v4(), |_| done());
        assert!(matches!(
            wait(independent.id).unwrap(),
            LandingResult::Submitted { .. }
        ));
        release.send(()).unwrap();
        wait(first.id).unwrap();
        wait(third.id).unwrap();
        assert!(job(alias_id).unwrap().unwrap().result.is_some());
        assert!(
            snapshot(third.queue_id)
                .unwrap()
                .jobs
                .iter()
                .all(|entry| entry.result.is_none())
        );
        assert!(!repo.0.join("must-not-exist").exists());
        assert_eq!(
            git_stdout(&repo.0, &["rev-parse", "HEAD"]).unwrap(),
            initial
        );
        assert_eq!(
            cancel(third.id).unwrap(),
            CancelLandingOutcome::AlreadyFinished
        );
        assert!(wait(second.id).is_err());
    }

    #[test]
    fn queued_source_changes_fail_and_panics_release_the_repository() {
        let repo = repo();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let first = add(&repo.0, Uuid::new_v4(), move |_| {
            started.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            done()
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let changed = add(&repo.0, Uuid::new_v4(), |_| {
            panic!("changed source must not execute")
        });
        git_success(&repo.0, &["commit", "--allow-empty", "-qm", "new work"]).unwrap();
        release.send(()).unwrap();
        wait(first.id).unwrap();
        assert!(
            matches!(wait(changed.id).unwrap(), LandingResult::Failed { message } if message.contains("worktree changed while queued"))
        );
        let panic = add(&repo.0, Uuid::new_v4(), |_| panic!("worker failure"));
        assert!(matches!(
            wait(panic.id).unwrap(),
            LandingResult::Failed { .. }
        ));
        let next = add(&repo.0, Uuid::new_v4(), |_| done());
        assert!(matches!(
            wait(next.id).unwrap(),
            LandingResult::Submitted { .. }
        ));
    }
}
