//! Durable capacity authority for a future qualified replacement adapter.
//!
//! Every launch in a provider/model pool must claim this broker before it can
//! qualify. No production adapter supplies one. Expiry fences publication; it
//! never frees a possibly running process. Dropping a pin releases only the
//! transaction lock, not the durable reservation.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::model::ProviderKind;

use crate::{
    guarded_lifecycle::{ReplacementCapacity, ReplacementPlacement, validate_replacement_capacity},
    provider_budget::StartupBudgetBinding,
    rotation_continuity::ContinuityLedger,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReplacementLease {
    pub id: Uuid,
    pub broker: Uuid,
    pub predecessor: Uuid,
    pub successor: Uuid,
    pub startup: StartupBudgetBinding,
    pub expires_at: u64,
    pub state: LeaseState,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum LeaseState {
    Requested,
    Held,
    Activated,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Slot {
    session: Uuid,
    runtime: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pool {
    id: Uuid,
    provider: ProviderKind,
    model: String,
    hard_cap: u64,
    slots: Vec<Slot>,
    leases: Vec<ReplacementLease>,
}

/// Private store selected by a trusted adapter, never a model-supplied path.
/// All admissions sharing the provider/model cap must use this same pool.
#[derive(Clone, Debug)]
pub struct ReplacementBroker {
    path: PathBuf,
    id: Uuid,
}

/// Pins the pool lock through ownership/grant publication. While pinned, expiry
/// and concurrent admission cannot revoke the slot. Crash drops the lock but
/// leaves the held record for restart reconciliation. Never acquire task/Boss
/// operation locks after acquiring this pin.
pub struct ReplacementPin {
    _lock: File,
    broker: ReplacementBroker,
    pool: Pool,
    lease: ReplacementLease,
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

#[cfg(test)]
thread_local! {
    static FAIL_PERSIST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn persist(path: &Path, pool: &Pool) -> anyhow::Result<()> {
    #[cfg(test)]
    anyhow::ensure!(
        !FAIL_PERSIST.with(|fail| fail.replace(false)),
        "injected broker persistence failure"
    );

    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let mut file = private_options().create_new(true).open(&temporary)?;
        file.write_all(&serde_json::to_vec(pool)?)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        File::open(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("broker parent missing"))?,
        )?
        .sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl ReplacementBroker {
    /// Provision a new pool explicitly. Existing pools must be reopened with
    /// their original identity; missing/corrupt stores are never recreated.
    pub fn create(
        path: &Path,
        provider: ProviderKind,
        model: String,
        hard_cap: u64,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            hard_cap > 0 && !model.is_empty(),
            "invalid replacement pool"
        );
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("broker parent missing"))?,
        )?;
        let id = Uuid::new_v4();
        let broker = Self {
            path: path.to_owned(),
            id,
        };
        let _lock = private_options()
            .create_new(true)
            .open(path.with_extension("lock"))?;
        _lock.lock()?;
        anyhow::ensure!(!path.exists(), "replacement pool already exists");
        persist(
            path,
            &Pool {
                id,
                provider,
                model,
                hard_cap,
                slots: vec![],
                leases: vec![],
            },
        )?;
        Ok(broker)
    }

    pub fn reopen(path: &Path, id: Uuid) -> anyhow::Result<Self> {
        let broker = Self {
            path: path.to_owned(),
            id,
        };
        broker.locked()?;
        Ok(broker)
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    fn locked(&self) -> anyhow::Result<(File, Pool)> {
        // An absent lock/store is loss of authority, not a fresh pool.
        let lock = private_options().open(self.path.with_extension("lock"))?;
        lock.try_lock()?;
        let pool: Pool = serde_json::from_slice(&fs::read(&self.path)?)?;
        anyhow::ensure!(
            pool.id == self.id && !self.id.is_nil(),
            "foreign replacement broker"
        );
        Ok((lock, pool))
    }

    /// Shared ordinary-launch admission. Call before starting any provider;
    /// release only after independently confirmed termination.
    pub fn claim_slot(&self, session: Uuid, runtime: Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            !session.is_nil() && !runtime.is_nil(),
            "invalid capacity owner"
        );
        let (_lock, mut pool) = self.locked()?;
        if let Some(slot) = pool.slots.iter().find(|s| s.session == session) {
            anyhow::ensure!(slot.runtime == runtime, "capacity runtime changed");
            return Ok(());
        }
        anyhow::ensure!(
            pool.slots.len() < usize::try_from(pool.hard_cap)?,
            "replacement hard cap exhausted"
        );
        pool.slots.push(Slot { session, runtime });
        persist(&self.path, &pool)
    }

    pub fn release_stopped_slot(&self, session: Uuid, runtime: Uuid) -> anyhow::Result<()> {
        let (_lock, mut pool) = self.locked()?;
        anyhow::ensure!(
            !pool
                .leases
                .iter()
                .any(|l| l.successor == session && l.state == LeaseState::Held),
            "replacement requires durable terminal reconciliation"
        );
        if let Some(slot) = pool.slots.iter().find(|s| s.session == session) {
            anyhow::ensure!(slot.runtime == runtime, "capacity runtime changed");
        }
        pool.slots.retain(|s| s.session != session);
        persist(&self.path, &pool)
    }

    /// Trusted lifecycle only: call after independently confirming that this
    /// exact successor runtime has stopped (or never launched). An expiry or a
    /// provider error is not termination evidence. Persist failure before release.
    pub fn failed_after_confirmed_stop(
        &self,
        ledger_path: &Path,
        id: Uuid,
        successor: Uuid,
        runtime: Uuid,
    ) -> anyhow::Result<()> {
        let ledger = ContinuityLedger::load(ledger_path)?;
        let lease = ledger
            .replacement_leases
            .iter()
            .find(|l| l.id == id)
            .ok_or_else(|| anyhow::anyhow!("lease intent missing"))?;
        anyhow::ensure!(
            lease.broker == self.id
                && lease.successor == successor
                && lease.startup.runtime_id == runtime,
            "foreign termination confirmation"
        );
        let (_lock, mut pool) = self.locked()?;
        let mut terminal = lease.clone();
        terminal.state = LeaseState::Failed;
        apply_terminal(&mut pool, &terminal)?;
        record_terminal(ledger_path, id, LeaseState::Failed)?;
        persist(&self.path, &pool)
    }

    /// Complete cleanup after a crash between the durable ownership commit and
    /// lease settlement. A pending intent or a pointer alone is insufficient.
    /// The daemon also checks its authoritative Boss pointer before calling.
    pub fn reconcile_committed(
        &self,
        ledger_path: &Path,
        journal: &crate::boss_rotation::RotationJournal,
    ) -> anyhow::Result<()> {
        let ledger = ContinuityLedger::load(ledger_path)?;
        for lease in &ledger.replacement_leases {
            if lease.broker != self.id {
                continue;
            }
            if matches!(lease.state, LeaseState::Activated | LeaseState::Failed) {
                self.settle(ledger_path, lease.id, lease.state)?;
                continue;
            }
            let committed = journal.intent.is_none()
                && journal.active_session_id == Some(lease.successor)
                && journal.generation == lease.startup.generation
                && journal.rotations.iter().any(|r| {
                    r.old_session_id == lease.predecessor
                        && r.new_session_id == lease.successor
                        && r.old_generation.checked_add(1) == Some(lease.startup.generation)
                        && r.committed_at.is_some()
                        && matches!(
                            r.diagnostics.terminal,
                            Some(crate::boss_rotation::RotationTerminal::Activated { .. })
                        )
                });
            if committed {
                record_terminal(ledger_path, lease.id, LeaseState::Activated)?;
                self.settle(ledger_path, lease.id, LeaseState::Activated)?;
            }
        }
        Ok(())
    }

    fn pin(
        &self,
        request: &ReplacementLease,
        now: u64,
        acquire: bool,
    ) -> anyhow::Result<ReplacementPin> {
        let (lock, mut pool) = self.locked()?;
        anyhow::ensure!(
            request.broker == pool.id && request.expires_at > now,
            "replacement lease lost or expired"
        );
        anyhow::ensure!(
            pool.provider == request.startup.provider && pool.model == request.startup.model,
            "replacement pool changed"
        );
        anyhow::ensure!(
            request.state == LeaseState::Requested || request.state == LeaseState::Held,
            "terminal lease cannot publish"
        );
        if let Some(lease) = pool.leases.iter().find(|l| l.id == request.id) {
            let mut saved = request.clone();
            saved.state = LeaseState::Held;
            anyhow::ensure!(*lease == saved, "stale or foreign replacement lease");
        } else {
            anyhow::ensure!(
                acquire && request.state == LeaseState::Requested,
                "replacement lease lost"
            );
            anyhow::ensure!(
                !pool.leases.iter().any(|l| l.successor == request.successor),
                "successor already reserved"
            );
            anyhow::ensure!(
                !pool.slots.iter().any(|s| s.session == request.successor),
                "successor slot lacks lease"
            );
            anyhow::ensure!(
                pool.slots.len() < usize::try_from(pool.hard_cap)?,
                "replacement hard cap exhausted"
            );
            pool.slots.push(Slot {
                session: request.successor,
                runtime: request.startup.runtime_id,
            });
            let mut lease = request.clone();
            lease.state = LeaseState::Held;
            pool.leases.push(lease);
            persist(&self.path, &pool)?;
        }
        anyhow::ensure!(
            pool.slots
                .iter()
                .any(|s| s.session == request.successor && s.runtime == request.startup.runtime_id),
            "replacement slot lost"
        );
        let pin = ReplacementPin {
            _lock: lock,
            broker: self.clone(),
            pool,
            lease: request.clone(),
        };
        pin.validate(&request.startup)?;
        Ok(pin)
    }

    /// Reconcile a durable terminal verdict. Activation converts the reservation
    /// into an ordinary live slot; failure releases it only after confirmed stop.
    /// Expired leases can settle, but cannot authorize publication.
    pub fn settle(&self, ledger_path: &Path, id: Uuid, terminal: LeaseState) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(terminal, LeaseState::Activated | LeaseState::Failed),
            "nonterminal lease settlement"
        );
        let ledger = ContinuityLedger::load(ledger_path)?;
        let request = ledger
            .replacement_leases
            .iter()
            .find(|l| l.id == id)
            .ok_or_else(|| anyhow::anyhow!("lease intent missing"))?;
        anyhow::ensure!(
            request.state == terminal && request.broker == self.id,
            "durable terminal verdict required"
        );
        let (_lock, mut pool) = self.locked()?;
        apply_terminal(&mut pool, request)?;
        persist(&self.path, &pool)
    }
}

impl ReplacementPin {
    pub fn id(&self) -> Uuid {
        self.lease.id
    }

    /// A lease is not a budget admission. Each guard still needs fresh arithmetic
    /// and an independently observed operation ID. Bind all stable launch fields.
    pub fn validate(&self, expected: &StartupBudgetBinding) -> anyhow::Result<()> {
        let mut stable = expected.clone();
        stable.operation_id = self.lease.startup.operation_id;
        anyhow::ensure!(
            stable == self.lease.startup,
            "replacement startup binding changed"
        );
        let predecessor_held = self
            .pool
            .slots
            .iter()
            .any(|s| s.session == self.lease.predecessor);
        validate_replacement_capacity(
            self.lease.predecessor,
            self.lease.successor,
            expected,
            Some(&ReplacementCapacity {
                predecessor: self.lease.predecessor,
                successor: self.lease.successor,
                startup: expected.clone(),
                reservation: self.lease.id.to_string(),
                hard_cap: self.pool.hard_cap,
                occupied: self.pool.slots.len().try_into()?,
                placement: if predecessor_held {
                    ReplacementPlacement::Overlap
                } else {
                    ReplacementPlacement::PredecessorReleased
                },
            }),
        )
    }

    /// Call only after the ownership/grant commit is durable. The slot remains
    /// occupied for the active successor. If ledger persistence fails, dropping
    /// the pin retains the reservation for retry.
    pub fn activated(mut self, ledger_path: &Path) -> anyhow::Result<()> {
        self.lease.state = LeaseState::Activated;
        apply_terminal(&mut self.pool, &self.lease)?;
        record_terminal(ledger_path, self.lease.id, LeaseState::Activated)?;
        persist(&self.broker.path, &self.pool)
    }
}

fn apply_terminal(pool: &mut Pool, request: &ReplacementLease) -> anyhow::Result<()> {
    let lease = pool
        .leases
        .iter_mut()
        .find(|l| l.id == request.id)
        .ok_or_else(|| anyhow::anyhow!("replacement lease lost"))?;
    let mut bound = request.clone();
    bound.state = lease.state;
    anyhow::ensure!(
        *lease == bound && (lease.state == LeaseState::Held || lease.state == request.state),
        "foreign or conflicting terminal lease"
    );
    lease.state = request.state;
    if request.state == LeaseState::Failed {
        pool.slots.retain(|s| s.session != request.successor);
    }
    Ok(())
}

fn record_terminal(path: &Path, id: Uuid, terminal: LeaseState) -> anyhow::Result<()> {
    ContinuityLedger::update(path, |ledger| -> anyhow::Result<()> {
        let lease = ledger
            .replacement_leases
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| anyhow::anyhow!("lease intent missing"))?;
        anyhow::ensure!(
            lease.state == LeaseState::Held || lease.state == terminal,
            "conflicting lease terminal"
        );
        lease.state = terminal;
        Ok(())
    })??;
    Ok(())
}

