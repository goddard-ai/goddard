use super::*;
use crate::boss_rotation::{
    BossRotationConfig, CompactionOrigin, RotationEventBoundary, RotationInstruction,
    RotationJournal, RotationSignal, RotationUsageSample,
};
use crate::model::MessageRole;
use waku_boss::boss_rotation::RotationCause;
use waku_boss::rotation_continuity::ContinuityLedger;
#[cfg(test)]
use waku_boss::rotation_oracle::{
    AdmissionBoundary, AdmissionDecision, DurableAction, DurableActionKind, DurableActionStatus,
    LifecycleObservation, OwnershipObservation, OwnershipState, ProviderAdmission,
    ProviderCompaction, ProviderTransport, ProviderTransportStatus, RotationObservation,
};
use waku_boss::rotation_oracle::{CompactionStage, ContextSampleSource, RotationLifecycleStage};

/// Every session id a Boss chat lineage has owned — each generation the
/// rotation journal names, oldest first, plus `session_id` itself. When
/// the journal does not name the session the answer is just the session,
/// so employee and planning targets keep single-session dedupe. A
/// notification parked or delivered on a retired generation counts as
/// landed for the lineage: each successor's transcript starts empty by
/// design, so a same-session scan alone would mint a second copy after
/// every refresh.
pub(super) fn boss_lineage_ids(journal_path: &std::path::Path, session_id: Uuid) -> Vec<Uuid> {
    let journal = RotationJournal::load(journal_path).unwrap_or_default();
    let named = journal.active_session_id == Some(session_id)
        || journal.intent.as_ref().is_some_and(|intent| {
            intent.old_session_id == session_id || intent.new_session_id == session_id
        })
        || journal.rotations.iter().any(|rotation| {
            rotation.old_session_id == session_id || rotation.new_session_id == session_id
        });
    if !named {
        return vec![session_id];
    }
    let mut lineage: Vec<Uuid> = Vec::new();
    let mut push = |id: Uuid| {
        if !lineage.contains(&id) {
            lineage.push(id);
        }
    };
    for rotation in &journal.rotations {
        push(rotation.old_session_id);
        push(rotation.new_session_id);
    }
    if let Some(intent) = &journal.intent {
        push(intent.old_session_id);
        push(intent.new_session_id);
    }
    if let Some(active) = journal.active_session_id {
        push(active);
    }
    push(session_id);
    lineage
}

/// The transcript's quiet lifecycle marker — the archived chat and its
/// successor each carry one at the boundary. The pre-refresh text stays
/// recognized so an intent staged before the rename cannot duplicate it.
const ROTATION_MARKERS: [&str; 2] = [
    "Boss session rotated.",
    "Boss session refreshed; active work preserved.",
];
const ROTATION_MARKER: &str = ROTATION_MARKERS[1];

/// Record a Boss compaction signal into the lifecycle journal — attempt,
/// start, completion, or failure, correlated to the session's owner
/// generation. Best-effort telemetry: a write failure logs and never
/// faults the caller.
pub(super) fn record_boss_compaction_signal(
    data_dir: &std::path::Path,
    session_id: Uuid,
    runtime_id: Option<Uuid>,
    stage: CompactionStage,
    origin: CompactionOrigin,
    at: u64,
) {
    let path = data_dir.join("boss/rotation.json");
    let mut journal = match RotationJournal::load(&path) {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("could not load Boss rotation journal for compaction telemetry: {error:#}");
            return;
        }
    };
    journal.record_compaction(session_id, runtime_id, stage, origin, at);
    if let Err(error) = journal.persist(&path) {
        eprintln!("could not record Boss compaction telemetry: {error:#}");
    }
}

impl WakuBackend {
    pub(super) fn boss_journal_path(&self) -> PathBuf {
        self.data_dir.join("boss/rotation.json")
    }

    pub(super) fn boss_continuity_path(&self) -> PathBuf {
        self.data_dir.join("boss/continuity.json")
    }

    /// Whether the journal or the durable session rows name `session_id` as
    /// a retired Boss chat. Fenced generations keep their sessions and their
    /// scoped credentials, but every action through either is denied: the
    /// pointer, not the credential, defines who may coordinate.
    pub(super) fn boss_fenced_chat(&self, session_id: Uuid) -> bool {
        if self.boss.is_boss(session_id) {
            return false;
        }
        let journal_fenced = RotationJournal::load(&self.boss_journal_path())
            .map(|journal| {
                journal.intent.as_ref().is_some_and(|intent| {
                    intent.old_session_id == session_id || intent.new_session_id == session_id
                }) || journal
                    .rotations
                    .iter()
                    .any(|rotation| rotation.old_session_id == session_id)
            })
            .unwrap_or(false);
        if journal_fenced {
            return true;
        }
        // Chats retired before the journal existed carry no rotation record:
        // a boss-managed archived session in the Boss project that is neither
        // employee nor planning session is one.
        let (identity, _) = self.boss.identity_and_session();
        self.task_state.lock().sessions.iter().any(|session| {
            session.id == session_id
                && session.boss_managed
                && session.project_id == identity.id
                && session.archived_at.is_some()
        }) && self.boss.employee_including_retired(session_id).is_none()
            && !self.boss.is_planning(session_id)
    }

    /// Every session a rotation ever named — fenced predecessors, the
    /// committed owner, and a staged-but-unpublished successor. The fence
    /// wraps them all so a staged session holds no authority before the
    /// pointer commits.
    fn boss_rotation_names(&self, session_id: Uuid) -> bool {
        RotationJournal::load(&self.boss_journal_path())
            .map(|journal| {
                journal.active_session_id == Some(session_id)
                    || journal.intent.as_ref().is_some_and(|intent| {
                        intent.old_session_id == session_id
                            || intent.new_session_id == session_id
                    })
                    || journal.rotations.iter().any(|rotation| {
                        rotation.old_session_id == session_id
                            || rotation.new_session_id == session_id
                    })
            })
            .unwrap_or(false)
    }

    /// Whether a compaction incident fenced this Boss session. An
    /// incident-fenced chat holds its queued work parked — recorded and
    /// visible — until the refresh commits, rather than delivering it to
    /// a provider boundary that can no longer be trusted.
    pub(super) fn boss_incident_fenced(&self, session_id: Uuid) -> bool {
        self.boss.is_boss(session_id)
            && ContinuityLedger::load(&self.boss_continuity_path())
                .map(|ledger| {
                    ledger.has_incident(session_id) || ledger.coordinator_frozen(session_id)
                })
                .unwrap_or(true)
    }

    /// The session a message addressed to the Boss should reach now. A
    /// retired chat's id aliases to the committed owner so events resolved
    /// against a stale pointer land on the successor exactly once.
    pub(super) fn boss_active_alias(&self, session_id: Uuid) -> Uuid {
        if !self.boss_fenced_chat(session_id) {
            return session_id;
        }
        self.boss.identity_and_session().1.unwrap_or(session_id)
    }

    /// The sessions a durable notification to `session_id` counts as
    /// landed on — the whole Boss generation chain when the rotation
    /// journal names this chat, just the session otherwise.
    pub(super) fn boss_lineage_sessions(&self, session_id: Uuid) -> Vec<Uuid> {
        boss_lineage_ids(&self.boss_journal_path(), session_id)
    }

    /// Whether a durable notification with this message id already reached
    /// the lineage — parked in or delivered to any generation, not only
    /// the session holding the pointer now.
    pub(super) fn boss_lineage_message_landed(&self, session_id: Uuid, message_id: Uuid) -> bool {
        let mut state = self.task_state.lock();
        self.boss_lineage_sessions(session_id).iter().any(|id| {
            state
                .sessions
                .iter_mut()
                .find(|session| session.id == *id)
                .is_some_and(|session| {
                    self.task_store.hydrate(session).is_ok()
                        && (session.queued_messages.iter().any(|m| m.id == message_id)
                            || session.messages.iter().any(|m| m.id == message_id))
                })
        })
    }

    /// The installed provider's prevention-contract evidence for the active
    /// Boss configuration. Unknown or undetected versions stay unqualified —
    /// the lifecycle may rotate for safety but must never claim prevention.
    pub(super) fn boss_provider_qualification(
        &self,
        provider: ProviderKind,
    ) -> waku_boss::rotation_continuity::ProviderQualification {
        // PATH probes cannot establish the effective launch contract.
        waku_boss::rotation_continuity::qualify_provider(provider, None)
    }

