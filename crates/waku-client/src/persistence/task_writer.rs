//! Mandatory task saves, ordered per connection. UI snapshots share history;
//! preparation, hashing, frame sizing, transport, and disposal run on workers.

use super::*;
use crate::DaemonClient;
use waku_protocol::{SessionDetailSaveOutcome, SessionSaveReceipt};

type Outcome = Result<(), (io::ErrorKind, String)>;

pub(super) struct SessionSnapshot {
    pub session: AgentSession,
    /// Reuse the version of an unchanged, unacknowledged snapshot.
    pub version: Option<u64>,
    pub baseline: Option<SaveCursor>,
}

pub(super) struct Snapshot {
    pub projects: Vec<Project>,
    pub live_session_ids: Vec<Uuid>,
    pub sessions: Vec<SessionSnapshot>,
}

pub(super) struct SessionResult {
    pub session_id: Uuid,
    pub version: u64,
    pub error: Option<String>,
}

#[derive(Default)]
struct Pending {
    issued: u64,
    durable: u64,
    finished: u64,
    next: Option<(u64, Snapshot)>,
    results: Vec<SessionResult>,
    error: Option<String>,
    outcome: Option<Outcome>,
    waiters: Vec<(u64, smol::channel::Sender<Outcome>)>,
    stopped: bool,
}

#[derive(Clone)]
pub(super) struct Writer {
    pub id: Uuid,
    client: DaemonClient,
    pending: Arc<Mutex<Pending>>,
    wake: crossbeam_channel::Sender<()>,
}

pub struct TaskSaveTicket {
    writer: Writer,
    generation: u64,
}

fn outcome(result: Outcome) -> io::Result<()> {
    result.map_err(|(kind, message)| io::Error::new(kind, message))
}

impl TaskSaveTicket {
    pub async fn wait(self) -> io::Result<()> {
        let receiver = {
            let mut pending = self.writer.pending.lock();
            if pending.durable >= self.generation {
                return Ok(());
            }
            if pending.finished >= self.generation || pending.stopped {
                return outcome(pending.outcome.clone().unwrap_or(Ok(())));
            }
            let (sender, receiver) = smol::channel::bounded(1);
            pending.waiters.retain(|(_, sender)| !sender.is_closed());
            pending.waiters.push((self.generation, sender));
            receiver
        };
        outcome(receiver.recv().await.map_err(|_| {
            io::Error::other("task save worker stopped before acknowledging the save")
        })?)
    }
}

struct WorkerLife(Arc<Mutex<Pending>>);

impl Drop for WorkerLife {
    fn drop(&mut self) {
        let mut pending = self.0.lock();
        pending.stopped = true;
        if pending.finished < pending.issued {
            let message = "task save worker stopped before acknowledging the save".to_owned();
            let error = Err((io::ErrorKind::BrokenPipe, message.clone()));
            pending.error = Some(message);
            pending.outcome = Some(error.clone());
            for (_, waiter) in pending.waiters.drain(..) {
                let _ = waiter.try_send(error.clone());
            }
        }
    }
}

/// Reclamation must not wait for a slow RPC or for idle CPU. Use the existing
/// async/blocking pool rather than spawning a new native thread per snapshot.
fn retire(value: impl Send + 'static) {
    smol::spawn(async move { smol::unblock(move || drop(value)).await }).detach();
}

impl Writer {
    pub(super) fn start(client: DaemonClient) -> io::Result<Self> {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let shared = Arc::clone(&pending);
        let (wake, receiver) = crossbeam_channel::bounded(1);
        let connection = client.clone();
        std::thread::Builder::new()
            .name("waku-task-save".into())
            .spawn(move || {
                let _life = WorkerLife(Arc::clone(&shared));
                let mut cursors = HashMap::new();
                let mut seeded = HashSet::new();
                let mut header_written = None;
                while receiver.recv().is_ok() {
                    loop {
                        let Some((generation, snapshot)) = shared.lock().next.take() else {
                            break;
                        };
                        let (result, results) = persist_snapshot(
                            &connection,
                            snapshot,
                            &mut cursors,
                            &mut seeded,
                            &mut header_written,
                        );
                        let mut pending = shared.lock();
                        pending.finished = generation;
                        if result.is_ok() {
                            pending.durable = generation;
                        } else if let Err((_, message)) = &result {
                            pending.error = Some(message.clone());
                        }
                        pending.results.extend(results);
                        pending.outcome = Some(result.clone());
                        pending.waiters.retain(|(requested, sender)| {
                            if *requested <= generation {
                                let _ = sender.try_send(result.clone());
                                false
                            } else {
                                !sender.is_closed()
                            }
                        });
                    }
                }
            })?;
        Ok(Self {
            id: Uuid::new_v4(),
            client,
            pending,
            wake,
        })
    }

    pub(super) fn matches(&self, client: &DaemonClient) -> bool {
        self.client.same_connection(client) && !self.pending.lock().stopped
    }

