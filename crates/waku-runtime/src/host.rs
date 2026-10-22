//! The stable runtime host.
//!
//! One `RuntimeHost` owns a namespace: its provider sessions, their durable
//! journal, and the single control lease. App and daemon replacement is a
//! change of controller, not of provider — sessions, generations, sequences,
//! and receipts survive controller detachment untouched.
//!
//! Invariants this type enforces:
//!
//! - Intent is durable before contact. A command is journaled before any
//!   dispatch, a fence before any termination, an event before any publish.
//! - One controller mutates. Every new-epoch mutation is fenced; stale
//!   controllers can read receipts but cannot record new work.
//! - Acceptance is honest. A command the provider cannot prove it accepted
//!   is held uncertain; nothing is resent automatically.
//! - Order is single-sourced. Event sequence is assigned at journal time,
//!   monotonic per generation, and identical for every replay.
//! - Bounds are enforced, never silently truncated. Data intake that
//!   exceeds its budget pauses a pausable provider or visibly interrupts
//!   one that cannot pause; control records keep protected capacity.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::adapter::{
    AdapterFactory, AdapterHandle, CancelOutcome, DispatchOutcome, SessionContext, Termination,
};
use crate::contract::*;
use crate::journal::{Frame, Journal, JournalError, JournalFault, JournalRecord, mint_credential};

/// Wall time in epoch milliseconds. Tests substitute `ManualClock` so
/// expiry and rollback behavior is deterministic.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Operating budgets. The defaults are the plan's proposed budgets — review
/// targets, not measured provider needs.
#[derive(Clone, Debug)]
pub struct Budgets {
    /// Retained replay and unresolved request payload bytes per session.
    pub session_memory_bytes: u64,
    /// Retained event bytes across all sessions.
    pub aggregate_memory_bytes: u64,
    /// Journaled event bytes per session.
    pub session_journal_bytes: u64,
    /// Total journal bytes.
    pub aggregate_journal_bytes: u64,
    /// Journal headroom reserved for control records — fences, dispositions,
    /// acknowledgements, terminal facts — so data traffic can never starve
    /// cancellation or completion.
    pub control_reserve_bytes: u64,
    /// One oversized provider message is refused with a sequenced drop
    /// marker rather than journaled whole.
    pub max_message_bytes: u64,
    pub max_sessions: usize,
    /// Bounded intake depth per session — the honest pause point for
    /// providers that can be paused.
    pub channel_depth: usize,
    /// Command identities stay redeemable for this long after recording.
    /// Afterwards their tombstones still reject. It is also the retention
    /// horizon for resolved ingress truth: a terminal record stays
    /// queryable for one retry window past its own deadline.
    pub retry_window_ms: u64,
    /// Retained tombstones for expired command identities. Tombstones are
    /// never evicted — each keeps a resolved identity rejecting — so this
    /// is also the host's effective command lifetime: at the limit NEW
    /// commands refuse with `HostError::Capacity` while recorded truth,
    /// replay, fencing and emergency control still answer.
    pub tombstone_limit: usize,
    /// Retained agent-ingress records per session generation. Terminal
    /// records are reclaimed one retry horizon after their deadline — a
    /// retry carries its original deadline, so it can only re-record as
    /// `Expired`, never re-dispatch. Pending, forwarded and uncertain
    /// records are unresolved evidence and are never reclaimed; at the
    /// limit new submissions refuse with `HostError::Capacity`.
    pub ingress_record_limit: usize,
    /// Rewrite the journal once it exceeds this fraction of the aggregate
    /// budget; retained state is re-emitted so recovery sees one truth.
    pub compact_at_fraction: f64,
    /// Provider-event records buffered while persistence is unavailable.
    /// Control records keep their own protected share; a data buffer this
    /// full interrupts the affected provider visibly.
    pub emergency_records: usize,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            session_memory_bytes: 16 * 1024 * 1024,
            aggregate_memory_bytes: 256 * 1024 * 1024,
            session_journal_bytes: 256 * 1024 * 1024,
            aggregate_journal_bytes: 2 * 1024 * 1024 * 1024,
            control_reserve_bytes: 8 * 1024 * 1024,
            max_message_bytes: 4 * 1024 * 1024,
            max_sessions: 64,
            channel_depth: 512,
            retry_window_ms: 24 * 60 * 60 * 1000,
            tombstone_limit: 262_144,
            ingress_record_limit: 8_192,
            compact_at_fraction: 0.75,
            emergency_records: 4096,
        }
    }
}

/// Construction inputs for `RuntimeHost::open`.
pub struct HostOptions {
    pub namespace: String,
    pub budgets: Budgets,
    /// Deterministic fault injection for journal writes — the harness's
    /// disk-failure barrier.
    pub journal_fault: Option<JournalFault>,
}

impl HostOptions {
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            budgets: Budgets::default(),
            journal_fault: None,
        }
    }
}

/// Credentials minted once per namespace and stored beside the journal with
/// owner-only permissions. Controller and emergency authority are distinct:
/// emergency stop must work while no daemon control exists.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub controller: String,
    pub emergency: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityFile {
    namespace: String,
    controller: String,
    emergency: String,
}

struct PendingRequest {
    answer: AnswerDisposition,
    consumed: bool,
    opened: Option<EventEnvelope>,
}

impl PendingRequest {
    fn retained_bytes(&self) -> u64 {
        self.opened
            .as_ref()
            .and_then(|opened| serde_json::to_vec(&opened.event).ok())
            .map_or(0, |bytes| bytes.len() as u64)
    }
}

/// A minted ingress grant as the host knows it — the presented token is
/// compared against this in constant time; expiry is immutable.
struct IssuedGrant {
    token: String,
    expires_at_ms: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TurnState {
    Open,
    Cancelling,
    Closed,
}

/// Everything sequenced and fenced within one provider generation.
/// Generations are kept after they end — their history replays and their
/// fences keep guarding.
struct Generation {
    number: u64,
    next_seq: u64,
    /// Lowest retained sequence. Replay below it is a blocked recovery.
    floor: u64,
    acknowledged: u64,
    events: VecDeque<EventEnvelope>,
    retained_bytes: u64,
    journal_bytes: u64,
    fence: Option<FenceKind>,
    hold: bool,
    held: VecDeque<Uuid>,
    pending: HashMap<String, PendingRequest>,
    turns: BTreeMap<Uuid, TurnState>,
    /// Ingress grants minted for this generation, by grant id.
    grants: HashMap<Uuid, IssuedGrant>,
    /// Agent requests accepted at scoped ingress, by request id.
    ingress: BTreeMap<Uuid, IngressRecord>,
    /// Live durable resource claims bound to this generation's work.
    claims: BTreeMap<Uuid, ResourceClaim>,
    /// The generation's last word was observed; nothing may follow it.
    ended: bool,
}

impl Generation {
    fn new(number: u64) -> Self {
        Self {
            number,
            next_seq: 1,
            floor: 1,
            acknowledged: 0,
            events: VecDeque::new(),
            retained_bytes: 0,
            journal_bytes: 0,
            fence: None,
            hold: false,
            held: VecDeque::new(),
            pending: HashMap::new(),
            turns: BTreeMap::new(),
            grants: HashMap::new(),
            ingress: BTreeMap::new(),
            claims: BTreeMap::new(),
            ended: false,
        }
    }

    fn high_water(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}

struct Session {
    id: Uuid,
    adapter_name: String,
    launch: Value,
    requested: RequestedConfig,
    /// Highest revision ever requested — reconfiguration must move forward.
    requested_revision: u64,
    effective: Option<EffectiveConfig>,
    status: SessionStatus,
    adapter: Option<Box<dyn AdapterHandle>>,
    rx: Option<mpsc::Receiver<ProviderEvent>>,
    /// Index 0 is generation 1. The last entry is the current generation.
    gens: Vec<Generation>,
}

impl Session {
    fn generation(&self) -> u64 {
        self.gens.last().map(|g| g.number).unwrap_or(1)
    }
    fn generation_at(&self, number: u64) -> Option<&Generation> {
        self.gens.iter().find(|g| g.number == number)
    }
    fn generation_at_mut(&mut self, number: u64) -> Option<&mut Generation> {
        self.gens.iter_mut().find(|g| g.number == number)
    }
    fn current(&self) -> &Generation {
        self.gens.last().expect("sessions always have a generation")
    }
    fn current_mut(&mut self) -> &mut Generation {
        self.gens
            .last_mut()
            .expect("sessions always have a generation")
    }
    fn inventory(&self) -> SessionInventory {
        let g = self.current();
        SessionInventory {
            adapter: self.adapter_name.clone(),
            requested: self.requested.clone(),
            requested_revision: self.requested_revision,
            session: SessionRef {
                session_id: self.id,
                generation: g.number,
            },
            status: self.status,
            high_water: g.high_water(),
            acknowledged: g.acknowledged,
            open_turns: g
                .turns
                .iter()
                .filter(|(_, t)| **t != TurnState::Closed)
                .map(|(id, _)| *id)
                .collect(),
            pending_requests: g
                .pending
                .iter()
                .filter(|(_, p)| !p.consumed)
                .map(|(id, _)| id.clone())
                .collect(),
            held_commands: g.held.len(),
            fence: g.fence,
            effective: self.effective.clone(),
            pending_ingress: g
                .ingress
                .values()
                .filter(|r| !r.disposition.is_terminal())
                .map(|r| r.request_id)
                .collect(),
            claims: g.claims.values().cloned().collect(),
            adapter_alive: self.adapter.as_ref().is_some_and(|a| a.alive()),
        }
    }
}

struct CommandState {
    session: Option<SessionRef>,
    action: CanonicalAction,
    retry_window_id: Uuid,
    expires_at_ms: u64,
    disposition: CommandDisposition,
}

pub struct RuntimeHost {
    namespace: String,
    instance: Uuid,
    creds: Credentials,
    journal: Journal,
    factory: Arc<dyn AdapterFactory>,
    clock: Box<dyn Clock>,
    budgets: Budgets,
    epoch: u64,
    next_epoch: u64,
    /// Retry windows ever issued, id → expiry. Commands bind one; its
    /// immutable expiry outlives the issuing epoch.
    windows: HashMap<Uuid, u64>,
    /// The window issued for the live epoch, re-emitted on compaction.
    current_window: Option<(u64, Uuid, u64)>,
    sessions: BTreeMap<Uuid, Session>,
    commands: BTreeMap<Uuid, CommandState>,
    /// Tombstoned command ids: retained only to keep rejecting.
    expired: HashSet<Uuid>,
    needs_reconcile: bool,
    unfenced_survivors: HashSet<Uuid>,
    build_identity: String,
    reconciled: bool,
    persistence_ok: bool,
    admission_open: bool,
    drained: bool,
    recovery: RecoveryState,
    tail_corrupt: bool,
    /// Records accepted but not yet durable — bounded emergency buffering
    /// while persistence is unavailable. Control records keep a protected
    /// share so cancellation and terminal facts never starve.
    pending_journal: VecDeque<JournalRecord>,
    /// Journal replay in progress — state application must not re-journal.
    replaying: bool,
    /// Held for the host's lifetime; dropping it frees the namespace.
    lock_file: File,
}

impl RuntimeHost {
    /// Open (or create) the runtime namespace at `root`. Exactly one live
    /// host may own it — the OS-held lock dies with its process, so a new
    /// owner can only appear after the old one is truly gone.
    pub fn open(
        root: &Path,
        options: HostOptions,
        factory: Arc<dyn AdapterFactory>,
        clock: Box<dyn Clock>,
    ) -> Result<Self, HostError> {
        #[cfg(unix)]
        crate::ipc::private_root(root)?;
        #[cfg(not(unix))]
        fs::create_dir_all(root).map_err(io_err)?;
        // The namespace root holds credentials, the journal, and the IPC
        // endpoint — owner-only, like the identity file inside it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
        }
        let lock_path = root.join("runtime.lock");
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(io_err)?;
        if lock_file.try_lock().is_err() {
            return Err(HostError::AlreadyOwned);
        }

        let creds = load_or_mint_identity(root, &options.namespace)?;
        let (journal, frames) = Journal::open(&root.join("journal.log"), options.journal_fault)
            .map_err(|e| HostError::Io(e.to_string()))?;