/// Persist the acquisition identity before touching the broker. Duplicate triggers
/// and restart query the same lease; they cannot choose a fresh ID or deadline.
/// Call under the coordinator operation lock, before submission/publication.
pub fn hold_replacement(
    ledger_path: &Path,
    broker: &ReplacementBroker,
    expected: &StartupBudgetBinding,
    now: u64,
) -> anyhow::Result<ReplacementPin> {
    let started = std::time::Instant::now();
    let request =
        ContinuityLedger::update(ledger_path, |ledger| -> anyhow::Result<ReplacementLease> {
            let frozen = ledger
                .frozen_coordinator
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("replacement not frozen"))?;
            anyhow::ensure!(
                expected.generation
                    == frozen
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("generation exhausted"))?,
                "replacement generation changed"
            );
            anyhow::ensure!(
                frozen
                    .package
                    .as_ref()
                    .is_some_and(|p| p.binding.session == frozen.successor
                        && p.exact_content().and_then(|p| p.digest()).ok().as_deref()
                            == Some(expected.package_digest.as_str())),
                "replacement package changed"
            );
            if let Some(lease) = ledger
                .replacement_leases
                .iter()
                .find(|l| l.successor == frozen.successor)
            {
                anyhow::ensure!(
                    lease.broker == broker.id && lease.predecessor == frozen.predecessor,
                    "replacement broker changed"
                );
                return Ok(lease.clone());
            }
            anyhow::ensure!(
                !expected.runtime_id.is_nil() && !expected.operation_id.is_nil(),
                "replacement runtime unavailable"
            );
            let lease = ReplacementLease {
                id: Uuid::new_v4(),
                broker: broker.id,
                predecessor: frozen.predecessor,
                successor: frozen.successor,
                startup: expected.clone(),
                expires_at: now
                    .checked_add(120)
                    .ok_or_else(|| anyhow::anyhow!("lease deadline overflow"))?,
                state: LeaseState::Requested,
            };
            ledger.replacement_leases.push(lease.clone());
            Ok(lease)
        })??;
    // Check current launch identity before acquisition, even for a retry.
    let mut stable = expected.clone();
    stable.operation_id = request.startup.operation_id;
    anyhow::ensure!(
        stable == request.startup,
        "replacement startup binding changed"
    );
    let checked_at = now
        .checked_add(started.elapsed().as_secs())
        .ok_or_else(|| anyhow::anyhow!("lease clock overflow"))?;
    let pin = broker.pin(&request, checked_at, request.state == LeaseState::Requested)?;
    ContinuityLedger::update(ledger_path, |ledger| -> anyhow::Result<()> {
        let lease = ledger
            .replacement_leases
            .iter_mut()
            .find(|l| l.id == request.id)
            .ok_or_else(|| anyhow::anyhow!("lease intent missing"))?;
        anyhow::ensure!(*lease == request, "replacement intent changed");
        lease.state = LeaseState::Held;
        Ok(())
    })??;
    Ok(pin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        boss_rotation::RotationJournal,
        coordinator_contract::{CoordinatorBinding, CoordinatorPackage},
        rotation_continuity::{ContinuityPackage, ContinuityScope},
    };

    struct Rig {
        root: PathBuf,
        ledger: PathBuf,
        broker: ReplacementBroker,
        expected: StartupBudgetBinding,
        old: Uuid,
        new: Uuid,
    }
    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    impl Rig {
        fn new(cap: u64) -> Self {
            let root = std::env::temp_dir().join(format!("replacement-lease-{}", Uuid::new_v4()));
            let ledger = root.join("continuity.json");
            let broker = ReplacementBroker::create(
                &root.join("pool.json"),
                ProviderKind::Codex,
                "fixture".into(),
                cap,
            )
            .unwrap();
            let old = Uuid::new_v4();
            let new = Uuid::new_v4();
            let package = CoordinatorPackage {
                binding: CoordinatorBinding {
                    provider: ProviderKind::Codex,
                    session: new,
                    generation: 1,
                    identity_revision: "identity".into(),
                    grants_revision: "grants".into(),
                    model: "fixture".into(),
                    scope: ContinuityScope::Coordinator,
                },
                content: ContinuityPackage {
                    identity_revision: "identity".into(),
                    scope: ContinuityScope::Coordinator,
                    instructions: "canonical fixture".into(),
                    context: "durable work".into(),
                },
                event_ids: vec![],
                work_ids: vec![],
                effect_ids: vec![],
            };
            let expected = StartupBudgetBinding {
                provider: ProviderKind::Codex,
                runtime_version: "test-only".into(),
                model: "fixture".into(),
                configuration_revision: "config".into(),
                instruction_revision: "identity".into(),
                runtime_id: Uuid::new_v4(),
                generation: 1,
                package_digest: package.exact_content().unwrap().digest().unwrap(),
                operation_id: Uuid::new_v4(),
            };
            ContinuityLedger::update(&ledger, |l| {
                l.freeze_coordinator(old, new, 0).unwrap();
                l.stage_successor("successor", &package, 0).unwrap();
            })
            .unwrap();
            Self {
                root,
                ledger,
                broker,
                expected,
                old,
                new,
            }
        }
        fn hold(&self, now: u64) -> anyhow::Result<ReplacementPin> {
            hold_replacement(&self.ledger, &self.broker, &self.expected, now)
        }
        fn request(&self) -> ReplacementLease {
            ContinuityLedger::load(&self.ledger)
                .unwrap()
                .replacement_leases[0]
                .clone()
        }
        fn pool(&self) -> Pool {
            self.broker.locked().unwrap().1
        }
        fn journal(&self) -> RotationJournal {
            let mut j = RotationJournal {
                active_session_id: Some(self.old),
                ..Default::default()
            };
            j.begin(Uuid::new_v4(), self.old, self.new, 1).unwrap();
            j
        }
    }

    #[test]
    fn replacement_lease_restarts_at_both_acquisition_boundaries_without_double_reserving() {
        let rig = Rig::new(2);
        rig.broker.claim_slot(rig.old, Uuid::new_v4()).unwrap();
        let pin = rig.hold(1).unwrap();
        let id = pin.id();
        drop(pin);
        // Simulate death after broker acquisition but before the ledger's Held
        // write. The original acquisition identity must reconcile idempotently.
        ContinuityLedger::update(&rig.ledger, |l| {
            l.replacement_leases[0].state = LeaseState::Requested
        })
        .unwrap();
        let broker = ReplacementBroker::reopen(&rig.broker.path, rig.broker.id()).unwrap();
        let mut fresh_operation = rig.expected.clone();
        fresh_operation.operation_id = Uuid::new_v4();
        for _ in 0..3 {
            let pin = hold_replacement(&rig.ledger, &broker, &fresh_operation, 2).unwrap();
            assert_eq!(pin.id(), id);
            drop(pin);
        }
        assert_eq!(rig.pool().slots.len(), 2);
        assert_eq!(rig.pool().leases.len(), 1);
        assert_eq!(rig.request().state, LeaseState::Held);
        assert!(broker.claim_slot(Uuid::new_v4(), Uuid::new_v4()).is_err());
    }

    #[test]
    fn replacement_lease_hard_cap_wait_preserves_request_and_requires_confirmed_old_release() {
        let rig = Rig::new(1);
        let runtime = Uuid::new_v4();
        rig.broker.claim_slot(rig.old, runtime).unwrap();
        assert!(rig.hold(1).is_err());
        let request = rig.request();
        assert_eq!(request.state, LeaseState::Requested);
        assert!(rig.pool().leases.is_empty());
        assert!(
            rig.broker
                .release_stopped_slot(rig.old, Uuid::new_v4())
                .is_err()
        );
        rig.broker.release_stopped_slot(rig.old, runtime).unwrap();
        let pin = rig.hold(2).unwrap();
        assert_eq!(pin.id(), request.id);
        drop(pin);
        assert_eq!(rig.pool().slots.len(), 1);
        // Ordinary launch admission cannot release an unresolved replacement.
        assert!(
            rig.broker
                .release_stopped_slot(rig.new, rig.expected.runtime_id)
                .is_err()
        );
    }

    #[test]
    fn replacement_lease_rejects_changed_launch_foreign_broker_expiry_and_loss() {
        let rig = Rig::new(1);
        drop(rig.hold(1).unwrap());
        for field in 0..8 {
            let mut changed = rig.expected.clone();
            match field {
                0 => changed.runtime_id = Uuid::new_v4(),
                1 => changed.generation += 1,
                2 => changed.model.push('x'),
                3 => changed.configuration_revision.push('x'),
                4 => changed.instruction_revision.push('x'),
                5 => changed.package_digest.push('x'),
                6 => changed.provider = ProviderKind::Pi,
                _ => changed.runtime_version.push('x'),
            }
            assert!(hold_replacement(&rig.ledger, &rig.broker, &changed, 2).is_err());
        }
        let foreign = ReplacementBroker::create(
            &rig.root.join("foreign.json"),
            ProviderKind::Codex,
            "fixture".into(),
            1,
        )
        .unwrap();
        assert!(hold_replacement(&rig.ledger, &foreign, &rig.expected, 2).is_err());
        assert!(ReplacementBroker::reopen(&rig.broker.path, Uuid::new_v4()).is_err());
        assert!(rig.hold(121).is_err()); // equality expires; not silently renewed
        assert_eq!(rig.pool().slots.len(), 1); // expired work still consumes cap
        let mut pool = rig.pool();
        pool.leases.clear();
        persist(&rig.broker.path, &pool).unwrap();
        assert!(rig.hold(2).is_err()); // Held cannot be reacquired after lease loss
        assert_eq!(rig.pool().slots.len(), 1);
        assert_eq!(rig.request().state, LeaseState::Held);
    }

    #[test]
    fn replacement_lease_publication_crashes_keep_capacity_until_durable_terminal() {
        // Faults before pointer, after pointer, and after journal ownership commit.
        for crash in 0..3 {
            let rig = Rig::new(1);
            let pin = rig.hold(1).unwrap();
            let id = pin.id();
            let journal_path = rig.root.join("rotation.json");
            let pointer_path = rig.root.join("pointer");
            let mut journal = rig.journal();
            journal.persist(&journal_path).unwrap();
            if crash >= 1 {
                fs::write(&pointer_path, rig.new.to_string()).unwrap();
            }
            if crash >= 2 {
                journal.commit(2).unwrap();
                journal.persist(&journal_path).unwrap();
            }
            drop(pin); // process loss at a publication boundary
            let recovered = RotationJournal::load(&journal_path).unwrap();
            rig.broker
                .reconcile_committed(&rig.ledger, &recovered)
                .unwrap();
            if crash < 2 {
                assert_eq!(rig.request().state, LeaseState::Held);
                let resumed = rig.hold(3).unwrap();
                assert_eq!(resumed.id(), id);
                fs::write(&pointer_path, rig.new.to_string()).unwrap();
                journal.commit(3).unwrap();
                journal.persist(&journal_path).unwrap();
                resumed.activated(&rig.ledger).unwrap();
            } else {
                assert!(rig.hold(3).is_err());
            }
            // Success released the replacement lifecycle, not its running slot.
            assert_eq!(rig.request().state, LeaseState::Activated);
            assert_eq!(rig.pool().leases[0].state, LeaseState::Activated);
            assert_eq!(rig.pool().slots.len(), 1);
            assert!(
                rig.broker
                    .claim_slot(Uuid::new_v4(), Uuid::new_v4())
                    .is_err()
            );
            rig.broker
                .reconcile_committed(&rig.ledger, &journal)
                .unwrap();
            rig.broker
                .release_stopped_slot(rig.new, rig.expected.runtime_id)
                .unwrap();
            assert!(rig.pool().slots.is_empty());
        }
    }

    #[test]
    fn replacement_lease_terminal_release_retries_after_restart_and_failure_verdict() {
        let rig = Rig::new(1);
        let pin = rig.hold(1).unwrap();
        let id = pin.id();
        drop(pin);
        assert!(
            rig.broker
                .settle(&rig.ledger, id, LeaseState::Failed)
                .is_err()
        );
        // Trusted fixture represents independently confirmed successor stop.
        assert!(
            rig.broker
                .failed_after_confirmed_stop(&rig.ledger, id, rig.new, Uuid::new_v4())
                .is_err()
        );
        FAIL_PERSIST.with(|fail| fail.set(true));
        assert!(
            rig.broker
                .failed_after_confirmed_stop(&rig.ledger, id, rig.new, rig.expected.runtime_id)
                .is_err()
        );
        assert_eq!(rig.request().state, LeaseState::Failed);
        assert_eq!(rig.pool().slots.len(), 1); // death before broker settlement
        let broker = ReplacementBroker::reopen(&rig.broker.path, rig.broker.id()).unwrap();
        for _ in 0..2 {
            broker
                .reconcile_committed(&rig.ledger, &rig.journal())
                .unwrap();
        }
        assert!(rig.pool().slots.is_empty());
        assert_eq!(rig.pool().leases[0].state, LeaseState::Failed);
        assert!(rig.hold(3).is_err());
        assert!(
            broker
                .settle(&rig.ledger, id, LeaseState::Activated)
                .is_err()
        );
    }

    #[test]
    fn replacement_lease_lock_covers_publication_and_blocks_competing_admission() {
        let rig = Rig::new(1);
        let pin = rig.hold(1).unwrap();
        let other = private_options()
            .open(rig.broker.path.with_extension("lock"))
            .unwrap();
        assert!(other.try_lock().is_err());
        assert!(
            rig.broker
                .failed_after_confirmed_stop(
                    &rig.ledger,
                    pin.id(),
                    rig.new,
                    rig.expected.runtime_id,
                )
                .is_err()
        );
        assert_eq!(rig.request().state, LeaseState::Held);
        // Pointer and eventual grant commit run inside this pin's critical section.
        fs::write(rig.root.join("pointer"), rig.new.to_string()).unwrap();
        assert!(other.try_lock().is_err());
        drop(pin);
        other.try_lock().unwrap();
        drop(other);
        assert!(
            rig.broker
                .claim_slot(Uuid::new_v4(), Uuid::new_v4())
                .is_err()
        );
    }

    #[test]
    fn replacement_lease_concurrent_launches_share_the_hard_cap() {
        let rig = Rig::new(1);
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let attempts: Vec<_> = (0..2)
            .map(|_| {
                let broker = rig.broker.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    broker.claim_slot(Uuid::new_v4(), Uuid::new_v4()).is_ok()
                })
            })
            .collect();
        start.wait();
        let successes = attempts
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|ok| *ok)
            .count();
        assert_eq!(successes, 1);
        assert_eq!(rig.pool().slots.len(), 1);
        assert!(rig.hold(1).is_err());
    }

    #[test]
    fn replacement_lease_persistence_failure_never_authorizes_or_drops_intent() {
        let rig = Rig::new(1);
        let saved = rig.root.join("saved-pool");
        fs::rename(&rig.broker.path, &saved).unwrap();
        fs::create_dir(&rig.broker.path).unwrap();
        assert!(rig.hold(1).is_err());
        assert_eq!(rig.request().state, LeaseState::Requested);
        fs::remove_dir(&rig.broker.path).unwrap();
        fs::rename(&saved, &rig.broker.path).unwrap();
        FAIL_PERSIST.with(|fail| fail.set(true));
        assert!(rig.hold(2).is_err());
        assert!(rig.pool().slots.is_empty());
        assert_eq!(rig.request().state, LeaseState::Requested);
        let pin = rig.hold(2).unwrap();
        let id = pin.id();
        // Fail the terminal ledger write after simulated ownership publication.
        let saved_ledger = rig.root.join("saved-ledger");
        fs::rename(&rig.ledger, &saved_ledger).unwrap();
        fs::create_dir(&rig.ledger).unwrap();
        assert!(pin.activated(&rig.ledger).is_err());
        fs::remove_dir(&rig.ledger).unwrap();
        fs::rename(&saved_ledger, &rig.ledger).unwrap();
        assert_eq!(rig.request().state, LeaseState::Held);
        assert_eq!(rig.pool().leases[0].id, id);
        assert_eq!(rig.pool().slots.len(), 1);
        let mut journal = rig.journal();
        journal.commit(4).unwrap();
        FAIL_PERSIST.with(|fail| fail.set(true));
        assert!(
            rig.broker
                .reconcile_committed(&rig.ledger, &journal)
                .is_err()
        );
        assert_eq!(rig.request().state, LeaseState::Activated);
        assert_eq!(rig.pool().leases[0].state, LeaseState::Held);
        rig.broker
            .reconcile_committed(&rig.ledger, &journal)
            .unwrap();
        assert_eq!(rig.pool().leases[0].state, LeaseState::Activated);
    }
}
