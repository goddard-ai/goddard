use super::*;
use waku_protocol::task_metadata::{TaskMetadataEdit, TaskMetadataField, TaskMetadataUpdate};

#[derive(Clone)]
pub(super) struct MetadataRequest {
    session_id: Uuid,
    sequence: u64,
    edited_at: u64,
    edit: TaskMetadataEdit,
    failures: u32,
    observed: Option<(u64, TaskMetadataEdit)>,
}

pub(super) struct MetadataResult {
    request: MetadataRequest,
    result: Result<AgentSession, String>,
}

#[derive(Default)]
pub(super) struct PendingTaskMetadata {
    sequence: u64,
    edits: HashMap<(Uuid, TaskMetadataField), MetadataRequest>,
}

impl PendingTaskMetadata {
    fn record(&mut self, session: &AgentSession, field: TaskMetadataField) -> MetadataRequest {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("metadata edit sequence exhausted");
        let mut edit = TaskMetadataEdit::capture(field, session);
        if let TaskMetadataEdit::Placement { dormant_at, .. } = &mut edit {
            // Expired stamps are history, not an intent to make the task dormant.
            *dormant_at = dormant_at.filter(|at| *at >= session.updated_at);
        }
        let request = MetadataRequest {
            session_id: session.id,
            sequence: self.sequence,
            edited_at: session.updated_at,
            edit,
            failures: 0,
            observed: None,
        };
        self.edits.insert((session.id, field), request.clone());
        request
    }

    fn current(&self, request: &MetadataRequest) -> bool {
        self.edits
            .get(&(request.session_id, request.edit.field()))
            .is_some_and(|current| current.sequence == request.sequence)
    }

    pub(super) fn remove(&mut self, id: &Uuid) {
        self.edits.retain(|(session_id, _), _| session_id != id);
    }

    pub(super) fn is_archiving(&self, id: &Uuid) -> bool {
        self.edits
            .get(&(*id, TaskMetadataField::Archive))
            .is_some_and(|request| {
                matches!(
                    request.edit,
                    TaskMetadataEdit::Archive {
                        archived_at: Some(_)
                    }
                )
            })
    }

    pub(super) fn archiving_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.edits.iter().filter_map(|((id, _), request)| {
            matches!(
                request.edit,
                TaskMetadataEdit::Archive {
                    archived_at: Some(_)
                }
            )
            .then_some(*id)
        })
    }

    pub(super) fn observe(&mut self, session: &mut AgentSession) {
        for field in [
            TaskMetadataField::Title,
            TaskMetadataField::Placement,
            TaskMetadataField::Archive,
        ] {
            if let Some(request) = self.edits.get_mut(&(session.id, field))
                && request
                    .observed
                    .as_ref()
                    .is_none_or(|(revision, _)| *revision <= session.metadata_revision)
            {
                request.observed = Some((
                    session.metadata_revision,
                    TaskMetadataEdit::capture(field, session),
                ));
            }
        }
        self.overlay(session);
    }

    pub(super) fn overlay(&self, session: &mut AgentSession) {
        // Pinning and dormancy share one atomic edit, so a later pin also
        // supersedes a failed dormant transition's unpin on retry.
        for field in [
            TaskMetadataField::Title,
            TaskMetadataField::Placement,
            TaskMetadataField::Archive,
        ] {
            if let Some(request) = self.edits.get(&(session.id, field)) {
                request.edit.apply(session);
                session.updated_at = session.updated_at.max(request.edited_at);
                if let TaskMetadataEdit::Placement {
                    dormant_at: Some(at),
                    ..
                } = request.edit
                {
                    session.dormant_at = Some(at.max(session.updated_at));
                }
            }
        }
    }

    fn acknowledge(
        &mut self,
        request: &MetadataRequest,
        saved: &AgentSession,
        local: &mut AgentSession,
    ) {
        local.metadata_revision = local.metadata_revision.max(saved.metadata_revision);
        if self.current(request) {
            let pending = self
                .edits
                .remove(&(request.session_id, request.edit.field()))
                .expect("current metadata edit");
            // A catalog may have observed another client's later edit while
            // this acknowledgement was queued. Release to that newer value.
            let (revision, confirmed) = pending
                .observed
                .filter(|(revision, _)| *revision > saved.metadata_revision)
                .unwrap_or_else(|| {
                    (
                        saved.metadata_revision,
                        TaskMetadataEdit::capture(request.edit.field(), saved),
                    )
                });
            local.metadata_revision = local.metadata_revision.max(revision);
            confirmed.apply(local);
        }
        self.overlay(local);
    }
}