        let mut host = Self {
            namespace: options.namespace,
            instance: Uuid::new_v4(),
            creds,
            journal,
            factory,
            clock,
            budgets: options.budgets,
            epoch: 0,
            next_epoch: 1,
            windows: HashMap::new(),
            current_window: None,
            sessions: BTreeMap::new(),
            commands: BTreeMap::new(),
            expired: HashSet::new(),
            needs_reconcile: false,
            unfenced_survivors: HashSet::new(),
            build_identity: env!("CARGO_PKG_VERSION").into(),
            reconciled: false,
            persistence_ok: true,
            admission_open: true,
            drained: false,
            recovery: RecoveryState::Fresh,
            tail_corrupt: false,
            pending_journal: VecDeque::new(),
            replaying: false,
            lock_file,
        };
        host.tail_corrupt = host.journal.tail_corrupt();
        let drained_seen = host.fold(&frames);
        let crashed = !frames.is_empty() && !drained_seen;

        if drained_seen {
            // A previous instance drained cleanly; its sessions are history.
            host.sessions.clear();
            host.commands.clear();
            host.recovery = RecoveryState::Clean;
        } else if crashed {
            let interrupted = host.fence_survivors();
            host.recovery = RecoveryState::Interrupted {
                sessions: interrupted,
            };
            host.needs_reconcile = !host.sessions.is_empty();
        }

