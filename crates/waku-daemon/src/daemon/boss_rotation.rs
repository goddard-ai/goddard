use super::*;
use crate::boss_rotation::{BossRotationConfig, RotationJournal};
use crate::model::MessageRole;
use waku_boss::rotation_continuity::{ContinuityDeliveryKind, ContinuityLedger};
#[cfg(test)]
use waku_boss::rotation_oracle::{
    AdmissionBoundary, AdmissionDecision, CompactionStage, DurableAction, DurableActionKind,
    DurableActionStatus, LifecycleObservation, OwnershipObservation, OwnershipState,
    ProviderAdmission, ProviderCompaction, ProviderTransport, ProviderTransportStatus,
    RotationLifecycleStage, RotationObservation,
};

/// The logical identifier of the bound handoff a successor must receive.
/// Retries reconcile onto the same delivery rather than duplicating it.
fn handoff_delivery_id(session_id: Uuid) -> String {
    format!("boss-handoff:{session_id}")
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
                journal
                    .intent
                    .as_ref()
                    .is_some_and(|intent| intent.old_session_id == session_id)
                    || journal
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
        ContinuityLedger::load(&self.boss_continuity_path())
            .map(|ledger| ledger.has_incident(session_id))
            .unwrap_or(false)
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

    /// The installed provider's prevention-contract evidence for the active
    /// Boss configuration. Unknown or undetected versions stay unqualified —
    /// the lifecycle may rotate for safety but must never claim prevention.
    pub(super) fn boss_provider_qualification(
        &self,
        provider: ProviderKind,
    ) -> waku_boss::rotation_continuity::ProviderQualification {
        let version = crate::model::provider_probe(provider, None)
            .path
            .as_deref()
            .and_then(crate::model::probe_provider_version);
        waku_boss::rotation_continuity::qualify_provider(provider, version.as_deref())
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
        if self.boss.is_boss(session_id) || self.boss_rotation_names(session_id) {
            #[cfg(test)]
            if let Some(ownership) = self.boss_rotation_ownership(session_id) {
                self.record_rotation_observation(RotationObservation::Ownership(ownership));
            }
            return DriverHandle::from_control(Arc::new(BossOwnershipDriver {
                inner: driver,
                session_id,
                #[cfg(test)]
                runtime_id,
                boss: self.boss.clone(),
                task_state: self.task_state.clone(),
                #[cfg(test)]
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

    /// The Boss scheduler checks settled chats, including after restart.
    /// Fresh chats use the same durable initialization and lazy provider
    /// launch as BossOperation::Open.
    pub(super) fn reconcile_boss_rotation(&self, now: u64) -> anyhow::Result<()> {
        let settings = self.settings.get();
        if !settings.boss_experiment_enabled || !self.boss.is_active() {
            return Ok(());
        }
        self.boss
            .with_operation_lock(|| self.rotate_boss_chat(&settings, now))
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

    fn rotate_boss_chat(&self, settings: &crate::DaemonSettings, now: u64) -> anyhow::Result<()> {
        let (identity, active) = self.boss.identity_and_session();
        let Some(active) = active else {
            return Ok(());
        };
        let path = self.boss_journal_path();
        let continuity_path = self.boss_continuity_path();
        let mut journal = RotationJournal::load(&path)?;
        let mut ledger = ContinuityLedger::load(&continuity_path)?;
        // A requested refresh (manual compact or a compaction incident)
        // triggers independently of the occupancy opt-out — both are
        // prevention requests, not threshold preferences.
        let requested = ledger.rotation_request.as_ref().map(|request| {
            if ledger.has_incident(active) {
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
            let Some(old) = state.session_mut(active) else {
                return Ok(());
            };
            self.task_store.hydrate(old)?;
            if !boss_chat_settled(old, &self.agent) {
                return Ok(());
            }
            let reason = if let Some(reason) = requested.clone() {
                reason
            } else {
                let Some(usage) = old.context_usage else {
                    return Ok(());
                };
                let policy = BossRotationConfig::from_settings(settings);
                if !policy.should_rotate(usage.tokens, usage.window) {
                    return Ok(());
                }
                eprintln!(
                    "Boss session rotation: boss_id={} old_session_id={} context_threshold={} context_tokens={} context_window={}",
                    identity.id,
                    active,
                    policy.context_threshold,
                    usage.tokens,
                    usage.window.unwrap_or_default(),
                );
                format!(
                    "boss_rotation_context_threshold={} exceeded (context_tokens={}, context_window={})",
                    policy.context_threshold,
                    usage.tokens,
                    usage.window.unwrap_or_default(),
                )
            };
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
        let marker = "Boss session rotated.";
        let predecessor = format!("<boss-rotation-link:{}>", intent.old_session_id);
        if active == intent.old_session_id {
            let old = state
                .session_mut(active)
                .ok_or_else(|| anyhow!("rotation source session is missing"))?;
            self.task_store.hydrate(old)?;
            // Hold the task lease through publication: prompt recording
            // cannot open a turn between this check and the archive flag.
            if !boss_chat_settled(old, &self.agent) {
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
                next.pending_provider_context = Some(boss_rotation_handoff(old, marker));
                next.push_message(MessageRole::System, marker);
                next.push_message(MessageRole::System, &predecessor);
                next.messages
                    .last_mut()
                    .expect("rotation link was appended")
                    .hidden = true;
                state.push_session(next);
                // Stage the initialized chat before publishing its id.
                self.task_store.save(&mut state)?;
            }
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
            // The bound identity/continuity package is prepared — it stays
            // pending until provider transport acceptance is recorded.
            {
                let delivery_id = handoff_delivery_id(intent.new_session_id);
                ledger.prepare(
                    &delivery_id,
                    intent.new_session_id,
                    intent.old_generation.saturating_add(1),
                    ContinuityDeliveryKind::SessionHandoff,
                    now,
                );
                if let Err(error) = ledger.persist(&continuity_path) {
                    eprintln!("could not record Boss continuity delivery: {error:#}");
                }
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
                        intent.provider.as_deref().is_none_or(|provider| {
                            provider == format!("{:?}", staged.provider)
                        }) && intent
                            .model
                            .as_deref()
                            .is_none_or(|model| model == staged.model.as_deref().unwrap_or(""))
                    });
                if !identity_current || !config_current {
                    eprintln!(
                        "Boss session rotation aborted: identity or configuration changed during staging (boss={} old_session_id={} new_session_id={})",
                        identity.id, intent.old_session_id, intent.new_session_id,
                    );
                    #[cfg(test)]
                    {
                        self.observe_rotation_lifecycle(&intent, RotationLifecycleStage::Failed);
                    }
                    state
                        .sessions
                        .retain(|session| session.id != intent.new_session_id);
                    self.task_store.save(&mut state)?;
                    // The staged delivery belongs to the discarded successor —
                    // drop it so the ledger holds no pending package for a
                    // session that never existed.
                    ledger
                        .deliveries
                        .retain(|delivery| delivery.session_id != intent.new_session_id);
                    if let Err(error) = ledger.persist(&continuity_path) {
                        eprintln!("could not unwind the Boss continuity record: {error:#}");
                    }
                    journal.abort();
                    journal.persist(&path)?;
                    return Ok(());
                }
            }
            self.boss
                .replace_session_id(active, intent.new_session_id)?;
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
        if !next
            .messages
            .iter()
            .any(|message| message.role == MessageRole::System && message.content == marker)
        {
            next.push_message(MessageRole::System, marker);
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
        if !old
            .messages
            .iter()
            .any(|message| message.role == MessageRole::System && message.content == marker)
        {
            old.push_message(MessageRole::System, marker);
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
        self.task_store.save(&mut state)?;
        drop(state);
        // Same runtime/credential retirement as ordinary archival. The
        // transcript stays in the task store under its old session id.
        self.retire_archived_runtimes(&[intent.old_session_id]);
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
        journal.persist(&path)?;
        // The refresh request — manual compact or incident — is consumed
        // by the committed swap. Incidents stay as evidence.
        if ledger.rotation_request.is_some() {
            ledger.resolve_rotation_request();
            if let Err(error) = ledger.persist(&continuity_path) {
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
        self.boss.router_defer_context(intent.new_session_id);
        if let Some(notifier) = self.task_notifier.lock().clone() {
            notifier();
        }
        Ok(())
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
    #[cfg(test)]
    runtime_id: Uuid,
    boss: Arc<crate::boss::BossService>,
    task_state: Arc<Mutex<PersistedState>>,
    #[cfg(test)]
    journal_path: PathBuf,
    continuity_path: PathBuf,
    #[cfg(test)]
    observer: Option<RotationObserver>,
    #[cfg(test)]
    next_operation: std::sync::atomic::AtomicU64,
}

impl BossOwnershipDriver {
    /// Whether this handle may admit context growth right now. Ownership
    /// reads the live pointer, not a value captured at wrap time — a
    /// mid-call rotation fences the predecessor immediately.
    #[cfg(test)]
    fn owner_generation(&self) -> Option<u64> {
        let journal = RotationJournal::load(&self.journal_path).unwrap_or_default();
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
        let archived = self.task_state.lock().sessions.iter().any(|session| {
            session.id == self.session_id && session.archived_at.is_some()
        });
        let incident = ContinuityLedger::load(&self.continuity_path)
            .map(|ledger| ledger.has_incident(self.session_id))
            .unwrap_or(false);
        let allowed = active && !archived && !incident;
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
    fn prompt(&self, prompt: String) {
        if !self.admit("prompt") {
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
        if self.admit("respond") {
            self.inner.respond(request_id, option_id);
        }
    }

    fn respond_user_input(
        &self,
        request_id: String,
        answers: Vec<crate::model::UserInputAnswer>,
    ) {
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
        if let Ok(mut ledger) = ContinuityLedger::load(&self.continuity_path) {
            ledger.request_rotation(
                crate::model::unix_time(),
                "manual compact request translated to a safe session refresh",
            );
            if let Err(error) = ledger.persist(&self.continuity_path) {
                eprintln!("could not record the Boss refresh request: {error:#}");
            }
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
        self.inner.apply_options(options)
    }

    fn rollback(
        &self,
        turns: usize,
    ) -> anyhow::Result<Option<crate::model::ProviderResumeCursor>> {
        anyhow::ensure!(self.admit("rollback"), "Boss session is fenced");
        self.inner.rollback(turns)
    }

    fn fork(
        &self,
        turns_to_remove: usize,
    ) -> anyhow::Result<crate::model::ProviderResumeCursor> {
        anyhow::ensure!(self.admit("fork"), "Boss session is fenced");
        self.inner.fork(turns_to_remove)
    }
}

fn boss_chat_settled(session: &AgentSession, agent: &crate::agent::AgentState) -> bool {
    session.archived_at.is_none()
        && !session.status.is_busy()
        && session.active_turn_id().is_none()
        && session.queued_messages.is_empty()
        && !agent.has_open_turn(session.id)
        && !agent.has_queued(session.id)
}

/// Bound the fresh context independently of the old transcript's size. The
/// previous task remains the authoritative source for details and tool output.
fn boss_rotation_handoff(old: &AgentSession, marker: &str) -> String {
    let mut recent = Vec::new();
    let mut remaining = 12_000;
    for message in old.messages.iter().rev().filter(|message| !message.hidden) {
        if remaining == 0 {
            break;
        }
        let text: String = message.content.chars().take(remaining.min(4_000)).collect();
        remaining -= text.chars().count();
        recent.push(format!("{:?}: {text}", message.role));
        if recent.len() == 6 {
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