    /// The last daemon-owned boundary every Boss provider handle crosses.
    /// Sessions the journal names get the ownership fence; other Boss
    /// principals keep the observation-only test wrapper.
    pub(super) fn boss_driver_boundary(
        &self,
        session_id: Uuid,
        #[cfg_attr(not(test), allow(unused_variables))] runtime_id: Uuid,
        driver: DriverHandle,
    ) -> DriverHandle {
        if self.boss.is_boss_principal(session_id) || self.boss_rotation_names(session_id) {
            #[cfg(test)]
            if let Some(ownership) = self.boss_rotation_ownership(session_id) {
                self.record_rotation_observation(RotationObservation::Ownership(ownership));
            }
            return DriverHandle::from_control(Arc::new(BossOwnershipDriver {
                inner: driver,
                session_id,
                runtime_id,
                boss: self.boss.clone(),
                task_state: self.task_state.clone(),
                journal_path: self.boss_journal_path(),
                continuity_path: self.boss_continuity_path(),
                #[cfg(test)]
                observer: self.runtime_lifecycles.rotation_observer.lock().clone(),
                #[cfg(test)]
                next_operation: std::sync::atomic::AtomicU64::new(0),
            }));
        }
        #[cfg(test)]
        return self.observe_boss_driver(session_id, runtime_id, driver);
        #[cfg(not(test))]
        return driver;
    }

    /// Repair only the durable pointer's chat. Callers hold the Boss operation
    /// lock, so rotation cannot retire this row while it is being restored.
    pub(super) fn recover_archived_active_boss_chat(&self) -> anyhow::Result<()> {
        let (identity, active) = self.boss.identity_and_session();
        let Some(active) = active else {
            return Ok(());
        };
        let mut state = self.task_state.lock();
        let Some(session) = state.session_mut(active) else {
            return Ok(());
        };
        if session.archived_at.is_none() {
            return Ok(());
        }
        self.task_store.hydrate(session)?;
        anyhow::ensure!(
            session.boss_managed && session.project_id == identity.id && session.planning.is_none(),
            "active Boss pointer does not name a Boss chat"
        );
        let archived_at = session.archived_at;
        let updated_at = session.updated_at;
        session.archived_at = None;
        session.updated_at = session.updated_at.max(crate::model::unix_time());
        state.mark_session_dirty(active);
        if let Err(error) = self.task_store.save(&mut state) {
            if let Some(session) = state.session_mut(active) {
                session.archived_at = archived_at;
                session.updated_at = updated_at;
            }
            state.mark_session_dirty(active);
            return Err(error.into());
        }
        eprintln!("recovered archived active Boss chat {active}");
        Ok(())
    }

    /// The Boss scheduler checks settled chats, including after restart.
    /// Fresh chats use the same durable initialization and lazy provider
    /// launch as BossOperation::Open. Every pass republishes the journal's
    /// lifecycle position so a delayed or paused refresh stays visible.
    pub(super) fn reconcile_boss_rotation(&self, now: u64) -> anyhow::Result<()> {
        let settings = self.settings.get();
        if !settings.boss_experiment_enabled || !self.boss.is_active() {
            return Ok(());
        }
        let mut retired = Vec::new();
        self.boss.with_operation_lock(|| {
            let result = self
                .recover_archived_active_boss_chat()
                .and_then(|()| self.rotate_boss_chat(&settings, now, &mut retired));
            if let Err(error) = &result {
                self.note_rotation_failure(now, error);
            }
            self.publish_rotation_status(now);
            result
        })?;
        // Runtime teardown parks on the retired session's event-pump gate,
        // which a stalled pump can hold indefinitely — never inside the
        // Boss operation lock (Boss chat commands serialize behind it) or on
        // this scheduler thread (employee dispatch queues behind it).
        if !retired.is_empty() {
            let lifecycles = self.runtime_lifecycles.clone();
            let sessions = self.sessions.clone();
            let agent = self.agent.clone();
            let repo_maps = self.repo_maps.clone();
            let retire = move || {
                for id in retired {
                    drop_detached(super::runtime::remove_runtime_entry(
                        &lifecycles,
                        &sessions,
                        &agent,
                        &repo_maps,
                        id,
                        None,
                        false,
                    ));
                }
            };
            if let Err(error) = std::thread::Builder::new()
                .name("boss-rotation-retire".into())
                .spawn(retire.clone())
            {
                eprintln!("could not spawn Boss rotation teardown: {error}");
                retire();
            }
        }
        Ok(())
    }

    /// Record a failed reconcile pass against the pending intent — the
    /// paused status carries its cause until the rotation resolves.
    fn note_rotation_failure(&self, now: u64, error: &anyhow::Error) {
        let path = self.boss_journal_path();
        let Ok(mut journal) = RotationJournal::load(&path) else {
            return;
        };
        if journal.intent.is_none() {
            return;
        }
        journal.record_attempt_failure(format!("{error:#}"), now);
        if let Err(error) = journal.persist(&path) {
            eprintln!("could not record Boss rotation failure: {error:#}");
        }
    }

    /// Republish the journal's lifecycle position onto the Boss document —
    /// `set_rotation_status` skips the write when nothing moved.
    fn publish_rotation_status(&self, now: u64) {
        let status = RotationJournal::load(&self.boss_journal_path())
            .map(|journal| journal.status(now))
            .unwrap_or_default();
        if let Err(error) = self.boss.set_rotation_status(status) {
            eprintln!("could not publish Boss rotation status: {error:#}");
        }
    }

    /// The canonical instruction identity carried through a rotation —
    /// persona id, shipped base revision, and a digest of the effective
    /// markdown. The instruction text itself is never logged.
    fn rotation_instruction(&self) -> Option<RotationInstruction> {
        use sha2::Digest;
        let document = self.boss.document();
        let persona = document
            .personas
            .iter()
            .find(|persona| persona.id == document.persona_id)?;
        let defaults = document
            .persona_defaults
            .get(waku_protocol::boss::PersonaDefaultRole::Boss);
        Some(RotationInstruction {
            persona_id: persona.id,
            base_revision: defaults.starting_revision.or(defaults.reviewed_revision),
            digest: Some(format!(
                "{:x}",
                sha2::Sha256::digest(persona.markdown.as_bytes())
            )),
        })
    }

    #[cfg(test)]
    fn record_rotation_observation(&self, observation: RotationObservation) {
        if let Some(observer) = self.runtime_lifecycles.rotation_observer.lock().clone() {
            observer(observation);
        }
    }

    #[cfg(test)]
    fn observe_rotation_lifecycle(
        &self,
        intent: &crate::boss_rotation::RotationIntent,
        stage: RotationLifecycleStage,
    ) {
        self.record_rotation_observation(RotationObservation::Lifecycle(
            LifecycleObservation {
                rotation_id: intent.new_session_id,
                old_session_id: intent.old_session_id,
                new_session_id: intent.new_session_id,
                old_generation: intent.old_generation,
                new_generation: intent.old_generation.saturating_add(1),
                stage,
            },
        ));
    }

    #[cfg(test)]
    fn observe_rotation_ownership(
        &self,
        session_id: Uuid,
        generation: u64,
        state: OwnershipState,
    ) {
        self.record_rotation_observation(RotationObservation::Ownership(
            OwnershipObservation {
                session_id,
                generation,
                state,
            },
        ));
    }

    #[cfg(test)]
    fn observe_rotation_durable_action(
        &self,
        intent: &crate::boss_rotation::RotationIntent,
        action: &str,
        session_id: Option<Uuid>,
        owner_generation: Option<u64>,
        kind: DurableActionKind,
    ) {
        self.record_rotation_observation(RotationObservation::DurableAction(
            DurableAction {
                action_id: format!("boss-rotation:{}:{action}", intent.new_session_id),
                session_id,
                owner_generation,
                kind,
                status: DurableActionStatus::Committed,
            },
        ));
    }

    #[cfg(test)]
    fn boss_rotation_ownership(&self, session_id: Uuid) -> Option<OwnershipObservation> {
        let journal = RotationJournal::load(&self.data_dir.join("boss/rotation.json"))
            .unwrap_or_default();
        let (_, active) = self.boss.identity_and_session();
        if let Some(intent) = &journal.intent {
            if intent.new_session_id == session_id {
                return Some(OwnershipObservation {
                    session_id,
                    generation: intent.old_generation.saturating_add(1),
                    state: OwnershipState::Published,
                });
            }
            if intent.old_session_id == session_id {
                return Some(OwnershipObservation {
                    session_id,
                    generation: intent.old_generation,
                    state: if active == Some(session_id) {
                        OwnershipState::Active
                    } else {
                        OwnershipState::Fenced
                    },
                });
            }
        }
        if active == Some(session_id) {
            return Some(OwnershipObservation {
                session_id,
                generation: journal.generation,
                state: OwnershipState::Active,
            });
        }
        if let Some(rotation) = journal
            .rotations
            .iter()
            .rev()
            .find(|rotation| {
                rotation.old_session_id == session_id || rotation.new_session_id == session_id
            })
        {
            if rotation.new_session_id == session_id {
                return Some(OwnershipObservation {
                    session_id,
                    generation: rotation.old_generation.saturating_add(1),
                    state: OwnershipState::Active,
                });
            }
            return Some(OwnershipObservation {
                session_id,
                generation: rotation.old_generation,
                state: OwnershipState::Fenced,
            });
        }
        self.task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .filter(|session| session.boss_managed)
            .map(|session| OwnershipObservation {
                session_id,
                generation: 0,
                state: if session.archived_at.is_some() {
                    OwnershipState::Fenced
                } else {
                    OwnershipState::Published
                },
            })
    }