        host.journal_record(JournalRecord::HostStarted {
            instance: host.instance,
            namespace: host.namespace.clone(),
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;

        // Record the instance in the lock file for observability; the lock
        // itself is the singleton proof.
        use std::io::Write as _;
        let _ = host.lock_file.set_len(0);
        let _ = writeln!(
            host.lock_file,
            "instance={} pid={}",
            host.instance,
            std::process::id()
        );
        Ok(host)
    }

    /// The credential set controllers and the emergency route use.
    pub fn credentials(&self) -> &Credentials {
        &self.creds
    }

    /// Set the immutable executable identity during host composition, before IPC publication.
    pub fn with_build_identity(mut self, build: String) -> Self {
        self.build_identity = build;
        self
    }

    pub fn instance(&self) -> Uuid {
        self.instance
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn recovery(&self) -> RecoveryState {
        self.recovery
    }

    /// The live control epoch, or 0 while no controller holds the lease.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The persisted clock floor: `now` can never observe a time before
    /// this, so a clock rollback cannot reopen an expired retry window.
    fn now(&self) -> u64 {
        self.clock.now_ms().max(self.journal.floor_ms())
    }

    fn journal_record(&mut self, record: JournalRecord) -> Result<(), JournalError> {
        if self.replaying {
            return Ok(());
        }
        // Buffered records flush first, preserving record order. If the
        // store is still down the new record buffers behind them.
        if !self.pending_journal.is_empty()
            && let Err(e) = self.flush_pending()
        {
            self.persistence_ok = false;
            return Err(e);
        }
        let at = self.now();
        match self.journal.append(at, record) {
            Ok(()) => {
                if self.pending_journal.is_empty() {
                    self.persistence_ok = true;
                }
                Ok(())
            }
            Err(e) => {
                self.persistence_ok = false;
                Err(e)
            }
        }
    }

    /// Write everything the emergency buffer holds. Stops at the first
    /// failure, leaving the rest queued for the next attempt.
    fn flush_pending(&mut self) -> Result<(), JournalError> {
        while let Some(record) = self.pending_journal.front() {
            let at = self.now();
            self.journal.append(at, record.clone())?;
            self.pending_journal.pop_front();
        }
        self.persistence_ok = true;
        Ok(())
    }

    /// Best-effort journal write for facts that must not fail the
    /// operation (evidence trails, status transitions). On failure the
    /// record enters bounded emergency buffering — never silently lost,
    /// never unbounded.
    fn journal_best_effort(&mut self, record: JournalRecord) {
        if self.journal_record(record.clone()).is_err() {
            self.buffer_record(record);
        }
    }

    fn buffer_record(&mut self, record: JournalRecord) {
        // Control records always buffer; data records share the bounded
        // pool. A full data pool interrupts the provider in `ingest`.
        if record.is_control() || self.pending_data_records() < self.budgets.emergency_records {
            self.pending_journal.push_back(record);
        }
    }

    fn pending_data_records(&self) -> usize {
        self.pending_journal
            .iter()
            .filter(|r| !r.is_control())
            .count()
    }

    fn check_controller(&self, credential: &str) -> Result<(), HostError> {
        if bool::from(
            credential
                .as_bytes()
                .ct_eq(self.creds.controller.as_bytes()),
        ) {
            Ok(())
        } else {
            Err(HostError::Unauthorized)
        }
    }

    fn check_epoch(&self, epoch: u64) -> Result<(), HostError> {
        if self.epoch == 0 {
            return Err(HostError::NoControl);
        }
        if epoch != self.epoch {
            return Err(HostError::StaleEpoch {
                presented: epoch,
                current: self.epoch,
            });
        }
        Ok(())
    }

    /// Authenticate and negotiate compatibility. Incompatible control is
    /// rejected while existing sessions survive untouched.
    pub fn hello(&mut self, credential: &str, client: &ClientHello) -> Result<Hello, HostError> {
        self.check_controller(credential)?;
        if PROTOCOL_VERSION < client.protocol_min || PROTOCOL_VERSION > client.protocol_max {
            return Err(HostError::Incompatible(format!(
                "host speaks protocol {PROTOCOL_VERSION}; client range is {}..={}",
                client.protocol_min, client.protocol_max
            )));
        }
        let mut capabilities = vec!["emergency-stop".into(), "reconfigure".into()];
        if cfg!(unix) {
            capabilities.push("private-ipc-v1".into());
        }
        if client
            .capabilities
            .iter()
            .any(|required| !capabilities.contains(required))
        {
            return Err(HostError::Incompatible(
                "required capability unavailable".into(),
            ));
        }
        Ok(Hello {
            protocol: PROTOCOL_VERSION,
            protocol_min: PROTOCOL_VERSION,
            protocol_max: PROTOCOL_VERSION,
            runtime_build: self.build_identity.clone(),
            adapter_versions: self.factory.adapter_versions(),
            namespace: self.namespace.clone(),
            runtime_instance: self.instance,
            recovery: self.recovery,
            capabilities,
        })
    }

    /// Take the exclusive mutation lease. The new epoch atomically
    /// invalidates the previous controller: control transfer prevents new
    /// stale dispatch, though it cannot undo an already-authorized
    /// execution.
    pub fn acquire_control(
        &mut self,
        credential: &str,
        client: &ClientHello,
    ) -> Result<ControlGrant, HostError> {
        self.hello(credential, client)?;
        self.pump();
        if self.drained {
            return Err(HostError::Drained);
        }
        let epoch = self.next_epoch;
        let window = Uuid::new_v4();
        let expires = self.now() + self.budgets.retry_window_ms;

        let grant = ControlGrant {
            epoch,
            retry_window_id: window,
            retry_window_expires_at_ms: expires,
        };
        self.journal_record(JournalRecord::ControllerAcquired {
            epoch,
            retry_window_id: window,
            expires_at_ms: expires,
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;
        self.epoch = epoch;
        self.next_epoch += 1;
        self.windows.insert(window, expires);
        self.current_window = Some((epoch, window, expires));
        Ok(grant)
    }

    /// Inventory truth the controller must consume after detachment or
    /// crash recovery before new admissions are permitted.
    pub fn reconcile(&mut self, credential: &str, epoch: u64) -> Result<Inventory, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        self.try_fence_recovered();
        if !self.unfenced_survivors.is_empty() {
            return Err(HostError::NotReconciled);
        }
        self.journal_record(JournalRecord::Reconciled { epoch })
            .map_err(|_| HostError::PersistenceUnavailable)?;
        self.reconciled = true;
        self.needs_reconcile = false;
        Ok(self.inventory(epoch))
    }

    fn inventory(&self, epoch: u64) -> Inventory {
        Inventory {
            runtime_instance: self.instance,
            epoch,
            recovery: self.recovery,
            sessions: self.sessions.values().map(Session::inventory).collect(),
            open_commands: self
                .commands
                .iter()
                .filter(|(_, c)| !c.disposition.is_terminal())
                .map(|(id, c)| CommandReceipt {
                    command_id: *id,
                    disposition: c.disposition.clone(),
                })
                .collect(),
        }
    }

    /// Execute one action. Identical canonical payloads return the recorded
    /// truth under any live credential and epoch; a mismatched payload under
    /// a recorded id is rejected. A timeout means unknown acceptance — the
    /// caller queries `command`, never invents another delivery.
    pub fn execute(
        &mut self,
        credential: &str,
        env: ActionEnvelope,
    ) -> Result<CommandReceipt, HostError> {
        self.check_controller(credential)?;
        if env.namespace != self.namespace {
            return Err(HostError::Malformed("wrong runtime namespace".into()));
        }
        self.pump();
        self.sweep_expired();
        if self.drained {
            return Err(HostError::Drained);
        }
        if !self.persistence_ok {
            return Err(HostError::PersistenceUnavailable);
        }

        // Deduplication precedes the epoch fence on purpose: a retry under a
        // new epoch returns the recorded disposition, while a stale-epoch
        // retry of recorded work still reads its truth. Only *new* work is
        // fenced.
        if let Some(existing) = self.commands.get(&env.command_id) {
            if existing.action != env.action || existing.session != env.session {
                return Err(HostError::PayloadMismatch);
            }
            return Ok(CommandReceipt {
                command_id: env.command_id,
                disposition: existing.disposition.clone(),
            });
        }
        if self.expired.contains(&env.command_id) {
            return Err(HostError::ExpiredWindow(env.command_id));
        }

        // New work names this live instance and epoch.
        if env.runtime_instance != self.instance {
            return Err(HostError::Malformed(
                "envelope names a different runtime instance".into(),
            ));
        }
        self.check_epoch(env.epoch)?;
        let expires = *self
            .windows
            .get(&env.retry_window_id)
            .ok_or(HostError::UnknownWindow(env.retry_window_id))?;
        if self.now() > expires {
            return Err(HostError::ExpiredWindow(env.retry_window_id));
        }
        // Tombstones are never evicted — the bound refuses new commands
        // rather than letting an expired identity become new work.
        if self.expired.len() >= self.budgets.tombstone_limit {
            return Err(HostError::Capacity(
                "expired-command tombstone budget exhausted".into(),
            ));
        }

        if self.journal.bytes() + self.budgets.control_reserve_bytes
            >= self.budgets.aggregate_journal_bytes
        {
            return Err(HostError::Capacity("journal budget exhausted".into()));
        }

        self.journal_record(JournalRecord::Command {
            command_id: env.command_id,
            session: env.session,
            action: env.action.clone(),
            retry_window_id: env.retry_window_id,
            expires_at_ms: expires,
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;

        self.commands.insert(
            env.command_id,
            CommandState {
                session: env.session,
                action: env.action.clone(),
                retry_window_id: env.retry_window_id,
                expires_at_ms: expires,
                disposition: CommandDisposition::Recorded,
            },
        );
        self.route(env.command_id);
        if !self.persistence_ok {
            return Err(HostError::PersistenceUnavailable);
        }
        Ok(CommandReceipt {
            command_id: env.command_id,
            disposition: self
                .commands
                .get(&env.command_id)
                .map(|c| c.disposition.clone())
                .unwrap_or(CommandDisposition::Recorded),
        })
    }

    /// Read the recorded disposition of a command — the answer a caller
    /// gets when a response was lost or a timeout left acceptance unknown.
    pub fn command(&self, credential: &str, command_id: Uuid) -> Option<CommandReceipt> {
        if self.check_controller(credential).is_err() {
            return None;
        }
        if self.expired.contains(&command_id) {
            return Some(CommandReceipt {
                command_id,
                disposition: CommandDisposition::Rejected {
                    code: RejectCode::Stale,
                    detail: "retry window expired".into(),
                },
            });
        }
        self.commands.get(&command_id).map(|c| CommandReceipt {
            command_id,
            disposition: c.disposition.clone(),
        })
    }

    /// Ordered replay from a durable applied cursor, then the live stream —
    /// the same call serves both because the journal is the single order.
    /// A cursor below the retained floor is a blocked recovery, not a gap.
    pub fn events(
        &mut self,
        credential: &str,
        epoch: u64,
        cursor: EventCursor,
    ) -> Result<EventPage, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        if !self.persistence_ok {
            return Err(HostError::PersistenceUnavailable);
        }
        let session = self
            .sessions
            .get(&cursor.session_id)
            .ok_or(HostError::UnknownSession(cursor.session_id))?;
        let g = session
            .generation_at(cursor.generation)
            .ok_or(HostError::GenerationMismatch {
                presented: cursor.generation,
                current: session.generation(),
            })?;
        if cursor.applied + 1 < g.floor {
            return Err(HostError::CursorExpired {
                cursor: cursor.applied,
                floor: g.floor,
            });
        }
        let events: Vec<EventEnvelope> = g
            .events
            .iter()
            .filter(|e| e.sequence > cursor.applied)
            .cloned()
            .collect();
        Ok(EventPage {
            events,
            high_water: g.high_water(),
            retained_floor: g.floor,
        })
    }

    /// Read one native request independently of replay retention. Fetching
    /// payloads individually keeps namespace inventory inside IPC bounds.
    /// `None` means the request is absent or consumption is confirmed.
    pub fn pending_request(
        &mut self,
        credential: &str,
        epoch: u64,
        target: SessionRef,
        request_id: &str,
    ) -> Result<Option<PendingRequestSnapshot>, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        if !self.persistence_ok {
            return Err(HostError::PersistenceUnavailable);
        }
        let session = self
            .sessions
            .get(&target.session_id)
            .ok_or(HostError::UnknownSession(target.session_id))?;
        let g = session
            .generation_at(target.generation)
            .ok_or(HostError::GenerationMismatch {
                presented: target.generation,
                current: session.generation(),
            })?;
        Ok(g.pending
            .get(request_id)
            .filter(|pending| !pending.consumed)
            .map(|pending| PendingRequestSnapshot {
                request_id: request_id.to_owned(),
                answer: pending.answer.clone(),
                opened: pending.opened.clone(),
            }))
    }

    /// The controller's durable applied cursor. Advances only when the
    /// caller has committed its projections and effect receipts; it is the
    /// signal that retained payloads are reconstructible and may be
    /// reclaimed.
    pub fn acknowledge(
        &mut self,
        credential: &str,
        epoch: u64,
        cursor: EventCursor,
    ) -> Result<(), HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        let valid = {
            let session = self
                .sessions
                .get(&cursor.session_id)
                .ok_or(HostError::UnknownSession(cursor.session_id))?;
            let g =
                session
                    .generation_at(cursor.generation)
                    .ok_or(HostError::GenerationMismatch {
                        presented: cursor.generation,
                        current: session.generation(),
                    })?;
            cursor.applied <= g.high_water()
        };
        if !valid {
            return Err(HostError::Malformed(
                "acknowledgement beyond the high-water mark".into(),
            ));
        }
        let session = self.sessions.get_mut(&cursor.session_id).unwrap();
        let g = session.generation_at_mut(cursor.generation).unwrap();
        if cursor.applied > g.acknowledged {
            self.journal_record(JournalRecord::Acknowledged {
                session_id: cursor.session_id,
                generation: cursor.generation,
                applied: cursor.applied,
            })
            .map_err(|_| HostError::PersistenceUnavailable)?;
            self.sessions
                .get_mut(&cursor.session_id)
                .unwrap()
                .generation_at_mut(cursor.generation)
                .unwrap()
                .acknowledged = cursor.applied;
            self.reclaim(cursor.session_id, cursor.generation);
            self.sweep_expired();
            self.maybe_compact();
        }
        Ok(())
    }

    /// Narrow emergency authority: stop a session's work while no daemon
    /// control exists. Serialized into the same fence ordering as
    /// controller stops — whoever fences first wins.
    pub fn emergency_stop(
        &mut self,
        credential: &str,
        session_id: Uuid,
    ) -> Result<SessionStatus, HostError> {
        if !bool::from(credential.as_bytes().ct_eq(self.creds.emergency.as_bytes())) {
            return Err(HostError::Unauthorized);
        }
        self.pump();
        if self.drained {
            return Err(HostError::Drained);
        }
        if !self.sessions.contains_key(&session_id) {
            return Err(HostError::UnknownSession(session_id));
        }
        self.fence_and_terminate(session_id, FenceKind::EmergencyStop);
        Ok(self.sessions[&session_id].status)
    }

    /// Cheap health view for authenticated callers of either authority.
    pub fn status(&mut self, credential: &str) -> Result<HostStatus, HostError> {
        let controller = bool::from(
            credential
                .as_bytes()
                .ct_eq(self.creds.controller.as_bytes()),
        );
        let emergency = bool::from(credential.as_bytes().ct_eq(self.creds.emergency.as_bytes()));
        if !controller && !emergency {
            return Err(HostError::Unauthorized);
        }
        self.pump();
        Ok(HostStatus {
            runtime_instance: self.instance,
            epoch: self.epoch,
            recovery: self.recovery,
            persistence_ok: self.persistence_ok,
            tail_corrupt: self.tail_corrupt,
            admission_open: self.admission_open,
            sessions: self.sessions.values().map(Session::inventory).collect(),
            retained_event_bytes: self
                .sessions
                .values()
                .flat_map(|s| s.gens.iter())
                .map(|g| g.retained_bytes)
                .sum(),
            journal_bytes: self.journal.bytes(),
            buffered_records: self.pending_journal.len(),
            expired_command_ids: self.expired.len(),
            ingress_records: self
                .sessions
                .values()
                .flat_map(|s| s.gens.iter())
                .map(|g| g.ingress.len())
                .sum(),
            unfenced_survivors: self
                .unfenced_survivors
                .iter()
                .map(|id| SessionRef {
                    session_id: *id,
                    generation: self.sessions[id].generation(),
                })
                .collect(),
        })
    }

    /// Shut down with explicit intent. `WhenIdle` defers while any session
    /// is live or stopping; `Forced` fences and terminates owners and
    /// persists terminal dispositions before releasing the namespace.
    pub fn drain(
        &mut self,
        credential: &str,
        epoch: u64,
        policy: DrainPolicy,
    ) -> Result<DrainReport, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        if self.drained {
            return Err(HostError::Drained);
        }
        let live: Vec<Uuid> = self
            .sessions
            .values()
            .filter(|s| matches!(s.status, SessionStatus::Live | SessionStatus::Stopping))
            .map(|s| s.id)
            .collect();
        if policy == DrainPolicy::WhenIdle && !live.is_empty() {
            return Ok(DrainReport::Deferred {
                live_sessions: live,
            });
        }
        self.admission_open = false;
        let count = live.len();
        for id in live {
            // Forced drain reports interruption, not a clean stop.
            self.interrupt_session(id, FenceKind::Drained);
        }
        self.try_fence_recovered();
        let mut unconfirmed: Vec<Uuid> = self
            .sessions
            .values()
            .filter(|s| s.adapter.is_some())
            .map(|s| s.id)
            .collect();
        unconfirmed.extend(self.unfenced_survivors.iter().copied());
        unconfirmed.sort();
        unconfirmed.dedup();
        if !unconfirmed.is_empty() {
            return Ok(DrainReport::Deferred {
                live_sessions: unconfirmed,
            });
        }
        self.journal_record(JournalRecord::Drained)
            .map_err(|_| HostError::PersistenceUnavailable)?;
        self.drained = true;
        Ok(DrainReport::Drained { sessions: count })
    }

    /// Mint a session-scoped ingress grant. The grant is durable before it
    /// exists — a grant the journal never saw must verify as nothing.
    pub fn issue_grant(
        &mut self,
        credential: &str,
        epoch: u64,
        session: SessionRef,
        ttl_ms: u64,
    ) -> Result<SessionGrant, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        let target = self
            .sessions
            .get(&session.session_id)
            .ok_or(HostError::UnknownSession(session.session_id))?;
        if target.generation() != session.generation {
            return Err(HostError::GenerationMismatch {
                presented: session.generation,
                current: target.generation(),
            });
        }
        let g = target.current();
        if g.fence.is_some() || g.ended {
            return Err(HostError::Malformed(
                "cannot grant ingress to a fenced generation".into(),
            ));
        }
        let grant = SessionGrant {
            grant_id: Uuid::new_v4(),
            session_id: session.session_id,
            generation: session.generation,
            token: mint_credential(),
            expires_at_ms: self.now().saturating_add(ttl_ms),
        };
        self.journal_record(JournalRecord::GrantIssued {
            session_id: grant.session_id,
            generation: grant.generation,
            grant_id: grant.grant_id,
            token: grant.token.clone(),
            expires_at_ms: grant.expires_at_ms,
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;
        self.sessions
            .get_mut(&session.session_id)
            .and_then(|s| s.generation_at_mut(session.generation))
            .unwrap()
            .grants
            .insert(
                grant.grant_id,
                IssuedGrant {
                    token: grant.token.clone(),
                    expires_at_ms: grant.expires_at_ms,
                },
            );
        Ok(grant)
    }

    /// Verify a presented grant: constant-time token comparison, session
    /// and generation scope, immutable expiry. Every failure is the same
    /// `Unauthorized` — a wrong token, a foreign session, a dead
    /// generation, and an expired window are indistinguishable to callers,
    /// and authority is never renewed.
    pub fn check_grant(&self, grant: &SessionGrant) -> Result<SessionRef, HostError> {
        let target = self.check_recorded_grant(grant)?;
        let session = self
            .sessions
            .get(&target.session_id)
            .ok_or(HostError::Unauthorized)?;
        let generation = session.current();
        if session.generation() != target.generation
            || generation.fence.is_some()
            || generation.ended
        {
            return Err(HostError::Unauthorized);
        }
        Ok(target)
    }

    // Historical outcomes remain queryable without authorizing new work.
    fn check_recorded_grant(&self, grant: &SessionGrant) -> Result<SessionRef, HostError> {
        let target = SessionRef {
            session_id: grant.session_id,
            generation: grant.generation,
        };
        let ok = self
            .sessions
            .get(&grant.session_id)
            .and_then(|s| s.generation_at(grant.generation))
            .and_then(|g| g.grants.get(&grant.grant_id))
            .is_some_and(|issued| {
                self.now() < issued.expires_at_ms
                    && grant.expires_at_ms == issued.expires_at_ms
                    && bool::from(grant.token.as_bytes().ct_eq(issued.token.as_bytes()))
            });
        if ok {
            Ok(target)
        } else {
            Err(HostError::Unauthorized)
        }
    }

    /// Accept one agent request under its session grant. Identity dedup is
    /// the same rule commands follow: the same request id with the same
    /// canonical payload returns the recorded truth; a different payload
    /// under a recorded id is refused. Recording is durable before any
    /// forwarding.
    pub fn ingress_submit(
        &mut self,
        grant: &SessionGrant,
        request: IngressRequest,
    ) -> Result<IngressRecord, HostError> {
        let target = self.check_grant(grant)?;
        self.pump();
        if !self.persistence_ok {
            return Err(HostError::PersistenceUnavailable);
        }
        let now = self.now();
        {
            let session = self.sessions.get(&target.session_id).unwrap();
            let g = session.generation_at(target.generation).unwrap();
            if let Some(existing) = g.ingress.get(&request.request_id) {
                if existing.class == request.class && existing.subject == request.subject {
                    return Ok(existing.clone());
                }
                return Err(HostError::PayloadMismatch);
            }
            if g.ingress.len() >= self.budgets.ingress_record_limit {
                return Err(HostError::Capacity(
                    "ingress record budget exhausted for this generation".into(),
                ));
            }
        }
        // A request landing on a fenced or ended generation is evidence,
        // not execution — recorded as refused. One already past its
        // deadline is expired on arrival.
        let disposition = {
            let session = self.sessions.get(&target.session_id).unwrap();
            let g = session.generation_at(target.generation).unwrap();
            if g.fence.is_some() || g.ended || session.status != SessionStatus::Live {
                IngressDisposition::Refused {
                    detail: "session generation is fenced".into(),
                }
            } else if request.deadline_ms <= now {
                IngressDisposition::Expired
            } else {
                IngressDisposition::Pending
            }
        };
        let record = IngressRecord {
            request_id: request.request_id,
            session: target,
            grant_id: grant.grant_id,
            class: request.class,
            subject: request.subject,
            deadline_ms: request.deadline_ms,
            forwarded: false,
            disposition,
        };
        self.journal_record(JournalRecord::IngressOpened {
            record: Box::new(record.clone()),
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;
        self.sessions
            .get_mut(&target.session_id)
            .and_then(|s| s.generation_at_mut(target.generation))
            .unwrap()
            .ingress
            .insert(record.request_id, record.clone());
        Ok(record)
    }

    /// Read one ingress record under the grant that opened it. A caller
    /// sees only its own session's requests — anything else is
    /// indistinguishable from a bad grant.
    pub fn ingress_query(
        &mut self,
        grant: &SessionGrant,
        request_id: Uuid,
    ) -> Result<IngressRecord, HostError> {
        let target = self.check_recorded_grant(grant)?;
        self.pump();
        let session = self.sessions.get(&target.session_id).unwrap();
        let g = session.generation_at(target.generation).unwrap();
        match g.ingress.get(&request_id) {
            Some(record) if record.grant_id == grant.grant_id => Ok(record.clone()),
            _ => Err(HostError::Unauthorized),
        }
    }

    /// Everything the current controller's service must answer. Pending
    /// records mark forwarded in the same step — the dispatch marker that
    /// decides between `Expired` and `Uncertain` later. Forwarded but
    /// unresolved records are returned again: a new epoch's service still
    /// owes them, and the daemon dedups by request identity.
    pub fn ingress_drain(
        &mut self,
        credential: &str,
        epoch: u64,
    ) -> Result<Vec<ForwardedRequest>, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        let now = self.now();
        let mut mark = Vec::new();
        let mut out = Vec::new();
        for session in self.sessions.values() {
            for g in &session.gens {
                for record in g.ingress.values() {
                    if record.disposition.is_terminal()
                        || matches!(record.disposition, IngressDisposition::Uncertain { .. })
                        || record.deadline_ms <= now
                    {
                        continue;
                    }
                    out.push(ForwardedRequest {
                        request_id: record.request_id,
                        session: record.session,
                        grant_id: record.grant_id,
                        class: record.class,
                        subject: record.subject.clone(),
                        deadline_ms: record.deadline_ms,
                    });
                    if !record.forwarded {
                        mark.push((session.id, g.number, record.request_id));
                    }
                }
            }
        }
        for (session_id, generation, request_id) in mark {
            self.journal_best_effort(JournalRecord::IngressForwarded { request_id });
            if let Some(s) = self.sessions.get_mut(&session_id)
                && let Some(g) = s.generation_at_mut(generation)
                && let Some(r) = g.ingress.get_mut(&request_id)
            {
                r.forwarded = true;
                r.disposition = IngressDisposition::Forwarded;
            }
        }
        Ok(out)
    }

    /// Record the service's outcome for one forwarded request. Outcomes
    /// write once: a later answer cannot rewrite a terminal truth, while
    /// `Uncertain` remains reconcilable by qualified evidence.
    pub fn ingress_resolve(
        &mut self,
        credential: &str,
        epoch: u64,
        request_id: Uuid,
        outcome: ForwardedOutcome,
    ) -> Result<IngressRecord, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        let (session_id, generation) = self
            .sessions
            .iter()
            .find_map(|(sid, s)| {
                s.gens.iter().find_map(|g| {
                    g.ingress
                        .contains_key(&request_id)
                        .then_some((*sid, g.number))
                })
            })
            .ok_or(HostError::Malformed("unknown ingress request".into()))?;
        {
            let s = self.sessions.get(&session_id).unwrap();
            let existing = &s.generation_at(generation).unwrap().ingress[&request_id];
            if existing.disposition.is_terminal() {
                return Ok(existing.clone());
            }
        }
        let disposition = match outcome {
            ForwardedOutcome::Completed { result } => IngressDisposition::Completed { result },
            ForwardedOutcome::Failed { detail } => IngressDisposition::Failed { detail },
            ForwardedOutcome::Uncertain { detail } => IngressDisposition::Uncertain { detail },
        };
        self.journal_best_effort(JournalRecord::IngressOutcome {
            request_id,
            disposition: disposition.clone(),
        });
        let s = self.sessions.get_mut(&session_id).unwrap();
        let g = s.generation_at_mut(generation).unwrap();
        let record = g.ingress.get_mut(&request_id).unwrap();
        record.disposition = disposition;
        Ok(record.clone())
    }

    /// Register a durable resource claim against runtime-owned work.
    /// Claims survive controller replacement; release is always an
    /// explicit decision after the owning work is reconciled or
    /// terminated, never a side effect of detachment.
    pub fn register_claim(
        &mut self,
        credential: &str,
        epoch: u64,
        claim: ResourceClaim,
    ) -> Result<(), HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        let session = self
            .sessions
            .get(&claim.session.session_id)
            .ok_or(HostError::UnknownSession(claim.session.session_id))?;
        if session.generation() != claim.session.generation {
            return Err(HostError::GenerationMismatch {
                presented: claim.session.generation,
                current: session.generation(),
            });
        }
        let g = session.current();
        if g.fence.is_some() || g.ended {
            return Err(HostError::Malformed(
                "cannot claim resources on a fenced generation".into(),
            ));
        }
        self.journal_record(JournalRecord::ClaimRegistered {
            claim: claim.clone(),
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;
        self.sessions
            .get_mut(&claim.session.session_id)
            .unwrap()
            .current_mut()
            .claims
            .insert(claim.claim_id, claim);
        Ok(())
    }

    /// Release a claim by identity. Only the current controller releases;
    /// the claim's absence afterwards is a journaled fact, not a lapse.
    pub fn release_claim(
        &mut self,
        credential: &str,
        epoch: u64,
        claim_id: Uuid,
    ) -> Result<(), HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        let found = self
            .sessions
            .iter()
            .find_map(|(sid, s)| {
                s.gens
                    .iter()
                    .find_map(|g| g.claims.contains_key(&claim_id).then_some((*sid, g.number)))
            })
            .ok_or(HostError::Malformed("unknown claim".into()))?;
        self.journal_record(JournalRecord::ClaimReleased {
            claim_id,
            session_id: found.0,
            generation: found.1,
        })
        .map_err(|_| HostError::PersistenceUnavailable)?;
        if let Some(s) = self.sessions.get_mut(&found.0)
            && let Some(g) = s.generation_at_mut(found.1)
        {
            g.claims.remove(&claim_id);
        }
        Ok(())
    }

    /// Drain adapter intake channels into the journal. This is the
    /// deterministic barrier: provider events become durable, ordered, and
    /// replayable exactly when the host drains them — including while no
    /// controller is attached.
    pub fn pump(&mut self) -> usize {
        let ids: Vec<Uuid> = self.sessions.keys().copied().collect();
        let mut drained = 0;
        for id in ids {
            drained += self.pump_session(id);
        }
        self.sweep_ingress();
        drained
    }

    // -- internals -----------------------------------------------------

    fn pump_session(&mut self, session_id: Uuid) -> usize {
        let mut count = 0;
        loop {
            let next = {
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return count;
                };
                match session.rx.as_ref().map(|rx| rx.try_recv()) {
                    Some(Ok(event)) if !session.current().ended => Some(event),
                    Some(Ok(_)) => continue,
                    Some(Err(mpsc::TryRecvError::Disconnected))
                        if !session.adapter.as_ref().is_some_and(|a| a.alive()) =>
                    {
                        // The adapter's sender dropped without a Terminated
                        // event — the transport is gone.
                        session.adapter = None;
                        session.rx = None;
                        if session.current().ended {
                            return count;
                        }
                        Some(ProviderEvent::Terminated {
                            reason: TerminateReason::Interrupted {
                                detail: "provider transport lost".into(),
                            },
                        })
                    }
                    _ => None,
                }
            };
            let Some(event) = next else { break };
            self.ingest(session_id, event);
            count += 1;
            if self.over_budget(session_id) {
                let pausable = self
                    .sessions
                    .get(&session_id)
                    .and_then(|s| s.adapter.as_ref())
                    .is_some_and(|a| a.offline_matrix().can_pause_intake);
                if pausable {
                    // Leave the rest queued in the bounded channel — the
                    // provider is paused at intake, which is the honest
                    // backpressure position.
                    break;
                }
                self.interrupt_session(session_id, FenceKind::Backpressure);
                break;
            }
        }
        count
    }

    fn over_budget(&self, session_id: Uuid) -> bool {
        let Some(session) = self.sessions.get(&session_id) else {
            return false;
        };
        let g = session.current();
        if g.retained_bytes > self.budgets.session_memory_bytes
            || g.journal_bytes > self.budgets.session_journal_bytes
        {
            return true;
        }
        let aggregate: u64 = self
            .sessions
            .values()
            .flat_map(|s| s.gens.iter())
            .map(|g| g.retained_bytes)
            .sum();
        aggregate > self.budgets.aggregate_memory_bytes
            || self.journal.bytes() > self.budgets.aggregate_journal_bytes
    }

    /// Sequence, journal, and apply one provider event. Journaling precedes
    /// visibility: replay and live readers see identical order because
    /// there is only one order.
    fn ingest(&mut self, session_id: Uuid, event: ProviderEvent) {
        let size = serde_json::to_vec(&event)
            .map(|v| v.len() as u64)
            .unwrap_or(0);
        let envelope = {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
            let g = session.current_mut();
            // One oversized message is refused with a sequenced marker —
            // visible in replay, never a silent gap.
            let event = match event {
                ProviderEvent::Terminated {
                    reason: TerminateReason::Requested,
                } => match g.fence {
                    Some(FenceKind::Backpressure) => ProviderEvent::Terminated {
                        reason: TerminateReason::Backpressure,
                    },
                    Some(FenceKind::Drained) | Some(FenceKind::Interrupted) => {
                        ProviderEvent::Terminated {
                            reason: TerminateReason::Interrupted {
                                detail: "provider owner interrupted".into(),
                            },
                        }
                    }
                    _ => ProviderEvent::Terminated {
                        reason: TerminateReason::Requested,
                    },
                },
                event => event,
            };
            let event = if size > self.budgets.max_message_bytes {
                ProviderEvent::Dropped {
                    reason: DropReason::Oversize,
                    bytes: size,
                }
            } else {
                event
            };
            let (delivery_id, mut turn_id, request_id, config_revision) = attribution(&event);
            if matches!(&event, ProviderEvent::RequestOpened { .. }) {
                // Native requests belong to the sole open turn when the
                // adapter has one. Never guess an owner among concurrent
                // turns, or resurrect a finished turn's request on attach.
                let mut open_turns = g
                    .turns
                    .iter()
                    .filter(|(_, state)| **state != TurnState::Closed)
                    .map(|(id, _)| *id);
                turn_id = open_turns.next().filter(|_| open_turns.next().is_none());
            }
            let sequence = g.next_seq;
            g.next_seq += 1;
            let envelope = EventEnvelope {
                runtime_instance: self.instance,
                session_id,
                generation: g.number,
                sequence,
                delivery_id,
                turn_id,
                request_id,
                config_revision,
                event,
            };
            let stored = serde_json::to_vec(&envelope.event)
                .map(|v| v.len() as u64)
                .unwrap_or(0);
            g.events.push_back(envelope.clone());
            g.retained_bytes += stored;
            g.journal_bytes += stored;
            envelope
        };
        self.journal_best_effort(JournalRecord::Event {
            envelope: Box::new(envelope.clone()),
        });
        self.apply_event(&envelope);
        // Bounded emergency buffering is exhausted: the provider cannot
        // keep emitting into a store that is not keeping up.
        if !self.persistence_ok && self.pending_data_records() >= self.budgets.emergency_records {
            self.interrupt_session(session_id, FenceKind::Backpressure);
        }
    }

    /// Derived state from a journaled event — used by live ingest and by
    /// recovery fold so both paths build identical truth.
    fn apply_event(&mut self, envelope: &EventEnvelope) {
        let session_id = envelope.session_id;
        let generation = envelope.generation;
        let sequence = envelope.sequence;
        let event = &envelope.event;
        let mut terminate: Option<TerminateReason> = None;
        let mut activated_revision: Option<u64> = None;
        {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
            let Some(g) = session.generation_at_mut(generation) else {
                return;
            };
            match event {
                ProviderEvent::TurnStarted { turn_id, .. } => {
                    g.turns.insert(*turn_id, TurnState::Open);
                }
                ProviderEvent::TurnCompleted { turn_id, .. }
                | ProviderEvent::TurnCancelled { turn_id } => {
                    g.turns.insert(*turn_id, TurnState::Closed);
                }
                ProviderEvent::RequestOpened { request_id, .. } => {
                    let pending = PendingRequest {
                        answer: AnswerDisposition::Pending,
                        consumed: false,
                        opened: Some(envelope.clone()),
                    };
                    // The attachment copy outlives replay acknowledgement
                    // and shares its memory budget, including after recovery.
                    g.retained_bytes += pending.retained_bytes();
                    if let Some(previous) = g.pending.insert(request_id.clone(), pending) {
                        g.retained_bytes =
                            g.retained_bytes.saturating_sub(previous.retained_bytes());
                    }
                }
                ProviderEvent::RequestAnswered { request_id } => {
                    if let Some(pending) = g.pending.get_mut(request_id) {
                        g.retained_bytes =
                            g.retained_bytes.saturating_sub(pending.retained_bytes());
                        pending.consumed = true;
                        pending.opened = None;
                    }
                }
                ProviderEvent::ConfigActivated {
                    revision,
                    boundary,
                    effective,
                } => {
                    session.effective = Some(EffectiveConfig {
                        revision: *revision,
                        boundary: *boundary,
                        config: effective.clone(),
                    });
                    activated_revision = Some(*revision);
                }
                ProviderEvent::Terminated { reason } => {
                    g.ended = true;
                    terminate = Some(reason.clone());
                }
                _ => {}
            }
        }
        if let Some(reason) = terminate
            && let Some(session) = self.sessions.get_mut(&session_id)
        {
            session.adapter = None;
            session.status = match reason {
                TerminateReason::Requested => SessionStatus::Terminated,
                TerminateReason::Exited { .. } => SessionStatus::Exited,
                TerminateReason::Interrupted { .. } | TerminateReason::Backpressure => {
                    SessionStatus::Interrupted
                }
            };
        }
        // Qualified adapter evidence reconciles an uncertain delivery —
        // never a blind resend or re-application.
        if let ProviderEvent::RequestAnswered { request_id } = event {
            let mut reconciled = false;
            if let Some(session) = self.sessions.get_mut(&session_id)
                && let Some(g) = session.generation_at_mut(generation)
                && let Some(p) = g.pending.get_mut(request_id)
                && p.consumed
                && matches!(
                    p.answer,
                    AnswerDisposition::Attempted | AnswerDisposition::Uncertain { .. }
                )
            {
                p.answer = AnswerDisposition::Accepted;
                reconciled = true;
            }
            if reconciled {
                self.journal_best_effort(JournalRecord::RequestDisposition {
                    session_id,
                    generation,
                    request_id: request_id.clone(),
                    disposition: AnswerDisposition::Accepted,
                    consumed: true,
                    opened: None,
                });
            }
            // Consumption evidence also terminally attributes the resolve
            // command — the answer provably reached the provider.
            let target = SessionRef {
                session_id,
                generation,
            };
            let resolves: Vec<Uuid> = self
                .commands
                .iter()
                .filter(|(_, c)| {
                    c.session == Some(target)
                        && !c.disposition.is_terminal()
                        && matches!(
                            &c.action,
                            CanonicalAction::ResolveRequest { request_id: r, .. } if r == request_id
                        )
                })
                .map(|(id, _)| *id)
                .collect();
            for id in resolves {
                self.set_disposition(
                    id,
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Completed,
                        event_sequence: Some(sequence),
                    },
                );
            }
        }
        if let Some(revision) = activated_revision {
            // The activated revision terminally attributes its reconfigure
            // command — acceptance and activation are separate facts, and
            // this is the activation.
            let target = SessionRef {
                session_id,
                generation,
            };
            let reconfigures: Vec<Uuid> = self
                .commands
                .iter()
                .filter(|(_, c)| {
                    c.session == Some(target)
                        && !c.disposition.is_terminal()
                        && matches!(
                            &c.action,
                            CanonicalAction::Reconfigure { revision: r, .. } if *r == revision
                        )
                })
                .map(|(id, _)| *id)
                .collect();
            for id in reconfigures {
                self.set_disposition(
                    id,
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Completed,
                        event_sequence: Some(sequence),
                    },
                );
            }
        }
        // Attribute the deciding event to commands still awaiting an
        // outcome — completion is attributed to a specific delivery/turn.
        match event {
            ProviderEvent::TurnCompleted { turn_id, .. } => {
                self.attribute_turn_end(session_id, generation, *turn_id, sequence, false);
            }
            ProviderEvent::TurnCancelled { turn_id } => {
                self.attribute_turn_end(session_id, generation, *turn_id, sequence, true);
            }
            ProviderEvent::Terminated { .. } => {
                // A stop command whose termination was unconfirmed resolves
                // when the generation's last word arrives.
                let target = SessionRef {
                    session_id,
                    generation,
                };
                let stops: Vec<Uuid> = self
                    .commands
                    .iter()
                    .filter(|(_, c)| {
                        c.session == Some(target)
                            && matches!(c.action, CanonicalAction::StopSession)
                            && matches!(
                                c.disposition,
                                CommandDisposition::Uncertain { .. }
                                    | CommandDisposition::ProviderAccepted
                            )
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in stops {
                    self.set_disposition(
                        id,
                        CommandDisposition::Terminal {
                            outcome: TerminalOutcome::Completed,
                            event_sequence: Some(sequence),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    fn attribute_turn_end(
        &mut self,
        session_id: Uuid,
        generation: u64,
        turn_id: Uuid,
        sequence: u64,
        cancelled: bool,
    ) {
        let target = SessionRef {
            session_id,
            generation,
        };
        let ids: Vec<Uuid> = self
            .commands
            .iter()
            .filter(|(_, c)| c.session == Some(target) && !c.disposition.is_terminal())
            .filter(|(_, c)| match &c.action {
                CanonicalAction::Submit { turn_id: t, .. } => *t == turn_id,
                CanonicalAction::CancelTurn { turn_id: t } => *t == turn_id,
                _ => false,
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let outcome = match self.commands.get(&id).map(|c| &c.action) {
                Some(CanonicalAction::CancelTurn { .. }) if !cancelled => {
                    // The turn completed before cancellation took effect.
                    TerminalOutcome::Refused
                }
                _ if cancelled => TerminalOutcome::Cancelled,
                _ => TerminalOutcome::Completed,
            };
            self.set_disposition(
                id,
                CommandDisposition::Terminal {
                    outcome,
                    event_sequence: Some(sequence),
                },
            );
        }
    }

    fn set_disposition(&mut self, command_id: Uuid, disposition: CommandDisposition) {
        if let Some(cmd) = self.commands.get_mut(&command_id) {
            cmd.disposition = disposition.clone();
        }
        self.journal_best_effort(JournalRecord::Disposition {
            command_id,
            disposition,
        });
    }

    fn route(&mut self, command_id: Uuid) {
        let (session, action) = match self.commands.get(&command_id) {
            Some(c) => (c.session, c.action.clone()),
            None => return,
        };
        match action {
            CanonicalAction::AdmitSession {
                session_id,
                adapter,
                launch,
                requested,
            } => self.route_admit(command_id, session_id, adapter, launch, requested),
            action => {
                let Some(target) = session else {
                    self.set_disposition(
                        command_id,
                        CommandDisposition::Rejected {
                            code: RejectCode::InvalidAction,
                            detail: "action requires a session reference".into(),
                        },
                    );
                    return;
                };
                self.route_session(command_id, target, action)
            }
        }
    }

    fn reject(&mut self, command_id: Uuid, code: RejectCode, detail: impl Into<String>) {
        self.set_disposition(
            command_id,
            CommandDisposition::Rejected {
                code,
                detail: detail.into(),
            },
        );
    }

    fn route_admit(
        &mut self,
        command_id: Uuid,
        session_id: Uuid,
        adapter: String,
        launch: Value,
        requested: RequestedConfig,
    ) {
        if self.drained || !self.admission_open {
            return self.reject(command_id, RejectCode::InvalidAction, "admission is closed");
        }
        if self.needs_reconcile {
            return self.reject(
                command_id,
                RejectCode::InvalidAction,
                "inventory reconciliation required first",
            );
        }
        if self.sessions.contains_key(&session_id) {
            return self.reject(
                command_id,
                RejectCode::InvalidAction,
                "session id is already live",
            );
        }
        if self.sessions.len() >= self.budgets.max_sessions {
            return self.reject(command_id, RejectCode::Capacity, "session count limit");
        }
        // Admission is durable before the provider is contacted — a crash
        // between them is reconciled, not rediscovered.
        if self
            .journal_record(JournalRecord::Admitted {
                session_id,
                generation: 1,
                adapter: adapter.clone(),
                launch: launch.clone(),
                requested: requested.clone(),
            })
            .is_err()
        {
            return self.reject(
                command_id,
                RejectCode::InvalidAction,
                "admission persistence failed; provider was not contacted",
            );
        }
        let (tx, rx) = mpsc::sync_channel(self.budgets.channel_depth);
        let ctx = SessionContext {
            session_id,
            generation: 1,
            namespace: self.namespace.clone(),
            adapter: adapter.clone(),
            launch: launch.clone(),
            requested: requested.clone(),
            sink: tx,
        };
        match self.factory.spawn(ctx) {
            Ok(handle) => {
                self.sessions.insert(
                    session_id,
                    Session {
                        id: session_id,
                        adapter_name: adapter,
                        launch,
                        requested,
                        requested_revision: 0,
                        effective: None,
                        status: SessionStatus::Live,
                        adapter: Some(handle),
                        rx: Some(rx),
                        gens: vec![Generation::new(1)],
                    },
                );
                // Admission ends here: the session exists and the command
                // has nothing further to await.
                self.set_disposition(
                    command_id,
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Completed,
                        event_sequence: None,
                    },
                );
            }
            Err(e) => {
                // The admission was authorized but the provider never
                // started. Keep the session as interrupted evidence so
                // reconciliation sees what was attempted.
                self.sessions.insert(
                    session_id,
                    Session {
                        id: session_id,
                        adapter_name: adapter,
                        launch,
                        requested,
                        requested_revision: 0,
                        effective: None,
                        status: SessionStatus::Interrupted,
                        adapter: None,
                        rx: None,
                        gens: vec![Generation::new(1)],
                    },
                );
                self.set_session_status(session_id, SessionStatus::Interrupted);
                self.reject(command_id, RejectCode::SpawnFailed, e.to_string());
            }
        }
    }

    /// Pre-dispatch checks shared by every session-scoped action.
    fn session_gate(&self, target: SessionRef) -> Result<(), (RejectCode, String)> {
        let Some(session) = self.sessions.get(&target.session_id) else {
            return Err((RejectCode::UnknownSession, "no such session".into()));
        };
        if session.generation() != target.generation {
            return Err((
                RejectCode::GenerationMismatch,
                format!(
                    "generation {} is not current {}",
                    target.generation,
                    session.generation()
                ),
            ));
        }
        let g = session.current();
        if g.fence.is_some() || g.ended {
            return Err((RejectCode::Fenced, "generation is fenced".into()));
        }
        if session.status != SessionStatus::Live {
            return Err((
                RejectCode::Interrupted,
                format!("session is {:?}", session.status),
            ));
        }
        Ok(())
    }

    fn route_session(&mut self, command_id: Uuid, target: SessionRef, action: CanonicalAction) {
        let session_id = target.session_id;
        if let Err((code, detail)) = self.session_gate(target) {
            return self.reject(command_id, code, detail);
        }
        match action {
            CanonicalAction::Submit { .. } => {
                if self.sessions[&session_id].current().hold {
                    // Dispatch is held until the settling cancellation is
                    // released by explicit controller policy.
                    self.set_disposition(command_id, CommandDisposition::DispatchPending);
                    self.sessions
                        .get_mut(&session_id)
                        .unwrap()
                        .current_mut()
                        .held
                        .push_back(command_id);
                    return;
                }
                self.dispatch(command_id, target, action);
            }
            CanonicalAction::CancelTurn { turn_id } => {
                let open = self.sessions[&session_id].current().turns.get(&turn_id)
                    == Some(&TurnState::Open);
                if !open {
                    return self.reject(
                        command_id,
                        RejectCode::InvalidAction,
                        "no open turn with that id",
                    );
                }
                if self
                    .journal_record(JournalRecord::DispatchHeld {
                        session_id,
                        generation: target.generation,
                    })
                    .is_err()
                {
                    self.set_disposition(
                        command_id,
                        CommandDisposition::Uncertain {
                            stage: UncertainStage::DispatchUnknown,
                            detail: "cancel hold was not durable; provider was not contacted"
                                .into(),
                        },
                    );
                    return;
                }
                {
                    let session = self.sessions.get_mut(&session_id).unwrap();
                    let g = session.current_mut();
                    g.hold = true;
                    g.turns.insert(turn_id, TurnState::Cancelling);
                }
                if !self.record_dispatch_attempt(command_id) {
                    return;
                }
                let outcome = self
                    .sessions
                    .get_mut(&session_id)
                    .and_then(|s| s.adapter.as_mut())
                    .map(|a| a.cancel(turn_id));
                match outcome {
                    Some(CancelOutcome::Confirmed) => {
                        self.set_disposition(command_id, CommandDisposition::ProviderAccepted)
                    }
                    Some(CancelOutcome::NotRunning) | Some(CancelOutcome::Failed { .. }) => self
                        .set_disposition(
                            command_id,
                            CommandDisposition::Terminal {
                                outcome: TerminalOutcome::Refused,
                                event_sequence: None,
                            },
                        ),
                    Some(CancelOutcome::Unknown { detail }) => self.set_disposition(
                        command_id,
                        CommandDisposition::Uncertain {
                            stage: UncertainStage::AcceptanceUnknown,
                            detail,
                        },
                    ),
                    None => self.set_disposition(
                        command_id,
                        CommandDisposition::Uncertain {
                            stage: UncertainStage::DispatchUnknown,
                            detail: "no adapter owns this generation".into(),
                        },
                    ),
                }
                self.pump();
            }
            CanonicalAction::StopSession => {
                self.fence_and_terminate(session_id, FenceKind::Stop);
                let status = self.sessions[&session_id].status;
                let disposition = if status == SessionStatus::Terminated {
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Completed,
                        event_sequence: None,
                    }
                } else {
                    CommandDisposition::Uncertain {
                        stage: UncertainStage::OutcomeUnknown,
                        detail: "termination is not confirmed".into(),
                    }
                };
                self.set_disposition(command_id, disposition);
            }
            CanonicalAction::PermitDispatch => {
                self.journal_best_effort(JournalRecord::DispatchPermitted {
                    session_id,
                    generation: target.generation,
                });
                let held: Vec<Uuid> = {
                    let session = self.sessions.get_mut(&session_id).unwrap();
                    let g = session.current_mut();
                    g.hold = false;
                    g.held.drain(..).collect()
                };
                self.set_disposition(
                    command_id,
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Completed,
                        event_sequence: None,
                    },
                );
                for held_id in held {
                    let pending = self
                        .commands
                        .get(&held_id)
                        .filter(|c| c.disposition == CommandDisposition::DispatchPending)
                        .map(|c| (c.session, c.action.clone()));
                    if let Some((Some(t), held_action)) = pending {
                        self.dispatch(held_id, t, held_action);
                    }
                }
            }
            CanonicalAction::ResolveRequest {
                request_id,
                resolution,
            } => self.route_resolve(command_id, target, request_id, resolution),
            CanonicalAction::Reconfigure {
                revision,
                target: cfg,
            } => {
                let stale = revision <= self.sessions[&session_id].requested_revision;
                if stale {
                    return self.reject(
                        command_id,
                        RejectCode::Stale,
                        "configuration revision did not advance",
                    );
                }
                let provider_changed =
                    self.sessions[&session_id].requested.provider.as_ref() != cfg.provider.as_ref();
                if provider_changed {
                    // Replacement terminates the old owner first and starts
                    // a new generation — machinery owned by later stages.
                    return self.reject(
                        command_id,
                        RejectCode::RequiresReplacement,
                        "provider change requires a new generation",
                    );
                }
                // The requested revision is durable before dispatch — a
                // crash cannot regress it into accepting something stale.
                self.journal_best_effort(JournalRecord::ConfigRequest {
                    session_id,
                    revision,
                    config: cfg.clone(),
                });
                {
                    let session = self.sessions.get_mut(&session_id).unwrap();
                    session.requested_revision = revision;
                    session.requested = cfg;
                }
                let requested = self.sessions[&session_id].requested.clone();
                self.dispatch(
                    command_id,
                    target,
                    CanonicalAction::Reconfigure {
                        revision,
                        target: requested,
                    },
                );
            }
            CanonicalAction::AdmitSession { .. } => unreachable!("handled in route"),
        }
    }

    fn route_resolve(
        &mut self,
        command_id: Uuid,
        target: SessionRef,
        request_id: String,
        resolution: Value,
    ) {
        let session_id = target.session_id;
        let blocked = {
            let session = &self.sessions[&session_id];
            let g = session.generation_at(target.generation).unwrap();
            match g.pending.get(&request_id) {
                None => Some((
                    RejectCode::UnknownRequest,
                    "no pending request with that identity".to_string(),
                )),
                // An answer is accepted once, under current authority and
                // the original request identity — never reused for a
                // different request.
                Some(p) if p.answer != AnswerDisposition::Pending || p.consumed => Some((
                    RejectCode::AlreadyResolved,
                    "request already resolved".to_string(),
                )),
                _ => None,
            }
        };
        if let Some((code, detail)) = blocked {
            return self.reject(command_id, code, detail);
        }
        if self
            .journal_record(JournalRecord::RequestDisposition {
                session_id,
                generation: target.generation,
                request_id: request_id.clone(),
                disposition: AnswerDisposition::Attempted,
                consumed: false,
                opened: None,
            })
            .is_err()
        {
            self.set_disposition(
                command_id,
                CommandDisposition::Uncertain {
                    stage: UncertainStage::DispatchUnknown,
                    detail: "reply intent was not durable; provider was not contacted".into(),
                },
            );
            return;
        }
        {
            let session = self.sessions.get_mut(&session_id).unwrap();
            let g = session.generation_at_mut(target.generation).unwrap();
            g.pending.get_mut(&request_id).unwrap().answer = AnswerDisposition::Attempted;
        }
        self.dispatch(
            command_id,
            target,
            CanonicalAction::ResolveRequest {
                request_id: request_id.clone(),
                resolution,
            },
        );
        // Dispatch can synchronously pump consumption evidence. Never
        // overwrite that evidence with a guessed post-dispatch disposition.
        if self.sessions[&session_id]
            .generation_at(target.generation)
            .and_then(|g| g.pending.get(&request_id))
            .is_some_and(|p| p.consumed)
        {
            return;
        }
        // The answer-delivery disposition follows the dispatch truth — a
        // separate recorded fact from the controller's decision.
        let answer = match self.commands.get(&command_id).map(|c| &c.disposition) {
            Some(CommandDisposition::ProviderAccepted) => AnswerDisposition::Accepted,
            Some(CommandDisposition::Uncertain { .. }) => AnswerDisposition::Uncertain {
                detail: "answer consumption is unproven".into(),
            },
            _ => AnswerDisposition::Rejected {
                detail: "provider refused the answer".into(),
            },
        };
        self.journal_best_effort(JournalRecord::RequestDisposition {
            session_id,
            generation: target.generation,
            request_id: request_id.clone(),
            disposition: answer.clone(),
            consumed: false,
            opened: None,
        });
        let session = self.sessions.get_mut(&session_id).unwrap();
        let g = session.generation_at_mut(target.generation).unwrap();
        g.pending.get_mut(&request_id).unwrap().answer = answer;
    }

    /// Record dispatch intent, then contact the provider. The order is the
    /// whole point of the journal.
    fn record_dispatch_attempt(&mut self, command_id: Uuid) -> bool {
        if self
            .journal_record(JournalRecord::Disposition {
                command_id,
                disposition: CommandDisposition::DispatchAttempted,
            })
            .is_err()
        {
            self.set_disposition(
                command_id,
                CommandDisposition::Uncertain {
                    stage: UncertainStage::DispatchUnknown,
                    detail: "dispatch intent was not durable; provider was not contacted".into(),
                },
            );
            return false;
        }
        if let Some(command) = self.commands.get_mut(&command_id) {
            command.disposition = CommandDisposition::DispatchAttempted;
        }
        true
    }

    fn dispatch(&mut self, command_id: Uuid, target: SessionRef, action: CanonicalAction) {
        if !self.record_dispatch_attempt(command_id) {
            return;
        }
        let outcome = self
            .sessions
            .get_mut(&target.session_id)
            .and_then(|s| s.adapter.as_mut())
            .map(|a| a.dispatch(&action));
        match outcome {
            Some(DispatchOutcome::Accepted) => {
                self.set_disposition(command_id, CommandDisposition::ProviderAccepted);
            }
            Some(DispatchOutcome::Rejected { code, detail }) => {
                self.set_disposition(
                    command_id,
                    CommandDisposition::Rejected {
                        code: RejectCode::ProviderRefused,
                        detail: format!("{code}: {detail}"),
                    },
                );
            }
            Some(DispatchOutcome::Unknown { detail }) => {
                self.set_disposition(
                    command_id,
                    CommandDisposition::Uncertain {
                        stage: UncertainStage::AcceptanceUnknown,
                        detail,
                    },
                );
            }
            None => {
                self.set_disposition(
                    command_id,
                    CommandDisposition::Uncertain {
                        stage: UncertainStage::DispatchUnknown,
                        detail: "no adapter owns this generation".into(),
                    },
                );
            }
        }
        // The provider may have emitted synchronously during dispatch;
        // capture it in order before returning.
        self.pump();
    }

    /// Fence first, terminate second. Once the fence is durable no delayed
    /// prompt, answer, or reconnection can dispatch in this generation —
    /// even if termination cannot be confirmed.
    fn fence_and_terminate(&mut self, session_id: Uuid, fence: FenceKind) {
        let generation = self
            .sessions
            .get(&session_id)
            .map(|s| s.generation())
            .unwrap_or(1);
        let recorded_fence = self
            .sessions
            .get(&session_id)
            .and_then(|s| s.current().fence);
        if recorded_fence.is_none() {
            self.journal_best_effort(JournalRecord::Fence {
                session_id,
                generation,
                fence,
            });
        }
        let held: Vec<Uuid> = {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
            let g = session.current_mut();
            g.fence = Some(recorded_fence.unwrap_or(fence));
            g.hold = false;
            g.held.drain(..).collect()
        };
        for id in held {
            if self
                .commands
                .get(&id)
                .is_some_and(|c| !c.disposition.is_terminal())
            {
                self.set_disposition(
                    id,
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Refused,
                        event_sequence: None,
                    },
                );
            }
        }
        let status = self.sessions[&session_id].status;
        if status != SessionStatus::Live && status != SessionStatus::Stopping {
            return;
        }
        if status == SessionStatus::Live {
            self.set_session_status(session_id, SessionStatus::Stopping);
        }
        let outcome = self
            .sessions
            .get_mut(&session_id)
            .and_then(|s| s.adapter.as_mut())
            .map(|a| a.terminate());
        if outcome == Some(Termination::Confirmed) {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.adapter = None;
            }
            // Capture facts already queued before the observed exit. The
            // adapter must not confirm exit while its intake worker can still
            // produce pre-terminal facts.
            loop {
                let event = self
                    .sessions
                    .get(&session_id)
                    .and_then(|s| s.rx.as_ref())
                    .and_then(|rx| rx.try_recv().ok());
                let Some(event) = event else {
                    break;
                };
                if !self.sessions[&session_id].current().ended {
                    self.ingest(session_id, event);
                }
            }
            if self.sessions[&session_id].current().ended {
                return;
            }
            let reason = match fence {
                FenceKind::Stop | FenceKind::EmergencyStop => TerminateReason::Requested,
                FenceKind::Backpressure => TerminateReason::Backpressure,
                _ => TerminateReason::Interrupted {
                    detail: "provider owner fenced".into(),
                },
            };
            self.ingest(session_id, ProviderEvent::Terminated { reason });
        }
        // Unconfirmed termination is reported honestly: the session stays
        // Stopping — never claimed stopped.
    }

    fn interrupt_session(&mut self, session_id: Uuid, fence: FenceKind) {
        let was_active = matches!(
            self.sessions[&session_id].status,
            SessionStatus::Live | SessionStatus::Stopping
        );
        self.fence_and_terminate(session_id, fence);
        // Whether or not termination was provable, the session's end was an
        // interruption — that is the truthful final status.
        if was_active {
            let status = if self.sessions[&session_id].adapter.is_some() {
                SessionStatus::Stopping
            } else {
                SessionStatus::Interrupted
            };
            self.set_session_status(session_id, status);
        }
    }

    fn set_session_status(&mut self, session_id: Uuid, status: SessionStatus) {
        let generation = {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
            if session.status == status {
                return;
            }
            session.status = status;
            session.generation()
        };
        self.journal_best_effort(JournalRecord::SessionStatus {
            session_id,
            generation,
            status,
        });
    }

    /// Move acknowledged payloads out of the retained replay window. The
    /// tombstone preserves the sequence positions; replay below the new
    /// floor is a blocked recovery, not silent loss.
    fn reclaim(&mut self, session_id: Uuid, generation: u64) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let Some(g) = session.generation_at_mut(generation) else {
            return;
        };
        let acknowledged = {
            let mut reclaimed_any = false;
            while g
                .events
                .front()
                .is_some_and(|e| e.sequence <= g.acknowledged)
            {
                let ev = g.events.pop_front().unwrap();
                let size = serde_json::to_vec(&ev.event)
                    .map(|v| v.len() as u64)
                    .unwrap_or(0);
                g.retained_bytes = g.retained_bytes.saturating_sub(size);
                reclaimed_any = true;
            }
            if reclaimed_any {
                g.floor = g.acknowledged + 1;
                Some(g.acknowledged)
            } else {
                None
            }
        };
        if let Some(through) = acknowledged {
            self.journal_best_effort(JournalRecord::EventTombstone {
                session_id,
                generation,
                through,
            });
        }
    }

    /// Rewrite the journal once it crosses the compact threshold, keeping
    /// exactly the records recovery needs. A failed rewrite leaves the old
    /// journal authoritative.
    fn maybe_compact(&mut self) {
        let threshold =
            (self.budgets.aggregate_journal_bytes as f64 * self.budgets.compact_at_fraction) as u64;
        if self.journal.bytes() < threshold || self.drained {
            return;
        }
        let frames = self.snapshot_frames();
        if self.journal.rewrite(self.now(), frames).is_err() {
            self.persistence_ok = false;
        }
    }

    /// The minimal record set that rebuilds current state. Compaction and
    /// recovery share this truth: live identity, sessions and their
    /// retained events, commands and tombstones, control metadata.
    fn snapshot_frames(&self) -> Vec<Frame> {
        let at = self.now();
        let mut frames = Vec::new();
        let mut push = |record| frames.push(Frame { at_ms: at, record });
        push(JournalRecord::HostStarted {
            instance: self.instance,
            namespace: self.namespace.clone(),
        });
        if let Some((epoch, window, expires)) = self.current_window {
            push(JournalRecord::ControllerAcquired {
                epoch,
                retry_window_id: window,
                expires_at_ms: expires,
            });
        }
        for session in self.sessions.values() {
            push(JournalRecord::Admitted {
                session_id: session.id,
                generation: session.generation(),
                adapter: session.adapter_name.clone(),
                launch: session.launch.clone(),
                requested: session.requested.clone(),
            });
            push(JournalRecord::SessionStatus {
                session_id: session.id,
                generation: session.generation(),
                status: session.status,
            });
            if session.requested_revision > 0 {
                push(JournalRecord::ConfigRequest {
                    session_id: session.id,
                    revision: session.requested_revision,
                    config: session.requested.clone(),
                });
            }
            for g in &session.gens {
                if let Some(fence) = g.fence {
                    push(JournalRecord::Fence {
                        session_id: session.id,
                        generation: g.number,
                        fence,
                    });
                }
                if g.hold {
                    push(JournalRecord::DispatchHeld {
                        session_id: session.id,
                        generation: g.number,
                    });
                }
                if g.floor > 1 {
                    push(JournalRecord::EventTombstone {
                        session_id: session.id,
                        generation: g.number,
                        through: g.floor - 1,
                    });
                }
                for envelope in &g.events {
                    push(JournalRecord::Event {
                        envelope: Box::new(envelope.clone()),
                    });
                }
                if g.acknowledged > 0 {
                    push(JournalRecord::Acknowledged {
                        session_id: session.id,
                        generation: g.number,
                        applied: g.acknowledged,
                    });
                }
                for (request_id, pending) in &g.pending {
                    push(JournalRecord::RequestDisposition {
                        session_id: session.id,
                        generation: g.number,
                        request_id: request_id.clone(),
                        disposition: pending.answer.clone(),
                        consumed: pending.consumed,
                        opened: pending.opened.clone().map(Box::new),
                    });
                }
                for (grant_id, issued) in &g.grants {
                    push(JournalRecord::GrantIssued {
                        session_id: session.id,
                        generation: g.number,
                        grant_id: *grant_id,
                        token: issued.token.clone(),
                        expires_at_ms: issued.expires_at_ms,
                    });
                }
                for record in g.ingress.values() {
                    push(JournalRecord::IngressOpened {
                        record: Box::new(record.clone()),
                    });
                    if record.forwarded {
                        push(JournalRecord::IngressForwarded {
                            request_id: record.request_id,
                        });
                    }
                    if !matches!(
                        record.disposition,
                        IngressDisposition::Pending | IngressDisposition::Forwarded
                    ) {
                        push(JournalRecord::IngressOutcome {
                            request_id: record.request_id,
                            disposition: record.disposition.clone(),
                        });
                    }
                }
                for claim in g.claims.values() {
                    push(JournalRecord::ClaimRegistered {
                        claim: claim.clone(),
                    });
                }
            }
        }
        for (id, cmd) in &self.commands {
            push(JournalRecord::Command {
                command_id: *id,
                session: cmd.session,
                action: cmd.action.clone(),
                retry_window_id: cmd.retry_window_id,
                expires_at_ms: cmd.expires_at_ms,
            });
            push(JournalRecord::Disposition {
                command_id: *id,
                disposition: cmd.disposition.clone(),
            });
        }
        for id in &self.expired {
            push(JournalRecord::CommandExpired { command_id: *id });
        }
        if self.reconciled {
            push(JournalRecord::Reconciled { epoch: self.epoch });
        }
        frames
    }

    /// Sweep ingress deadlines. `Pending` past its deadline expires — it
    /// provably never dispatched. `Forwarded` becomes uncertain: the
    /// daemon may already have observed or completed the work, and the
    /// deadline's passing cannot prove otherwise.
    fn sweep_ingress(&mut self) {
        let now = self.now();
        let mut stale = Vec::new();
        for session in self.sessions.values() {
            for g in &session.gens {
                for record in g.ingress.values() {
                    if matches!(record.disposition, IngressDisposition::Pending)
                        && (session.generation() != g.number
                            || g.fence.is_some()
                            || g.ended
                            || !g
                                .grants
                                .get(&record.grant_id)
                                .is_some_and(|grant| now < grant.expires_at_ms))
                    {
                        stale.push((record.request_id, IngressDisposition::Refused {
                            detail: "original ingress authority expired or was revoked before dispatch".into(),
                        }));
                        continue;
                    }
                    if record.deadline_ms > now {
                        continue;
                    }
                    match record.disposition {
                        IngressDisposition::Pending => {
                            stale.push((record.request_id, IngressDisposition::Expired))
                        }
                        IngressDisposition::Forwarded => stale.push((
                            record.request_id,
                            IngressDisposition::Uncertain {
                                detail: "deadline passed while awaiting the service".into(),
                            },
                        )),
                        _ => {}
                    }
                }
            }
        }
        for (request_id, disposition) in stale {
            self.journal_best_effort(JournalRecord::IngressOutcome {
                request_id,
                disposition: disposition.clone(),
            });
            for session in self.sessions.values_mut() {
                for g in &mut session.gens {
                    if let Some(r) = g.ingress.get_mut(&request_id) {
                        r.disposition = disposition.clone();
                    }
                }
            }
        }
        // Resolved-metadata retention: an expired grant can never verify
        // again, and a terminal record one retry horizon past its deadline
        // can only re-present as a fresh `Expired` record — its original
        // deadline travels with the retry, so it can never re-dispatch.
        // Pending, forwarded and uncertain records are unresolved evidence
        // and are never reclaimed.
        let horizon = self.budgets.retry_window_ms;
        for session in self.sessions.values_mut() {
            for g in &mut session.gens {
                g.grants.retain(|_, grant| now < grant.expires_at_ms);
                g.ingress.retain(|_, record| {
                    !(record.disposition.is_terminal()
                        && now > record.deadline_ms.saturating_add(horizon))
                });
            }
        }
    }

    /// Tombstone commands whose retry window elapsed. Expired identities
    /// keep rejecting; unresolved commands are never swept.
    fn sweep_expired(&mut self) {
        let now = self.now();
        let expired: Vec<Uuid> = self
            .commands
            .iter()
            .filter(|(_, c)| c.disposition.is_terminal() && c.expires_at_ms < now)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.commands.remove(&id);
            self.expired.insert(id);
            self.journal_best_effort(JournalRecord::CommandExpired { command_id: id });
        }
    }

    /// Replay the journal into state. Returns whether a clean drain ended
    /// the previous instance.
    fn fold(&mut self, frames: &[Frame]) -> bool {
        self.replaying = true;
        let mut drained = false;
        // Commands that became held, in journal order, so post-recovery the
        // dispatch queue is identical to what the controller last saw.
        let mut held_order: Vec<Uuid> = Vec::new();
        for frame in frames {
            match &frame.record {
                JournalRecord::HostStarted { .. } => {}
                JournalRecord::ControllerAcquired {
                    epoch,
                    retry_window_id,
                    expires_at_ms,
                } => {
                    // New epochs continue forward; the old lease itself is
                    // dead until someone re-acquires control.
                    self.next_epoch = self.next_epoch.max(epoch + 1);
                    self.windows.insert(*retry_window_id, *expires_at_ms);
                    self.current_window = Some((*epoch, *retry_window_id, *expires_at_ms));
                }
                JournalRecord::Admitted {
                    session_id,
                    generation,
                    adapter,
                    launch,
                    requested,
                } => {
                    self.sessions.insert(
                        *session_id,
                        Session {
                            id: *session_id,
                            adapter_name: adapter.clone(),
                            launch: launch.clone(),
                            requested: requested.clone(),
                            requested_revision: 0,
                            effective: None,
                            status: SessionStatus::Live,
                            adapter: None,
                            rx: None,
                            gens: vec![Generation::new(*generation)],
                        },
                    );
                }
                JournalRecord::SessionStatus {
                    session_id, status, ..
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id) {
                        s.status = *status;
                    }
                }
                JournalRecord::Fence {
                    session_id,
                    generation,
                    fence,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        g.fence = Some(*fence);
                    }
                }
                JournalRecord::DispatchHeld {
                    session_id,
                    generation,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        g.hold = true;
                    }
                }
                JournalRecord::DispatchPermitted {
                    session_id,
                    generation,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        g.hold = false;
                        g.held.clear();
                    }
                }
                JournalRecord::Command {
                    command_id,
                    session,
                    action,
                    retry_window_id,
                    expires_at_ms,
                } => {
                    self.commands.insert(
                        *command_id,
                        CommandState {
                            session: *session,
                            action: action.clone(),
                            retry_window_id: *retry_window_id,
                            expires_at_ms: *expires_at_ms,
                            disposition: CommandDisposition::Recorded,
                        },
                    );
                }
                JournalRecord::Disposition {
                    command_id,
                    disposition,
                } => {
                    if let Some(c) = self.commands.get_mut(command_id) {
                        if *disposition == CommandDisposition::DispatchPending {
                            held_order.push(*command_id);
                        } else {
                            held_order.retain(|id| id != command_id);
                        }
                        c.disposition = disposition.clone();
                    }
                }
                JournalRecord::CommandExpired { command_id } => {
                    self.commands.remove(command_id);
                    self.expired.insert(*command_id);
                    held_order.retain(|id| id != command_id);
                }
                JournalRecord::Event { envelope } => {
                    let envelope = (**envelope).clone();
                    let Some(session) = self.sessions.get_mut(&envelope.session_id) else {
                        continue;
                    };
                    if session.generation_at(envelope.generation).is_none() {
                        session.gens.push(Generation::new(envelope.generation));
                    }
                    {
                        let g = session.generation_at_mut(envelope.generation).unwrap();
                        g.next_seq = g.next_seq.max(envelope.sequence + 1);
                        let size = serde_json::to_vec(&envelope.event)
                            .map(|v| v.len() as u64)
                            .unwrap_or(0);
                        g.retained_bytes += size;
                        g.journal_bytes += size;
                        g.events.push_back(envelope.clone());
                    }
                    self.apply_event(&envelope);
                }
                JournalRecord::EventTombstone {
                    session_id,
                    generation,
                    through,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        g.floor = g.floor.max(through + 1);
                        // A compacted generation may retain no events at
                        // all. Its tombstone still owns those sequence
                        // positions and must preserve the high-water mark.
                        g.next_seq = g.next_seq.max(g.floor);
                        while g.events.front().is_some_and(|e| e.sequence <= *through) {
                            g.events.pop_front();
                        }
                    }
                }
                JournalRecord::RequestDisposition {
                    session_id,
                    generation,
                    request_id,
                    disposition,
                    consumed,
                    opened,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        // Compaction re-emits request state wholesale, so
                        // a request the daemon already acknowledged
                        // still resolves under its original identity.
                        let p = g
                            .pending
                            .entry(request_id.clone())
                            .or_insert(PendingRequest {
                                answer: AnswerDisposition::Pending,
                                consumed: false,
                                opened: None,
                            });
                        g.retained_bytes = g.retained_bytes.saturating_sub(p.retained_bytes());
                        p.answer = disposition.clone();
                        p.consumed = *consumed;
                        if let Some(opened) = opened {
                            p.opened = Some((**opened).clone());
                        }
                        if p.consumed {
                            p.opened = None;
                        }
                        g.retained_bytes += p.retained_bytes();
                    }
                }
                JournalRecord::ConfigRequest {
                    session_id,
                    revision,
                    config,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id) {
                        s.requested_revision = s.requested_revision.max(*revision);
                        s.requested = config.clone();
                    }
                }
                JournalRecord::Acknowledged {
                    session_id,
                    generation,
                    applied,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        g.acknowledged = g.acknowledged.max(*applied);
                    }
                }
                JournalRecord::GrantIssued {
                    session_id,
                    generation,
                    grant_id,
                    token,
                    expires_at_ms,
                } => {
                    if let Some(s) = self.sessions.get_mut(session_id)
                        && let Some(g) = s.generation_at_mut(*generation)
                    {
                        g.grants.insert(
                            *grant_id,
                            IssuedGrant {
                                token: token.clone(),
                                expires_at_ms: *expires_at_ms,
                            },
                        );
                    }
                }
                JournalRecord::IngressOpened { record } => {
                    if let Some(s) = self.sessions.get_mut(&record.session.session_id)
                        && let Some(g) = s.generation_at_mut(record.session.generation)
                    {
                        g.ingress.insert(record.request_id, (**record).clone());
                    }
                }
                JournalRecord::IngressForwarded { request_id } => {
                    for s in self.sessions.values_mut() {
                        for g in &mut s.gens {
                            if let Some(r) = g.ingress.get_mut(request_id) {
                                r.forwarded = true;
                                if r.disposition == IngressDisposition::Pending {
                                    r.disposition = IngressDisposition::Forwarded;
                                }
                            }
                        }
                    }
                }
                JournalRecord::IngressOutcome {
                    request_id,
                    disposition,
                } => {
                    for s in self.sessions.values_mut() {
                        for g in &mut s.gens {
                            if let Some(r) = g.ingress.get_mut(request_id) {
                                r.disposition = disposition.clone();
                            }
                        }
                    }
                }
                JournalRecord::ClaimRegistered { claim } => {
                    if let Some(s) = self.sessions.get_mut(&claim.session.session_id)
                        && let Some(g) = s.generation_at_mut(claim.session.generation)
                    {
                        g.claims.insert(claim.claim_id, claim.clone());
                    }
                }
                JournalRecord::ClaimReleased { claim_id, .. } => {
                    for s in self.sessions.values_mut() {
                        for g in &mut s.gens {
                            g.claims.remove(claim_id);
                        }
                    }
                }
                JournalRecord::Reconciled { .. } => {
                    self.reconciled = true;
                }
                JournalRecord::Drained => {
                    drained = true;
                }
            }
        }
        // Rebuild the held queue and in-flight cancellations in journal
        // order so recovery sees the exact pre-crash dispatch position.
        for command_id in held_order {
            if let Some(cmd) = self.commands.get(&command_id)
                && cmd.disposition == CommandDisposition::DispatchPending
                && let Some(target) = cmd.session
                && let Some(s) = self.sessions.get_mut(&target.session_id)
                && let Some(g) = s.generation_at_mut(target.generation)
            {
                g.held.push_back(command_id);
            }
        }
        for cmd in self.commands.values() {
            if let (
                Some(target),
                CanonicalAction::CancelTurn { turn_id },
                CommandDisposition::ProviderAccepted | CommandDisposition::Uncertain { .. },
            ) = (cmd.session, &cmd.action, &cmd.disposition)
                && let Some(s) = self.sessions.get_mut(&target.session_id)
                && let Some(g) = s.generation_at_mut(target.generation)
                && g.turns.get(turn_id) == Some(&TurnState::Open)
            {
                g.turns.insert(*turn_id, TurnState::Cancelling);
            }
        }
        self.replaying = false;
        drained
    }

    /// Crash recovery: surviving sessions keep their history but lose
    /// their owner. They are fenced — never respawned, never replayed —
    /// and their unfinished commands become honestly uncertain.
    fn try_fence_recovered(&mut self) {
        let ids: Vec<Uuid> = self.unfenced_survivors.iter().copied().collect();
        for id in ids {
            let session = &self.sessions[&id];
            let owner = crate::adapter::RecoveredOwner {
                namespace: self.namespace.clone(),
                session: SessionRef {
                    session_id: id,
                    generation: session.generation(),
                },
                adapter: session.adapter_name.clone(),
                launch: session.launch.clone(),
                effective: session.effective.clone(),
            };
            if self.factory.fence_recovered(&owner) == Termination::Confirmed {
                self.set_session_status(id, SessionStatus::Interrupted);
                self.unfenced_survivors.remove(&id);
            }
        }
    }

    fn fence_survivors(&mut self) -> usize {
        let mut interrupted = 0;
        let ids: Vec<Uuid> = self.sessions.keys().copied().collect();
        for id in ids {
            let status = self.sessions[&id].status;
            if matches!(status, SessionStatus::Live | SessionStatus::Stopping) {
                interrupted += 1;
                let generation = self.sessions[&id].generation();
                self.journal_best_effort(JournalRecord::Fence {
                    session_id: id,
                    generation,
                    fence: FenceKind::Interrupted,
                });
                {
                    let session = self.sessions.get_mut(&id).unwrap();
                    session.current_mut().fence = Some(FenceKind::Interrupted);
                    session.status = SessionStatus::Stopping;
                }
                self.journal_best_effort(JournalRecord::SessionStatus {
                    session_id: id,
                    generation,
                    status: SessionStatus::Stopping,
                });
                self.unfenced_survivors.insert(id);
            }
        }
        // Forwarded-but-unresolved agent requests cannot prove their
        // outcome across an owner loss — uncertain, never replayed.
        // Unforwarded ones stay pending: a new service still owes them.
        let mut uncertain = Vec::new();
        for session in self.sessions.values_mut() {
            for g in &mut session.gens {
                for record in g.ingress.values_mut() {
                    if record.disposition == IngressDisposition::Forwarded {
                        record.disposition = IngressDisposition::Uncertain {
                            detail: "runtime restarted while the request was with the service"
                                .into(),
                        };
                        uncertain.push(record.request_id);
                    }
                }
            }
        }
        for request_id in uncertain {
            self.journal_best_effort(JournalRecord::IngressOutcome {
                request_id,
                disposition: IngressDisposition::Uncertain {
                    detail: "runtime restarted while the request was with the service".into(),
                },
            });
        }
        // Commands whose evidence chain broke with the instance.
        let open: Vec<Uuid> = self
            .commands
            .iter()
            .filter(|(_, c)| !c.disposition.is_terminal())
            .map(|(id, _)| *id)
            .collect();
        for id in open {
            let disposition = match self.commands[&id].disposition {
                CommandDisposition::DispatchAttempted => CommandDisposition::Uncertain {
                    stage: UncertainStage::DispatchUnknown,
                    detail: "runtime restarted mid-dispatch".into(),
                },
                CommandDisposition::ProviderAccepted => CommandDisposition::Uncertain {
                    stage: UncertainStage::OutcomeUnknown,
                    detail: "provider accepted; runtime lost before the outcome".into(),
                },
                _ => CommandDisposition::Rejected {
                    code: RejectCode::Interrupted,
                    detail: "runtime restarted before dispatch".into(),
                },
            };
            self.set_disposition(id, disposition);
        }
        self.try_fence_recovered();
        interrupted
    }
}

