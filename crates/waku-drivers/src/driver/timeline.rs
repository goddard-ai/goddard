//! Bounded, best-effort Devin diagnostics. Payloads deliberately cannot carry
//! prompt text, provider errors, paths, or credentials. All disk work is owned
//! by one background writer per daemon; producers only try a bounded queue.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

const SEGMENT_BYTES: u64 = 1_000_000;
const MAX_SEGMENTS: usize = 20;
const RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelSource {
    Controller,
    ResponseTimeout,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopCause {
    Complete,
    Cancelled,
    Preempted,
    External,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionReason {
    Idle,
    MemoryPressure,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    RuntimeLaunchRequested {
        resuming: bool,
    },
    RuntimeCreated,
    Submission {
        message_id: Uuid,
        runtime_reused: bool,
    },
    PhaseStarted {
        phase: &'static str,
    },
    PhaseFinished {
        phase: &'static str,
        elapsed_ms: u64,
        outcome: Outcome,
    },
    PromptDispatched,
    Connected,
    FirstSessionUpdate {
        replaying: bool,
    },
    ShutdownRequested,
    ProviderStarted,
    TurnFinished {
        success: bool,
    },
    CancellationRequested {
        source: CancelSource,
    },
    Evicted {
        reason: EvictionReason,
    },
    ProviderStopped {
        cause: StopCause,
    },
    ConnectionExited,
    ProviderError,
}

#[derive(Serialize)]
struct Record {
    at_ms: u64,
    task_id: Uuid,
    runtime_id: Uuid,
    turn_id: Option<Uuid>,
    dropped_before: u64,
    #[serde(flatten)]
    event: Event,
}

#[derive(Clone, Default)]
pub struct Timeline {
    sender: Option<mpsc::SyncSender<Record>>,
    dropped: Arc<AtomicU64>,
}

impl Timeline {
    /// No filesystem access here. A failed writer/queue never fails a prompt.
    pub fn start(directory: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Record>(1024);
        let dropped = Arc::new(AtomicU64::new(0));
        let worker_dropped = dropped.clone();
        let worker = std::thread::Builder::new()
            .name("devin-timeline".into())
            .spawn(move || {
                let writer = Writer {
                    directory,
                    segment_bytes: SEGMENT_BYTES,
                    max_segments: MAX_SEGMENTS,
                    retention_ms: RETENTION_MS,
                };
                // Also sweep when idle, including after a daemon restart. Expiry
                // can lag by at most this interval while the daemon is running.
                let _ = writer.prune(now_ms(), false);
                loop {
                    match receiver.recv_timeout(Duration::from_secs(60)) {
                        Ok(record) => {
                            if writer.append(&record, now_ms()).is_err() {
                                worker_dropped.fetch_add(
                                    record.dropped_before.saturating_add(1),
                                    Ordering::Relaxed,
                                );
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let _ = writer.prune(now_ms(), false);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        Self {
            sender: worker.ok().map(|_| sender),
            dropped,
        }
    }

    pub fn runtime(&self, task_id: Uuid, runtime_id: Uuid, resuming: bool) -> RuntimeTimeline {
        let runtime = RuntimeTimeline {
            timeline: self.clone(),
            task_id,
            runtime_id,
            state: Arc::new(Mutex::new(RuntimeState::default())),
            context_known: Arc::new(AtomicBool::new(true)),
        };
        runtime.record(Event::RuntimeLaunchRequested { resuming });
        runtime
    }
}

#[derive(Clone)]
pub struct RuntimeTimeline {
    timeline: Timeline,
    task_id: Uuid,
    runtime_id: Uuid,
    state: Arc<Mutex<RuntimeState>>,
    context_known: Arc<AtomicBool>,
}

#[derive(Default)]
struct RuntimeState {
    turn_id: Option<Uuid>,
    submitted: bool,
    update_seen: bool,
}

impl RuntimeTimeline {
    pub fn submission(&self, turn_id: Uuid, message_id: Uuid) {
        let Some(mut state) = self.state.try_lock() else {
            // A missed submission must not attribute its later events to the
            // previous turn. Leave the turn unknown until the next submission.
            self.context_known.store(false, Ordering::Relaxed);
            self.timeline.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.context_known.store(true, Ordering::Relaxed);
        let reused = state.submitted;
        *state = RuntimeState {
            turn_id: Some(turn_id),
            submitted: true,
            update_seen: false,
        };
        self.enqueue(
            state.turn_id,
            Event::Submission {
                message_id,
                runtime_reused: reused,
            },
        );
    }

    pub fn first_update(&self, replaying: bool) {
        let Some(mut state) = self.state.try_lock() else {
            return;
        };
        if !state.update_seen {
            state.update_seen = true;
            self.enqueue(state.turn_id, Event::FirstSessionUpdate { replaying });
        }
    }

    pub fn record(&self, event: Event) {
        let Some(state) = self.state.try_lock() else {
            self.timeline.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.enqueue(state.turn_id, event);
    }

    fn enqueue(&self, turn_id: Option<Uuid>, event: Event) {
        let Some(sender) = &self.timeline.sender else {
            return;
        };
        let turn_id = self
            .context_known
            .load(Ordering::Relaxed)
            .then_some(turn_id)
            .flatten();
        let dropped = self.timeline.dropped.swap(0, Ordering::Relaxed);
        let record = Record {
            at_ms: now_ms(),
            task_id: self.task_id,
            runtime_id: self.runtime_id,
            turn_id,
            event,
            dropped_before: dropped,
        };
        if sender.try_send(record).is_err() {
            self.timeline
                .dropped
                .fetch_add(dropped.saturating_add(1), Ordering::Relaxed);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

struct Writer {
    directory: PathBuf,
    segment_bytes: u64,
    max_segments: usize,
    retention_ms: u64,
}

impl Writer {
    // Only our named regular files belong to this store. Never follow links
    // or remove unrelated files. Each segment expires from its FIRST record,
    // so later writes cannot keep older records alive beyond retention.
    fn prune(&self, now: u64, reserve_segment: bool) -> io::Result<Vec<(u64, PathBuf, u64)>> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(rest) = name
                .strip_prefix("trace-")
                .and_then(|s| s.strip_suffix(".jsonl"))
            else {
                continue;
            };
            let Some((stamp, id)) = rest.split_once('_') else {
                continue;
            };
            let (Ok(stamp), Ok(_)) = (stamp.parse::<u64>(), Uuid::parse_str(id)) else {
                continue;
            };
            let metadata = entry.path().symlink_metadata()?;
            if !metadata.is_file() {
                continue;
            }
            if now.saturating_sub(stamp) >= self.retention_ms || metadata.len() > self.segment_bytes
            {
                fs::remove_file(entry.path())?;
            } else {
                files.push((stamp, entry.path(), metadata.len()));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let limit = self
            .max_segments
            .saturating_sub(usize::from(reserve_segment));
        while files.len() > limit {
            fs::remove_file(&files[0].1)?;
            files.remove(0);
        }
        Ok(files)
    }

    fn append(&self, record: &Record, now: u64) -> io::Result<()> {
        if now.saturating_sub(record.at_ms) >= self.retention_ms {
            self.prune(now, false)?;
            return Ok(());
        }
        let mut bytes = serde_json::to_vec(record)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > self.segment_bytes {
            return Ok(());
        }
        crate::fs_ext::create_private_dir_all(&self.directory)?;
        let files = self.prune(now, false)?;
        let current = files.last().filter(|(stamp, _, size)| {
            *stamp <= record.at_ms && size + bytes.len() as u64 <= self.segment_bytes
        });
        let mut file = if let Some((_, path, _)) = current {
            OpenOptions::new().append(true).open(path)?
        } else {
            self.prune(now, true)?;
            let path = self.directory.join(format!(
                "trace-{}_{}.jsonl",
                now.min(record.at_ms),
                Uuid::new_v4()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)?
        };
        file.write_all(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_and_expiry_remain_bounded_across_restart() {
        let directory = std::env::temp_dir().join(format!("devin-timeline-{}", Uuid::new_v4()));
        let writer = Writer {
            directory: directory.clone(),
            segment_bytes: 600,
            max_segments: 3,
            retention_ms: 1000,
        };
        let record = Record {
            at_ms: 100,
            task_id: Uuid::new_v4(),
            runtime_id: Uuid::new_v4(),
            turn_id: Some(Uuid::new_v4()),
            dropped_before: 0,
            event: Event::PromptDispatched,
        };
        for at in 100..150 {
            writer.append(&record, at).unwrap();
        }
        let files = writer.prune(150, false).unwrap();
        assert!(files.len() <= 3);
        assert!(files.iter().map(|f| f.2).sum::<u64>() <= 1800);
        assert!(files.iter().all(|f| f.2 <= 600));
        fs::write(directory.join("unrelated.txt"), "keep").unwrap();
        // A fresh writer discovers retention from on-disk names. Time spent
        // waiting in the queue does not extend the first event's lifetime.
        let restarted = Writer {
            directory: directory.clone(),
            ..writer
        };
        assert!(restarted.prune(1100, false).unwrap().is_empty());
        assert!(directory.join("unrelated.txt").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn native_event_trace_keeps_ids_and_excludes_payloads() {
        let (sender, receiver) = mpsc::sync_channel(32);
        let timeline = Timeline {
            sender: Some(sender),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let task = Uuid::new_v4();
        let runtime = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let context = timeline.runtime(task, runtime, false);
        let (events, _native_events) = crate::driver::test_event_channel();
        let events = events.with_timeline(context.clone());
        context.submission(turn, Uuid::new_v4());
        events.trace_first_update(false);
        events.trace_first_update(false); // streaming updates do not fill the store
        events
            .send(crate::model::DriverEvent::TextDelta(
                "private prompt or response".into(),
            ))
            .unwrap();
        events
            .send(crate::model::DriverEvent::Error(
                "credential-shaped provider error".into(),
            ))
            .unwrap();
        events.send(crate::model::DriverEvent::TurnStarted).unwrap();
        events
            .send(crate::model::DriverEvent::TurnFinished {
                success: false,
                summary: None,
                summary_i18n: None,
            })
            .unwrap();
        let records: Vec<_> = receiver.try_iter().collect();
        assert_eq!(records.len(), 6);
        assert!(
            records[1..]
                .iter()
                .all(|r| r.task_id == task && r.runtime_id == runtime && r.turn_id == Some(turn))
        );
        let json = serde_json::to_string(&records).unwrap();
        assert!(!json.contains("private prompt"));
        assert!(!json.contains("credential-shaped"));
        assert_eq!(json.matches("first_session_update").count(), 1);
    }

    #[test]
    fn storage_failure_does_not_touch_unrelated_data() {
        let path = std::env::temp_dir().join(format!("devin-timeline-blocked-{}", Uuid::new_v4()));
        fs::write(&path, b"unrelated data").unwrap();
        let writer = Writer {
            directory: path.clone(),
            segment_bytes: 600,
            max_segments: 3,
            retention_ms: 1000,
        };
        let record = Record {
            at_ms: 100,
            task_id: Uuid::new_v4(),
            runtime_id: Uuid::new_v4(),
            turn_id: None,
            dropped_before: 0,
            event: Event::PromptDispatched,
        };
        assert!(writer.append(&record, 100).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"unrelated data");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn overload_never_waits_and_records_dropped_count_with_correlated_ids() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let timeline = Timeline {
            sender: Some(sender),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let task = Uuid::new_v4();
        let runtime = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let message = Uuid::new_v4();
        let context = timeline.runtime(task, runtime, true);
        context.submission(turn, message); // full: drop instead of blocking
        receiver.recv().unwrap();
        context.record(Event::PromptDispatched);
        let record = receiver.recv().unwrap();
        assert_eq!(
            (record.task_id, record.runtime_id, record.turn_id),
            (task, runtime, Some(turn))
        );
        assert_eq!(record.dropped_before, 1);
        let json = serde_json::to_value(record).unwrap();
        assert_eq!(json["event"], "prompt_dispatched");
        assert!(json.get("content").is_none());
        let held = context.state.lock();
        context.submission(Uuid::new_v4(), Uuid::new_v4());
        drop(held);
        context.record(Event::PromptDispatched);
        let unknown = receiver.recv().unwrap();
        assert_eq!(unknown.turn_id, None);
        assert_eq!(unknown.dropped_before, 1);
    }
}
