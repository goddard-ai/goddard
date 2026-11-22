use super::*;
use waku_boss::counsel::counsel_identity_seed;
use waku_protocol::boss::{BossOperation, BossResult};
use waku_protocol::counsel::{
    COUNSEL_REVIEW_PROMPT, CounselDecision, CounselOperation, CounselRecommendation, CounselSource,
    CounselState, CounselStatus,
};

// Older daemons parked this exact text in the durable queue.
const LEGACY_RECOMMEND: &str = "Your last answer settled the human's question. Now act on your own initiative: review my recent sessions and current priorities, including recently archived work. Offer the clearest useful next action — propose it with `goddard-agent boss counsel propose` when it is actionable — or say there is no clear next step. Respect my previous Later and Dismiss choices, and check results of instructions already sent to Boss.";

/// After Jev judges an answer settled, a hidden follow-up reviews recent work.
const RECOMMEND: &str = "Your last answer settled the human's question. Review my recent sessions and current priorities, including recently archived work. Offer the clearest useful next action, using a recommendation card if it would help, or say there is no clear next step. Respect my previous Later and Dismiss choices, and check results of instructions already sent to Boss.";

const FEATURE: &str = "counsel-recommendation";

pub(super) const DIRECT_HANDOFF_PREFIX: &str =
    "Direct instruction from the human, relayed by Counsel:\n\n";

pub(super) fn is_automatic_counsel_prompt(entry: &crate::agent::AgentPrompt) -> bool {
    entry.hidden
        && entry.sender.is_none()
        && matches!(
            entry.prompt.as_str(),
            COUNSEL_REVIEW_PROMPT | RECOMMEND | LEGACY_RECOMMEND
        )
}