    /// Wrap the live provider handle at the last daemon-owned boundary. This
    /// catches direct internal prompt/steer calls as well as RPC commands; the
    /// future guard can replace `Unguarded` with its measured decision here.
    #[cfg(test)]
    pub(super) fn observe_boss_driver(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        driver: DriverHandle,
    ) -> DriverHandle {
        if !self.boss.is_boss_principal(session_id) {
            return driver;
        }
        let Some(observer) = self.runtime_lifecycles.rotation_observer.lock().clone() else {
            return driver;
        };
        if let Some(ownership) = self.boss_rotation_ownership(session_id) {
            self.record_rotation_observation(RotationObservation::Ownership(ownership));
        }
        DriverHandle::from_control(Arc::new(RotationObservedDriver {
            inner: driver,
            session_id,
            runtime_id,
            owner_generation: self
                .boss_rotation_ownership(session_id)
                .map(|ownership| ownership.generation),
            observer,
            next_operation: std::sync::atomic::AtomicU64::new(0),
        }))
    }

    /// `retired` collects sessions whose runtimes must be torn down after
    /// the caller drops the Boss operation lock — teardown waits on the
    /// session's event-pump gate, which must never park the lock.
    /// Validate an exact checkpoint before either publication or recovered
    /// publication can retire a generation. No runtime is launched here: all
    /// current adapters lack qualified admission and replacement capacity.
    fn validate_rotation_successor(
        &self,
        state: &PersistedState,
        intent: &waku_boss::boss_rotation::RotationIntent,
        now: u64,
    ) -> anyhow::Result<()> {
        use waku_boss::coordinator_contract::{CoordinatorBinding, CoordinatorPackage};
        use waku_boss::rotation_continuity::{ContinuityPackage, ContinuityScope};
        let next = state
            .sessions
            .iter()
            .find(|s| s.id == intent.new_session_id)
            .ok_or_else(|| anyhow!("successor checkpoint unavailable"))?;
        let old = state
            .sessions
            .iter()
            .find(|s| s.id == intent.old_session_id)
            .ok_or_else(|| anyhow!("predecessor checkpoint unavailable"))?;
        let identity_revision = self.boss.identity_digest();
        anyhow::ensure!(
            intent.identity_digest.as_ref() == Some(&identity_revision),
            "Boss refresh identity is stale or unbound"
        );
        anyhow::ensure!(
            intent.provider.as_deref() == Some(format!("{:?}", next.provider).as_str())
                && intent.model == next.model
                && old.provider == next.provider
                && old.model == next.model,
            "Boss refresh configuration changed"
        );
        let document = self.boss.document();
        // A staged session is deliberately not yet a principal. Render identity
        // from the current principal, independently of the narrative handoff.
        let principal = self
            .boss
            .identity_and_session()
            .1
            .ok_or_else(|| anyhow!("coordinator principal unavailable"))?;
        let package = CoordinatorPackage {
            binding: CoordinatorBinding {
                provider: next.provider,
                session: next.id,
                generation: intent
                    .old_generation
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("Boss generation exhausted"))?,
                identity_revision: identity_revision.clone(),
                grants_revision: serde_json::to_string(&document.resource_policy)?,
                model: next.model.clone().unwrap_or_default(),
                scope: ContinuityScope::Coordinator,
            },
            content: ContinuityPackage {
                identity_revision,
                scope: ContinuityScope::Coordinator,
                instructions: self.boss.canonical_boss_instructions(principal)?,
                context: next
                    .pending_provider_context
                    .clone()
                    .ok_or_else(|| anyhow!("continuity context unavailable"))?,
            },
            event_ids: old
                .messages
                .iter()
                .map(|m| m.id)
                .chain(old.queued_messages.iter().map(|m| m.id))
                .chain(next.queued_messages.iter().map(|m| m.id))
                .collect(),
            work_ids: document
                .employees
                .iter()
                .map(|e| e.session_id)
                .chain(document.planning.iter().map(|p| p.id))
                .chain(document.outcomes.iter().map(|o| o.id))
                .chain(document.deliverables.iter().map(|d| d.id))
                .collect(),
            effect_ids: ContinuityLedger::load(&self.boss_continuity_path())?
                .coordinator_effects
                .iter()
                .filter(|e| e.resolution.is_none())
                .map(|e| e.id.clone())
                .collect(),
        };
        let id = format!("boss-successor:{}", next.id);
        ContinuityLedger::update(&self.boss_continuity_path(), |ledger| {
            ledger.stage_successor(&id, &package, now)
        })??;
        let ledger = ContinuityLedger::load(&self.boss_continuity_path())?;
        let attempt = ledger
            .deliveries
            .iter()
            .find(|d| d.delivery_id == id)
            .and_then(|d| d.attempt.as_ref())
            .ok_or_else(|| anyhow!("Boss refresh paused: exact provider acceptance unavailable"))?;
        let (runtime_id, driver) = self
            .sessions
            .lock()
            .get(&next.id)
            .map(|entry| (entry.runtime_id, entry.driver.clone()))
            .ok_or_else(|| anyhow!("Boss refresh paused: prepared runtime unavailable"))?;
        anyhow::ensure!(
            attempt.runtime_id == runtime_id,
            "successor runtime changed"
        );
        // No adapter currently supplies effective, independently trusted bounds
        // or policy enforcement. Never infer launch revisions from a receipt or
        // copy adapter evidence into its independently expected binding.
        let expected = waku_boss::provider_budget::StartupBudgetBinding {
            provider: next.provider,
            runtime_version: String::new(),
            model: package.binding.model.clone(),
            configuration_revision: String::new(),
            instruction_revision: package.binding.identity_revision.clone(),
            runtime_id,
            generation: package.binding.generation,
            package_digest: package.exact_content()?.digest()?,
            operation_id: Uuid::new_v4(),
        };
        if ledger.deliveries.iter().any(|delivery| {
            delivery.delivery_id == id
                && delivery.status
                    == waku_boss::rotation_continuity::ContinuityDeliveryStatus::Submitted
        }) {
            // Query the original attempt only. Unknown reconciliation cannot
            // create a replacement attempt or replay a provider effect.
            crate::driver::deliver_coordinator_package(
                driver.coordinator_control(),
                &self.boss_continuity_path(),
                &id,
                &package,
                &package.binding,
                &expected,
            )?;
        }
        let latest = ContinuityLedger::load(&self.boss_continuity_path())?;
        let adapter = driver.coordinator_control();
        let observed = adapter.coordinator_startup_observation();
        latest.validate_successor(
            &package,
            attempt,
            &expected,
            observed.as_ref(),
            adapter.coordinator_policy_enforced(),
        )
    }

    fn rotate_boss_chat(
        &self,
        settings: &crate::DaemonSettings,
        now: u64,
        retired: &mut Vec<Uuid>,
    ) -> anyhow::Result<()> {
        let (identity, active) = self.boss.identity_and_session();
        let Some(active) = active else {
            return Ok(());
        };
        let path = self.boss_journal_path();
        let continuity_path = self.boss_continuity_path();
        let mut journal = RotationJournal::load(&path)?;
        let ledger = ContinuityLedger::load(&continuity_path)?;
        // A requested refresh (manual compact or a compaction incident)
        // triggers independently of the occupancy opt-out — both are
        // prevention requests, not threshold preferences.
        let fenced_at = ledger
            .compaction_incidents
            .iter()
            .filter(|entry| entry.session_id == active)
            .map(|entry| entry.detected_at)
            .min();
        let incident = fenced_at.is_some();
        let requested = ledger.rotation_request.as_ref().map(|request| {
            if incident {
                format!(
                    "provider compaction detected on session {active}: {}",
                    request.reason
                )
            } else {
                request.reason.clone()
            }
        });
        if journal.intent.is_none() && settings.boss_rotation_disabled && requested.is_none() {
            return Ok(());
        }
        let mut state = self.task_state.lock();
        if journal.intent.is_none() {
            if let Some(fenced_at) = fenced_at {
                self.unwind_fence_orphaned_turn(&mut state, active, fenced_at)?;
            }
            let Some(old) = state.session_mut(active) else {
                return Ok(());
            };
            self.task_store.hydrate(old)?;
            // Trigger and freeze independently of queue emptiness or an open
            // provider turn. Quiescence is checked only after the durable fence.
            let policy = BossRotationConfig::from_settings(settings);
            let reason = if let Some(reason) = requested.clone() {
                reason
            } else {
                let Some(usage) = old.context_usage else {
                    return Ok(());
                };
                if !policy.should_rotate(usage.tokens, usage.window) {
                    return Ok(());
                }
                format!(
                    "boss_rotation_context_threshold={} exceeded (context_tokens={}, context_window={})",
                    policy.context_threshold,
                    usage.tokens,
                    usage.window.unwrap_or_default(),
                )
            };
            if old.messages.is_empty() {
                // The task store omits empty drafts even when their queues are
                // populated. Anchor the predecessor durably before freezing so
                // restart cannot lose a first-turn input or unfinished intake.
                old.push_message(MessageRole::System, "Boss coordination checkpoint.");
                old.messages.last_mut().expect("checkpoint appended").hidden = true;
                self.task_store.save(&mut state)?;
            }
            let old = state
                .session_mut(active)
                .expect("rotation source is present");
            journal.active_session_id = Some(active);
            journal.begin(identity.id, active, Uuid::new_v4(), now)?;
            {
                // Bind the coherent revision the successor is built for:
                // identity (persona, defaults, grants), provider/model, and
                // the prevention-contract evidence at intent time.
                let intent = journal.intent.as_mut().expect("begun rotation");
                intent.reason = Some(reason);
                intent.identity_digest = Some(self.boss.identity_digest());
                intent.provider = Some(format!("{:?}", old.provider));
                intent.model = old.model.clone();
                intent.provider_qualification =
                    Some(self.boss_provider_qualification(old.provider));
                // The diagnostics record reports what this trigger measured
                // and the cause it classified — a compaction incident or a
                // requested refresh is never a threshold rotation.
                let diagnostics = &mut intent.diagnostics;
                diagnostics.cause = if ledger.has_incident(active) {
                    RotationCause::Incident
                } else if requested.is_some() {
                    RotationCause::Requested
                } else {
                    RotationCause::ContextThreshold
                };
                diagnostics.provider = Some(old.provider);
                diagnostics.model = old.model.clone();
                diagnostics.context_threshold = Some(policy.context_threshold);
                diagnostics.usage = old.context_usage.map(|usage| RotationUsageSample {
                    context_tokens: Some(usage.tokens),
                    context_window: usage.window,
                    source: ContextSampleSource::ProviderReport,
                });
                diagnostics.event_boundary = Some(RotationEventBoundary {
                    session_id: active,
                    message_count: old.messages.len(),
                    last_message_id: old.messages.last().map(|message| message.id),
                });
                diagnostics.instruction = self.rotation_instruction();
                eprintln!(
                    "Boss session rotation: boss_id={} old_session_id={} new_session_id={} reason={:?} provider={} qualification={:?}",
                    identity.id,
                    active,
                    intent.new_session_id,
                    intent.reason,
                    intent.provider.as_deref().unwrap_or_default(),
                    intent.provider_qualification,
                );
            }
            journal.record_stage(RotationLifecycleStage::Triggered, now);
            journal.record_stage(RotationLifecycleStage::IntentPersisted, now);
            journal.persist(&path)?;
            #[cfg(test)]
            {
                self.observe_rotation_ownership(active, journal.generation, OwnershipState::Active);
                self.observe_rotation_lifecycle(
                    journal.intent.as_ref().expect("begun rotation"),
                    RotationLifecycleStage::Triggered,
                );
                self.observe_rotation_lifecycle(
                    journal.intent.as_ref().expect("begun rotation"),
                    RotationLifecycleStage::IntentPersisted,
                );
                self.observe_rotation_durable_action(
                    journal.intent.as_ref().expect("begun rotation"),
                    "intent",
                    Some(active),
                    Some(journal.generation),
                    DurableActionKind::PersistedIntent,
                );
            }
        }
        let intent = journal.intent.as_ref().expect("pending rotation").clone();
        anyhow::ensure!(
            intent.boss_id == identity.id,
            "rotation belongs to another Boss"
        );
        anyhow::ensure!(
            active == intent.old_session_id || active == intent.new_session_id,
            "Boss rotation session pointer diverged"
        );
        ContinuityLedger::update(&continuity_path, |ledger| {
            ledger.freeze_coordinator(
                intent.old_session_id,
                intent.new_session_id,
                intent.old_generation,
            )
        })??;
        let predecessor = format!("<boss-rotation-link:{}>", intent.old_session_id);
        if active == intent.old_session_id {
            if let Some(fenced_at) = fenced_at {
                self.unwind_fence_orphaned_turn(&mut state, active, fenced_at)?;
            }
            let old = state
                .session_mut(active)
                .ok_or_else(|| anyhow!("rotation source session is missing"))?;
            self.task_store.hydrate(old)?;
            // Hold the task lease through publication: prompt recording
            // cannot open a turn between this check and the archive flag.
            if !boss_chat_settled(old, &self.agent, true) {
                return Ok(());
            }
            if !state
                .sessions
                .iter()
                .any(|session| session.id == intent.new_session_id)
            {
                let old = state
                    .sessions
                    .iter()
                    .find(|session| session.id == active)
                    .expect("rotation source is present");
                let mut next = AgentSession::new(old.project_id, old.provider);
                next.id = intent.new_session_id;
                next.title = identity.name;
                next.model = old.model.clone();
                next.runtime_mode = old.runtime_mode;
                next.environment = old.environment();
                next.reasoning_effort = old.reasoning_effort.clone();
                next.service_tier = old.service_tier.clone();
                next.context_window = old.context_window.clone();
                next.agent_preset = old.agent_preset.clone();
                next.agent_rename_allowed = false;
                next.boss_managed = true;
                next.pending_provider_context = Some(boss_rotation_handoff(old, ROTATION_MARKER));
                next.push_message(MessageRole::System, ROTATION_MARKER);
                next.push_message(MessageRole::System, &predecessor);
                next.messages
                    .last_mut()
                    .expect("rotation link was appended")
                    .hidden = true;
                state.push_session(next);
                // Stage the initialized chat before publishing its id.
                self.task_store.save(&mut state)?;
            }
            journal.record_stage(RotationLifecycleStage::SuccessorPrepared, now);
            journal.persist(&path)?;
            #[cfg(test)]
            {
                self.observe_rotation_lifecycle(&intent, RotationLifecycleStage::SuccessorPrepared);
                self.observe_rotation_durable_action(
                    &intent,
                    "successor",
                    Some(intent.new_session_id),
                    Some(intent.old_generation.saturating_add(1)),
                    DurableActionKind::SuccessorPreparation,
                );
            }
            // Revalidate the bound revision before the pointer switch: a
            // persona, grant or provider change during staging means this
            // successor was built for a stale revision. Never activate
            // stale grants — unwind the stage and let the next pass rebind.
            {
                let identity_current = intent
                    .identity_digest
                    .as_deref()
                    .is_none_or(|bound| bound == self.boss.identity_digest());
                let config_current = state
                    .sessions
                    .iter()
                    .find(|session| session.id == intent.new_session_id)
                    .is_some_and(|staged| {
                        intent
                            .provider
                            .as_deref()
                            .is_none_or(|provider| provider == format!("{:?}", staged.provider))
                            && intent
                                .model
                                .as_deref()
                                .is_none_or(|model| model == staged.model.as_deref().unwrap_or(""))
                    });
                if !identity_current || !config_current {
                    // Do not discard a submitted package or an uncertain effect.
                    // A changed revision requires explicit reconciliation; the old
                    // generation stays frozen and the checkpoint remains durable.
                    anyhow::bail!(
                        "Boss refresh paused: authoritative identity or configuration changed"
                    );
                }
            }
            self.validate_rotation_successor(&state, &intent, now)?;
            self.boss
                .replace_session_id(active, intent.new_session_id)?;
            journal.record_stage(RotationLifecycleStage::PointerPublished, now);
            journal.persist(&path)?;
            #[cfg(test)]
            {
                self.observe_rotation_lifecycle(&intent, RotationLifecycleStage::PointerPublished);
                self.observe_rotation_ownership(
                    intent.new_session_id,
                    intent.old_generation.saturating_add(1),
                    OwnershipState::Published,
                );
            }
        }
        // Older daemons may have published before validation. Recovery must
        // not archive the predecessor or call that successor active on restart.
        self.validate_rotation_successor(&state, &intent, now)?;
        anyhow::ensure!(
            state
                .sessions
                .iter()
                .any(|session| session.id == intent.new_session_id),
            "published rotation destination is missing"
        );
        let next = state
            .session_mut(intent.new_session_id)
            .ok_or_else(|| anyhow!("rotation destination is missing"))?;
        self.task_store.hydrate(next)?;
        anyhow::ensure!(
            next.boss_managed && next.project_id == identity.id,
            "rotation destination is not a Boss chat"
        );
        if !next.messages.iter().any(|message| {
            message.role == MessageRole::System
                && ROTATION_MARKERS.contains(&message.content.as_str())
        }) {
            next.push_message(MessageRole::System, ROTATION_MARKER);
        }
        if !next
            .messages
            .iter()
            .any(|message| message.content == predecessor)
        {
            next.push_message(MessageRole::System, &predecessor);
            next.messages
                .last_mut()
                .expect("rotation link was appended")
                .hidden = true;
        }
        let old = state
            .session_mut(intent.old_session_id)
            .ok_or_else(|| anyhow!("rotation source session is missing"))?;
        self.task_store.hydrate(old)?;
        if !old.messages.iter().any(|message| {
            message.role == MessageRole::System
                && ROTATION_MARKERS.contains(&message.content.as_str())
        }) {
            old.push_message(MessageRole::System, ROTATION_MARKER);
        }
        let old_link = format!("<boss-rotation-link:{}>", intent.old_session_id);
        if !old
            .messages
            .iter()
            .any(|message| message.content == old_link)
        {
            old.push_message(MessageRole::System, &old_link);
            old.messages
                .last_mut()
                .expect("rotation link was appended")
                .hidden = true;
        }
        old.archived_at.get_or_insert(now);
        old.updated_at = now;
        // Parked follow-ups belong to the chat, not the retiring id — an
        // incident fence held this queue, and the successor's drain now
        // owns it. Entries keep their ids so delivery dedupe still holds.
        let carried = std::mem::take(&mut old.queued_messages);
        if !carried.is_empty() {
            let next = state
                .session_mut(intent.new_session_id)
                .ok_or_else(|| anyhow!("rotation destination is missing"))?;
            for queued in carried {
                if !next.queued_messages.iter().any(|held| held.id == queued.id) {
                    next.queued_messages.push(queued);
                }
            }
        }
        self.task_store.save(&mut state)?;
        drop(state);
        // Same runtime/credential retirement as ordinary archival, run by
        // the caller once the Boss operation lock is dropped. The transcript
        // stays in the task store under its old session id.
        retired.push(intent.old_session_id);
        journal.record_stage(RotationLifecycleStage::PredecessorFenced, now);
        #[cfg(test)]
        {
            self.observe_rotation_lifecycle(&intent, RotationLifecycleStage::PredecessorFenced);
            self.observe_rotation_ownership(
                intent.old_session_id,
                intent.old_generation,
                OwnershipState::Fenced,
            );
        }
        journal.commit(now)?;
        // A generation that closed without any compaction signal gets an
        // explicit gap record — silence is not evidence of absence.
        if !journal.observed_compaction_telemetry(intent.old_session_id) {
            journal.record_signal(RotationSignal::CompactionTelemetryGap {
                at: now,
                session_id: intent.old_session_id,
                generation: Some(intent.old_generation),
                rotation_id: Some(intent.new_session_id),
            });
        }
        journal.persist(&path)?;
        // The refresh request — manual compact or incident — is consumed
        // by the committed swap. Incidents stay as evidence.
        if ledger.rotation_request.is_some() {
            if let Err(error) = ContinuityLedger::update(&continuity_path, |latest| {
                if latest.rotation_request == ledger.rotation_request
                    && !latest.has_incident(intent.new_session_id)
                {
                    latest.resolve_rotation_request();
                }
            }) {
                eprintln!("could not resolve the Boss rotation request: {error:#}");
            }
        }
        #[cfg(test)]
        {
            self.observe_rotation_lifecycle(&intent, RotationLifecycleStage::OwnershipCommitted);
            self.observe_rotation_ownership(
                intent.new_session_id,
                journal.generation,
                OwnershipState::Active,
            );
            self.observe_rotation_durable_action(
                &intent,
                "ownership",
                Some(intent.new_session_id),
                Some(journal.generation),
                DurableActionKind::OwnershipCommit,
            );
        }
        // The retired chat's unsent composer draft belongs to its
        // replacement — re-key the persisted slot so clients that load
        // drafts after the swap still see the text. A draft-store failure
        // must not fault the committed rotation.
        if settings.composer_drafts_experiment_enabled
            && let Err(error) = self.composer_drafts.move_draft(
                crate::persistence::ComposerDraftKey::Session(intent.old_session_id),
                crate::persistence::ComposerDraftKey::Session(intent.new_session_id),
            )
        {
            eprintln!("Boss session rotation: composer draft carry failed: {error}");
        }
        // The in-memory queue follows the same re-key the document took —
        // the successor's drain delivers what the incident fence held. A
        // user stop held on the retired id is the same kind of session-keyed
        // state: the halt still applies to the chat that replaced it.
        self.agent
            .migrate_queue(intent.old_session_id, intent.new_session_id);
        self.agent
            .migrate_user_stop(intent.old_session_id, intent.new_session_id);
        self.wake_summon_queue();
        self.boss.router_defer_context(intent.new_session_id);
        if let Some(notifier) = self.task_notifier.lock().clone() {
            notifier();
        }
        Ok(())
    }

    /// `BossOperation::Clear` under the Boss operation lock — retire the
    /// pointed chat, publish a fresh one under the same configuration, and
    /// name every earlier boss-owned chat generation for removal. Unlike
    /// a rotation the successor carries no transcript handoff: the
    /// conversation starts empty, and the swap is recorded in the journal
    /// as a committed `Requested` generation so the removed sessions stay
    /// fenced by name.
    ///
    /// Planning sessions keep their own lifecycle — they back durable
    /// plan records — and employees live outside the Boss project; only
    /// the chat generations leave. Returns the reply and the retired ids:
    /// `remove_session` parks on each runtime's event-pump gate, so the
    /// caller runs the removals after this lock is dropped.
    pub(super) fn clear_boss_chat(
        &self,
    ) -> anyhow::Result<(waku_protocol::boss::BossResult, Vec<Uuid>)> {
        use waku_protocol::boss::BossResult;
        let settings = self.settings.get();
        let now = crate::model::unix_time();
        let (identity, active) = self.boss.identity_and_session();
        let mut project = Project::from_path(self.boss.owned_workspace()?);
        project.id = identity.id;
        project.name = "Boss".into();
        let new_id = Uuid::new_v4();
        let journal_path = self.boss_journal_path();
        let continuity_path = self.boss_continuity_path();
        let mut journal = RotationJournal::load(&journal_path)?;
        // A rotation staged but uncommitted dies with the clear — its
        // successor was never published, so it joins the retired rows.
        journal.abort();
        let ledger = ContinuityLedger::load(&continuity_path)?;
        // A durable refresh request — manual compact or incident — is
        // answered by the clear itself: the fresh chat must not inherit
        // the request and immediately rotate again.
        // Repoint before staging: a pointer divergence fails the compare-
        // and-swap while nothing has changed, and a later failure leaves a
        // missing session `Open` repairs rather than a stray live chat.
        match active {
            Some(active) => self.boss.replace_session_id(active, new_id)?,
            None => self.boss.set_session_id(new_id)?,
        }
        let next = {
            let mut state = self.task_state.lock();
            if let Some(existing) = state
                .projects
                .iter_mut()
                .find(|entry| entry.id == project.id)
            {
                *existing = project.clone();
            } else {
                state.projects.push(project.clone());
            }
            let mut old =
                active.and_then(|id| state.sessions.iter_mut().find(|session| session.id == id));
            if let Some(old) = &mut old {
                // The archive flag fences the retired chat immediately —
                // the journal commit below gives it a durable generation
                // record, and the removal pass deletes the row outright.
                old.archived_at.get_or_insert(now);
                old.updated_at = now;
            }
            let old = old.map(|session| session.clone());
            let mut next = AgentSession::new(
                project.id,
                old.as_ref()
                    .map(|session| session.provider)
                    .unwrap_or(state.last_provider),
            );
            next.id = new_id;
            next.title = identity.name.clone();
            next.agent_rename_allowed = false;
            next.boss_managed = true;
            if let Some(old) = &old {
                next.model = old.model.clone();
                next.runtime_mode = old.runtime_mode;
                next.environment = old.environment();
                next.reasoning_effort = old.reasoning_effort.clone();
                next.service_tier = old.service_tier.clone();
                next.context_window = old.context_window.clone();
                next.agent_preset = old.agent_preset.clone();
            }
            next.push_message(MessageRole::System, "Boss session cleared.");
            state.push_session(next.clone());
            self.task_store.save(&mut state)?;
            next
        };
        // The clear is a committed swap, not an in-place edit: the retired
        // chat enters the generation chain it was fenced under, and the
        // journal's pointer continues to track the live owner.
        if let Some(active) = active {
            journal.active_session_id = Some(active);
            journal.begin(identity.id, active, new_id, now)?;
            let intent = journal.intent.as_mut().expect("begun clear intent");
            intent.reason = Some("user cleared the Boss chat".into());
            intent.diagnostics.cause = RotationCause::Requested;
            journal.commit(now)?;
        } else {
            journal.active_session_id = Some(new_id);
        }
        journal.persist(&journal_path)?;
        // Every earlier chat generation leaves the store — the retired
        // chats, an aborted stage's successor, and the session the clear
        // just replaced. Ledger deliveries bound for a removed session go
        // with it; a still-pointed delivery belongs to the fresh chat.
        let retired: Vec<Uuid> = {
            let state = self.task_state.lock();
            state
                .sessions
                .iter()
                .filter(|session| {
                    session.boss_managed
                        && session.project_id == identity.id
                        && session.id != new_id
                        && session.planning.is_none()
                        && self.boss.employee_including_retired(session.id).is_none()
                })
                .map(|session| session.id)
                .collect()
        };
        if let Err(error) = ContinuityLedger::update(&continuity_path, |latest| {
            latest.deliveries.retain(|delivery| {
                !retired.contains(&delivery.session_id)
            });
            if latest.rotation_request == ledger.rotation_request
                && !latest.has_incident(new_id)
            {
                latest.resolve_rotation_request();
            }
        }) {
            eprintln!("could not record the Boss clear in the continuity ledger: {error:#}");
        }
        for id in &retired {
            self.boss.reset_context(*id);
        }
        self.boss.router_defer_context(new_id);
        // The retired chat's unsent composer draft belongs to its
        // replacement — same re-key a committed rotation performs.
        if settings.composer_drafts_experiment_enabled
            && let Some(active) = active
            && let Err(error) = self.composer_drafts.move_draft(
                crate::persistence::ComposerDraftKey::Session(active),
                crate::persistence::ComposerDraftKey::Session(new_id),
            )
        {
            eprintln!("Boss chat clear: composer draft carry failed: {error}");
        }
        self.wake_summon_queue();
        if let Some(notifier) = self.task_notifier.lock().clone() {
            notifier();
        }
        Ok((
            BossResult::Session {
                session: Box::new(next),
                project: Box::new(project),
            },
            retired,
        ))
    }

    /// Close the provider-unconfirmed turn a fence-denied prompt left open.
    /// Only a turn opened after the fence rose is certainly orphaned — an
    /// earlier unconfirmed turn's prompt may still be in flight and its
    /// `TurnStarted` can still arrive; the fence denies sends, not
    /// receipts. A provider-confirmed running turn still blocks the
    /// refresh — it holds live work the transcript must finish first.
    fn unwind_fence_orphaned_turn(
        &self,
        state: &mut PersistedState,
        session_id: Uuid,
        fenced_at: u64,
    ) -> anyhow::Result<bool> {
        let Some(session) = state.session_mut(session_id) else {
            return Ok(false);
        };
        self.task_store.hydrate(session)?;
        let orphaned = session.turns.last().is_some_and(|turn| {
            turn.status == crate::model::TurnStatus::Running
                && !turn.provider_turn_started
                && turn.started_at > fenced_at
        });
        if !orphaned {
            return Ok(false);
        }
        session.interrupt_active_turn(crate::model::TurnInterruption::Daemon);
        if session.active_turn_id().is_none() {
            session.status = crate::model::SessionStatus::Idle;
        }
        session.updated_at = crate::model::unix_time();
        state.mark_session_dirty(session_id);
        self.task_store.save(state)?;
        Ok(true)
    }
}

