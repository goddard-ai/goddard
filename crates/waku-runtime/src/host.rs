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
    /// Retained (not yet acknowledged) event bytes per session.
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
    /// Afterwards their tombstones still reject.
    pub retry_window_ms: u64,
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
                .filter(|(_, p)| p.answer == AnswerDisposition::Pending)
                .map(|(id, _)| id.clone())
                .collect(),
            held_commands: g.held.len(),
            fence: g.fence,
            effective: self.effective.clone(),
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
        fs::create_dir_all(root).map_err(io_err)?;
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

        host.journal_best_effort(JournalRecord::HostStarted {
            instance: host.instance,
            namespace: host.namespace.clone(),
        });

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

    pub fn instance(&self) -> Uuid {
        self.instance
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn recovery(&self) -> RecoveryState {
        self.recovery
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
        Ok(Hello {
            protocol: PROTOCOL_VERSION,
            namespace: self.namespace.clone(),
            runtime_instance: self.instance,
            recovery: self.recovery,
            capabilities: vec!["emergency-stop".into(), "reconfigure".into()],
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
        self.epoch = self.next_epoch;
        self.next_epoch += 1;
        let window = Uuid::new_v4();
        let expires = self.now() + self.budgets.retry_window_ms;
        self.windows.insert(window, expires);
        self.current_window = Some((self.epoch, window, expires));
        let grant = ControlGrant {
            epoch: self.epoch,
            retry_window_id: window,
            retry_window_expires_at_ms: expires,
        };
        self.journal_best_effort(JournalRecord::ControllerAcquired {
            epoch: self.epoch,
            retry_window_id: window,
            expires_at_ms: expires,
        });
        Ok(grant)
    }

    /// Inventory truth the controller must consume after detachment or
    /// crash recovery before new admissions are permitted.
    pub fn reconcile(&mut self, credential: &str, epoch: u64) -> Result<Inventory, HostError> {
        self.check_controller(credential)?;
        self.check_epoch(epoch)?;
        self.pump();
        self.reconciled = true;
        self.needs_reconcile = false;
        self.journal_best_effort(JournalRecord::Reconciled { epoch });
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

        if self.journal.bytes() + self.budgets.control_reserve_bytes
            >= self.budgets.aggregate_journal_bytes
        {
            return Err(HostError::Capacity("journal budget exhausted"));
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
            g.acknowledged = cursor.applied;
            self.journal_best_effort(JournalRecord::Acknowledged {
                session_id: cursor.session_id,
                generation: cursor.generation,
                applied: cursor.applied,
            });
            self.reclaim(cursor.session_id, cursor.generation);
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
        self.drained = true;
        self.journal_best_effort(JournalRecord::Drained);
        Ok(DrainReport::Drained { sessions: count })
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
                    Some(Ok(event)) => Some(event),
                    Some(Err(mpsc::TryRecvError::Disconnected))
                        if !session.adapter.as_ref().is_some_and(|a| a.alive()) =>
                    {
                        // The adapter's sender dropped without a Terminated
                        // event — the transport is gone.
                        session.adapter = None;
                        session.rx = None;
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
            let event = if size > self.budgets.max_message_bytes {
                ProviderEvent::Dropped {
                    reason: DropReason::Oversize,
                    bytes: size,
                }
            } else {
                event
            };
            let (delivery_id, turn_id, request_id, config_revision) = attribution(&event);
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
        self.apply_event(
            session_id,
            envelope.generation,
            envelope.sequence,
            &envelope.event,
        );
        // Bounded emergency buffering is exhausted: the provider cannot
        // keep emitting into a store that is not keeping up.
        if !self.persistence_ok && self.pending_data_records() >= self.budgets.emergency_records {
            self.interrupt_session(session_id, FenceKind::Backpressure);
        }
    }

    /// Derived state from a journaled event — used by live ingest and by
    /// recovery fold so both paths build identical truth.
    fn apply_event(
        &mut self,
        session_id: Uuid,
        generation: u64,
        sequence: u64,
        event: &ProviderEvent,
    ) {
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
                    g.pending.insert(
                        request_id.clone(),
                        PendingRequest {
                            answer: AnswerDisposition::Pending,
                            consumed: false,
                        },
                    );
                }
                ProviderEvent::RequestAnswered { request_id } => {
                    if let Some(pending) = g.pending.get_mut(request_id) {
                        pending.consumed = true;
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
                && matches!(p.answer, AnswerDisposition::Uncertain { .. })
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
        self.journal_best_effort(JournalRecord::Admitted {
            session_id,
            generation: 1,
            adapter: adapter.clone(),
            launch: launch.clone(),
            requested: requested.clone(),
        });
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
                self.set_disposition(command_id, CommandDisposition::DispatchAttempted);
                self.journal_best_effort(JournalRecord::DispatchHeld {
                    session_id,
                    generation: target.generation,
                });
                {
                    let session = self.sessions.get_mut(&session_id).unwrap();
                    let g = session.current_mut();
                    g.hold = true;
                    g.turns.insert(turn_id, TurnState::Cancelling);
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
        self.journal_best_effort(JournalRecord::RequestDisposition {
            session_id,
            generation: target.generation,
            request_id: request_id.clone(),
            disposition: AnswerDisposition::Attempted,
            consumed: false,
        });
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
        });
        let session = self.sessions.get_mut(&session_id).unwrap();
        let g = session.generation_at_mut(target.generation).unwrap();
        g.pending.get_mut(&request_id).unwrap().answer = answer;
    }

    /// Record dispatch intent, then contact the provider. The order is the
    /// whole point of the journal.
    fn dispatch(&mut self, command_id: Uuid, target: SessionRef, action: CanonicalAction) {
        self.set_disposition(command_id, CommandDisposition::DispatchAttempted);
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
        self.journal_best_effort(JournalRecord::Fence {
            session_id,
            generation,
            fence,
        });
        let held: Vec<Uuid> = {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
            let g = session.current_mut();
            g.fence = Some(fence);
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
            self.set_session_status(session_id, SessionStatus::Terminated);
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
            self.set_session_status(session_id, SessionStatus::Interrupted);
        }
    }

    fn set_session_status(&mut self, session_id: Uuid, status: SessionStatus) {
        let generation = {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
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
                    self.apply_event(
                        envelope.session_id,
                        envelope.generation,
                        envelope.sequence,
                        &envelope.event,
                    );
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
                            });
                        p.answer = disposition.clone();
                        p.consumed = *consumed;
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
                    session.status = SessionStatus::Interrupted;
                }
                self.journal_best_effort(JournalRecord::SessionStatus {
                    session_id: id,
                    generation,
                    status: SessionStatus::Interrupted,
                });
            }
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