impl WakuBackend {
    /// Called under the Boss operation lock; both halves commit together on
    /// SQLite stores. The caller publishes notifications after releasing state.
    fn save_counsel_sessions(
        &self,
        state: &mut PersistedState,
        expected: u64,
        counsel: CounselState,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.boss.uses_database(),
            "Counsel requires transactional daemon storage"
        );
        let batch = self.task_store.save_batch(state);
        let mut host = self.task_store.session_commit(&batch);
        self.boss.save_counsel(expected, counsel, Some(&mut host))?;
        state.unmark_sessions_dirty(&batch.dirty_ids);
        Ok(())
    }

    /// Counsel rides on the Boss experiment plus its own opt-in: both flags
    /// must be on before a Counsel session launches or its operations run.
    /// Identity-only checks keep `boss.is_counsel` — they preserve records
    /// across toggles rather than admitting work.
    pub(super) fn counsel_enabled(&self) -> bool {
        let settings = self.settings.get();
        settings.boss_experiment_enabled && settings.counsel_experiment_enabled
    }

    pub(super) fn handle_counsel_operation(
        &self,
        caller: Option<Uuid>,
        action: CounselOperation,
        events: &EventSink,
    ) -> anyhow::Result<BossResult> {
        anyhow::ensure!(
            self.counsel_enabled(),
            "Counsel is disabled by the experiment setting"
        );
        match action {
            CounselOperation::Open {
                provider,
                model,
                mode,
            } => {
                anyhow::ensure!(caller.is_none(), "only the human can open Counsel");
                let (session, project) = self.boss.with_operation_lock(|| {
                    let document = self.boss.document();
                    let mut counsel = document.counsel.clone();
                    let expected = counsel.revision;
                    let mut project = Project::from_path(self.boss.owned_workspace()?);
                    project.id = document.identity.id;
                    project.name = "Boss".into();
                    let mut state = self.task_state.lock();
                    if !state.projects.iter().any(|entry| entry.id == project.id) {
                        state.projects.push(project.clone());
                    }
                    let existing = counsel
                        .session_id
                        .and_then(|id| state.sessions.iter().position(|session| session.id == id));
                    let created = existing.is_none();
                    let index = existing.unwrap_or_else(|| {
                        let mut session = AgentSession::new(project.id, provider);
                        session.title = "Counsel".into();
                        session.model = model;
                        session.runtime_mode = mode;
                        session.agent_rename_allowed = false;
                        session.boss_managed = true;
                        counsel.session_id = Some(session.id);
                        state.push_session(session);
                        state.sessions.len() - 1
                    });
                    let id = state.sessions[index].id;
                    self.task_store.hydrate(&mut state.sessions[index])?;
                    // The managed identity predates documents without one:
                    // seed lazily so an existing chat upgrades on next open.
                    let seeded_identity = counsel.identity.is_none();
                    if seeded_identity {
                        let mut identity = counsel_identity_seed(&document);
                        identity.avatar_style = document.identity.avatar_style;
                        counsel.identity = Some(identity);
                    }
                    if created || seeded_identity || state.sessions[index].archived_at.is_some() {
                        let previous = state.sessions[index].clone();
                        state.sessions[index].archived_at = None;
                        state.mark_session_dirty(id);
                        if let Err(error) =
                            self.save_counsel_sessions(&mut state, expected, counsel)
                        {
                            if created {
                                state.sessions.remove(index);
                                state.unmark_sessions_dirty(&HashSet::from([id]));
                            } else {
                                state.sessions[index] = previous;
                            }
                            return Err(error);
                        }
                    }
                    Ok::<_, anyhow::Error>((state.sessions[index].clone(), project))
                })?;
                self.boss.notify_session_commit();
                Ok(BossResult::Session {
                    session: Box::new(session),
                    project: Box::new(project),
                })
            }
            CounselOperation::View => {
                self.require_counsel_reader(caller)?;
                Ok(BossResult::Counsel {
                    state: self.boss.document().counsel,
                })
            }
            CounselOperation::Propose {
                title,
                reason,
                instruction,
                sources,
                replaces,
            } => {
                self.require_counsel_reader(caller)?;
                for (label, text, cap) in [
                    ("title", &title, 160),
                    ("reason", &reason, 2000),
                    ("instruction", &instruction, 6000),
                ] {
                    anyhow::ensure!(
                        !text.trim().is_empty() && text.chars().count() <= cap,
                        "Counsel {label} must contain 1–{cap} characters"
                    );
                }
                anyhow::ensure!(
                    !sources.is_empty() && sources.len() <= 5,
                    "provide one to five source sessions"
                );
                anyhow::ensure!(
                    sources.iter().copied().collect::<HashSet<_>>().len() == sources.len(),
                    "Counsel source sessions must be distinct"
                );
                self.boss.with_operation_lock(|| {
                    let mut counsel = self.boss.document().counsel;
                    let expected = counsel.revision;
                    let mut state = self.task_state.lock();
                    // The card anchors after Counsel's latest message — its
                    // originating position in history.
                    let anchor = counsel
                        .session_id
                        .and_then(|id| state.sessions.iter_mut().find(|s| s.id == id))
                        .map(|session| {
                            self.task_store.hydrate(session)?;
                            Ok::<_, std::io::Error>(session.messages.last().map(|message| message.id))
                        })
                        .transpose()?
                        .flatten();
                    let sources: Vec<_> = sources.iter().map(|id| {
                        anyhow::ensure!(Some(*id) != counsel.session_id, "Counsel's own suggestions are not source evidence");
                        let session = state.sessions.iter().find(|session| session.id == *id)
                            .ok_or_else(|| anyhow!("unknown Counsel source session {id}"))?;
                        Ok(CounselSource { session_id: *id, updated_at: session.updated_at })
                    }).collect::<anyhow::Result<_>>()?;
                    drop(state);
                    // Identical retries and dismissed advice are stable until
                    // the source evidence actually changes.
                    if replaces.is_none() && counsel.recommendations.iter().any(|record| record.instruction == instruction.trim() && record.sources == sources) {
                        return Ok(BossResult::Counsel { state: counsel });
                    }
                    let now = crate::model::unix_time();
                    if let Some(id) = replaces {
                        let previous = counsel.recommendations.iter_mut().find(|record| record.id == id)
                            .ok_or_else(|| anyhow!("unknown recommendation to revise"))?;
                        anyhow::ensure!(matches!(previous.status, CounselStatus::Proposed | CounselStatus::Later), "only pending recommendations can be revised");
                        previous.status = CounselStatus::Dismissed;
                        previous.updated_at = now;
                        previous.revision += 1;
                    }
                    anyhow::ensure!(counsel.recommendations.iter().filter(|record| record.status == CounselStatus::Proposed).count() < 3, "Counsel already has three pending recommendations; revise or resolve one first");
                    counsel.recommendations.push(CounselRecommendation {
                        id: Uuid::new_v4(), revision: 1, title: title.trim().into(),
                        reason: reason.trim().into(), instruction: instruction.trim().into(), sources,
                        status: CounselStatus::Proposed, created_at: now, updated_at: now,
                        boss_session_id: None, completion_evidence: None,
                        anchor, delivery_error: None, approved_revision: None,
                    });
                    self.boss.save_counsel(expected, counsel, None)?;
                    Ok(BossResult::Counsel { state: self.boss.document().counsel })
                })
            }
            CounselOperation::Decide {
                id,
                expected_revision,
                decision,
            } => {
                anyhow::ensure!(
                    caller.is_none(),
                    "only the human can approve, defer, or dismiss Counsel recommendations"
                );
                // Open may acquire the operation lock itself. Do it before
                // the decision transaction, then revalidate its revision.
                let boss_session = if decision == CounselDecision::Approve {
                    let document = self.boss.document();
                    let record = document
                        .counsel
                        .recommendations
                        .iter()
                        .find(|record| record.id == id)
                        .ok_or_else(|| anyhow!("unknown Counsel recommendation"))?;
                    anyhow::ensure!(
                        record.revision == expected_revision
                            && matches!(
                                record.status,
                                CounselStatus::Proposed | CounselStatus::Later
                            ),
                        "recommendation changed or was resolved; refresh before deciding"
                    );
                    let counsel_id = document
                        .counsel
                        .session_id
                        .ok_or_else(|| anyhow!("open Counsel first"))?;
                    let source = self
                        .task_state
                        .lock()
                        .sessions
                        .iter()
                        .find(|session| session.id == counsel_id)
                        .cloned()
                        .ok_or_else(|| anyhow!("Counsel chat is unavailable"))?;
                    let BossResult::Session { session, .. } = self.handle_boss_operation(
                        None,
                        BossOperation::Open {
                            provider: source.provider,
                            model: source.model,
                            mode: source.runtime_mode,
                        },
                        events,
                    )?
                    else {
                        bail!("Boss chat is unavailable")
                    };
                    Some(session.id)
                } else {
                    None
                };
                let (counsel, resume) = self.boss.with_operation_lock(|| {
                    let document = self.boss.document();
                    // A rotation may have committed after Open returned. The
                    // approval resume binds to Counsel only; delivery resolves
                    // the live boss inside Send's own lock.
                    let _ = boss_session;
                    let mut counsel = document.counsel;
                    let expected = counsel.revision;
                    let record = counsel.recommendations.iter_mut().find(|record| record.id == id)
                        .ok_or_else(|| anyhow!("unknown Counsel recommendation"))?;
                    anyhow::ensure!(record.revision == expected_revision, "recommendation changed; refresh before deciding");
                    anyhow::ensure!(matches!(record.status, CounselStatus::Proposed | CounselStatus::Later), "recommendation has already been resolved");
                    match decision {
                        CounselDecision::Approve => {
                            // Bind approval to the exact displayed revision;
                            // the instruction is immutable per record, so the
                            // revision carries the message binding too.
                            record.approved_revision = Some(expected_revision);
                            record.status = CounselStatus::Approved;
                        }
                        CounselDecision::Later => record.status = CounselStatus::Later,
                        CounselDecision::Dismiss => record.status = CounselStatus::Dismissed,
                    }
                    record.revision += 1;
                    record.updated_at = crate::model::unix_time();
                    let counsel_id = counsel.session_id.ok_or_else(|| anyhow!("Counsel chat is unavailable"))?;
                    // Approval never sends. Counsel resumes with a bounded
                    // instruction naming the record and its approved revision.
                    let resume = (decision == CounselDecision::Approve).then(|| {
                        let mut message = crate::model::QueuedMessage::agent(
                            format!(
                                "The human approved recommendation {id} \"{}\" at revision {expected_revision}. Deliver its approved message to Boss now with `goddard-agent boss counsel send {id} --revision {expected_revision}` — the instruction is fixed by the approval; do not reword it, send it to anyone else, or send anything else.",
                                record.title
                            ),
                            None,
                        );
                        message.hidden = true;
                        message
                    });
                    let mut state = self.task_state.lock();
                    let mut previous = Vec::new();
                    if let Some(session) = state.sessions.iter_mut().find(|session| session.id == counsel_id) {
                        self.task_store.hydrate(session)?;
                        previous.push((counsel_id, session.clone()));
                        if let Some(message) = &resume {
                            session.queued_messages.push(message.clone());
                        }
                        let decision_text = match decision {
                            CounselDecision::Approve => {
                                format!("Approved — delivering to Boss through Counsel: {}", record.title)
                            }
                            CounselDecision::Later => format!("Saved for later: {}", record.title),
                            CounselDecision::Dismiss => format!("Dismissed: {}", record.title),
                        };
                        session.push_message(crate::model::MessageRole::Assistant, &decision_text);
                        state.mark_session_dirty(counsel_id);
                    }
                    if let Err(error) = self.save_counsel_sessions(&mut state, expected, counsel) {
                        for (id, old) in previous {
                            if let Some(session) = state.sessions.iter_mut().find(|session| session.id == id) { *session = old; }
                        }
                        return Err(error);
                    }
                    Ok((self.boss.document().counsel, resume))
                })?;
                self.boss.notify_session_commit();
                if let Some(message) = resume {
                    let counsel_id = counsel.session_id.ok_or_else(|| anyhow!("Counsel chat is unavailable"))?;
                    if self.agent.user_stopped(counsel_id) {
                        // A stopped Counsel keeps the parked instruction — the
                        // approval is durable and resumes with the chat.
                        return Ok(BossResult::Counsel { state: counsel });
                    }
                    if let Err(error) = self.queue_agent_message(counsel_id, message, events) {
                        eprintln!(
                            "Approved Counsel recommendation remains parked for Counsel: {error:#}"
                        );
                    }
                }
                Ok(BossResult::Counsel { state: counsel })
            }
            CounselOperation::Send {
                id,
                expected_revision,
            } => self.send_counsel_recommendation(caller, id, expected_revision, events),
            CounselOperation::Handoff { instruction } => {
                let counsel =
                    caller.ok_or_else(|| anyhow!("only Counsel can hand off an instruction"))?;
                let (provider, model, mode) = self.require_counsel_human_turn(counsel)?;
                anyhow::ensure!(
                    !instruction.trim().is_empty(),
                    "Boss handoff requires an instruction"
                );
                if self.boss.document().session_id.is_none() {
                    self.handle_boss_operation(
                        None,
                        BossOperation::Open {
                            provider,
                            model,
                            mode,
                        },
                        events,
                    )?;
                }
                let prompt = format!("{DIRECT_HANDOFF_PREFIX}{instruction}");
                let message = crate::model::QueuedMessage::agent(&prompt, Some(counsel));
                let boss = self.boss.with_operation_lock(|| {
                    let boss = self
                        .boss
                        .document()
                        .session_id
                        .ok_or_else(|| anyhow!("Boss chat is unavailable"))?;
                    anyhow::ensure!(
                        !self.boss_incident_fenced(boss) && !self.session_quarantined(boss),
                        "Boss is not accepting instructions right now"
                    );
                    mirror_agent_queued_message(
                        &self.task_state,
                        &self.task_store,
                        boss,
                        message.clone(),
                    )?;
                    Ok::<_, anyhow::Error>(boss)
                })?;
                self.deliver_counsel_instruction(boss, counsel, message.id, prompt, events)
                    .context("could not deliver the handoff to Boss; it remains queued")?;
                Ok(BossResult::Saved)
            }
            CounselOperation::Rename { name } => {
                anyhow::ensure!(caller.is_none(), "only the human can rename Counsel");
                anyhow::ensure!(
                    !name.trim().is_empty() && name.chars().count() <= 100,
                    "name must contain 1–100 characters"
                );
                self.boss.with_operation_lock(|| {
                    let document = self.boss.document();
                    let mut counsel = document.counsel.clone();
                    let expected = counsel.revision;
                    let identity = counsel
                        .identity
                        .get_or_insert_with(|| counsel_identity_seed(&document));
                    identity.name = name.trim().into();
                    self.boss.save_counsel(expected, counsel, None)?;
                    Ok(BossResult::Counsel {
                        state: self.boss.document().counsel,
                    })
                })
            }
            CounselOperation::RegenerateAvatar => {
                anyhow::ensure!(
                    caller.is_none(),
                    "only the human can re-roll Counsel's avatar"
                );
                self.boss.with_operation_lock(|| {
                    let document = self.boss.document();
                    let mut counsel = document.counsel.clone();
                    let expected = counsel.revision;
                    let identity = counsel
                        .identity
                        .get_or_insert_with(|| counsel_identity_seed(&document));
                    identity.avatar_seed = Uuid::new_v4().to_string();
                    self.boss.save_counsel(expected, counsel, None)?;
                    Ok(BossResult::Counsel {
                        state: self.boss.document().counsel,
                    })
                })
            }
            CounselOperation::Complete {
                id,
                expected_revision,
                evidence,
            } => {
                self.require_counsel_reader(caller)?;
                anyhow::ensure!(
                    !evidence.trim().is_empty() && evidence.chars().count() <= 2000,
                    "completion requires bounded reported evidence"
                );
                self.boss.with_operation_lock(|| {
                    let mut counsel = self.boss.document().counsel;
                    let expected = counsel.revision;
                    let record = counsel
                        .recommendations
                        .iter_mut()
                        .find(|record| record.id == id)
                        .ok_or_else(|| anyhow!("unknown Counsel recommendation"))?;
                    anyhow::ensure!(
                        record.revision == expected_revision
                            && record.status == CounselStatus::Sent,
                        "only the current sent recommendation can be completed"
                    );
                    record.status = CounselStatus::Completed;
                    record.completion_evidence = Some(evidence.trim().into());
                    record.updated_at = crate::model::unix_time();
                    record.revision += 1;
                    self.boss.save_counsel(expected, counsel, None)?;
                    Ok(BossResult::Counsel {
                        state: self.boss.document().counsel,
                    })
                })
            }
        }
    }

    /// Outbound messages require the authenticated Counsel session to be
    /// answering a human prompt. The persona determines whether that prompt
    /// actually instructs a send. Hidden reviews and agent prompts carry no
    /// authority, even if they contain text that looks like an instruction.
    pub(super) fn require_counsel_human_turn(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<(ProviderKind, Option<String>, crate::model::RuntimeMode)> {
        anyhow::ensure!(
            self.boss.is_counsel(session_id) && !self.agent.user_stopped(session_id),
            "only active Counsel can relay this instruction"
        );
        let mut state = self.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| anyhow!("Counsel chat is unavailable"))?;
        self.task_store.hydrate(session)?;
        let human_input = session.active_turn_id().is_some_and(|turn| {
            session
                .messages
                .iter()
                .rev()
                .find(|message| {
                    message.turn_id == Some(turn) && message.role == crate::model::MessageRole::User
                })
                .is_some_and(|message| !message.hidden && message.sent_by_task.is_none())
        });
        anyhow::ensure!(
            human_input,
            "Counsel can relay instructions only while answering a human message"
        );
        Ok((
            session.provider,
            session.model.clone(),
            session.runtime_mode,
        ))
    }

    /// Send the exact approved recommendation to Boss. The durable mirror carries the
    /// record's id so parked delivery, transcript landing, and rotation
    /// lineage all dedupe on the same identity — a retry can never send the
    /// approved instruction twice.
    fn send_counsel_recommendation(
        &self,
        caller: Option<Uuid>,
        id: Uuid,
        expected_revision: u64,
        events: &EventSink,
    ) -> anyhow::Result<BossResult> {
        anyhow::ensure!(
            caller.is_some_and(|caller| self.boss.is_counsel(caller)),
            "only Counsel can deliver an approved recommendation"
        );
        enum Plan {
            Already { state: CounselState },
            Deliver { counsel: Uuid, boss: Uuid, prompt: String },
        }
        let plan = self.boss.with_operation_lock(|| {
            let document = self.boss.document();
            let mut counsel = document.counsel;
            let expected = counsel.revision;
            let counsel_id = counsel
                .session_id
                .ok_or_else(|| anyhow!("Counsel chat is unavailable"))?;
            let record = counsel
                .recommendations
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| anyhow!("unknown Counsel recommendation"))?;
            anyhow::ensure!(
                record.approved_revision == Some(expected_revision),
                "recommendation revision does not match the approved one; run `goddard-agent boss counsel view` and resend the approved revision"
            );
            match record.status {
                CounselStatus::Sent | CounselStatus::Completed => {
                    return Ok(Plan::Already { state: counsel });
                }
                CounselStatus::Approved
                | CounselStatus::DeliveryPending
                | CounselStatus::DeliveryFailed => {}
                _ => anyhow::bail!(
                    "only a human-approved recommendation can be sent to Boss"
                ),
            }
            let boss = self
                .boss_active_alias(
                    document
                        .session_id
                        .ok_or_else(|| anyhow!("Boss chat is unavailable"))?,
                );
            anyhow::ensure!(
                !self.boss_incident_fenced(boss) && !self.session_quarantined(boss),
                "Boss is not accepting instructions right now"
            );
            let prompt = format!(
                "Human-approved Counsel recommendation: {}\n\n{}\n\nWhy now: {}\n\nApproved in [Counsel](goddard://task/{counsel_id}). Supporting sessions: {}.\nCarry out this approved instruction and report the result here.",
                record.title,
                record.instruction,
                record.reason,
                record.sources.iter().map(|source| format!("[Session](goddard://task/{})", source.session_id)).collect::<Vec<_>>().join(", ")
            );
            // Park the durable chip under the record id — unless a previous
            // attempt already landed the instruction in the transcript.
            let mut state = self.task_state.lock();
            let mut previous = Vec::new();
            let session = state
                .sessions
                .iter_mut()
                .find(|session| session.id == boss)
                .ok_or_else(|| anyhow!("Boss chat is unavailable"))?;
            self.task_store.hydrate(session)?;
            previous.push((boss, session.clone()));
            let landed = session.messages.iter().any(|message| message.id == id);
            if !landed && !session.queued_messages.iter().any(|queued| queued.id == id) {
                let mut mirror = crate::model::QueuedMessage::agent(&prompt, Some(counsel_id));
                mirror.id = id;
                session.queued_messages.push(mirror);
            }
            state.mark_session_dirty(boss);
            record.status = CounselStatus::DeliveryPending;
            record.boss_session_id = Some(boss);
            record.delivery_error = None;
            record.updated_at = crate::model::unix_time();
            record.revision += 1;
            let result = self.save_counsel_sessions(&mut state, expected, counsel);
            if let Err(error) = result {
                for (id, old) in previous {
                    if let Some(session) =
                        state.sessions.iter_mut().find(|session| session.id == id)
                    {
                        *session = old;
                    }
                }
                return Err(error);
            }
            Ok::<_, anyhow::Error>(Plan::Deliver {
                counsel: counsel_id,
                boss,
                prompt,
            })
        })?;
        self.boss.notify_session_commit();
        let Plan::Deliver {
            counsel: counsel_id,
            boss,
            prompt,
        } = plan
        else {
            let Plan::Already { state } = plan else {
                unreachable!()
            };
            return Ok(BossResult::Counsel { state });
        };
        let delivery = self.deliver_counsel_instruction(boss, counsel_id, id, prompt.clone(), events);
        let outcome = self.boss.with_operation_lock(|| {
            let mut counsel = self.boss.document().counsel;
            let expected = counsel.revision;
            let mut state = self.task_state.lock();
            let mut previous = Vec::new();
            let mut receipt: Option<String> = None;
            {
                let record = counsel
                    .recommendations
                    .iter_mut()
                    .find(|record| record.id == id)
                    .ok_or_else(|| anyhow!("unknown Counsel recommendation"))?;
                // A stale outcome must not overwrite a newer state — the
                // record still carries this attempt's pending marker+target.
                if record.status == CounselStatus::DeliveryPending
                    && record.boss_session_id == Some(boss)
                {
                    match &delivery {
                        Ok(()) => {
                            record.status = CounselStatus::Sent;
                            receipt = Some(format!(
                                "Sent to [Boss](goddard://task/{boss}): {}\n\n{}",
                                record.title, record.instruction
                            ));
                        }
                        Err(error) => {
                            record.status = CounselStatus::DeliveryFailed;
                            record.delivery_error =
                                Some(crate::model::truncate_chars(&format!("{error:#}"), 500));
                        }
                    }
                    record.updated_at = crate::model::unix_time();
                    record.revision += 1;
                }
            }
            if let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == counsel_id)
            {
                self.task_store.hydrate(session)?;
                previous.push((counsel_id, session.clone()));
                if let Some(receipt) = receipt {
                    session.push_message(crate::model::MessageRole::Assistant, &receipt);
                    state.mark_session_dirty(counsel_id);
                }
            }
            if let Err(error) = self.save_counsel_sessions(&mut state, expected, counsel) {
                for (id, old) in previous {
                    if let Some(session) =
                        state.sessions.iter_mut().find(|session| session.id == id)
                    {
                        *session = old;
                    }
                }
                return Err(error);
            }
            Ok::<_, anyhow::Error>(self.boss.document().counsel)
        })?;
        self.boss.notify_session_commit();
        delivery.map(|()| BossResult::Counsel { state: outcome })
    }

    /// Immediate delivery of a human-authorized instruction into Boss's live turn.
    /// A steerable runtime takes a steer — never the normal prompt queue —
    /// an unsteerable one is interrupted so the durable mirror drains as
    /// soon as the turn settles, and an idle boss starts on the drain.
    fn deliver_counsel_instruction(
        &self,
        target: Uuid,
        counsel: Uuid,
        record: Uuid,
        prompt: String,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let target = self.boss_active_alias(target);
        // The human's instruction owns delivery — it outranks a stale
        // stop the way a Boss correction does.
        self.agent.clear_user_stop(target);
        if let Some(driver) = self.steerable_driver(target) {
            let transport = agent_prompt_envelope(&self.task_state, target, Some(counsel), &prompt);
            if self.agent.record_pending_steer(
                target,
                crate::agent::AgentPrompt {
                    prompt: prompt.clone(),
                    transport: transport.clone(),
                    sender: Some(counsel),
                    // The pending steer's queued id lets the accepted echo
                    // unmirror the parked chip and write the transcript row
                    // under the record id — one identity end to end.
                    queued_id: Some(record),
                    submission_id: None,
                    context: None,
                    hidden: false,
                    report_trigger: None,
                },
            ) {
                driver.steer(transport.unwrap_or(prompt));
            }
            return Ok(());
        }
        let fallback = self
            .sessions
            .lock()
            .get(&target)
            .map(|entry| entry.driver.clone())
            .filter(|driver| !driver.supports_steer() && self.agent.has_open_turn(target));
        self.agent.enqueue(
            target,
            crate::agent::AgentPrompt {
                prompt,
                transport: None,
                sender: Some(counsel),
                queued_id: Some(record),
                submission_id: None,
                context: None,
                hidden: false,
                report_trigger: None,
            },
        );
        if let Some(driver) = fallback {
            // Unsupported steering: the parked mirror is already durable, so
            // interrupt the turn — its settle drains this prompt next rather
            // than waiting behind it.
            self.agent.expect_daemon_interrupt(target);
            driver.cancel();
            self.wake_summon_queue();
        }
        let (runtime_id, driver) = self.ensure_agent_runtime(target, events)?;
        let sink = events.for_session(target, runtime_id);
        self.drain_agent_queue(target, &driver, &sink)
    }

    /// The settled turn whose ending Jev should classify — `None` when it
    /// is not a candidate: no completed latest turn, no human-authored
    /// question in it, a stopped Counsel, or a source turn already claimed.
    /// Daemon/hidden prompts — including RECOMMEND itself — produce no
    /// human message, so a generated recommendation turn can never
    /// recursively wake another.
    pub(super) fn counsel_recommendation_candidate(
        &self,
        session_id: Uuid,
    ) -> Option<(Uuid, Value)> {
        let mut state = self.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)?;
        if self.task_store.hydrate(session).is_err() {
            return None;
        }
        let turn = session
            .turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Completed)?;
        let human_authored = session.messages.iter().any(|message| {
            message.turn_id == Some(turn.id)
                && message.role == crate::model::MessageRole::User
                && !message.hidden
                && message.sent_by_task.is_none()
        });
        if !human_authored || self.agent.user_stopped(session_id) {
            return None;
        }
        let counsel = self.boss.document().counsel;
        if counsel.session_id != Some(session_id)
            || counsel.has_outstanding_recommendation()
            || counsel.recommendation_turns.contains(&turn.id)
        {
            return None;
        }
        Some((
            turn.id,
            waku_protocol::auto_prompts::turn_state(session, turn.id),
        ))
    }

    /// Claim a source turn and park the recommendation prompt. Only runs
    /// after Jev judged the settled answer `answered`; the claim and the
    /// parked message commit together so a replayed settle or restart
    /// finds the turn handled and stops. Staleness is re-checked under
    /// the operation lock: intervening human input, a newer turn, a
    /// user stop, or a rotated Counsel session voids the verdict.
    pub(super) fn claim_counsel_recommendation_turn(
        &self,
        session_id: Uuid,
        source_turn: Uuid,
        events: &EventSink,
    ) -> bool {
        let mut message = crate::model::QueuedMessage::agent(RECOMMEND, None);
        message.hidden = true;
        let result = self.boss.with_operation_lock(|| -> std::io::Result<bool> {
            let mut counsel = self.boss.document().counsel;
            let expected = counsel.revision;
            let mut state = self.task_state.lock();
            let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            else {
                return Ok(false);
            };
            self.task_store.hydrate(session)?;
            // The verdict is stale when the conversation moved on — a
            // newer turn or a waiting human prompt wins.
            let latest = session.turns.last().is_some_and(|turn| turn.id == source_turn)
                && session.active_turn_id().is_none()
                && !session
                    .queued_messages
                    .iter()
                    .any(|queued| !queued.is_agent_owned());
            // Re-verify authorship under the lock: only a turn that carried
            // a human question may claim a pass, even if the caller path
            // already checked it at admission.
            let human_authored = session.messages.iter().any(|message| {
                message.turn_id == Some(source_turn)
                    && message.role == crate::model::MessageRole::User
                    && !message.hidden
                    && message.sent_by_task.is_none()
            });
            let eligible = latest
                && human_authored
                && counsel.session_id == Some(session_id)
                && !counsel.has_outstanding_recommendation()
                && !counsel.recommendation_turns.contains(&source_turn)
                && !self.agent.user_stopped(session_id);
            if !eligible {
                return Ok(false);
            }
            counsel.recommendation_turns.push(source_turn);
            session.queued_messages.push(message.clone());
            state.mark_session_dirty(session_id);
            let committed = self
                .save_counsel_sessions(&mut state, expected, counsel)
                .is_ok();
            if !committed
                && let Some(session) = state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
            {
                session
                    .queued_messages
                    .retain(|queued| queued.id != message.id);
            }
            Ok(committed)
        });
        let committed = match result {
            Ok(committed) => committed,
            Err(error) => {
                eprintln!("could not claim a Counsel recommendation turn: {error:#}");
                false
            }
        };
        if committed
            && let Err(error) = self.queue_agent_message(session_id, message, events)
        {
            eprintln!("Counsel's recommendation turn remains parked: {error:#}");
        }
        committed
    }

    /// A settled Counsel turn's ending classification can wake the separate
    /// recommendation turn. The eligibility check runs cheaply on the event
    /// forwarder; the eval itself runs off-thread like every other Jev call.
    pub(super) fn capture_counsel_answer_settled(&self, session_id: Uuid, events: EventSink) {
        if !self.counsel_enabled() {
            return;
        }
        let Ok(admission) = self.idle_shutdown.enter() else {
            return;
        };
        if self.resolved_eval().is_none() {
            return;
        }
        let Some((source_turn, state_for_eval)) = self.counsel_recommendation_candidate(session_id)
        else {
            return;
        };
        let (settings, secrets) = (self.settings.clone(), self.inference_secrets.clone());
        let this = self.weak_self();
        let _ = std::thread::Builder::new()
            .name("counsel-recommend".into())
            .spawn(move || {
                let _admission = admission;
                let questions = BTreeMap::from([(
                    waku_protocol::eval::TURN_ENDING_QUESTION.to_owned(),
                    waku_protocol::eval::turn_ending_question(),
                )]);
                let Ok(evaluation) =
                    evaluate_with_feature(&settings, &secrets, state_for_eval, questions, FEATURE, None)
                else {
                    return;
                };
                if !evaluation
                    .answers
                    .get(waku_protocol::eval::TURN_ENDING_QUESTION)
                    .is_some_and(|answer| {
                        waku_protocol::eval::turn_ending_cleared(
                            answer,
                            waku_protocol::eval::TURN_ENDING_ANSWERED,
                            waku_protocol::eval::TURN_ENDING_ANSWERED_THRESHOLD,
                        )
                    })
                {
                    return;
                }
                let Some(backend) = this.upgrade() else {
                    return;
                };
                backend.claim_counsel_recommendation_turn(session_id, source_turn, &events);
            });
    }

    fn require_counsel_reader(&self, caller: Option<Uuid>) -> anyhow::Result<()> {
        anyhow::ensure!(
            caller.is_none_or(|id| self.boss.is_counsel(id)),
            "only Counsel or the human can inspect or propose advice"
        );
        Ok(())
    }
}