#[cfg(test)]
struct RotationObservedDriver {
    inner: DriverHandle,
    session_id: Uuid,
    runtime_id: Uuid,
    owner_generation: Option<u64>,
    observer: RotationObserver,
    next_operation: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl RotationObservedDriver {
    fn admit(&self, boundary: AdmissionBoundary) {
        (self.observer)(RotationObservation::ProviderAdmission(
            ProviderAdmission {
                operation_id: format!(
                    "boss-driver:{}:{}:{boundary:?}:{}",
                    self.session_id,
                    self.runtime_id,
                    self.next_operation
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                ),
                session_id: self.session_id,
                runtime_id: self.runtime_id,
                owner_generation: self.owner_generation,
                boundary,
                decision: AdmissionDecision::Unguarded,
            },
        ));
    }
}

#[cfg(test)]
impl crate::driver::DriverControl for RotationObservedDriver {
    fn prompt(&self, prompt: String) {
        self.admit(AdmissionBoundary::Request);
        self.inner.prompt(prompt);
    }

    fn prompt_with_attachments(
        &self,
        prompt: String,
        attachments: Vec<crate::model::MessageAttachment>,
    ) {
        self.admit(AdmissionBoundary::Request);
        self.inner.prompt_with_attachments(prompt, attachments);
    }

