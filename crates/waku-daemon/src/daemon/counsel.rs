use super::*;
use waku_protocol::boss::{BossOperation, BossResult};
use waku_protocol::counsel::{
    CounselDecision, CounselOperation, CounselRecommendation, CounselSource, CounselState,
    CounselStatus,
};

const REVIEW: &str = "Review my recent sessions and current priorities. Check the relevant transcript endings, including recently archived work. Offer the clearest useful next action with supporting session links, or say there is no clear next step. Respect my previous Later and Dismiss choices, and check results of instructions already sent to Boss.";

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

    pub(super) fn handle_counsel_operation(
        &self,
        caller: Option<Uuid>,
        action: CounselOperation,
        events: &EventSink,
    ) -> anyhow::Result<BossResult> {
        match action {
            CounselOperation::Open {
                provider,
                model,
                mode,
            } => {
                anyhow::ensure!(caller.is_none(), "only the human can open Counsel");
                let (session, project, review) = self.boss.with_operation_lock(|| {
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
                    let (_, activity) =
                        waku_boss::counsel::recent_context(&state.projects, &state.sessions, id);
                    let changed = counsel.reviewed_activity.as_ref() != Some(&activity);
                    // A queued review reads fresh context at delivery. Reuse
                    // it when more work changes while Counsel is busy/offline.
                    let review_pending =
                        state.sessions[index].queued_messages.iter().any(|message| {
                            message.is_agent_owned() && message.hidden && message.content == REVIEW
                        });
                    let review = if changed && !review_pending {
                        let mut message = crate::model::QueuedMessage::agent(REVIEW, None);
                        message.hidden = true;
                        Some(message)
                    } else {
                        None
                    };
                    if created || changed || state.sessions[index].archived_at.is_some() {
                        let previous = state.sessions[index].clone();
                        state.sessions[index].archived_at = None;
                        if let Some(message) = &review {
                            state.sessions[index].queued_messages.push(message.clone());
                        }
                        counsel.reviewed_activity = Some(activity);
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
                    Ok::<_, anyhow::Error>((state.sessions[index].clone(), project, review))
                })?;
                self.boss.notify_session_commit();
                if let Some(message) = review {
                    // A failed launch remains durable; report it to the client
                    // and let the next Open retry that same queued identity.
                    self.queue_agent_message(session.id, message, events)
                        .context("Counsel is saved, but its queued review could not start")?;
                } else if !self.agent.user_stopped(session.id)
                    && session
                        .queued_messages
                        .iter()
                        .any(|message| message.is_agent_owned())
                {
                    let (runtime, driver) = self
                        .ensure_agent_runtime(session.id, events)
                        .context("Counsel's saved review could not resume")?;
                    self.drain_agent_queue(
                        session.id,
                        &driver,
                        &events.for_session(session.id, runtime),
                    )?;
                }
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
                    let state = self.task_state.lock();
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
                let boss_session = if decision == CounselDecision::Send {
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
                let (counsel, message) = self.boss.with_operation_lock(|| {
                    let document = self.boss.document();
                    // A rotation may have committed after Open returned. Bind
                    // the queue and approval to the live owner under this lock.
                    let boss_session = boss_session.map(|_| document.session_id.ok_or_else(|| anyhow!("Boss chat is unavailable"))).transpose()?;
                    let mut counsel = document.counsel;
                    let expected = counsel.revision;
                    let record = counsel.recommendations.iter_mut().find(|record| record.id == id)
                        .ok_or_else(|| anyhow!("unknown Counsel recommendation"))?;
                    anyhow::ensure!(record.revision == expected_revision, "recommendation changed; refresh before deciding");
                    anyhow::ensure!(matches!(record.status, CounselStatus::Proposed | CounselStatus::Later), "recommendation has already been resolved");
                    record.status = match decision { CounselDecision::Send => CounselStatus::Sent, CounselDecision::Later => CounselStatus::Later, CounselDecision::Dismiss => CounselStatus::Dismissed };
                    record.revision += 1;
                    record.updated_at = crate::model::unix_time();
                    record.boss_session_id = boss_session;
                    let counsel_id = counsel.session_id.ok_or_else(|| anyhow!("Counsel chat is unavailable"))?;
                    let message = boss_session.map(|target| {
                        let links = record.sources.iter().map(|source| format!("[Session](goddard://task/{})", source.session_id)).collect::<Vec<_>>().join(", ");
                        let prompt = format!("Human-approved Counsel recommendation: {}\n\n{}\n\nWhy now: {}\n\nApproved in [Counsel](goddard://task/{counsel_id}). Supporting sessions: {links}.\nCarry out this approved instruction and report the result here.", record.title, record.instruction, record.reason);
                        let mut message = crate::model::QueuedMessage::agent(prompt, Some(counsel_id));
                        message.id = id;
                        (target, message)
                    });
                    let mut state = self.task_state.lock();
                    let mut previous = Vec::new();
                    if let Some((target, message)) = &message {
                        let session = state.sessions.iter_mut().find(|session| session.id == *target).ok_or_else(|| anyhow!("Boss chat is unavailable"))?;
                        self.task_store.hydrate(session)?;
                        previous.push((*target, session.clone()));
                        session.queued_messages.push(message.clone());
                        state.mark_session_dirty(*target);
                    }
                    if let Some(session) = state.sessions.iter_mut().find(|session| session.id == counsel_id) {
                        self.task_store.hydrate(session)?;
                        previous.push((counsel_id, session.clone()));
                        let decision_text = match boss_session {
                            Some(target) => format!("Sent to [Boss](goddard://task/{target}): {}\n\n{}", record.title, record.instruction),
                            None => format!("{}: {}", if decision == CounselDecision::Later { "Saved for later" } else { "Dismissed" }, record.title),
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
                    Ok((self.boss.document().counsel, message))
                })?;
                self.boss.notify_session_commit();
                if let Some((target, message)) = message {
                    if let Err(error) = self.queue_agent_message(target, message, events) {
                        eprintln!(
                            "Approved Counsel instruction remains queued for Boss: {error:#}"
                        );
                    }
                }
                Ok(BossResult::Counsel { state: counsel })
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

    fn require_counsel_reader(&self, caller: Option<Uuid>) -> anyhow::Result<()> {
        anyhow::ensure!(
            caller.is_none_or(|id| self.boss.is_counsel(id)),
            "only Counsel or the human can inspect or propose advice"
        );
        Ok(())
    }
}