impl Waku {
    pub(super) fn record_task_metadata(&mut self, id: Uuid, field: TaskMetadataField) {
        let Some(session) = self.state.sessions.iter().find(|session| session.id == id) else {
            return;
        };
        // Draft names remain local until the first prompt creates a daemon row.
        if !session.has_started() && !session.boss_managed {
            return;
        }
        let request = self.pending_task_metadata.record(session, field);
        self.enqueue_task_metadata(request);
    }

    fn enqueue_task_metadata(&mut self, request: MetadataRequest) {
        let key = self.daemons.session_owner(request.session_id);
        if !self.task_metadata_workers.contains_key(&key) {
            let (tx, rx) = unbounded::<MetadataRequest>();
            let daemons = self.daemons.clone();
            let results = self.task_metadata_tx.clone();
            let wake = self.event_wake_tx.clone();
            // One queue per daemon preserves rapid edit order without letting
            // a slow remote host stall another host or the UI thread.
            let spawned = std::thread::Builder::new()
                .name(format!("task-metadata-{key:?}"))
                .spawn(move || {
                    while let Ok(request) = rx.recv() {
                        // Bound each batch while combining bulk sidebar actions
                        // into one daemon transaction instead of one per row.
                        let requests = std::iter::once(request)
                            .chain(rx.try_iter().take(127))
                            .collect::<Vec<_>>();
                        let result = (|| -> anyhow::Result<Vec<AgentSession>> {
                            let supervisor = daemons
                                .supervisor(key)
                                .ok_or_else(|| anyhow::anyhow!("task host is disconnected"))?;
                            let reply = supervisor.client().request(
                                Uuid::nil(),
                                Uuid::nil(),
                                waku_client::Command::UpdateTaskMetadata {
                                    updates: requests
                                        .iter()
                                        .map(|request| TaskMetadataUpdate {
                                            session_id: request.session_id,
                                            edit: request.edit.clone(),
                                        })
                                        .collect(),
                                },
                            )?;
                            let waku_client::ResponsePayload::TaskStateSaved { sessions } = reply
                            else {
                                anyhow::bail!("invalid task save response");
                            };
                            Ok(sessions)
                        })()
                        .map_err(|error| error.to_string());
                        for request in requests {
                            let result = match &result {
                                Ok(sessions) => sessions
                                    .iter()
                                    .find(|session| session.id == request.session_id)
                                    .cloned()
                                    .ok_or_else(|| "task save was not acknowledged".to_owned()),
                                Err(error) => Err(error.clone()),
                            };
                            if results.send(MetadataResult { request, result }).is_err() {
                                return;
                            }
                        }
                        signal_event_pump(&wake);
                    }
                });
            if let Err(error) = spawned {
                let _ = self.task_metadata_tx.send(MetadataResult {
                    request,
                    result: Err(error.to_string()),
                });
                signal_event_pump(&self.event_wake_tx);
                return;
            }
            self.task_metadata_workers.insert(key, tx);
        }
        if let Some(worker) = self.task_metadata_workers.get(&key)
            && let Err(error) = worker.send(request)
        {
            self.task_metadata_workers.remove(&key);
            let _ = self.task_metadata_tx.send(MetadataResult {
                request: error.0,
                result: Err("task save worker stopped".into()),
            });
            signal_event_pump(&self.event_wake_tx);
        }
    }