    fn supports_steer(&self) -> bool {
        self.inner.supports_steer()
    }

    fn agent_surface_delivery(&self) -> crate::driver::AgentSurfaceDelivery {
        self.inner.agent_surface_delivery()
    }

    fn steer(&self, prompt: String) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.steer(prompt);
    }

    fn cancel(&self) {
        self.inner.cancel();
    }

    fn begin_shutdown(&self) {
        self.inner.begin_shutdown();
    }

    fn cancel_computer_use(&self) {
        self.inner.cancel_computer_use();
    }

    fn computer_use_available(&self) -> bool {
        self.inner.computer_use_available()
    }

    fn refresh_background_work(&self) {
        self.inner.refresh_background_work();
    }

    fn stop_background_work(
        &self,
        key: crate::model::BackgroundWorkKey,
        control_id: String,
    ) {
        self.inner.stop_background_work(key, control_id);
    }

    fn respond(&self, request_id: String, option_id: String) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.respond(request_id, option_id);
    }

    fn respond_user_input(
        &self,
        request_id: String,
        answers: Vec<crate::model::UserInputAnswer>,
    ) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.respond_user_input(request_id, answers);
    }

    fn supports_user_input_actions(&self) -> bool {
        self.inner.supports_user_input_actions()
    }

    fn clarify_user_input(&self, request_id: String, content: String) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.clarify_user_input(request_id, content);
    }

    fn cancel_user_input(&self, request_id: String) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.cancel_user_input(request_id);
    }

    fn goal(&self, operation: crate::model::GoalOperation) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.goal(operation);
    }

    fn compact(&self) {
        (self.observer)(RotationObservation::ProviderCompaction(
            ProviderCompaction {
                session_id: self.session_id,
                runtime_id: self.runtime_id,
                owner_generation: self.owner_generation,
                stage: CompactionStage::Attempted,
            },
        ));
        self.inner.compact();
    }

    fn delete_provider_session(&self) {
        self.inner.delete_provider_session();
    }

    fn run_computer_tool(&self, request: crate::computer_use::ComputerToolRequest) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.run_computer_tool(request);
    }

    fn reject_computer_tool(
        &self,
        request: crate::computer_use::ComputerToolRequest,
        reason: String,
    ) {
        self.admit(AdmissionBoundary::Continuation);
        self.inner.reject_computer_tool(request, reason);
    }

    fn apply_options(&self, options: crate::driver::SessionOptions) -> bool {
        self.inner.apply_options(options)
    }

    fn rollback(
        &self,
        turns: usize,
    ) -> anyhow::Result<Option<crate::model::ProviderResumeCursor>> {
        self.admit(AdmissionBoundary::Resume);
        self.inner.rollback(turns)
    }

    fn fork(
        &self,
        turns_to_remove: usize,
    ) -> anyhow::Result<crate::model::ProviderResumeCursor> {
        self.admit(AdmissionBoundary::Resume);
        self.inner.fork(turns_to_remove)
    }
}