    /// Each replacement must include all still-unacknowledged live rows, not
    /// just newly dirty rows. That makes coalescing and ticket coverage safe.
    pub(super) fn queue(&self, mut snapshot: Snapshot) -> io::Result<Vec<(Uuid, u64)>> {
        let (versions, retired) = {
            let mut pending = self.pending.lock();
            if pending.stopped {
                return Err(io::Error::other("task save worker is closed"));
            }
            pending.issued = pending
                .issued
                .checked_add(1)
                .ok_or_else(|| io::Error::other("task save generation exhausted"))?;
            let generation = pending.issued;
            let versions = snapshot
                .sessions
                .iter_mut()
                .map(|row| {
                    let version = *row.version.get_or_insert(generation);
                    (row.session.id, version)
                })
                .collect();
            (versions, pending.next.replace((generation, snapshot)))
        };
        if retired.is_some() {
            retire(retired);
        }
        match self.wake.try_send(()) {
            Ok(()) | Err(crossbeam_channel::TrySendError::Full(())) => Ok(versions),
            Err(crossbeam_channel::TrySendError::Disconnected(())) => {
                Err(io::Error::other("task save worker is closed"))
            }
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        let pending = self.pending.lock();
        pending.finished < pending.issued && !pending.stopped
    }

    pub(super) fn ticket(&self) -> TaskSaveTicket {
        TaskSaveTicket {
            writer: self.clone(),
            generation: self.pending.lock().issued,
        }
    }

    pub(super) fn status(&self) -> (bool, Vec<SessionResult>, Option<String>) {
        let mut pending = self.pending.lock();
        (
            pending.finished < pending.issued && !pending.stopped,
            std::mem::take(&mut pending.results),
            pending.error.take(),
        )
    }
}

fn persist_snapshot(
    client: &DaemonClient,
    snapshot: Snapshot,
    cursors: &mut HashMap<Uuid, SaveCursor>,
    seeded: &mut HashSet<Uuid>,
    header_written: &mut Option<u64>,
) -> (Outcome, Vec<SessionResult>) {
    let header = serde_json::to_vec(&(&snapshot.projects, &snapshot.live_session_ids))
        .map(|bytes| content_fingerprint(&bytes));
    let header = match header {
        Ok(header) => header,
        Err(error) => {
            return (
                Err((io::ErrorKind::InvalidData, error.to_string())),
                Vec::new(),
            );
        }
    };
    if snapshot.sessions.is_empty() && *header_written == Some(header) {
        return (Ok(()), Vec::new());
    }
    let live = snapshot
        .live_session_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    cursors.retain(|id, _| live.contains(id));
    seeded.retain(|id| live.contains(id));
    for row in &snapshot.sessions {
        if seeded.insert(row.session.id)
            && let Some(cursor) = row.baseline
        {
            cursors.insert(row.session.id, cursor);
        }
    }
    let rows = snapshot
        .sessions
        .iter()
        .map(|row| (row.session.id, row))
        .collect::<HashMap<_, _>>();
    let mut results = HashMap::<Uuid, Option<String>>::new();
    let prepared = snapshot
        .sessions
        .iter()
        .map(|row| prepare(&row.session, cursors.get(&row.session.id).copied()))
        .collect();
    let mut first_error = None;
    let mut retry = HashSet::new();
    for frame in pack_save_frames(prepared) {
        match send_frame(client, &snapshot, frame, &rows, cursors, &mut results) {
            Ok(required) => retry.extend(required),
            Err(error) => {
                first_error = Some(error);
                break;
            }
        }
    }
    // A rejected splice gets one immediate full retry on this same connection.
    // Server deletion fences still apply if the task was removed meanwhile.
    if first_error.is_none() && !retry.is_empty() {
        let prepared = retry
            .iter()
            .filter_map(|id| rows.get(id))
            .map(|row| prepare(&row.session, None))
            .collect();
        for frame in pack_save_frames(prepared) {
            match send_frame(client, &snapshot, frame, &rows, cursors, &mut results) {
                Ok(required) => {
                    for id in required {
                        results.insert(
                            id,
                            Some("daemon rejected the full transcript snapshot".into()),
                        );
                    }
                }
                Err(error) => {
                    first_error = Some(error);
                    break;
                }
            }
        }
    }
    let mut completed = Vec::with_capacity(rows.len());
    for row in &snapshot.sessions {
        let error = results.remove(&row.session.id).unwrap_or_else(|| {
            Some(first_error.as_ref().map_or_else(
                || "daemon did not acknowledge the task save".into(),
                ToString::to_string,
            ))
        });
        if let Some(message) = &error {
            first_error.get_or_insert_with(|| io::Error::other(message.clone()));
        }
        completed.push(SessionResult {
            session_id: row.session.id,
            version: row.version.expect("queued row has a version"),
            error,
        });
    }
    if first_error.is_none() {
        *header_written = Some(header);
    }
    (
        first_error.map_or(Ok(()), |error| Err((error.kind(), error.to_string()))),
        completed,
    )
}

fn prepare(session: &AgentSession, cursor: Option<SaveCursor>) -> SizedSaveSession {
    let mut tails = Vec::new();
    let (wire, cursor) = wire_session(session, cursor, &mut tails);
    SizedSaveSession {
        bytes: serialized_len(&wire).unwrap_or(usize::MAX),
        wire,
        tail: tails.into_iter().next(),
        cursor,
    }
}

fn send_frame(
    client: &DaemonClient,
    snapshot: &Snapshot,
    frame: Vec<SizedSaveSession>,
    rows: &HashMap<Uuid, &SessionSnapshot>,
    cursors: &mut HashMap<Uuid, SaveCursor>,
    results: &mut HashMap<Uuid, Option<String>>,
) -> io::Result<HashSet<Uuid>> {
    let mut frame_cursors = HashMap::new();
    let mut tails = Vec::new();
    let mut oversized = HashSet::new();
    let mut ids = Vec::new();
    let sessions = frame
        .into_iter()
        .map(|item| {
            let id = item.wire.id;
            ids.push(id);
            if rows[&id].session.detail_loaded && !item.wire.detail_loaded {
                oversized.insert(id);
            }
            if let Some(cursor) = item.cursor {
                frame_cursors.insert(id, cursor);
            }
            if let Some(tail) = item.tail {
                tails.push(tail);
            }
            item.wire
        })
        .collect();
    let reply = client
        .request_bulk(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: snapshot.projects.clone(),
                live_session_ids: snapshot.live_session_ids.clone(),
                sessions,
                session_tails: tails,
                compact_response: Some(true),
            },
        )
        .map_err(to_io_error)?;
    let ResponsePayload::TaskStateSaved { sessions, receipts } = reply else {
        return Err(io::Error::other(
            "daemon returned an invalid task save acknowledgement",
        ));
    };
    let saved = sessions
        .into_iter()
        .map(|session| (session.id, session))
        .collect::<HashMap<_, _>>();
    let receipts = receipts.map(|receipts| {
        receipts
            .into_iter()
            .map(|receipt: SessionSaveReceipt| (receipt.session_id, receipt.detail))
            .collect::<HashMap<_, _>>()
    });
    let mut retry = HashSet::new();
    for id in ids {
        let row = rows[&id];
        if receipts
            .as_ref()
            .is_some_and(|receipts| receipts.get(&id) == Some(&SessionDetailSaveOutcome::Removed))
        {
            results.insert(id, None);
            cursors.remove(&id);
            continue;
        }
        let Some(saved) = saved.get(&id) else {
            results.insert(id, Some("daemon did not accept the task save".into()));
            cursors.remove(&id);
            continue;
        };
        if oversized.contains(&id) {
            results.insert(
                id,
                Some("task transcript exceeds the save frame budget".into()),
            );
            cursors.remove(&id);
            continue;
        }
        if let Some(receipts) = &receipts {
            match receipts.get(&id) {
                Some(SessionDetailSaveOutcome::Applied) => {
                    if let Some(cursor) = frame_cursors.remove(&id) {
                        cursors.insert(id, cursor);
                    }
                    results.insert(id, None);
                }
                Some(
                    SessionDetailSaveOutcome::Rewritten | SessionDetailSaveOutcome::DaemonOwned,
                ) => {
                    cursors.remove(&id);
                    results.insert(id, None);
                }
                Some(SessionDetailSaveOutcome::FullRequired) => {
                    cursors.remove(&id);
                    retry.insert(id);
                }
                Some(SessionDetailSaveOutcome::MetadataOnly) if !row.session.detail_loaded => {
                    cursors.remove(&id);
                    results.insert(id, None);
                }
                _ => {
                    cursors.remove(&id);
                    results.insert(
                        id,
                        Some("daemon did not confirm the submitted transcript".into()),
                    );
                }
            }
        } else {
            // Older daemons echo the canonical detail. It can confirm a
            // prefix, but a list-only echo cannot acknowledge loaded data.
            if row.session.detail_loaded && !saved.detail_loaded {
                cursors.remove(&id);
                results.insert(
                    id,
                    Some("daemon did not confirm the submitted transcript".into()),
                );
                continue;
            }
            if let Some(cursor) = frame_cursors.remove(&id) {
                if saved.messages.len() == cursor.messages
                    && saved.transcript_blocks.len() == cursor.blocks
                    && detail_prefix_signature(&saved.messages, &saved.transcript_blocks)
                        == cursor.signature
                {
                    cursors.insert(id, cursor);
                } else {
                    cursors.remove(&id);
                }
            }
            results.insert(id, None);
        }
    }
    Ok(retry)
}
