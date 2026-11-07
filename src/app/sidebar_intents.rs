//! Client-owned desired sidebar state. The coordinator journals before dispatch,
//! serializes each host independently, and retains failed intent for recovery.
use super::*;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write};
use waku_client::DaemonKey;
use waku_protocol::boss::BossState;
use waku_protocol::sidebar_intent::{SidebarEdit, SidebarIntent, SidebarIntentResult};

const MAX_ATTEMPTS: u32 = 8;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
enum Phase {
    Queued,
    InFlight,
    Blocked,
    Confirmed(u64),
    Preflight,
    Resolving,
    Removed,
}
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Entry {
    pub key: DaemonKey,
    pub intent: SidebarIntent,
    batch: Uuid,
    adopting: bool,
    at: u64,
    attempts: u32,
    due: u64,
    phase: Phase,
    reason: Option<String>,
}
impl Entry {
    fn identity(&self) -> (DaemonKey, String) {
        (self.key, self.intent.edit.key())
    }
    fn archiving(&self) -> bool {
        self.phase != Phase::Removed
            && matches!(
                &self.intent.edit,
                SidebarEdit::Task {
                    edit: waku_protocol::task_metadata::TaskMetadataEdit::Archive {
                        archived_at: Some(_)
                    },
                    ..
                } | SidebarEdit::DeliverableArchive { archived: true, .. }
            )
    }
    fn unresolved(&self) -> bool {
        !matches!(self.phase, Phase::Confirmed(_) | Phase::Removed)
    }
}
#[derive(Deserialize, Serialize)]
struct Journal {
    version: u32,
    client: Uuid,
    sequence: u64,
    entries: Vec<Entry>,
    last_archive: Option<Uuid>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            version: 1,
            client: Uuid::new_v4(),
            sequence: 0,
            entries: Vec::new(),
            last_archive: None,
        }
    }
}
enum Control {
    Record(Entry),
    Forget(Entry),
    ClearUndo(Uuid),
    Wake,
}
struct Completion {
    entry: Entry,
    result: Result<SidebarIntentResult, String>,
}
pub(super) struct SidebarIntents {
    last_archive: Option<Uuid>,
    task_entries: HashMap<Uuid, Vec<(DaemonKey, String)>>,
    removed_tasks: HashSet<Uuid>,
    archives: HashSet<Uuid>,
    pending_count: usize,
    blocked_count: usize,
    reason: Option<String>,
    hosts: Vec<DaemonKey>,
    bases: RefCell<HashMap<String, SidebarEdit>>,
    base_revisions: RefCell<HashMap<String, u64>>,
    pub landing_rows: HashMap<Uuid, usize>,
    pub batch: Option<Uuid>,
    client: Uuid,
    sequence: u64,
    pub revision: u64,
    entries: HashMap<(DaemonKey, String), Entry>,
    tx: Sender<Control>,
    events: Receiver<Completion>,
    pub error: Option<String>,
}
fn write_journal(path: &Path, journal: &Journal) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&serde_json::to_vec(journal)?)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
impl SidebarIntents {
    pub fn open(
        path: PathBuf,
        daemons: waku_client::DaemonMap,
        wake: smol::channel::Sender<()>,
    ) -> Self {
        let lock = (|| -> std::io::Result<fs::File> {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path.with_extension("lock"))?;
            file.try_lock().map_err(std::io::Error::other)?;
            Ok(file)
        })();
        let lock_error = lock.as_ref().err().map(|_| {
            "Another window owns sidebar synchronization, or its journal is unavailable.".to_owned()
        });
        let loaded = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Journal>(&bytes)
                .and_then(|j| {
                    if j.version == 1 {
                        Ok(j)
                    } else {
                        Err(serde::de::Error::custom(
                            "unsupported sidebar journal version",
                        ))
                    }
                })
                .map_err(|_| {
                    "Sidebar changes could not be loaded; synchronization is paused.".to_owned()
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Journal::default()),
            Err(_) => {
                Err("Sidebar changes could not be loaded; synchronization is paused.".to_owned())
            }
        };
        let error = lock_error.or_else(|| loaded.as_ref().err().cloned());
        let mut journal = loaded.unwrap_or_default();
        for entry in &mut journal.entries {
            if entry.phase == Phase::InFlight {
                entry.phase = if entry.attempts >= MAX_ATTEMPTS {
                    Phase::Blocked
                } else if entry.adopting {
                    Phase::Resolving
                } else {
                    Phase::Queued
                };
            }
            if entry.phase == Phase::Preflight {
                entry.reason = Some("Archive needs a fresh safety check.".into());
            }
        }
        let entries = journal
            .entries
            .iter()
            .cloned()
            .map(|e| (e.identity(), e))
            .collect();
        let (tx, rx) = unbounded();
        let (results, events) = unbounded();
        let client = journal.client;
        let sequence = journal.sequence;
        let last_archive = journal.last_archive;
        let writable = error.is_none();
        let spawn = std::thread::Builder::new()
            .name("sidebar-intents".into())
            .spawn(move || {
                let _lock = lock.ok();
                coordinate(path, journal, daemons, rx, results, writable, wake);
            });
        let mut this = Self {
            last_archive,
            task_entries: HashMap::new(),
            removed_tasks: HashSet::new(),
            archives: HashSet::new(),
            pending_count: 0,
            blocked_count: 0,
            reason: None,
            hosts: Vec::new(),
            bases: RefCell::new(HashMap::new()),
            base_revisions: RefCell::new(HashMap::new()),
            landing_rows: HashMap::new(),
            batch: None,
            client,
            sequence,
            revision: 0,
            entries,
            tx,
            events,
            error: error.or_else(|| {
                spawn
                    .err()
                    .map(|_| "Sidebar synchronization worker could not start.".into())
            }),
        };
        this.reindex();
        this.wake();
        this
    }
    pub fn wake(&self) {
        let _ = self.tx.send(Control::Wake);
    }
    pub fn record(
        &mut self,
        key: DaemonKey,
        edit: SidebarEdit,
        expected_revision: Option<u64>,
        preflight: bool,
    ) -> Uuid {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("sidebar sequence exhausted");
        let previous_batch = self
            .entries
            .get(&(key, edit.key()))
            .filter(|e| e.phase == Phase::Preflight)
            .map(|e| e.batch);
        let entry = Entry {
            key,
            intent: SidebarIntent {
                client_id: self.client,
                generation: self.sequence,
                operation_id: Uuid::new_v4(),
                expected_revision,
                expected: self.bases.borrow().get(&edit.key()).cloned(),
                edit,
            },
            batch: self.batch.or(previous_batch).unwrap_or_else(Uuid::new_v4),
            adopting: false,
            at: unix_time(),
            attempts: 0,
            due: 0,
            phase: if preflight {
                Phase::Preflight
            } else {
                Phase::Queued
            },
            reason: None,
        };
        if entry.archiving() {
            self.last_archive = Some(entry.batch);
        }
        self.entries.insert(entry.identity(), entry.clone());
        self.reindex();
        self.revision = self.revision.wrapping_add(1);
        let operation_id = entry.intent.operation_id;
        if self.tx.send(Control::Record(entry)).is_err() {
            self.error = Some("Sidebar synchronization worker stopped.".into());
        }
        operation_id
    }
    fn remember_task(&self, session: &AgentSession) {
        for field in [
            waku_protocol::task_metadata::TaskMetadataField::Title,
            waku_protocol::task_metadata::TaskMetadataField::Placement,
            waku_protocol::task_metadata::TaskMetadataField::Archive,
        ] {
            let edit = SidebarEdit::Task {
                id: session.id,
                edit: waku_protocol::task_metadata::TaskMetadataEdit::capture(field, session),
            };
            let key = edit.key();
            if self
                .base_revisions
                .borrow()
                .get(&key)
                .is_none_or(|revision| *revision <= session.metadata_revision)
            {
                self.base_revisions
                    .borrow_mut()
                    .insert(key.clone(), session.metadata_revision);
                self.bases.borrow_mut().insert(key, edit);
            }
        }
    }
    pub fn observe_task(&self, session: &mut AgentSession) {
        self.remember_task(session);
        self.overlay_task(session);
    }
    pub fn overlay_task(&self, session: &mut AgentSession) {
        let snapshot_revision = session.metadata_revision;
        for entry in self
            .task_entries
            .get(&session.id)
            .into_iter()
            .flatten()
            .filter_map(|key| self.entries.get(key))
        {
            if let SidebarEdit::Task { id, edit } = &entry.intent.edit
                && *id == session.id
            {
                if matches!(entry.phase, Phase::Preflight | Phase::Removed) {
                    continue;
                }
                if let Phase::Confirmed(revision) = entry.phase {
                    if snapshot_revision > revision {
                        continue;
                    }
                    session.metadata_revision = session.metadata_revision.max(revision);
                }
                edit.apply(session);
            }
        }
    }
    pub fn observe_boss(&self, key: DaemonKey, state: &mut BossState) {
        for d in &state.deliverables {
            for edit in [
                SidebarEdit::DeliverableArchive {
                    id: d.id,
                    archived: d.archived_at.is_some(),
                },
                SidebarEdit::DeliverablePin {
                    id: d.id,
                    pinned: d.pinned_at.is_some(),
                },
                SidebarEdit::DeliverableDormant {
                    id: d.id,
                    dormant: d.dormant_at.is_some(),
                },
            ] {
                let key = edit.key();
                if self
                    .base_revisions
                    .borrow()
                    .get(&key)
                    .is_none_or(|revision| *revision <= state.revision)
                {
                    self.base_revisions
                        .borrow_mut()
                        .insert(key.clone(), state.revision);
                    self.bases.borrow_mut().insert(key, edit);
                }
            }
        }
        self.overlay_boss(key, state);
    }
    pub fn overlay_boss(&self, key: DaemonKey, state: &mut BossState) {
        let removed = self
            .entries
            .values()
            .filter(|e| e.key == key && e.phase == Phase::Removed)
            .filter_map(|e| match e.intent.edit {
                SidebarEdit::DeliverableArchive { id, .. }
                | SidebarEdit::DeliverablePin { id, .. }
                | SidebarEdit::DeliverableDormant { id, .. } => Some(id),
                _ => None,
            })
            .collect::<HashSet<_>>();
        state.deliverables.retain(|d| !removed.contains(&d.id));
        let positions = state
            .deliverables
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id, i))
            .collect::<HashMap<_, _>>();
        for entry in self.entries.values().filter(|e| e.key == key) {
            if let Phase::Confirmed(revision) = entry.phase
                && state.revision > revision
            {
                continue;
            }
            let at = entry.at;
            match entry.intent.edit {
                SidebarEdit::DeliverableArchive { id, archived } => {
                    if let Some(d) = positions
                        .get(&id)
                        .and_then(|i| state.deliverables.get_mut(*i))
                    {
                        d.archived_at = archived.then_some(at);
                    }
                }
                SidebarEdit::DeliverablePin { id, pinned } => {
                    if let Some(d) = positions
                        .get(&id)
                        .and_then(|i| state.deliverables.get_mut(*i))
                    {
                        d.pinned_at = pinned.then_some(at);
                    }
                }
                SidebarEdit::DeliverableDormant { id, dormant } => {
                    if let Some(d) = positions
                        .get(&id)
                        .and_then(|i| state.deliverables.get_mut(*i))
                    {
                        d.dormant_at = dormant.then_some(at);
                        if !dormant {
                            d.updated_at = d.updated_at.max(at);
                        }
                    }
                }
                SidebarEdit::Task { .. } => {}
            }
        }
    }
    pub fn cancel_preflight(&mut self, id: Uuid) {
        let key = self.entries.iter().find_map(|(key, entry)| {
            (entry.phase == Phase::Preflight && matches!(&entry.intent.edit, SidebarEdit::Task { id: target, .. } if *target == id)).then(|| key.clone())
        });
        if let Some(key) = key
            && let Some(entry) = self.entries.remove(&key)
        {
            let _ = self.tx.send(Control::Forget(entry));
            self.reindex();
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn preflight_token(&self, id: Uuid) -> Option<Uuid> {
        self.entries.values().find_map(|e| {
            (e.phase == Phase::Preflight
                && matches!(&e.intent.edit, SidebarEdit::Task { id: target, .. } if *target == id))
            .then_some(e.intent.operation_id)
        })
    }
    pub fn preflight_current(&self, id: Uuid, operation_id: Uuid) -> bool {
        self.entries.values().any(|e| {
            e.intent.operation_id == operation_id
                && e.phase == Phase::Preflight
                && matches!(&e.intent.edit, SidebarEdit::Task { id: target, .. } if *target == id)
        })
    }
    pub fn preflight_failed(&mut self, id: Uuid, reason: String) {
        if let Some(entry) = self.entries.values_mut().find(|e| {
            e.phase == Phase::Preflight
                && matches!(&e.intent.edit, SidebarEdit::Task { id: target, .. } if *target == id)
        }) {
            entry.reason = Some(reason);
            let _ = self.tx.send(Control::Record(entry.clone()));
            self.reindex();
        }
    }
    pub fn task_removed(&self, id: Uuid) -> bool {
        self.removed_tasks.contains(&id)
    }
    pub fn is_archiving(&self, id: &Uuid) -> bool {
        self.archives.contains(id)
    }
    /// Transcript saves carry confirmed metadata; only the durable intent path
    /// may commit an unresolved organizational edit.
    pub fn strip_pending_task_metadata(
        &self,
        state: &mut PersistedState,
    ) -> Vec<(Uuid, waku_protocol::task_metadata::TaskMetadataEdit)> {
        let mut restore = Vec::new();
        let bases = self.bases.borrow();
        for entry in self
            .entries
            .values()
            .filter(|e| e.unresolved() && e.phase != Phase::Preflight)
        {
            if let SidebarEdit::Task { id, edit } = &entry.intent.edit
                && let Some(SidebarEdit::Task {
                    edit: confirmed, ..
                }) = bases.get(&entry.intent.edit.key())
                && let Some(session) = state.session_mut(*id)
            {
                restore.push((
                    *id,
                    waku_protocol::task_metadata::TaskMetadataEdit::capture(edit.field(), session),
                ));
                confirmed.apply(session);
            }
        }
        restore
    }
    pub fn cleanup_ids(&self) -> HashSet<Uuid> {
        self.entries
            .values()
            .filter(|e| !matches!(e.phase, Phase::Preflight | Phase::Removed))
            .filter_map(|e| match &e.intent.edit {
                SidebarEdit::Task {
                    id,
                    edit:
                        waku_protocol::task_metadata::TaskMetadataEdit::Archive {
                            archived_at: Some(_),
                        },
                } => Some(*id),
                _ => None,
            })
            .collect()
    }
    pub fn next_cleanup_wake(&self) -> Option<Duration> {
        let now = unix_time();
        self.entries
            .values()
            .filter(|e| e.archiving() && matches!(&e.intent.edit, SidebarEdit::Task { .. }))
            .filter_map(|e| e.at.saturating_add(10).checked_sub(now))
            .filter(|seconds| *seconds > 0)
            .min()
            .map(Duration::from_secs)
    }
    pub fn cleanup_deferred(&self, id: Uuid) -> bool {
        self.entries.values().any(|e| matches!(&e.intent.edit, SidebarEdit::Task { id: target, edit: waku_protocol::task_metadata::TaskMetadataEdit::Archive { archived_at: Some(_) } } if *target == id) && (e.unresolved() || unix_time() < e.at.saturating_add(10)))
    }
    fn recover(&mut self, adopt: bool) {
        let entries = self
            .entries
            .values()
            .filter(|e| {
                if adopt {
                    e.unresolved()
                } else {
                    e.phase == Phase::Blocked
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        for mut entry in entries {
            self.sequence = self
                .sequence
                .checked_add(1)
                .expect("sidebar sequence exhausted");
            entry.intent.generation = self.sequence;
            entry.intent.operation_id = Uuid::new_v4();
            entry.intent.expected_revision = None;
            entry.intent.expected = None;
            entry.attempts = 0;
            entry.due = 0;
            entry.reason = None;
            entry.adopting = adopt;
            entry.phase = if adopt {
                Phase::Resolving
            } else {
                Phase::Queued
            };
            self.entries.insert(entry.identity(), entry.clone());
            let _ = self.tx.send(Control::Record(entry));
        }
        self.reindex();
        self.revision = self.revision.wrapping_add(1);
    }
    fn reindex(&mut self) {
        self.task_entries.clear();
        for (key, entry) in &self.entries {
            if let SidebarEdit::Task { id, .. } = entry.intent.edit {
                self.task_entries.entry(id).or_default().push(key.clone());
            }
        }
        self.archives.clear();
        self.removed_tasks.clear();
        for entry in self.entries.values().filter(|e| e.phase == Phase::Removed) {
            if let SidebarEdit::Task { id, .. } = entry.intent.edit {
                self.archives.insert(id);
                self.removed_tasks.insert(id);
            }
        }
        self.pending_count = 0;
        self.blocked_count = 0;
        self.reason = None;
        self.hosts.clear();
        for e in self.entries.values().filter(|e| e.unresolved()) {
            self.pending_count += 1;
            if !self.hosts.contains(&e.key) {
                self.hosts.push(e.key);
            }
            if self.reason.is_none() {
                self.reason = e.reason.clone();
            }
            if matches!(e.phase, Phase::Blocked | Phase::Preflight) {
                self.blocked_count += 1;
            }
            if let SidebarEdit::Task {
                id,
                edit:
                    waku_protocol::task_metadata::TaskMetadataEdit::Archive {
                        archived_at: Some(_),
                    },
            } = e.intent.edit
            {
                self.archives.insert(id);
            }
        }
    }
    pub fn pending_count(&self) -> usize {
        self.pending_count
    }
    pub fn blocked_count(&self) -> usize {
        self.blocked_count
    }
}

fn coordinate(
    path: PathBuf,
    mut journal: Journal,
    daemons: waku_client::DaemonMap,
    rx: Receiver<Control>,
    ui: Sender<Completion>,
    writable: bool,
    wake: smol::channel::Sender<()>,
) {
    let (done_tx, done_rx) = unbounded::<Completion>();
    let mut workers: HashMap<DaemonKey, Sender<Entry>> = HashMap::new();
    let mut busy = HashSet::new();
    let mut journal_dirty = false;
    let mut notifications = Vec::new();
    let mut disk_failed = false;
    loop {
        let now = unix_time();
        let wait = if journal_dirty && writable {
            Duration::from_secs(1)
        } else {
            journal
                .entries
                .iter()
                .filter(|e| matches!(e.phase, Phase::Queued | Phase::Resolving) && e.due > now)
                .map(|e| Duration::from_secs(e.due - now))
                .min()
                .unwrap_or(Duration::from_secs(3600))
        };
        let mut received_completion = None;
        let command = crossbeam_channel::select! {
            recv(rx) -> command => match command { Ok(command) => Some(command), Err(_) => break },
            recv(done_rx) -> completion => { received_completion = completion.ok(); None },
            default(wait) => None,
        };
        let mut dirty = journal_dirty;
        for command in command.into_iter().chain(rx.try_iter()) {
            match command {
                Control::Wake => {}
                Control::Record(entry) => {
                    journal.sequence = journal.sequence.max(entry.intent.generation);
                    if entry.archiving() && !entry.adopting {
                        journal.last_archive = Some(entry.batch);
                    }
                    journal.entries.retain(|e| e.identity() != entry.identity());
                    journal.entries.push(entry);
                    dirty = true;
                }
                Control::ClearUndo(batch) => {
                    if journal.last_archive == Some(batch) {
                        journal.last_archive = None;
                        dirty = true;
                    }
                }
                Control::Forget(entry) => {
                    journal
                        .entries
                        .retain(|e| e.intent.operation_id != entry.intent.operation_id);
                    dirty = true;
                }
            }
        }
        for done in received_completion.into_iter().chain(done_rx.try_iter()) {
            busy.remove(&done.entry.key);
            if let Some(entry) = journal
                .entries
                .iter_mut()
                .find(|e| e.intent.operation_id == done.entry.intent.operation_id)
            {
                {
                    match (&mut entry.intent.edit, &done.result) {
                        (
                            SidebarEdit::Task { edit, .. },
                            Ok(SidebarIntentResult::Task { session }),
                        ) => {
                            *edit = waku_protocol::task_metadata::TaskMetadataEdit::capture(
                                edit.field(),
                                session,
                            )
                        }
                        (edit, Ok(SidebarIntentResult::Deliverables { state })) => match edit {
                            SidebarEdit::DeliverableArchive { id, archived } => {
                                if let Some(d) = state.deliverables.iter().find(|d| d.id == *id) {
                                    *archived = d.archived_at.is_some();
                                }
                            }
                            SidebarEdit::DeliverablePin { id, pinned } => {
                                if let Some(d) = state.deliverables.iter().find(|d| d.id == *id) {
                                    *pinned = d.pinned_at.is_some();
                                }
                            }
                            SidebarEdit::DeliverableDormant { id, dormant } => {
                                if let Some(d) = state.deliverables.iter().find(|d| d.id == *id) {
                                    *dormant = d.dormant_at.is_some();
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
                match &done.result {
                    Ok(SidebarIntentResult::Task { session }) => {
                        entry.phase = Phase::Confirmed(session.metadata_revision)
                    }
                    Ok(SidebarIntentResult::Deliverables { state }) => {
                        entry.phase = Phase::Confirmed(state.revision)
                    }
                    Ok(SidebarIntentResult::Blocked { reason }) => {
                        entry.phase = Phase::Blocked;
                        entry.reason = Some(reason.clone());
                    }
                    Ok(SidebarIntentResult::Removed) => {
                        entry.phase = Phase::Removed;
                        entry.reason = None;
                    }
                    Ok(SidebarIntentResult::Superseded) => {
                        entry.phase = Phase::Blocked;
                        entry.reason =
                            Some("This change was superseded. Refresh before retrying.".into());
                    }
                    Err(_) => {
                        entry.phase = if entry.attempts >= MAX_ATTEMPTS {
                            Phase::Blocked
                        } else if entry.adopting {
                            Phase::Resolving
                        } else {
                            Phase::Queued
                        };
                        // Stable per-operation jitter avoids synchronized reconnect bursts.
                        entry.due = unix_time()
                            + (1u64 << entry.attempts.min(5)).min(30)
                            + u64::from(entry.intent.operation_id.as_bytes()[0] % 3);
                        entry.reason = Some("The host has not acknowledged this change.".into());
                    }
                }
                eprintln!(
                    "sidebar-intents operation={} generation={} attempts={} phase={:?}",
                    entry.intent.operation_id, entry.intent.generation, entry.attempts, entry.phase
                );
                notifications.push(Completion {
                    entry: entry.clone(),
                    result: done.result,
                });
                dirty = true;
            }
        }
        journal_dirty = dirty;
        if !writable {
            continue;
        }
        if dirty && write_journal(&path, &journal).is_err() {
            if !disk_failed {
                if let Some(entry) = journal.entries.first() {
                    let _ = ui.send(Completion { entry: entry.clone(), result: Err("Sidebar changes could not be saved to disk; synchronization is paused.".into()) });
                    signal_event_pump(&wake);
                }
            }
            disk_failed = true;
            continue;
        }
        journal_dirty = false;
        if disk_failed {
            if let Some(entry) = journal.entries.first() {
                let _ = ui.send(Completion {
                    entry: entry.clone(),
                    result: Err(String::new()),
                });
            }
            signal_event_pump(&wake);
            disk_failed = false;
        }
        if !notifications.is_empty() {
            for notification in notifications.drain(..) {
                let _ = ui.send(notification);
            }
            signal_event_pump(&wake);
        }
        let ready = journal
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                matches!(e.phase, Phase::Queued | Phase::Resolving)
                    && e.due <= unix_time()
                    && !busy.contains(&e.key)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        for index in ready {
            let entry = &journal.entries[index];
            if busy.contains(&entry.key) {
                continue;
            }
            let Some(supervisor) = daemons.supervisor(entry.key) else {
                continue;
            };
            if supervisor.client().is_disconnected() {
                continue;
            }
            let key = entry.key;
            if !workers.contains_key(&key) {
                let (tx, work) = unbounded::<Entry>();
                let done = done_tx.clone();
                let daemons = daemons.clone();
                let spawned = std::thread::Builder::new().name(format!("sidebar-host-{key:?}")).spawn(move || {
                    while let Ok(entry) = work.recv() {
                        let result = (|| -> anyhow::Result<SidebarIntentResult> {
                            let host = daemons.supervisor(key).ok_or_else(|| anyhow::anyhow!("host disconnected"))?;
                            if entry.adopting {
                                return match &entry.intent.edit {
                                    SidebarEdit::Task { id, .. } => match host.client().request(Uuid::nil(), Uuid::nil(), waku_client::Command::LoadTaskState)? {
                                        waku_client::ResponsePayload::TaskState { sessions, .. } => Ok(sessions.into_iter().find(|s| s.id == *id)
                                            .map(|s| SidebarIntentResult::Task { session: Box::new(s) }).unwrap_or(SidebarIntentResult::Removed)),
                                        _ => anyhow::bail!("invalid catalog"),
                                    },
                                    _ => match host.client().request(Uuid::nil(), Uuid::nil(), waku_client::Command::Boss { operation: waku_protocol::boss::BossOperation::View })? {
                                        waku_client::ResponsePayload::Boss { result: waku_protocol::boss::BossResult::State { state } } => Ok(SidebarIntentResult::Deliverables { state: Box::new(state) }),
                                        _ => anyhow::bail!("invalid deliverable catalog"),
                                    }
                                };
                            }
                            match host.client().request(Uuid::nil(), Uuid::nil(), waku_client::Command::ApplySidebarIntent { intent: entry.intent.clone() })? {
                                waku_client::ResponsePayload::SidebarIntent { result } => Ok(result),
                                _ => anyhow::bail!("unsupported sidebar acknowledgement"),
                            }
                        })().or_else(|error| {
                            let message = error.to_string().to_ascii_lowercase();
                            if message.contains("unknown variant") || message.contains("unsupported") {
                                Ok(SidebarIntentResult::Blocked { reason: "Update the owning daemon to synchronize sidebar changes.".into() })
                            } else if message.contains("unauthorized") || message.contains("forbidden") || message.contains("permission denied") {
                                Ok(SidebarIntentResult::Blocked { reason: "Reconnect with permission to save changes on this host.".into() })
                            } else { Err("The host did not acknowledge sidebar changes.".to_owned()) }
                        });
                        if done.send(Completion { entry, result }).is_err() { break; }
                    }
                });
                if spawned.is_err() {
                    continue;
                }
                workers.insert(key, tx);
            }
            journal.entries[index].phase = Phase::InFlight;
            journal.entries[index].attempts += 1;
            if write_journal(&path, &journal).is_err() {
                journal.entries[index].phase = if journal.entries[index].adopting {
                    Phase::Resolving
                } else {
                    Phase::Queued
                };
                journal.entries[index].attempts = journal.entries[index].attempts.saturating_sub(1);
                journal_dirty = true;
                continue;
            }
            let entry = journal.entries[index].clone();
            if workers[&key].send(entry).is_ok() {
                busy.insert(key);
            } else {
                workers.remove(&key);
                journal.entries[index].phase = Phase::Queued;
            }
        }
    }
}

impl Waku {
    pub(super) fn drain_sidebar_intents(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok(done) = self.sidebar_intents.events.try_recv() {
            if let Err(reason) = &done.result
                && (reason.is_empty() || reason.contains("disk"))
            {
                self.sidebar_intents.error = if reason.is_empty() {
                    None
                } else {
                    Some(reason.clone())
                };
                changed = true;
                continue;
            }
            let identity = done.entry.identity();
            if self
                .sidebar_intents
                .entries
                .get(&identity)
                .is_some_and(|e| e.intent.generation > done.entry.intent.generation)
            {
                continue;
            }
            self.sidebar_intents.sequence = self
                .sidebar_intents
                .sequence
                .max(done.entry.intent.generation);
            self.sidebar_intents
                .entries
                .insert(identity, done.entry.clone());
            match done.result {
                Ok(SidebarIntentResult::Task { session }) => {
                    self.sidebar_intents.remember_task(&session);
                    if let SidebarEdit::Task { id, edit } = &done.entry.intent.edit {
                        if done.entry.adopting {
                            self.pending_task_metadata
                                .edits
                                .remove(&(*id, edit.field()));
                        }
                        let request = self
                            .pending_task_metadata
                            .edits
                            .get(&(*id, edit.field()))
                            .cloned();
                        if let Some(local) = self.state.session_mut(*id) {
                            if let Some(request) = request {
                                self.pending_task_metadata
                                    .acknowledge(&request, &session, local);
                            } else if local.metadata_revision <= session.metadata_revision {
                                waku_protocol::task_metadata::TaskMetadataEdit::capture(
                                    edit.field(),
                                    &session,
                                )
                                .apply(local);
                                local.metadata_revision = session.metadata_revision;
                            }
                        }
                    }
                }
                Ok(SidebarIntentResult::Deliverables { state }) => {
                    let _ = self.boss_tx.send((done.entry.key, *state));
                }
                Ok(SidebarIntentResult::Removed) => {
                    if let SidebarEdit::Task { id, .. } = done.entry.intent.edit {
                        self.pending_task_metadata.remove(&id);
                        self.pending_workspace_cleanups.remove(&id);
                        self.state.sessions.retain(|session| session.id != id);
                        if self.state.selected_session == Some(id) {
                            self.state.selected_session = None;
                        }
                    }
                }
                Ok(SidebarIntentResult::Blocked { .. } | SidebarIntentResult::Superseded) => {}
                Err(reason) if reason.is_empty() => self.sidebar_intents.error = None,
                Err(reason) if reason.contains("disk") => self.sidebar_intents.error = Some(reason),
                Err(_) => {}
            }
            self.sidebar_intents.reindex();
            self.sidebar_intents.revision = self.sidebar_intents.revision.wrapping_add(1);
            changed = true;
        }
        if changed {
            self.boss_ui.revision = self.boss_ui.revision.wrapping_add(1);
            cx.notify();
        }
        changed
    }
    pub(super) fn record_deliverable_intent(
        &mut self,
        key: DaemonKey,
        edit: SidebarEdit,
        cx: &mut Context<Self>,
    ) {
        let archived = matches!(
            &edit,
            SidebarEdit::DeliverableArchive { archived: true, .. }
        );
        let field_key = edit.key();
        self.sidebar_intents.record(key, edit, None, false);
        if archived && let Some(entry) = self.sidebar_intents.entries.get(&(key, field_key)) {
            self.show_sidebar_archive_toast(entry.batch);
        }
        if let Some(state) = self.boss_ui.states.get_mut(&key) {
            self.sidebar_intents.overlay_boss(key, state);
        }
        self.boss_ui.revision = self.boss_ui.revision.wrapping_add(1);
        cx.notify();
    }
}

impl Waku {
    pub(super) fn sidebar_task_archiving(&self, id: &Uuid) -> bool {
        self.pending_task_metadata.is_archiving(id) || self.sidebar_intents.is_archiving(id)
    }
}

impl Waku {
    pub(super) fn sidebar_archive_available(&self) -> bool {
        self.sidebar_intents.last_archive.is_some_and(|batch| {
            self.sidebar_intents
                .entries
                .values()
                .any(|e| e.batch == batch && e.archiving())
        })
    }
    pub(super) fn undo_sidebar_archive_batch(&mut self, batch: Uuid, cx: &mut Context<Self>) {
        if self.sidebar_intents.last_archive == Some(batch) {
            self.sidebar_intents.last_archive = None;
        }
        let _ = self.sidebar_intents.tx.send(Control::ClearUndo(batch));
        let entries = self
            .sidebar_intents
            .entries
            .values()
            .filter(|e| e.batch == batch && e.phase != Phase::Removed)
            .cloned()
            .collect::<Vec<_>>();
        for entry in entries {
            match entry.intent.edit {
                SidebarEdit::Task {
                    id,
                    edit:
                        waku_protocol::task_metadata::TaskMetadataEdit::Archive {
                            archived_at: Some(_),
                        },
                } => {
                    if entry.phase == Phase::Preflight {
                        self.archive_preview_pending.remove(&id);
                        self.discard_archive_confirmation(id);
                        self.sidebar_intents.cancel_preflight(id);
                    } else {
                        self.unarchive_session(id, false, cx);
                    }
                }
                SidebarEdit::DeliverableArchive { id, archived: true } => {
                    self.set_deliverable_archived(entry.key, id, false, cx)
                }
                _ => {}
            }
        }
        cx.notify();
    }
    pub(super) fn undo_sidebar_archive(&mut self, cx: &mut Context<Self>) -> bool {
        let batch = self.sidebar_intents.last_archive;
        let Some(batch) = batch else {
            return false;
        };
        self.undo_sidebar_archive_batch(batch, cx);
        true
    }
    fn retry_sidebar_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preflight = self
            .sidebar_intents
            .entries
            .values()
            .filter(|e| e.phase == Phase::Preflight)
            .filter_map(|e| {
                if let SidebarEdit::Task { id, .. } = e.intent.edit {
                    Some(id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        self.sidebar_intents.recover(false);
        for id in preflight {
            self.archive_session(id, window, cx);
        }
        cx.notify();
    }
    #[track_caller]
    pub(super) fn render_sidebar_intent_status(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let count = self.sidebar_intents.pending_count();
        if count == 0 && self.sidebar_intents.error.is_none() {
            return None;
        }
        let theme = Theme::current(cx);
        let label = if self.sidebar_intents.error.is_some() {
            tr!("sidebar.changes_not_saved")
        } else if self.sidebar_intents.blocked_count() > 0 {
            tr!("sidebar.changes_blocked", count = count)
        } else {
            tr!("sidebar.changes_syncing", count = count)
        };
        let detail = self
            .sidebar_intents
            .error
            .clone()
            .or_else(|| self.sidebar_intents.reason.clone())
            .unwrap_or_else(|| tr!("sidebar.changes_detail"));
        let hosts = self
            .sidebar_intents
            .hosts
            .iter()
            .map(|key| match key {
                DaemonKey::Local => tr!("sidebar.changes_local_host"),
                DaemonKey::Remote(id) => self
                    .state
                    .remote_hosts
                    .iter()
                    .find(|host| host.id == *id)
                    .map(|host| host.name.clone())
                    .unwrap_or_else(|| tr!("sidebar.changes_removed_host")),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let retry_focus = self.transcript_control_focus("sidebar-intent-retry", cx);
        let undo_focus = self.transcript_control_focus("sidebar-intent-undo", cx);
        let adopt_focus = self.transcript_control_focus("sidebar-intent-adopt", cx);
        let button = |id: &'static str, label: String, focus: &FocusHandle| {
            div()
                .id(id)
                .track_focus(focus)
                .tab_index(0)
                .px(px(5.0))
                .py(px(3.0))
                .rounded(px(4.0))
                .cursor_default()
                .focus_visible(|s| s.bg(theme.focus_highlight()))
                .hover(|s| s.bg(theme.overlay))
                .child(label)
        };
        Some(
            div()
                .px(px(12.0))
                .py(px(5.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .text_size(sp(11.0))
                .text_color(theme.text_secondary)
                .child(div().id("sidebar-intent-summary").child(label))
                .child(div().child(hosts))
                .when(
                    self.sidebar_intents.blocked_count() > 0
                        || self.sidebar_intents.error.is_some(),
                    |row| row.child(div().child(detail)),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(
                            button(
                                "sidebar-intent-retry",
                                tr!("sidebar.changes_retry"),
                                &retry_focus,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.retry_sidebar_changes(window, cx)
                            }))
                            .on_key_down(cx.listener(
                                |this, event: &KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.retry_sidebar_changes(window, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            )),
                        )
                        .child(
                            button(
                                "sidebar-intent-undo",
                                tr!("sidebar.changes_undo"),
                                &undo_focus,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.undo_sidebar_archive(cx);
                            }))
                            .on_key_down(cx.listener(
                                |this, event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.undo_sidebar_archive(cx);
                                        cx.stop_propagation();
                                    }
                                },
                            )),
                        )
                        .child(
                            button(
                                "sidebar-intent-adopt",
                                tr!("sidebar.changes_adopt"),
                                &adopt_focus,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_intents.recover(true);
                                cx.notify();
                            }))
                            .on_key_down(cx.listener(
                                |this, event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.sidebar_intents.recover(true);
                                        cx.stop_propagation();
                                    }
                                },
                            )),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::task_metadata::{TaskMetadataEdit, TaskMetadataField};
    fn queue() -> SidebarIntents {
        let (tx, _) = unbounded();
        let (_, events) = unbounded();
        SidebarIntents {
            last_archive: None,
            task_entries: HashMap::new(),
            removed_tasks: HashSet::new(),
            archives: HashSet::new(),
            pending_count: 0,
            blocked_count: 0,
            reason: None,
            hosts: Vec::new(),
            bases: RefCell::new(HashMap::new()),
            base_revisions: RefCell::new(HashMap::new()),
            landing_rows: HashMap::new(),
            batch: None,
            client: Uuid::new_v4(),
            sequence: 0,
            revision: 0,
            entries: HashMap::new(),
            tx,
            events,
            error: None,
        }
    }
    #[test]
    fn sidebar_journal_restart_preserves_latest_intent_budget_and_field_independence() {
        let mut queue = queue();
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        queue.overlay_task(&mut session);
        let id = session.id;
        queue.record(
            DaemonKey::Local,
            SidebarEdit::Task {
                id,
                edit: TaskMetadataEdit::Rename {
                    title: "New title".into(),
                },
            },
            None,
            false,
        );
        queue.record(
            DaemonKey::Local,
            SidebarEdit::Task {
                id,
                edit: TaskMetadataEdit::Archive {
                    archived_at: Some(1),
                },
            },
            None,
            false,
        );
        queue.record(
            DaemonKey::Local,
            SidebarEdit::Task {
                id,
                edit: TaskMetadataEdit::Archive { archived_at: None },
            },
            None,
            false,
        );
        for entry in queue.entries.values_mut() {
            entry.attempts = MAX_ATTEMPTS;
            entry.phase = Phase::Blocked;
        }
        let root = std::env::temp_dir().join(format!("sidebar-journal-{}", Uuid::new_v4()));
        let path = root.join("intents.json");
        write_journal(
            &path,
            &Journal {
                version: 1,
                client: queue.client,
                sequence: queue.sequence,
                entries: queue.entries.values().cloned().collect(),
                last_archive: queue.last_archive,
            },
        )
        .unwrap();
        let stored: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut restored = self::queue();
        restored.client = stored.client;
        restored.sequence = stored.sequence;
        restored.entries = stored
            .entries
            .into_iter()
            .map(|e| (e.identity(), e))
            .collect();
        restored.reindex();
        restored.overlay_task(&mut session);
        assert_eq!(session.title, "New title");
        assert!(session.archived_at.is_none());
        assert_eq!(restored.blocked_count(), 2);
        assert!(
            restored
                .entries
                .values()
                .all(|e| e.attempts == MAX_ATTEMPTS)
        );
        assert_eq!(restored.client, queue.client);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sidebar_outbox_journals_before_send_and_persists_retry_exhaustion() {
        use waku_protocol::{ClientMessage, ResponseOutcome, ServerMessage};
        let root = std::env::temp_dir().join(format!("sidebar-transport-{}", Uuid::new_v4()));
        let path = root.join("intents.json");
        let mut seed = queue();
        seed.record(
            DaemonKey::Local,
            SidebarEdit::DeliverableArchive {
                id: Uuid::new_v4(),
                archived: true,
            },
            None,
            false,
        );
        let mut entry = seed.entries.values().next().unwrap().clone();
        entry.attempts = MAX_ATTEMPTS - 1;
        write_journal(
            &path,
            &Journal {
                version: 1,
                client: seed.client,
                sequence: seed.sequence,
                entries: vec![entry],
                last_archive: seed.last_archive,
            },
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let checked_path = path.clone();
        let (sent, observed) = unbounded();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            while let Ok(message) = socket.read() {
                let tungstenite::Message::Text(text) = message else {
                    continue;
                };
                let response = match serde_json::from_str::<ClientMessage>(&text).unwrap() {
                    ClientMessage::Hello { .. } => ServerMessage::Hello {
                        protocol_version: waku_protocol::PROTOCOL_VERSION,
                        daemon_version: "test".into(),
                        daemon_commit: None,
                        agent_cli_available: false,
                        task_views_available: false,
                        transcript_windows_available: false,
                        landing_queue_available: false,
                    },
                    ClientMessage::Request(request) => {
                        let payload = match &request.command {
                            waku_client::Command::GetSettings => {
                                waku_client::ResponsePayload::Settings {
                                    settings: Default::default(),
                                }
                            }
                            waku_client::Command::ApplySidebarIntent { intent } => {
                                let persisted: Journal =
                                    serde_json::from_slice(&fs::read(&checked_path).unwrap())
                                        .unwrap();
                                assert_eq!(
                                    persisted.entries[0].intent.operation_id,
                                    intent.operation_id
                                );
                                assert_eq!(persisted.entries[0].phase, Phase::InFlight);
                                assert_eq!(persisted.entries[0].attempts, MAX_ATTEMPTS);
                                sent.send(()).unwrap();
                                // An invalid acknowledgement must never clear intent.
                                waku_client::ResponsePayload::Ack
                            }
                            _ => waku_client::ResponsePayload::Ack,
                        };
                        ServerMessage::Response {
                            request_id: request.request_id,
                            outcome: ResponseOutcome::Ok { payload },
                        }
                    }
                    _ => break,
                };
                if socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&response).unwrap().into(),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
        let supervisor =
            waku_client::DaemonSupervisor::connect(&address, "test-token".into()).unwrap();
        let (wake, _) = smol::channel::bounded(1);
        let service = SidebarIntents::open(
            path.clone(),
            waku_client::DaemonMap::new(supervisor.clone()),
            wake,
        );
        assert!(service.error.is_none());
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let completion = service.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.entry.phase, Phase::Blocked);
        let persisted: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted.entries[0].attempts, MAX_ATTEMPTS);
        assert_eq!(persisted.entries[0].phase, Phase::Blocked);
        assert!(persisted.entries[0].archiving());
        drop(service);
        supervisor.client().force_disconnect();
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sidebar_pending_metadata_cannot_escape_through_transcript_saves() {
        let mut queue = queue();
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/sidebar-save-fixture"));
        let session = &mut state.sessions[0];
        session.begin_turn("Preserve transcript");
        queue.observe_task(session);
        let original = session.title.clone();
        let id = session.id;
        session.title = "Pending title".into();
        queue.record(
            DaemonKey::Local,
            SidebarEdit::Task {
                id,
                edit: TaskMetadataEdit::Rename {
                    title: session.title.clone(),
                },
            },
            None,
            false,
        );
        let restore = queue.strip_pending_task_metadata(&mut state);
        assert_eq!(state.sessions[0].title, original);
        assert_eq!(state.sessions[0].messages[0].content, "Preserve transcript");
        for (id, edit) in restore {
            edit.apply(state.session_mut(id).unwrap());
        }
        assert_eq!(state.sessions[0].title, "Pending title");
    }
    #[test]
    fn sidebar_preflight_hides_without_committing_archive_and_cancel_reveals() {
        let mut queue = queue();
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        queue.record(
            DaemonKey::Local,
            SidebarEdit::Task {
                id: session.id,
                edit: TaskMetadataEdit::Archive {
                    archived_at: Some(1),
                },
            },
            None,
            true,
        );
        queue.overlay_task(&mut session);
        assert!(queue.is_archiving(&session.id));
        assert!(session.archived_at.is_none());
        queue.cancel_preflight(session.id);
        assert!(!queue.is_archiving(&session.id));
        assert_eq!(queue.pending_count(), 0);
    }
    #[test]
    fn sidebar_removed_targets_hide_stale_rows_without_retry_or_cleanup() {
        let mut queue = queue();
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Deleted task");
        queue.record(
            DaemonKey::Local,
            SidebarEdit::Task {
                id: session.id,
                edit: TaskMetadataEdit::Archive {
                    archived_at: Some(1),
                },
            },
            None,
            false,
        );
        queue.entries.values_mut().next().unwrap().phase = Phase::Removed;
        queue.reindex();
        queue.overlay_task(&mut session);
        assert!(
            queue.is_archiving(&session.id),
            "stale catalog rows must remain hidden"
        );
        assert!(
            session.archived_at.is_none(),
            "a tombstone is not an archive mutation"
        );
        assert_eq!(queue.pending_count(), 0);
        assert!(queue.cleanup_ids().is_empty());
        assert!(!queue.entries.values().next().unwrap().archiving());
    }
    #[test]
    fn sidebar_confirmed_fences_overlay_old_snapshots_but_release_to_newer_remote_edits() {
        let mut queue = queue();
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        let stale = session.clone();
        for (field, edit, revision) in [
            (
                TaskMetadataField::Archive,
                TaskMetadataEdit::Archive {
                    archived_at: Some(1),
                },
                4,
            ),
            (
                TaskMetadataField::Title,
                TaskMetadataEdit::Rename {
                    title: "Renamed".into(),
                },
                5,
            ),
        ] {
            queue.record(
                DaemonKey::Local,
                SidebarEdit::Task {
                    id: session.id,
                    edit,
                },
                None,
                false,
            );
            let key = (
                DaemonKey::Local,
                SidebarEdit::Task {
                    id: session.id,
                    edit: TaskMetadataEdit::capture(field, &session),
                }
                .key(),
            );
            queue.entries.get_mut(&key).unwrap().phase = Phase::Confirmed(revision);
        }
        queue.overlay_task(&mut session);
        assert_eq!(session.title, "Renamed");
        assert_eq!(session.archived_at, Some(1));
        assert_eq!(session.metadata_revision, 5);
        let mut later = stale;
        later.metadata_revision = 6;
        queue.overlay_task(&mut later);
        assert!(later.archived_at.is_none());
        assert_ne!(later.title, "Renamed");
    }
}

impl Waku {
    pub(super) fn sidebar_archive_batch(&self, id: Uuid) -> Option<Uuid> {
        self.sidebar_intents.entries.values().find_map(|e| {
            matches!(&e.intent.edit, SidebarEdit::Task { id: target, .. } if *target == id)
                .then_some(e.batch)
        })
    }
    pub(super) fn show_sidebar_archive_toast(&mut self, batch: Uuid) {
        let count = self
            .sidebar_intents
            .entries
            .values()
            .filter(|e| e.batch == batch)
            .count();
        let message = tr!("sidebar.archived_many", count = count);
        if let Some(toast) = self.toast.as_mut()
            && matches!(&toast.action, Some(ToastAction { kind: ToastActionKind::SidebarArchive(current), .. }) if *current == batch)
        {
            toast.message = message;
            toast.duration_remaining = DEFAULT_TOAST_DURATION;
            toast.timer_started = None;
            self.toast_generation = self.toast_generation.wrapping_add(1);
            toast.timer_generation = self.toast_generation;
        } else {
            self.show_toast_with_tone(
                message,
                ToastTone::Success,
                Some(ToastAction {
                    label: tr!("common.undo").into(),
                    kind: ToastActionKind::SidebarArchive(batch),
                }),
            );
        }
    }
}