/// The production ownership fence on a Boss chat's provider handle. Every
/// context-growing call must clear the same boundary: the durable pointer
/// still names this session the coordinator, the session is not archived,
/// and no compaction incident fenced it. Calls that fail are dropped here —
/// the last daemon-owned point before the provider — so a stale-generation
/// runtime cannot act even while its process and credential still live.
///
/// `compact` is never forwarded: a compaction request for the Boss becomes
/// a durable safe-refresh request the reconciler settles as a rotation.
struct BossOwnershipDriver {
    inner: DriverHandle,
    session_id: Uuid,
    runtime_id: Uuid,
    boss: Arc<crate::boss::BossService>,
    task_state: Arc<Mutex<PersistedState>>,
    journal_path: PathBuf,
    continuity_path: PathBuf,
    #[cfg(test)]
    observer: Option<RotationObserver>,
    #[cfg(test)]
    next_operation: std::sync::atomic::AtomicU64,
}

impl BossOwnershipDriver {
    fn hold_prompt(&self, prompt: &str) {
        let result = (|| -> anyhow::Result<()> {
            use waku_boss::coordinator_contract::{CoordinatorBinding, CoordinatorPackage};
            use waku_boss::rotation_continuity::{ContinuityPackage, ContinuityScope};
            let document = self.boss.document();
            let generation = RotationJournal::load(&self.journal_path)?.generation;
            let scope = if self.boss.is_planning(self.session_id) {
                ContinuityScope::Planning
            } else {
                ContinuityScope::Coordinator
            };
            let (provider, model, event_ids) = {
                let state = self.task_state.lock();
                let session = state
                    .sessions
                    .iter()
                    .find(|s| s.id == self.session_id)
                    .ok_or_else(|| anyhow::anyhow!("missing coordinator session"))?;
                (
                    session.provider,
                    session.model.clone().unwrap_or_default(),
                    session
                        .queued_messages
                        .iter()
                        .map(|m| m.id)
                        .chain(session.messages.iter().map(|m| m.id))
                        .collect(),
                )
            };
            let identity_revision = self.boss.identity_digest();
            let package = CoordinatorPackage {
                binding: CoordinatorBinding {
                    provider,
                    session: self.session_id,
                    generation,
                    identity_revision: identity_revision.clone(),
                    grants_revision: serde_json::to_string(&document.resource_policy)?,
                    model,
                    scope,
                },
                content: ContinuityPackage {
                    identity_revision,
                    scope,
                    instructions: self.boss.canonical_boss_instructions(self.session_id)?,
                    context: prompt.into(),
                },
                event_ids,
                work_ids: document
                    .employees
                    .iter()
                    .map(|e| e.session_id)
                    .chain(document.planning.iter().map(|p| p.id))
                    .chain(document.outcomes.iter().map(|o| o.id))
                    .chain(document.deliverables.iter().map(|d| d.id))
                    .collect(),
                effect_ids: ContinuityLedger::load(&self.continuity_path)?
                    .coordinator_effects
                    .iter()
                    .filter(|e| e.resolution.is_none())
                    .map(|e| e.id.clone())
                    .collect(),
            };
            let id = format!(
                "coordinator-held:{}:{}",
                self.session_id,
                package.exact_content()?.digest()?
            );
            ContinuityLedger::update(&self.continuity_path, |ledger| {
                ledger.prepare_coordinator(&id, &package, crate::model::unix_time())
            })??;
            let provider = self
                .task_state
                .lock()
                .sessions
                .iter()
                .find(|s| s.id == self.session_id)
                .map(|s| s.provider)
                .ok_or_else(|| anyhow::anyhow!("missing provider"))?;
            let expected = waku_boss::provider_budget::StartupBudgetBinding {
                provider,
                runtime_version: String::new(),
                model: package.binding.model.clone(),
                configuration_revision: String::new(),
                instruction_revision: package.binding.identity_revision.clone(),
                runtime_id: self.runtime_id,
                generation,
                package_digest: package.exact_content()?.digest()?,
                operation_id: Uuid::new_v4(),
            };
            // Preparation is connected to the adapter seam, but absent trusted
            // effective observations stop before any submission in current adapters.
            crate::driver::deliver_coordinator_package(
                self.inner.coordinator_control(),
                &self.continuity_path,
                &id,
                &package,
                &package.binding,
                &expected,
            )
            .map(|_| ())
        })();
        if let Err(_blocker) = result {
            // Input remains in the durable transcript and/or prepared package.
            // No provider submission or acknowledgement is inferred.
        }
    }