    pub(super) fn drain_task_metadata_results(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok(MetadataResult {
            mut request,
            result,
        }) = self.task_metadata_events.try_recv()
        {
            match result {
                Ok(saved) => {
                    if let Some(local) = self.state.session_mut(request.session_id) {
                        self.pending_task_metadata
                            .acknowledge(&request, &saved, local);
                        changed = true;
                    }
                }
                Err(error) if self.pending_task_metadata.current(&request) => {
                    request.failures = request.failures.saturating_add(1);
                    eprintln!("could not save task changes: {error}");
                    if request.failures == 3 {
                        self.show_toast(tr!("errors.task_changes_retrying"));
                        changed = true;
                    }
                    let delay = Duration::from_secs(2u64.saturating_pow(request.failures.min(5)));
                    cx.spawn(async move |waku, cx| {
                        cx.background_executor().timer(delay).await;
                        let _ = waku.update(cx, |waku, _| {
                            if waku.pending_task_metadata.current(&request) {
                                waku.enqueue_task_metadata(request);
                            }
                        });
                    })
                    .detach();
                }
                Err(_) => {}
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::runtime::merge_remote_session_catalog;

    #[test]
    fn task_metadata_waits_for_the_latest_acknowledgement_and_rejects_old_catalogs() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        let stale = session.list_projection();
        let mut pending = PendingTaskMetadata::default();
        // Pin -> unpin -> pin within one clock tick. A snapshot matching an
        // intermediate value must never be mistaken for an acknowledgement.
        session.pinned_at = Some(1);
        let first = pending.record(&session, TaskMetadataField::Placement);
        session.pinned_at = None;
        let second = pending.record(&session, TaskMetadataField::Placement);
        session.pinned_at = Some(2);
        let third = pending.record(&session, TaskMetadataField::Placement);
        let mut remote = stale.clone();
        pending.observe(&mut remote);
        assert_eq!(remote.pinned_at, Some(2));
        assert!(pending.current(&third));

        for (request, pin, revision) in [
            (&first, Some(1), 1),
            (&second, None, 2),
            (&third, Some(2), 3),
        ] {
            let mut saved = stale.clone();
            saved.pinned_at = pin;
            saved.metadata_revision = revision;
            pending.acknowledge(request, &saved, &mut session);
            assert_eq!(session.pinned_at, Some(2));
        }
        assert!(pending.edits.is_empty());
        let mut catalog = vec![session];
        merge_remote_session_catalog(
            &mut catalog,
            vec![stale.clone()],
            |_| true,
            |_| false,
            false,
        );
        assert_eq!(catalog[0].pinned_at, Some(2));

        let mut changed_elsewhere = stale;
        changed_elsewhere.metadata_revision = 4;
        merge_remote_session_catalog(
            &mut catalog,
            vec![changed_elsewhere],
            |_| true,
            |_| false,
            false,
        );
        assert_eq!(catalog[0].pinned_at, None);
    }

    #[test]
    fn task_metadata_keeps_independent_edits_and_supersedes_dormancy_retries() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        let mut pending = PendingTaskMetadata::default();
        session.set_title_from_user("Renamed");
        let rename = pending.record(&session, TaskMetadataField::Title);
        session.archived_at = Some(1);
        pending.record(&session, TaskMetadataField::Archive);
        assert!(pending.is_archiving(&session.id));
        let mut unarchived_snapshot = session.list_projection();
        unarchived_snapshot.archived_at = None;
        pending.observe(&mut unarchived_snapshot);
        assert_eq!(unarchived_snapshot.archived_at, Some(1));
        session.archived_at = None;
        pending.record(&session, TaskMetadataField::Archive);
        assert!(!pending.is_archiving(&session.id));
        session.dormant_at = Some(session.updated_at);
        session.pinned_at = None;
        let dormant = pending.record(&session, TaskMetadataField::Placement);
        session.pinned_at = Some(2);
        pending.record(&session, TaskMetadataField::Placement);
        assert!(!pending.current(&dormant));

        let mut remote = session.list_projection();
        remote.title = "Old name".into();
        remote.archived_at = Some(1);
        remote.pinned_at = None;
        pending.observe(&mut remote);
        assert_eq!(remote.title, "Renamed");
        assert_eq!(remote.archived_at, None);
        assert_eq!(remote.pinned_at, Some(2));

        // A later external rename can arrive before the older save reply.
        let mut newer = session.list_projection();
        newer.metadata_revision = 5;
        newer.title = "Changed elsewhere".into();
        pending.observe(&mut newer);
        let mut saved = session.list_projection();
        saved.metadata_revision = 1;
        pending.acknowledge(&rename, &saved, &mut session);
        assert_eq!(session.title, "Changed elsewhere");
        assert_eq!(session.metadata_revision, 5);
        assert_eq!(session.pinned_at, Some(2));
        // A superseded request's failure/retry is ignored.
        assert!(!pending.current(&rename));
    }
}