fn attribution(event: &ProviderEvent) -> (Option<Uuid>, Option<Uuid>, Option<String>, Option<u64>) {
    match event {
        ProviderEvent::TurnStarted {
            turn_id,
            delivery_id,
        } => (Some(*delivery_id), Some(*turn_id), None, None),
        ProviderEvent::TurnDelta { turn_id, .. }
        | ProviderEvent::TurnCompleted { turn_id, .. }
        | ProviderEvent::TurnCancelled { turn_id } => (None, Some(*turn_id), None, None),
        ProviderEvent::RequestOpened { request_id, .. }
        | ProviderEvent::RequestAnswered { request_id } => {
            (None, None, Some(request_id.clone()), None)
        }
        ProviderEvent::ConfigActivated { revision, .. } => (None, None, None, Some(*revision)),
        _ => (None, None, None, None),
    }
}

/// Read the namespace's stored credentials. This is how the daemon's
/// controller and the emergency route obtain authority: same-user file
/// access is the credential distribution, and `identity.json` is
/// owner-only beside the journal.
pub fn load_credentials(root: &Path) -> Result<Credentials, HostError> {
    let path = root.join("identity.json");
    let raw = fs::read(&path).map_err(io_err)?;
    let id: IdentityFile =
        serde_json::from_slice(&raw).map_err(|e| HostError::Malformed(e.to_string()))?;
    Ok(Credentials {
        controller: id.controller,
        emergency: id.emergency,
    })
}

fn load_or_mint_identity(root: &Path, namespace: &str) -> Result<Credentials, HostError> {
    let path = root.join("identity.json");
    if path.exists() {
        let raw = fs::read(&path).map_err(io_err)?;
        let id: IdentityFile =
            serde_json::from_slice(&raw).map_err(|e| HostError::Malformed(e.to_string()))?;
        if id.namespace != namespace {
            return Err(HostError::Unauthorized);
        }
        return Ok(Credentials {
            controller: id.controller,
            emergency: id.emergency,
        });
    }
    let id = IdentityFile {
        namespace: namespace.to_string(),
        controller: mint_credential(),
        emergency: mint_credential(),
    };
    let raw = serde_json::to_vec_pretty(&id).map_err(|e| HostError::Malformed(e.to_string()))?;
    fs::write(&path, &raw).map_err(io_err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(Credentials {
        controller: id.controller,
        emergency: id.emergency,
    })
}

fn io_err(e: std::io::Error) -> HostError {
    HostError::Io(e.to_string())
}

#[cfg(test)]
#[path = "host_qualification_tests.rs"]
mod qualification_tests;