    /// Whether this handle may admit context growth right now. Ownership
    /// reads the live pointer, not a value captured at wrap time — a
    /// mid-call rotation fences the predecessor immediately.
    fn owner_generation(&self) -> Option<u64> {
        let journal = RotationJournal::load(&self.journal_path).ok()?;
        if let Some(intent) = &journal.intent {
            if intent.new_session_id == self.session_id {
                return Some(intent.old_generation.saturating_add(1));
            }
            if intent.old_session_id == self.session_id {
                return Some(intent.old_generation);
            }
        }
        if self.boss.is_boss(self.session_id) {
            return Some(journal.generation);
        }
        journal
            .rotations
            .iter()
            .rev()
            .find_map(|rotation| {
                if rotation.new_session_id == self.session_id {
                    Some(rotation.old_generation.saturating_add(1))
                } else if rotation.old_session_id == self.session_id {
                    Some(rotation.old_generation)
                } else {
                    None
                }
            })
    }

    /// `true` while this session still holds coordination: it is the
    /// pointed Boss chat, unarchived, and unfenced by a compaction incident.
    /// A staged successor answers `false` until the pointer publishes it —
    /// no authority exists before ownership commits.
    fn admit(&self, boundary: &str) -> bool {
        let active = self.boss.is_boss(self.session_id);
        let archived = self.task_state.lock().sessions.iter()
            .find(|session| session.id == self.session_id)
            .is_none_or(|session| session.archived_at.is_some());
        let ledger = ContinuityLedger::load(&self.continuity_path);
        let incident = ledger
            .as_ref()
            .map(|ledger| ledger.has_incident(self.session_id))
            .unwrap_or(true);
        // Every context-growing DriverControl boundary revalidates. The current
        // adapters expose no trusted effective bounds; never reuse a startup
        // check or infer a continuation budget from reported occupancy.
        let generation = self.owner_generation();
        let expected = waku_boss::provider_budget::StartupBudgetBinding {
            provider: self
                .task_state
                .lock()
                .sessions
                .iter()
                .find(|s| s.id == self.session_id)
                .map(|s| s.provider)
                .unwrap_or(ProviderKind::Codex),
            runtime_version: String::new(),
            model: String::new(),
            configuration_revision: String::new(),
            instruction_revision: self.boss.identity_digest(),
            runtime_id: self.runtime_id,
            generation: generation.unwrap_or_default(),
            package_digest: String::new(),
            operation_id: Uuid::new_v4(),
        };
        let budget_valid = ledger.as_ref().is_ok_and(|ledger| {
            generation.is_some_and(|generation| {
                waku_boss::guarded_lifecycle::validate_growth(
                    ledger,
                    self.session_id,
                    generation,
                    &expected,
                    None,
                )
                .is_ok()
            })
        });
        // No current adapter has a proven policy or effective budget contract.
        // This also fences planning and handles created before qualification changed.
        let policy = self
            .inner
            .coordinator_control()
            .coordinator_policy_enforced();
        // Readiness must be independently bound and committed by a future lifecycle.
        // Adapter self-report alone cannot authorize existing handles.
        let readiness_committed = false;
        let allowed =
            active && !archived && !incident && policy && readiness_committed && budget_valid;
        if !allowed {
            eprintln!(
                "Boss driver fence denied {boundary} for session {} (active={active} archived={archived} incident={incident})",
                self.session_id,
            );
        }
        #[cfg(test)]
        if let Some(observer) = &self.observer {
            observer(RotationObservation::ProviderAdmission(ProviderAdmission {
                operation_id: format!(
                    "boss-driver:{}:{}:{boundary}:{}",
                    self.session_id,
                    self.runtime_id,
                    self.next_operation
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                ),
                session_id: self.session_id,
                runtime_id: self.runtime_id,
                owner_generation: self.owner_generation(),
                boundary: match boundary {
                    "prompt" => AdmissionBoundary::Request,
                    "rollback" | "fork" => AdmissionBoundary::Resume,
                    _ => AdmissionBoundary::Continuation,
                },
                decision: if allowed {
                    AdmissionDecision::Unguarded
                } else {
                    AdmissionDecision::Blocked { bounds: None }
                },
            }));
        }
        allowed
    }

    /// Report the pending bound delivery this prompt carried, for oracle
    /// transport evidence. Only the submission boundary records it.
    #[cfg(test)]
    fn observe_delivery_submitted(&self) {
        let Some(observer) = &self.observer else {
            return;
        };
        let Ok(ledger) = ContinuityLedger::load(&self.continuity_path) else {
            return;
        };
        let Some(delivery) = ledger.pending_for_session(self.session_id) else {
            return;
        };
        observer(RotationObservation::ProviderTransport(ProviderTransport {
            delivery_id: delivery.delivery_id.clone(),
            session_id: self.session_id,
            runtime_id: self.runtime_id,
            owner_generation: Some(delivery.generation),
            boundary: AdmissionBoundary::Request,
            status: ProviderTransportStatus::Submitted,
        }));
    }
}

impl crate::driver::DriverControl for BossOwnershipDriver {
    // Receipt queries do not admit context growth or authorize a resend.
    fn reconcile_coordinator_package(
        &self,
        attempt: &waku_boss::rotation_continuity::ContinuityAttempt,
    ) -> waku_boss::coordinator_contract::PackageOutcome {
        if attempt.session_id != self.session_id || attempt.runtime_id != self.runtime_id {
            return waku_boss::coordinator_contract::PackageOutcome::Unknown;
        }
        self.inner
            .coordinator_control()
            .reconcile_coordinator_package(attempt)
    }

    fn coordinator_startup_observation(
        &self,
    ) -> Option<waku_boss::provider_budget::StartupBudgetEvidence> {
        self.inner
            .coordinator_control()
            .coordinator_startup_observation()
    }

    fn coordinator_policy_enforced(&self) -> bool {
        self.inner
            .coordinator_control()
            .coordinator_policy_enforced()
    }

    fn prompt(&self, prompt: String) {
        if !self.admit("prompt") {
            self.hold_prompt(&prompt);
            return;
        }
        self.inner.prompt(prompt);
        #[cfg(test)]
        self.observe_delivery_submitted();
    }

    fn prompt_with_attachments(
        &self,
        prompt: String,
        attachments: Vec<crate::model::MessageAttachment>,
    ) {
        if !self.admit("prompt") {
            self.hold_prompt(&prompt);
            return;
        }
        self.inner.prompt_with_attachments(prompt, attachments);
        #[cfg(test)]
        self.observe_delivery_submitted();
    }

    fn supports_steer(&self) -> bool {
        self.inner.supports_steer()
    }

    fn agent_surface_delivery(&self) -> crate::driver::AgentSurfaceDelivery {
        self.inner.agent_surface_delivery()
    }

    fn steer(&self, prompt: String) {
        if self.admit("steer") {
            self.inner.steer(prompt);
        } else {
            self.hold_prompt(&prompt);
        }
    }

    fn cancel(&self) {
        // Cancellation is a deterministic human control — always allowed.
        self.inner.cancel();
    }

    fn begin_shutdown(&self) {
        self.inner.begin_shutdown();
    }

    fn cancel_computer_use(&self) {
        self.inner.cancel_computer_use();
    }

    fn computer_use_available(&self) -> bool {
        false
    }

    fn refresh_background_work(&self) {
        // Unqualified provider discovery may trigger autonomous activity.
    }

    fn stop_background_work(&self, key: crate::model::BackgroundWorkKey, control_id: String) {
        self.inner.stop_background_work(key, control_id);
    }

    fn respond(&self, request_id: String, option_id: String) {
        if self.admit("respond") {
            self.inner.respond(request_id, option_id);
        }
    }

    fn respond_user_input(&self, request_id: String, answers: Vec<crate::model::UserInputAnswer>) {
        if self.admit("respond_user_input") {
            self.inner.respond_user_input(request_id, answers);
        }
    }

    fn supports_user_input_actions(&self) -> bool {
        self.inner.supports_user_input_actions()
    }

    fn clarify_user_input(&self, request_id: String, content: String) {
        if self.admit("clarify_user_input") {
            self.inner.clarify_user_input(request_id, content);
        }
    }

    fn cancel_user_input(&self, request_id: String) {
        self.inner.cancel_user_input(request_id);
    }

    fn goal(&self, operation: crate::model::GoalOperation) {
        if self.admit("goal") {
            self.inner.goal(operation);
        }
    }

    fn compact(&self) {
        // Provider compaction is never invoked for the Boss: a manual
        // compact becomes a durable safe-refresh request the reconciler
        // settles as a fresh session. This is the fail-closed path for
        // every provider, qualified or not.
        if !self.boss.is_boss(self.session_id) {
            return;
        }
        if let Err(error) = ContinuityLedger::update(&self.continuity_path, |ledger| {
            ledger.request_rotation(
                crate::model::unix_time(),
                "manual compact request translated to a safe session refresh",
            );
        }) {
            eprintln!("could not record the Boss refresh request: {error:#}");
            return;
        }
        eprintln!(
            "Boss compaction request on session {} translated to a rotation request",
            self.session_id
        );
    }

    fn delete_provider_session(&self) {
        self.inner.delete_provider_session();
    }

    fn run_computer_tool(&self, request: crate::computer_use::ComputerToolRequest) {
        // The coordinator never holds a computer-use grant — direct host
        // actions are employee work. Answer the request with a denial so
        // the kernel does not hang on it.
        self.inner.reject_computer_tool(
            request,
            "the boss delegates computer use to granted employees".into(),
        );
    }

    fn reject_computer_tool(
        &self,
        request: crate::computer_use::ComputerToolRequest,
        reason: String,
    ) {
        self.inner.reject_computer_tool(request, reason);
    }

    fn apply_options(&self, options: crate::driver::SessionOptions) -> bool {
        let _ = options;
        false
    }

    fn rollback(&self, turns: usize) -> anyhow::Result<Option<crate::model::ProviderResumeCursor>> {
        anyhow::ensure!(self.admit("rollback"), "Boss session is fenced");
        self.inner.rollback(turns)
    }

    fn fork(&self, turns_to_remove: usize) -> anyhow::Result<crate::model::ProviderResumeCursor> {
        anyhow::ensure!(self.admit("fork"), "Boss session is fenced");
        self.inner.fork(turns_to_remove)
    }
}

/// Whether the Boss chat may hand coordination to a successor right now.
/// `queued_work_carried` is `true` when a compaction incident fences the
/// session: the fence itself holds the parked queue, so it can never
/// drain — the committed swap re-homes it onto the successor instead of
/// waiting for it to empty.
fn boss_chat_settled(
    session: &AgentSession,
    agent: &crate::agent::AgentState,
    queued_work_carried: bool,
) -> bool {
    session.archived_at.is_none()
        && !session.status.is_busy()
        && session.active_turn_id().is_none()
        && (queued_work_carried || session.queued_messages.is_empty())
        && !agent.has_open_turn(session.id)
        && (queued_work_carried || !agent.has_queued(session.id))
}

/// Bound the fresh context independently of the old transcript's size. The
/// previous task remains the authoritative source for details and tool output.
fn boss_rotation_handoff(old: &AgentSession, marker: &str) -> String {
    let mut recent = Vec::new();
    // Durable pins and the current work brief carry continuity. This tail
    // only orients the conversation; detailed history is pulled on demand.
    let mut remaining = 2_000;
    for message in old.messages.iter().rev().filter(|message| !message.hidden) {
        if remaining == 0 {
            break;
        }
        let text: String = message.content.chars().take(remaining.min(1_000)).collect();
        remaining -= text.chars().count();
        recent.push(format!("{:?}: {text}", message.role));
        if recent.len() == 2 {
            break;
        }
    }
    recent.reverse();
    format!(
        "<boss-session-rotation>\n{marker}\nContinue supervising the existing work. Your identity, employees, plans and memory are unchanged. Read the previous transcript with `goddard-agent boss transcript {}` when needed. Recent conversation (historical context):\n{}\n</boss-session-rotation>",
        old.id,
        recent.join("\n\n"),
    )
}

impl WakuBackend {
    /// Stage canonical authority and enumerable work before any provider launch.
    /// Current adapters cannot supply trusted prelaunch observations; keep the
    /// package durable and return a blocker before startup/resume context grows.
    pub(super) fn prepare_coordinator_startup(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        model: Option<&str>,
    ) -> anyhow::Result<()> {
        use waku_boss::coordinator_contract::{CoordinatorBinding, CoordinatorPackage};
        use waku_boss::provider_budget::{StartupBudgetBinding, validate_startup_budget};
        use waku_boss::rotation_continuity::{ContinuityPackage, ContinuityScope};
        let journal = RotationJournal::load(&self.boss_journal_path())?;
        let document = self.boss.document();
        let scope = if self.boss.is_planning(session_id) {
            ContinuityScope::Planning
        } else {
            ContinuityScope::Coordinator
        };
        let identity_revision = self.boss.identity_digest();
        let grants_revision = serde_json::to_string(&document.resource_policy)?;
        let binding = CoordinatorBinding {
            provider,
            session: session_id,
            generation: journal.generation,
            identity_revision: identity_revision.clone(),
            grants_revision,
            model: model.unwrap_or_default().into(),
            scope,
        };
        let (event_ids, context) = {
            let state = self.task_state.lock();
            let session = state
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .ok_or_else(|| anyhow::anyhow!("coordinator session unavailable"))?;
            (
                session
                    .queued_messages
                    .iter()
                    .map(|message| message.id)
                    .collect(),
                session.pending_provider_context.clone().unwrap_or_default(),
            )
        };
        let instructions = self.boss.canonical_boss_instructions(session_id)?;
        let package = CoordinatorPackage {
            binding,
            content: ContinuityPackage {
                identity_revision: identity_revision.clone(),
                scope,
                instructions,
                context,
            },
            event_ids,
            // Employee identities enumerate work independently of a bounded digest.
            work_ids: document
                .employees
                .iter()
                .map(|e| e.session_id)
                .chain(document.planning.iter().map(|p| p.id))
                .chain(document.outcomes.iter().map(|o| o.id))
                .chain(document.deliverables.iter().map(|d| d.id))
                .collect(),
            effect_ids: ContinuityLedger::load(&self.boss_continuity_path())?
                .coordinator_effects
                .iter()
                .filter(|e| e.resolution.is_none())
                .map(|e| e.id.clone())
                .collect(),
        };
        let digest = package.exact_content()?.digest()?;
        let id = format!("coordinator-startup:{session_id}:{digest}");
        // Model absence blocks admission, but must not discard staged content.
        ContinuityLedger::update(&self.boss_continuity_path(), |ledger| {
            ledger.prepare_coordinator(&id, &package, crate::model::unix_time())
        })??;
        let expected = StartupBudgetBinding {
            provider,
            runtime_version: String::new(),
            model: package.binding.model,
            configuration_revision: String::new(),
            instruction_revision: identity_revision,
            runtime_id,
            generation: journal.generation,
            package_digest: digest,
            operation_id: Uuid::new_v4(),
        };
        // No PATH/config-file inference and no copying evidence into expectations.
        let result = validate_startup_budget(&expected, None);
        anyhow::bail!(
            "Boss coordination paused: startup contract unavailable ({:?}); continuity preserved",
            result.err()
        );
    }
}
